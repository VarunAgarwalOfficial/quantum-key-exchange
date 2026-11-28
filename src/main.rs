//! The `qkx` command: demo, bob, alice, rates.

use std::io::{self, Write as _};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

use qkx::cascade;
use qkx::channel::{Channel, Eve, Strategy};
use qkx::photon::Basis;
use qkx::protocol::{binary_entropy, Estimate, Settings, MARGIN_BITS, VERIFY_BITS};
use qkx::rng::{OsRandom, Random, Xoshiro};
use qkx::session::{self, AliceConfig, Party, MAX_PHOTONS};
use qkx::simulate::{self, Record, Report, Verdict};

const HELP: &str = "\
qkx: BB84 quantum key distribution with simulated photons.

Usage:
  qkx demo [MESSAGE] [OPTIONS]   Alice, the line and Bob in one process, every step shown
  qkx bob [--host H] [--port P]  wait for Alice, then print the key and her message
  qkx alice [MESSAGE] [OPTIONS]  connect to Bob and run the exchange
  qkx rates [--photons N] [--seed N]
                                 secret key rate against channel error, and what
                                 each eavesdropper strategy does
  qkx help                       this text

Options for demo and alice:
  --eve STRATEGY   none, random, rect or diag: the basis Eve measures in
                   (intercept and resend). Default none.
  --fraction F     share of photons Eve intercepts, 0 to 1. Default 1.
  --noise P        chance that the line flips a photon, 0 to 0.5. Default 0.01.
  --photons N      photons to send. Default 20000.
  --sample F       share of the sifted key revealed to estimate errors. Default 0.25.
  --limit E        error rate above which both stop. Default 0.11.
  --seed N         demo only: repeat a run exactly.

QKX_HOST and QKX_PORT set where bob listens and alice connects (default
127.0.0.1:5050); --host and --port override them. The photons are simulated:
see docs/limits.md for what that does and does not show.

Exit status: 0 when a key was agreed and the message delivered, 1 on an
error, 2 for a wrong command line, 3 when the exchange stopped (too many
errors, failed verification, or no key left).
";

const TABLE_WIDTH: usize = 32;
const TIMEOUT: Duration = Duration::from_secs(30);

enum Failure {
    Usage(String),
    Error(String),
    Stopped,
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Self {
        Failure::Error(e.to_string())
    }
}

impl From<qkx::wire::Error> for Failure {
    fn from(e: qkx::wire::Error) -> Self {
        Failure::Error(e.to_string())
    }
}

/// Command line words: the positional ones and `--name value` pairs.
struct Args {
    positional: Vec<String>,
    options: Vec<(String, String)>,
}

