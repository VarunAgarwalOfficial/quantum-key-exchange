//! Alice and Bob as two programs talking over a stream (a TCP socket in
//! `qkx alice` and `qkx bob`). The conversation, in order:
//!
//! ```text
//! Alice -> Bob   Hello      sample fraction and error limit
//! Alice -> Bob   Photons    the simulated quantum channel
//! Bob -> Alice   Bases      the bases Bob measured in, after measuring
//! Alice -> Bob   Bases      the bases Alice prepared in
//!                           both keep the positions where the bases match
//! Alice -> Bob   Sample     random positions of the sifted key and her bits there
//! Bob -> Alice   Bits       his bits at those positions
//!                           both compute the error rate; above the limit both stop
//! Alice -> Bob   Correct    seed for the Cascade shuffles
//! Bob -> Alice   Queries    parity questions ...
//! Alice -> Bob   Parities   ... and answers, repeated
//! Bob -> Alice   Corrected  bits he flipped, parities he was told
//! Alice -> Bob   Verify     Toeplitz seed and the 64-bit hash of her key
//! Bob -> Alice   Verdict    whether his key hashes the same
//! Alice -> Bob   Amplify    Toeplitz seed and final key length
//! Alice -> Bob   Cipher     the message under the one-time pad
//! Bob -> Alice   Verdict    received
//! ```
//!
//! Everything after `Photons` is public. Each side computes every number it
//! can on its own and checks what the other side announces against it.

use std::io::{Read, Write};

use crate::cascade::{self, Layout, Oracle, Query};
use crate::channel::Channel;
use crate::photon::{Basis, Photon};
use crate::protocol::{self, Estimate, Settings, VERIFY_BITS};
use crate::rng::Random;
use crate::simulate::Verdict;
use crate::toeplitz;
use crate::wire::{protocol_error, receive, send, Error, Message};

/// Most photons one exchange may carry.
pub const MAX_PHOTONS: usize = 4_000_000;

/// What Alice chooses before the exchange.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AliceConfig {
    pub photons: usize,
    pub settings: Settings,
    /// Noise and eavesdropper, simulated on the photons' way out of Alice's program.
    pub channel: Channel,
}

/// What Eve learned, known only because the simulation runs her inside Alice's program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EveNotes {
    pub intercepted: usize,
    /// Sifted positions where she measured in Alice's basis: she knows those bits.
    pub known: usize,
}

/// What one side saw of the exchange.
#[derive(Debug, Clone, PartialEq)]
pub struct Party {
    pub photons: usize,
    pub sifted: usize,
    /// The error limit Alice announced.
    pub limit: f64,
    pub estimate: Estimate,
    /// Key length after the sample is removed.
    pub raw_len: usize,
    pub correction: cascade::Outcome,
    pub final_len: usize,
    pub verdict: Verdict,
    pub key: Vec<bool>,
    /// The ciphertext that crossed the public channel.
    pub cipher: Option<Vec<u8>>,
    /// Bob: the decrypted message.
    pub message: Option<Vec<u8>>,
    /// Why no message was sent, when the key was too short for it.
    pub note: Option<String>,
    pub eve: Option<EveNotes>,
}

impl Party {
    fn new(photons: usize, sifted: usize, limit: f64) -> Self {
        Self {
            photons,
            sifted,
            limit,
            estimate: Estimate::default(),
            raw_len: 0,
            correction: cascade::Outcome::default(),
            final_len: 0,
            verdict: Verdict::TooManyErrors,
            key: Vec::new(),
            cipher: None,
            message: None,
            note: None,
            eve: None,
        }
    }
}

fn unexpected(got: Message, wanted: &str) -> Error {
    match got {
        Message::Abort(reason) => protocol_error(format!("the other side stopped: {reason}")),
        other => protocol_error(format!("expected {wanted}, got {other:?}")),
    }
}

fn bases_from_bits(bits: &[bool]) -> Vec<Basis> {
    bits.iter().map(|&b| Basis::from_bit(b)).collect()
}

fn bits_from_bases(bases: &[Basis]) -> Vec<bool> {
    bases.iter().map(|b| b.as_bit()).collect()
}

/// The error rate used for the key length: the larger of the estimate and
/// the share of bits error correction actually flipped.
fn line_error(estimate: &Estimate, corrected: usize, len: usize) -> f64 {
    estimate.qber().max(corrected as f64 / len.max(1) as f64)
}

