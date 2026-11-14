//! The whole exchange in one process, with a view of everything, including
//! what no party in a real exchange could see (Eve's notes next to Alice's
//! bits). Used for the rate tables, the browser view and the tests. The same
//! step functions run between two programs in [`crate::session`].

use crate::cascade::{self, Layout, Local};
use crate::channel::{Channel, Tap};
use crate::photon::{Basis, Photon};
use crate::protocol::{self, Estimate, Settings, VERIFY_BITS};
use crate::rng::Random;
use crate::toeplitz;

/// Everything about one photon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub alice_bit: bool,
    pub alice_basis: Basis,
    pub eve: Tap,
    pub bob_basis: Basis,
    pub bob_bit: bool,
}

impl Record {
    /// Both used the same basis, so the position survives sifting.
    pub fn kept(&self) -> bool {
        self.alice_basis == self.bob_basis
    }

    /// Kept, but Bob's bit differs from Alice's.
    pub fn error(&self) -> bool {
        self.kept() && self.alice_bit != self.bob_bit
    }
}

/// How an exchange ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Both hold the same secret key of `Report::final_len` bits.
    Key,
    /// The estimated error rate was above the threshold.
    TooManyErrors,
    /// Error correction left the keys different and the hash showed it.
    VerificationFailed,
    /// Everything passed, but nothing is left after privacy amplification.
    NothingLeft,
}

/// The numbers of one exchange.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub records: Vec<Record>,
    pub sifted: usize,
    pub estimate: Estimate,
    /// Error counts over all sifted bits per basis as (bits, errors); only the
    /// simulation can know these.
    pub true_errors: [(usize, usize); 2],
    /// Sifted bits Eve measured in the right basis: she knows these for certain.
    pub eve_known: usize,
    /// Length of the key after the sample is thrown away.
    pub raw_len: usize,
    pub correction: cascade::Outcome,
    pub final_len: usize,
    pub verdict: Verdict,
    /// The final key, if there is one. Alice's and Bob's are equal by then.
    pub key: Vec<bool>,
}

impl Report {
    /// Leaked parity bits divided by the Shannon limit n * h(e), with e the
    /// share of bits that were corrected. 1.0 would be perfect.
    pub fn efficiency(&self) -> Option<f64> {
        let e = self.correction.corrected as f64 / self.raw_len.max(1) as f64;
        let limit = self.raw_len as f64 * protocol::binary_entropy(e);
        (limit > 0.0).then(|| self.correction.leaked as f64 / limit)
    }
}

/// Sends `photons` photons through `channel` and has Bob measure them.
pub fn exchange(photons: usize, channel: &Channel, rng: &mut dyn Random) -> Vec<Record> {
    (0..photons)
        .map(|_| {
            let alice_bit = rng.bit();
            let alice_basis = Basis::random(rng);
            let (mut arrived, eve) = channel.carry(Photon::prepare(alice_bit, alice_basis), rng);
            let bob_basis = Basis::random(rng);
            let bob_bit = arrived.measure(bob_basis, rng);
            Record {
                alice_bit,
                alice_basis,
                eve,
                bob_basis,
                bob_bit,
            }
        })
        .collect()
}

/// Runs the full chain. With `enforce_threshold` off, a high error rate does
/// not stop the run, which shows where the key length reaches zero by itself.
pub fn run(
    photons: usize,
    channel: &Channel,
    settings: &Settings,
    enforce_threshold: bool,
    rng: &mut dyn Random,
) -> Report {
    let records = exchange(photons, channel, rng);
    let kept: Vec<&Record> = records.iter().filter(|r| r.kept()).collect();
    let mut true_errors = [(0, 0); 2];
    for record in &kept {
        let slot = &mut true_errors[usize::from(record.alice_basis.as_bit())];
        slot.0 += 1;
        slot.1 += usize::from(record.error());
    }
    let eve_known = kept
        .iter()
        .filter(|r| r.eve.is_some_and(|(basis, _)| basis == r.alice_basis))
        .count();

    let alice: Vec<bool> = kept.iter().map(|r| r.alice_bit).collect();
    let bob: Vec<bool> = kept.iter().map(|r| r.bob_bit).collect();
    let bases: Vec<Basis> = kept.iter().map(|r| r.alice_basis).collect();
    let sample = protocol::pick_sample(kept.len(), settings.sample_fraction, rng);
    let (alice_sample, alice_key) = protocol::split(&alice, &sample);
    let (bob_sample, mut bob_key) = protocol::split(&bob, &sample);
    let sample_bases: Vec<Basis> = sample.iter().map(|&i| bases[i as usize]).collect();
    let estimate = Estimate::new(&alice_sample, &bob_sample, &sample_bases);

    let mut report = Report {
        sifted: kept.len(),
        estimate,
        true_errors,
        eve_known,
        raw_len: alice_key.len(),
        correction: cascade::Outcome::default(),
        final_len: 0,
        verdict: Verdict::TooManyErrors,
        key: Vec::new(),
        records,
    };
    if enforce_threshold && estimate.qber() > settings.threshold {
        return report;
    }

    let layout = Layout::new(alice_key.len(), estimate.qber(), rng.next_u64());
    let mut oracle = Local {
        layout: &layout,
        key: &alice_key,
    };
    let Ok(correction) = cascade::correct(&mut bob_key, &layout, &mut oracle);
    report.correction = correction;

    let seed = rng.bits(toeplitz::seed_len(alice_key.len(), VERIFY_BITS));
    if toeplitz::hash(&alice_key, &seed, VERIFY_BITS)
        != toeplitz::hash(&bob_key, &seed, VERIFY_BITS)
    {
        report.verdict = Verdict::VerificationFailed;
        return report;
    }

    let error_rate = estimate
        .qber()
        .max(correction.corrected as f64 / alice_key.len().max(1) as f64);
    report.final_len = protocol::final_key_len(alice_key.len(), error_rate, correction.leaked);
    if report.final_len == 0 {
        report.verdict = Verdict::NothingLeft;
        return report;
    }
    let seed = rng.bits(toeplitz::seed_len(alice_key.len(), report.final_len));
    report.key = toeplitz::hash(&alice_key, &seed, report.final_len);
    debug_assert_eq!(
        report.key,
        toeplitz::hash(&bob_key, &seed, report.final_len)
    );
    report.verdict = Verdict::Key;
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{Eve, Strategy};
    use crate::rng::Xoshiro;

    #[test]
    fn a_fixed_basis_eve_leaves_errors_only_in_the_other_basis() {
        let channel = Channel {
            noise: 0.0,
            eve: Some(Eve {
                strategy: Strategy::Rectilinear,
                fraction: 1.0,
            }),
        };
        let report = run(
            40_000,
            &channel,
            &Settings::default(),
            true,
            &mut Xoshiro::new(5),
        );
        let [(_, rect_errors), (diag_bits, diag_errors)] = report.true_errors;
        assert_eq!(rect_errors, 0);
        let diag_rate = diag_errors as f64 / diag_bits as f64;
        assert!((0.47..0.53).contains(&diag_rate), "{diag_rate}");
        assert_eq!(report.verdict, Verdict::TooManyErrors);

        // A quiet line: no errors, and only the fixed costs come off the key.
        let report = run(
            40_000,
            &Channel::default(),
            &Settings::default(),
            true,
            &mut Xoshiro::new(5),
        );
        assert_eq!(report.estimate.errors, 0);
        assert_eq!(report.verdict, Verdict::Key);
        assert!(report.raw_len - report.final_len < 128 + 500);
    }
}
