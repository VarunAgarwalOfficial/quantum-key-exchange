//! A plain C-ABI interface for the browser view: no wasm-bindgen, no imports.
//!
//! The page calls [`qkx_run`] with the line settings, reads the JSON it
//! returns and gives the buffer back with [`qkx_free`]. The numbers on the
//! page come from the same library code the command line tool runs.

use std::fmt::Write as _;

use qkx::channel::{Channel, Eve, Strategy};
use qkx::photon::Basis;
use qkx::protocol::Settings;
use qkx::rng::Xoshiro;
use qkx::simulate::{self, Report, Verdict};

/// Releases a buffer returned by [`qkx_run`]: `len` is 4 plus the JSON length.
///
/// # Safety
/// `pointer` and `len` must describe exactly one such buffer, not yet freed.
#[no_mangle]
pub unsafe extern "C" fn qkx_free(pointer: *mut u8, len: usize) {
    drop(Vec::from_raw_parts(pointer, 0, len));
}

/// Runs one exchange of `photons` photons and describes it as JSON.
///
/// `strategy`: 0 no eavesdropper, 1 random basis, 2 always +, 3 always x.
/// `enforce`: 0 lets a high error rate run on to show where the key reaches
/// zero by itself. `shown`: how many photons to list one by one.
///
/// Returns a buffer that starts with the JSON length as a little-endian u32.
#[no_mangle]
pub extern "C" fn qkx_run(
    photons: u32,
    noise: f64,
    strategy: u32,
    fraction: f64,
    seed: u32,
    enforce: u32,
    shown: u32,
) -> *mut u8 {
    let json = describe(
        photons,
        noise,
        strategy,
        fraction,
        seed,
        enforce != 0,
        shown,
    );
    let mut out = Vec::with_capacity(4 + json.len());
    out.extend_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(json.as_bytes());
    let mut out = std::mem::ManuallyDrop::new(out.into_boxed_slice());
    out.as_mut_ptr()
}

fn channel(noise: f64, strategy: u32, fraction: f64) -> Channel {
    let strategy = match strategy {
        1 => Some(Strategy::Random),
        2 => Some(Strategy::Rectilinear),
        3 => Some(Strategy::Diagonal),
        _ => None,
    };
    Channel {
        noise: noise.clamp(0.0, 0.5),
        eve: strategy.map(|strategy| Eve {
            strategy,
            fraction: fraction.clamp(0.0, 1.0),
        }),
    }
}

fn verdict_name(verdict: Verdict) -> &'static str {
    match verdict {
        Verdict::Key => "key",
        Verdict::TooManyErrors => "errors",
        Verdict::VerificationFailed => "hash",
        Verdict::NothingLeft => "empty",
    }
}

/// One photon as bit flags: 1 Alice's bit, 2 Alice's basis is x, 4 Eve
/// measured it, 8 Eve's basis is x, 16 Eve read 1, 32 Bob's basis is x,
/// 64 Bob read 1.
fn flags(r: &simulate::Record) -> u8 {
    let mut f = u8::from(r.alice_bit) | (u8::from(r.alice_basis.as_bit()) << 1);
    if let Some((basis, bit)) = r.eve {
        f |= 4 | (u8::from(basis.as_bit()) << 3) | (u8::from(bit) << 4);
    }
    f | (u8::from(r.bob_basis.as_bit()) << 5) | (u8::from(r.bob_bit) << 6)
}

fn describe(
    photons: u32,
    noise: f64,
    strategy: u32,
    fraction: f64,
    seed: u32,
    enforce: bool,
    shown: u32,
) -> String {
    let photons = (photons as usize).clamp(1, 1_000_000);
    let channel = channel(noise, strategy, fraction);
    let mut rng = Xoshiro::new(u64::from(seed));
    let report: Report = simulate::run(photons, &channel, &Settings::default(), enforce, &mut rng);
    let e = &report.estimate;

    let mut json = String::from("{\"records\":[");
    for (i, r) in report.records.iter().take(shown as usize).enumerate() {
        let comma = if i == 0 { "" } else { "," };
        let _ = write!(json, "{comma}{}", flags(r));
    }
    let key: String = report
        .key
        .iter()
        .take(256)
        .map(|&b| if b { '1' } else { '0' })
        .collect();
    let _ = write!(
        json,
        "],\"photons\":{photons},\"intercepted\":{},\"sifted\":{},\"sample\":{},\"errors\":{},\
         \"qber\":{:.6},\"qberRect\":{:.6},\"qberDiag\":{:.6},\"eveKnown\":{},\"rawLen\":{},\
         \"corrected\":{},\"leaked\":{},\"efficiency\":{:.4},\"finalLen\":{},\"verdict\":\"{}\",\
         \"key\":\"{key}\"}}",
        report.records.iter().filter(|r| r.eve.is_some()).count(),
        report.sifted,
        e.size,
        e.errors,
        e.qber(),
        e.qber_in(Basis::Rectilinear),
        e.qber_in(Basis::Diagonal),
        report.eve_known,
        report.raw_len,
        report.correction.corrected,
        report.correction.leaked,
        report.efficiency().unwrap_or(0.0),
        report.final_len,
        verdict_name(report.verdict),
    );
    json
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_an_exchange_with_and_without_eve() {
        let quiet = describe(4000, 0.0, 0, 1.0, 1, true, 3);
        assert!(quiet.starts_with("{\"records\":["));
        assert!(quiet.contains("\"intercepted\":0,"));
        assert!(quiet.contains("\"verdict\":\"key\""));
        let tapped = describe(4000, 0.0, 1, 1.0, 1, true, 3);
        assert!(tapped.contains("\"intercepted\":4000,"));
        assert!(tapped.contains("\"verdict\":\"errors\""));
    }
}