/// Runs Alice's side and sends `message` under the key.
pub fn alice<S: Read + Write>(
    stream: &mut S,
    config: &AliceConfig,
    message: &[u8],
    rng: &mut dyn Random,
) -> Result<Party, Error> {
    let n = config.photons;
    if n == 0 || n > MAX_PHOTONS {
        return Err(protocol_error(format!(
            "photons must be 1 to {MAX_PHOTONS}"
        )));
    }
    send(stream, &Message::Hello(config.settings))?;

    let mut bits = Vec::with_capacity(n);
    let mut bases = Vec::with_capacity(n);
    let mut taps = Vec::with_capacity(n);
    let mut photons = Vec::with_capacity(n);
    for _ in 0..n {
        let (bit, basis) = (rng.bit(), Basis::random(rng));
        let (arrived, tap) = config.channel.carry(Photon::prepare(bit, basis), rng);
        bits.push(bit);
        bases.push(basis);
        taps.push(tap);
        photons.push(arrived.to_wire());
    }
    send(stream, &Message::Photons(photons))?;

    let bob_bases = match receive(stream)? {
        Message::Bases(b) if b.len() == n => bases_from_bits(&b),
        other => return Err(unexpected(other, "Bob's bases")),
    };
    send(stream, &Message::Bases(bits_from_bases(&bases)))?;
    let kept = protocol::sift(&bases, &bob_bases);
    let mut party = Party::new(n, kept.len(), config.settings.threshold);
    if config.channel.eve.is_some() {
        party.eve = Some(EveNotes {
            intercepted: taps.iter().filter(|t| t.is_some()).count(),
            known: kept
                .iter()
                .filter(|&&i| taps[i as usize].is_some_and(|(b, _)| b == bases[i as usize]))
                .count(),
        });
    }

    let sifted: Vec<bool> = kept.iter().map(|&i| bits[i as usize]).collect();
    let sample = protocol::pick_sample(kept.len(), config.settings.sample_fraction, rng);
    let (mine, key) = protocol::split(&sifted, &sample);
    send(
        stream,
        &Message::Sample {
            positions: sample.clone(),
            bits: mine.clone(),
        },
    )?;
    let theirs = match receive(stream)? {
        Message::Bits(b) if b.len() == sample.len() => b,
        other => return Err(unexpected(other, "Bob's sample bits")),
    };
    let sample_bases: Vec<Basis> = sample
        .iter()
        .map(|&p| bases[kept[p as usize] as usize])
        .collect();
    party.estimate = Estimate::new(&mine, &theirs, &sample_bases);
    party.raw_len = key.len();
    if party.estimate.qber() > config.settings.threshold {
        return Ok(party);
    }

    let seed = rng.next_u64();
    send(stream, &Message::Correct { seed })?;
    let layout = Layout::new(key.len(), party.estimate.qber(), seed);
    let mut leaked = 0usize;
    let corrected = loop {
        match receive(stream)? {
            Message::Queries(queries) => {
                if !queries.iter().all(|q| layout.accepts(q)) {
                    return Err(protocol_error("parity question outside the key"));
                }
                leaked += queries.len();
                let answers = queries.iter().map(|q| layout.parity(&key, q)).collect();
                send(stream, &Message::Parities(answers))?;
            }
            Message::Corrected {
                corrected,
                leaked: told,
            } => {
                if told as usize != leaked {
                    return Err(protocol_error(format!(
                        "Bob counts {told} parities, Alice sent {leaked}"
                    )));
                }
                break corrected as usize;
            }
            other => return Err(unexpected(other, "a parity question")),
        }
    };
    party.correction = cascade::Outcome { corrected, leaked };

    let seed = rng.bits(toeplitz::seed_len(key.len(), VERIFY_BITS));
    let tag = protocol::tag_value(&toeplitz::hash(&key, &seed, VERIFY_BITS));
    send(stream, &Message::Verify { seed, tag })?;
    match receive(stream)? {
        Message::Verdict(true) => {}
        Message::Verdict(false) => {
            party.verdict = Verdict::VerificationFailed;
            return Ok(party);
        }
        other => return Err(unexpected(other, "Bob's verdict")),
    }

    let error = line_error(&party.estimate, corrected, key.len());
    party.final_len = protocol::final_key_len(key.len(), error, leaked);
    let seed = rng.bits(toeplitz::seed_len(key.len(), party.final_len));
    party.key = toeplitz::hash(&key, &seed, party.final_len);
    send(
        stream,
        &Message::Amplify {
            seed,
            len: party.final_len as u32,
        },
    )?;
    if party.final_len == 0 {
        party.verdict = Verdict::NothingLeft;
        return Ok(party);
    }
    party.verdict = Verdict::Key;

    let Some(cipher) = protocol::one_time_pad(message, &party.key) else {
        let note = format!(
            "the key has {} bits and the message needs {}; send more photons",
            party.key.len(),
            message.len() * 8
        );
        send(stream, &Message::Abort(note.clone()))?;
        party.note = Some(note);
        return Ok(party);
    };
    send(stream, &Message::Cipher(cipher.clone()))?;
    party.cipher = Some(cipher);
    match receive(stream)? {
        Message::Verdict(true) => Ok(party),
        other => Err(unexpected(other, "Bob's receipt")),
    }
}

/// Bob's end of the parity dialogue: every question goes over the stream.
struct Remote<'a, S> {
    stream: &'a mut S,
}