impl Args {
    fn parse(words: &[String], known: &[&str]) -> Result<Self, Failure> {
        let mut args = Args {
            positional: Vec::new(),
            options: Vec::new(),
        };
        let mut words = words.iter();
        while let Some(word) = words.next() {
            if let Some(name) = word.strip_prefix("--") {
                if !known.contains(&name) {
                    return Err(Failure::Usage(format!("unknown option --{name}")));
                }
                let value = words
                    .next()
                    .ok_or_else(|| Failure::Usage(format!("--{name} needs a value")))?;
                args.options.push((name.to_string(), value.clone()));
            } else {
                args.positional.push(word.clone());
            }
        }
        if args.positional.len() > 1 {
            return Err(Failure::Usage(
                "put the message in quotes: one positional argument only".into(),
            ));
        }
        Ok(args)
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn number<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, Failure> {
        match self.get(name) {
            None => Ok(default),
            Some(text) => text
                .parse()
                .map_err(|_| Failure::Usage(format!("--{name}: cannot read {text:?} as a number"))),
        }
    }

    fn fraction(&self, name: &str, default: f64, max: f64) -> Result<f64, Failure> {
        let value: f64 = self.number(name, default)?;
        if (0.0..=max).contains(&value) {
            Ok(value)
        } else {
            Err(Failure::Usage(format!(
                "--{name} must be between 0 and {max}"
            )))
        }
    }

    fn message(&self) -> &str {
        self.positional.first().map_or("", String::as_str)
    }
}

const EXCHANGE_OPTIONS: [&str; 9] = [
    "eve", "fraction", "noise", "photons", "sample", "limit", "seed", "host", "port",
];

/// The settings, line and photon count shared by demo and alice.
fn exchange_config(args: &Args) -> Result<AliceConfig, Failure> {
    let strategy = match args.get("eve") {
        None => None,
        Some(name) => Strategy::parse(name)
            .ok_or_else(|| Failure::Usage(format!("--eve: unknown strategy {name:?}")))?,
    };
    let fraction = args.fraction("fraction", 1.0, 1.0)?;
    let photons: usize = args.number("photons", 20_000)?;
    if photons == 0 || photons > MAX_PHOTONS {
        return Err(Failure::Usage(format!(
            "--photons must be 1 to {MAX_PHOTONS}"
        )));
    }
    let sample_fraction = args.fraction("sample", 0.25, 0.99)?;
    Ok(AliceConfig {
        photons,
        settings: Settings {
            sample_fraction,
            threshold: args.fraction("limit", 0.11, 0.5)?,
        },
        channel: Channel {
            noise: args.fraction("noise", 0.01, 0.5)?,
            eve: strategy.map(|strategy| Eve { strategy, fraction }),
        },
    })
}

fn address(args: &Args) -> Result<(String, u16), Failure> {
    let host = match args.get("host") {
        Some(h) => h.to_string(),
        None => std::env::var("QKX_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
    };
    let port = match args.get("port") {
        Some(p) => p.to_string(),
        None => std::env::var("QKX_PORT").unwrap_or_else(|_| "5050".into()),
    };
    let port = port
        .parse()
        .map_err(|_| Failure::Usage(format!("port {port:?} is not a number from 0 to 65535")))?;
    Ok((host, port))
}

fn percent(x: f64) -> String {
    format!("{:.1}%", 100.0 * x)
}

fn row(label: &str, cells: impl Iterator<Item = char>) -> String {
    let cells: Vec<String> = cells.map(String::from).collect();
    format!("  {label:<12} {}", cells.join(" "))
}

fn bit(b: bool) -> char {
    if b {
        '1'
    } else {
        '0'
    }
}

fn photon_table(records: &[Record], with_eve: bool) -> Vec<String> {
    let shown = &records[..records.len().min(TABLE_WIDTH)];
    let mut lines = vec![
        row("Alice bit", shown.iter().map(|r| bit(r.alice_bit))),
        row("Alice basis", shown.iter().map(|r| r.alice_basis.symbol())),
    ];
    if with_eve {
        lines.push(row(
            "Eve basis",
            shown.iter().map(|r| r.eve.map_or('.', |(b, _)| b.symbol())),
        ));
    }
    lines.push(row("Bob basis", shown.iter().map(|r| r.bob_basis.symbol())));
    lines.push(row("Bob bit", shown.iter().map(|r| bit(r.bob_bit))));
    lines.push(row(
        "kept",
        shown.iter().map(|r| if r.kept() { '*' } else { '.' }),
    ));
    if shown.iter().any(Record::error) {
        lines.push(row(
            "error",
            shown.iter().map(|r| if r.error() { 'x' } else { '.' }),
        ));
    }
    lines
}

/// The numbers both programs can know, in protocol order.
struct Steps<'a> {
    photons: usize,
    sifted: usize,
    estimate: &'a Estimate,
    raw_len: usize,
    correction: cascade::Outcome,
    final_len: usize,
    verdict: Verdict,
    limit: f64,
}

impl Steps<'_> {
    fn lines(&self) -> Vec<String> {
        let e = self.estimate;
        let mut out = vec![
            format!(
                "  sifted       {} of {} photons ({}): the bases matched",
                self.sifted,
                self.photons,
                percent(self.sifted as f64 / self.photons.max(1) as f64)
            ),
            format!("  sample       {} bits revealed and thrown away", e.size),
            format!(
                "  error rate   {} ({} of {}); + basis {}, x basis {}; limit {}",
                percent(e.qber()),
                e.errors,
                e.size,
                percent(e.qber_in(Basis::Rectilinear)),
                percent(e.qber_in(Basis::Diagonal)),
                percent(self.limit)
            ),
        ];
        if e.qber() > self.limit {
            out.push(
                "  stopped      the error rate is above the limit: someone may be listening".into(),
            );
            return out;
        }
        let c = self.correction;
        let shannon =
            self.raw_len as f64 * binary_entropy(c.corrected as f64 / self.raw_len.max(1) as f64);
        let efficiency = if shannon > 0.0 {
            format!(", {:.2} x the Shannon limit", c.leaked as f64 / shannon)
        } else {
            String::new()
        };
        out.push(format!(
            "  Cascade      {} wrong bits fixed in a {}-bit key; {} parities revealed{efficiency}",
            c.corrected, self.raw_len, c.leaked
        ));
        if self.verdict == Verdict::VerificationFailed {
            out.push(
                "  stopped      the 64-bit verification hashes differ: errors survived".into(),
            );
            return out;
        }
        out.push("  verified     64-bit Toeplitz hashes of both keys match".into());
        let error = e
            .qber()
            .max(c.corrected as f64 / self.raw_len.max(1) as f64);
        out.push(format!(
            "  amplified    {} bits = {} - h({}) x {} - {} parities - {} hash - {} margin",
            self.final_len,
            self.raw_len,
            percent(error),
            self.raw_len,
            c.leaked,
            VERIFY_BITS,
            MARGIN_BITS
        ));
        if self.verdict == Verdict::NothingLeft {
            out.push(
                "  stopped      nothing is left of the key after privacy amplification".into(),
            );
        } else {
            out.push(format!(
                "  key rate     {:.4} secret bits per photon sent",
                self.final_len as f64 / self.photons.max(1) as f64
            ));
        }
        out
    }
}