impl<S: Read + Write> Oracle for Remote<'_, S> {
    type Error = Error;
    fn parities(&mut self, queries: &[Query]) -> Result<Vec<bool>, Error> {
        send(self.stream, &Message::Queries(queries.to_vec()))?;
        match receive(self.stream)? {
            Message::Parities(p) if p.len() == queries.len() => Ok(p),
            other => Err(unexpected(other, "parities")),
        }
    }
}

/// Runs Bob's side and returns what he received.
pub fn bob<S: Read + Write>(stream: &mut S, rng: &mut dyn Random) -> Result<Party, Error> {
    let settings = match receive(stream)? {
        Message::Hello(s)
            if (0.0..1.0).contains(&s.sample_fraction) && (0.0..=0.5).contains(&s.threshold) =>
        {
            s
        }
        other => return Err(unexpected(other, "valid settings")),
    };
    let photons = match receive(stream)? {
        Message::Photons(p) if !p.is_empty() && p.len() <= MAX_PHOTONS => p,
        other => return Err(unexpected(other, "photons")),
    };
    let n = photons.len();
    let mut bases = Vec::with_capacity(n);
    let mut bits = Vec::with_capacity(n);
    for byte in photons {
        let mut photon = Photon::from_wire(byte).ok_or_else(|| protocol_error("not a photon"))?;
        let basis = Basis::random(rng);
        bits.push(photon.measure(basis, rng));
        bases.push(basis);
    }
    send(stream, &Message::Bases(bits_from_bases(&bases)))?;
    let alice_bases = match receive(stream)? {
        Message::Bases(b) if b.len() == n => bases_from_bits(&b),
        other => return Err(unexpected(other, "Alice's bases")),
    };
    let kept = protocol::sift(&alice_bases, &bases);
    let mut party = Party::new(n, kept.len(), settings.threshold);

    let (positions, theirs) = match receive(stream)? {
        Message::Sample { positions, bits }
            if bits.len() == positions.len()
                && positions.windows(2).all(|w| w[0] < w[1])
                && positions.last().is_none_or(|&p| (p as usize) < kept.len()) =>
        {
            (positions, bits)
        }
        other => return Err(unexpected(other, "a sample of the sifted key")),
    };
    let sifted: Vec<bool> = kept.iter().map(|&i| bits[i as usize]).collect();
    let (mine, mut key) = protocol::split(&sifted, &positions);
    send(stream, &Message::Bits(mine.clone()))?;
    let sample_bases: Vec<Basis> = positions
        .iter()
        .map(|&p| bases[kept[p as usize] as usize])
        .collect();
    party.estimate = Estimate::new(&theirs, &mine, &sample_bases);
    party.raw_len = key.len();
    if party.estimate.qber() > settings.threshold {
        return Ok(party);
    }

    let seed = match receive(stream)? {
        Message::Correct { seed } => seed,
        other => return Err(unexpected(other, "the start of error correction")),
    };
    let layout = Layout::new(key.len(), party.estimate.qber(), seed);
    let correction = cascade::correct(&mut key, &layout, &mut Remote { stream })?;
    party.correction = correction;
    send(
        stream,
        &Message::Corrected {
            corrected: correction.corrected as u32,
            leaked: correction.leaked as u32,
        },
    )?;

    let ok = match receive(stream)? {
        Message::Verify { seed, tag }
            if seed.len() == toeplitz::seed_len(key.len(), VERIFY_BITS) =>
        {
            protocol::tag_value(&toeplitz::hash(&key, &seed, VERIFY_BITS)) == tag
        }
        other => return Err(unexpected(other, "the verification hash")),
    };
    send(stream, &Message::Verdict(ok))?;
    if !ok {
        party.verdict = Verdict::VerificationFailed;
        return Ok(party);
    }

    let error = line_error(&party.estimate, correction.corrected, key.len());
    let expected = protocol::final_key_len(key.len(), error, correction.leaked);
    let seed = match receive(stream)? {
        Message::Amplify { seed, len } if len as usize == expected => seed,
        Message::Amplify { len, .. } => {
            return Err(protocol_error(format!(
                "Alice wants a {len}-bit key, Bob computes {expected} bits"
            )))
        }
        other => return Err(unexpected(other, "privacy amplification")),
    };
    party.final_len = expected;
    if expected == 0 {
        party.verdict = Verdict::NothingLeft;
        return Ok(party);
    }
    if seed.len() != toeplitz::seed_len(key.len(), expected) {
        return Err(protocol_error("hash seed has the wrong length"));
    }
    party.key = toeplitz::hash(&key, &seed, expected);
    party.verdict = Verdict::Key;

    match receive(stream)? {
        Message::Cipher(cipher) => {
            let plain = protocol::one_time_pad(&cipher, &party.key)
                .ok_or_else(|| protocol_error("message is longer than the key"))?;
            party.message = Some(plain);
            party.cipher = Some(cipher);
            send(stream, &Message::Verdict(true))?;
        }
        Message::Abort(reason) => party.note = Some(reason),
        other => return Err(unexpected(other, "the message")),
    }
    Ok(party)
}