fn steps_of_party(party: &Party) -> Steps<'_> {
    Steps {
        photons: party.photons,
        sifted: party.sifted,
        estimate: &party.estimate,
        raw_len: party.raw_len,
        correction: party.correction,
        final_len: party.final_len,
        verdict: party.verdict,
        limit: party.limit,
    }
}

fn steps_of_report(report: &Report, limit: f64) -> Steps<'_> {
    Steps {
        photons: report.records.len(),
        sifted: report.sifted,
        estimate: &report.estimate,
        raw_len: report.raw_len,
        correction: report.correction,
        final_len: report.final_len,
        verdict: report.verdict,
        limit,
    }
}

fn hex(bytes: &[u8]) -> String {
    let text: String = bytes.iter().take(32).map(|b| format!("{b:02x}")).collect();
    if bytes.len() > 32 {
        format!("{text}... ({} bytes)", bytes.len())
    } else if bytes.is_empty() {
        "(empty message)".into()
    } else {
        text
    }
}

fn key_preview(key: &[bool]) -> String {
    let shown: String = key.iter().take(48).map(|&b| bit(b)).collect();
    if key.len() > 48 {
        format!("{shown}...")
    } else {
        shown
    }
}

fn describe_line(channel: &Channel) -> String {
    let noise = format!("line noise {}", percent(channel.noise));
    match channel.eve {
        None => format!("{noise}, no eavesdropper"),
        Some(eve) => format!(
            "{noise}, Eve intercepts {} of the photons, basis {}",
            percent(eve.fraction),
            eve.strategy.name()
        ),
    }
}

fn demo(args: &Args) -> Result<(), Failure> {
    let config = exchange_config(args)?;
    let seed = match args.get("seed") {
        Some(_) => args.number("seed", 0u64)?,
        None => OsRandom::new()?.next_u64(),
    };
    let mut rng = Xoshiro::new(seed);
    let report = simulate::run(
        config.photons,
        &config.channel,
        &config.settings,
        true,
        &mut rng,
    );
    let message = args.message().as_bytes();

    println!(
        "{} photons, {}, seed {seed}",
        config.photons,
        describe_line(&config.channel)
    );
    println!();
    println!(
        "1. Photons (first {} of {}). + and x are the two bases.",
        report.records.len().min(TABLE_WIDTH),
        report.records.len()
    );
    for line in photon_table(&report.records, config.channel.eve.is_some()) {
        println!("{line}");
    }
    println!();
    println!("2. Sifting, error estimate, error correction, verification, privacy amplification");
    for line in steps_of_report(&report, config.settings.threshold).lines() {
        println!("{line}");
    }
    if config.channel.eve.is_some() {
        let intercepted = report.records.iter().filter(|r| r.eve.is_some()).count();
        println!();
        println!(
            "   Eve (only the simulation can see this): measured {intercepted} photons and knew"
        );
        println!(
            "   {} of the {} sifted bits for certain before privacy amplification.",
            report.eve_known, report.sifted
        );
    }
    println!();
    if report.verdict != Verdict::Key {
        println!("3. Message: not sent. No part of it left Alice.");
        return Err(Failure::Stopped);
    }
    println!("3. Message");
    println!("  key          {}", key_preview(&report.key));
    match qkx::protocol::one_time_pad(message, &report.key) {
        Some(cipher) => {
            let plain = qkx::protocol::one_time_pad(&cipher, &report.key).unwrap_or_default();
            println!("  ciphertext   {}", hex(&cipher));
            println!("  Bob reads    {}", String::from_utf8_lossy(&plain));
            Ok(())
        }
        None => Err(Failure::Error(format!(
            "the key has {} bits and the message needs {}; use more --photons",
            report.key.len(),
            message.len() * 8
        ))),
    }
}

fn connect_setup(stream: &TcpStream) -> io::Result<()> {
    // Cascade's binary search is many tiny round trips: send each frame at once.
    stream.set_nodelay(true)?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))
}

fn bob(args: &Args) -> Result<(), Failure> {
    let (host, port) = address(args)?;
    let listener = TcpListener::bind((host.as_str(), port))
        .map_err(|e| Failure::Error(format!("cannot listen on {host}:{port}: {e}")))?;
    println!("Bob is listening on {}", listener.local_addr()?);
    io::stdout().flush()?;
    let (mut stream, peer) = listener.accept()?;
    connect_setup(&stream)?;
    println!("Alice connected from {peer}");
    let party = session::bob(&mut stream, &mut OsRandom::new()?)?;
    report_party(&party, "Bob")?;
    match (&party.message, &party.note) {
        (Some(m), _) => {
            println!(
                "  ciphertext   {}",
                hex(party.cipher.as_deref().unwrap_or(&[]))
            );
            println!("  message      {}", String::from_utf8_lossy(m));
            Ok(())
        }
        (None, Some(note)) => Err(Failure::Error(format!("no message: {note}"))),
        (None, None) => Ok(()),
    }
}

fn report_party(party: &Party, who: &str) -> Result<(), Failure> {
    for line in steps_of_party(party).lines() {
        println!("{line}");
    }
    if party.verdict != Verdict::Key {
        println!("{who} stops. No message was sent.");
        return Err(Failure::Stopped);
    }
    println!("  key          {}", key_preview(&party.key));
    Ok(())
}

fn alice(args: &Args) -> Result<(), Failure> {
    let config = exchange_config(args)?;
    if args.get("seed").is_some() {
        return Err(Failure::Usage(
            "--seed is for demo; alice and bob always use the system's random source".into(),
        ));
    }
    let (host, port) = address(args)?;
    let mut stream = TcpStream::connect((host.as_str(), port))
        .map_err(|e| Failure::Error(format!("cannot reach Bob at {host}:{port}: {e}")))?;
    connect_setup(&stream)?;
    println!(
        "Alice connected to Bob at {host}:{port}; {} photons, {}",
        config.photons,
        describe_line(&config.channel)
    );
    let message = args.message().as_bytes();
    let party = session::alice(&mut stream, &config, message, &mut OsRandom::new()?)?;
    if let Some(eve) = party.eve {
        println!(
            "  Eve          (simulated in this program) measured {} photons and knew {} of the {} sifted bits",
            eve.intercepted, eve.known, party.sifted
        );
    }
    report_party(&party, "Alice")?;
    if let Some(note) = &party.note {
        return Err(Failure::Error(format!("no message sent: {note}")));
    }
    println!(
        "  ciphertext   {}",
        hex(party.cipher.as_deref().unwrap_or(&[]))
    );
    println!("  delivered    Bob confirmed receipt");
    Ok(())
}

fn verdict_name(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Key => "key",
        Verdict::TooManyErrors => "stopped: errors",
        Verdict::VerificationFailed => "stopped: hash",
        Verdict::NothingLeft => "no key left",
    }
}

fn rates(args: &Args) -> Result<(), Failure> {
    let photons: usize = args.number("photons", 200_000)?;
    if !(1000..=MAX_PHOTONS).contains(&photons) {
        return Err(Failure::Usage(format!(
            "--photons must be 1000 to {MAX_PHOTONS}"
        )));
    }
    let seed: u64 = args.number("seed", 1)?;
    let settings = Settings::default();
    let keep = 0.5 * (1.0 - settings.sample_fraction);

    println!("Secret key rate against channel error: {photons} photons per row, no eavesdropper,");
    println!(
        "sample {} of the sifted key, the error limit switched off. Seed {seed}.",
        percent(settings.sample_fraction)
    );
    println!();
    println!(
        "{:>6} {:>8} {:>8} {:>9} {:>8} {:>5} {:>8} {:>11} {:>11}  outcome",
        "noise", "QBER", "raw key", "corrected", "leaked", "f", "final", "bits/photon", "ideal"
    );
    for step in 0..=12 {
        let noise = step as f64 / 100.0;
        let channel = Channel { noise, eve: None };
        let mut rng = Xoshiro::new(seed.wrapping_add(step));
        let report = simulate::run(photons, &channel, &settings, false, &mut rng);
        // Shor-Preskill with perfect error correction and no finite-size cost.
        let ideal = (keep * (1.0 - 2.0 * binary_entropy(noise))).max(0.0);
        let f = report
            .efficiency()
            .map_or_else(|| "-".to_string(), |f| format!("{f:.2}"));
        println!(
            "{:>6} {:>8} {:>8} {:>9} {:>8} {:>5} {:>8} {:>11.4} {:>11.4}  {}",
            percent(noise),
            format!("{:.2}%", 100.0 * report.estimate.qber()),
            report.raw_len,
            report.correction.corrected,
            report.correction.leaked,
            f,
            report.final_len,
            report.final_len as f64 / photons as f64,
            ideal,
            verdict_name(report.verdict)
        );
    }
    println!();
    println!("f is the parities revealed over the Shannon limit n h(e). ideal is");
    println!("{keep} (1 - 2 h(e)): sifting and sampling keep {keep} of the photons, and the");
    println!("rest is the asymptotic BB84 rate with perfect error correction.");
    println!();
    println!("Eavesdropper strategies: intercept and resend, no line noise, error limit on.");
    println!();
    println!(
        "{:<9} {:>8} {:>7} {:>8} {:>8} {:>10} {:>7}  outcome",
        "basis", "fraction", "QBER", "+ basis", "x basis", "Eve knew", "final"
    );
    let cases = [
        (Strategy::Random, 1.0),
        (Strategy::Random, 0.25),
        (Strategy::Random, 0.1),
        (Strategy::Rectilinear, 1.0),
        (Strategy::Diagonal, 0.3),
    ];
    for (i, (strategy, fraction)) in cases.into_iter().enumerate() {
        let channel = Channel {
            noise: 0.0,
            eve: Some(Eve { strategy, fraction }),
        };
        let mut rng = Xoshiro::new(seed.wrapping_add(100 + i as u64));
        let report = simulate::run(photons, &channel, &settings, true, &mut rng);
        let e = &report.estimate;
        println!(
            "{:<9} {:>8.2} {:>7} {:>8} {:>8} {:>10} {:>7}  {}",
            strategy.name(),
            fraction,
            percent(e.qber()),
            percent(e.qber_in(Basis::Rectilinear)),
            percent(e.qber_in(Basis::Diagonal)),
            format!(
                "{}%",
                (100 * report.eve_known)
                    .checked_div(report.sifted)
                    .unwrap_or(0)
            ),
            report.final_len,
            verdict_name(report.verdict)
        );
    }
    println!();
    println!("Eve knew: share of the sifted bits she measured in Alice's basis. A fixed");
    println!("basis shows up as errors in the other basis only. When a key survives, privacy");
    println!("amplification has shortened it by at least the h(e) share Eve can know.");
    Ok(())
}

fn run(words: &[String]) -> Result<(), Failure> {
    let Some(command) = words.first() else {
        print!("{HELP}");
        return Err(Failure::Usage(String::new()));
    };
    let rest = &words[1..];
    match command.as_str() {
        "demo" => demo(&Args::parse(rest, &EXCHANGE_OPTIONS[..7])?),
        "alice" => alice(&Args::parse(rest, &EXCHANGE_OPTIONS)?),
        "bob" => {
            let args = Args::parse(rest, &["host", "port"])?;
            if !args.positional.is_empty() {
                return Err(Failure::Usage("bob takes no message".into()));
            }
            bob(&args)
        }
        "rates" => rates(&Args::parse(rest, &["photons", "seed"])?),
        "help" | "-h" | "--help" => {
            print!("{HELP}");
            Ok(())
        }
        "-V" | "--version" => {
            println!("qkx {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => Err(Failure::Usage(format!("unknown command {other:?}"))),
    }
}

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    match run(&words) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(text)) => {
            if !text.is_empty() {
                eprintln!("qkx: {text}");
                eprintln!("Try `qkx help`.");
            }
            ExitCode::from(2)
        }
        Err(Failure::Error(text)) => {
            eprintln!("qkx: {text}");
            ExitCode::from(1)
        }
        Err(Failure::Stopped) => ExitCode::from(3),
    }
}
