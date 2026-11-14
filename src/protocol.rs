//! The classical steps of BB84 as pure functions: sifting, error estimation
//! and the length of the final key.

use crate::photon::Basis;
use crate::rng::Random;

/// Bits of the verification hash sent after error correction.
pub const VERIFY_BITS: usize = 64;
/// Bits given up as security margin in privacy amplification: 2 * log2(1 / epsilon)
/// for epsilon = 2^-32 in the leftover hash lemma.
pub const MARGIN_BITS: usize = 64;

/// Protocol settings both sides must agree on. Alice announces them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    /// Share of the sifted bits revealed to estimate the error rate.
    pub sample_fraction: f64,
    /// Estimated error rate above which the exchange stops.
    pub threshold: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sample_fraction: 0.25,
            // 1 - 2h(e) reaches zero at e = 11.0%: above it no key can be distilled
            // with one-way post-processing.
            threshold: 0.11,
        }
    }
}

/// h(p) = -p log2 p - (1 - p) log2 (1 - p), the entropy of a biased coin.
pub fn binary_entropy(p: f64) -> f64 {
    if p <= 0.0 || p >= 1.0 {
        return 0.0;
    }
    -p * p.log2() - (1.0 - p) * (1.0 - p).log2()
}

/// Positions where both used the same basis. About half of them.
pub fn sift(alice: &[Basis], bob: &[Basis]) -> Vec<u32> {
    (0..alice.len().min(bob.len()))
        .filter(|&i| alice[i] == bob[i])
        .map(|i| i as u32)
        .collect()
}

/// A random subset of `0..len` of the size the sample fraction asks for, sorted.
pub fn pick_sample(len: usize, fraction: f64, rng: &mut dyn Random) -> Vec<u32> {
    let size = ((len as f64 * fraction).round() as usize).min(len);
    let mut pool: Vec<u32> = (0..len as u32).collect();
    for i in 0..size {
        pool.swap(i, i + rng.below((len - i) as u64) as usize);
    }
    pool.truncate(size);
    pool.sort_unstable();
    pool
}

/// The bits at `positions` and the bits everywhere else, in order.
/// `positions` must be sorted.
pub fn split(bits: &[bool], positions: &[u32]) -> (Vec<bool>, Vec<bool>) {
    let mut taken = Vec::with_capacity(positions.len());
    let mut rest = Vec::with_capacity(bits.len().saturating_sub(positions.len()));
    let mut next = positions.iter().peekable();
    for (i, &bit) in bits.iter().enumerate() {
        if next.peek().is_some_and(|&&p| p as usize == i) {
            taken.push(bit);
            next.next();
        } else {
            rest.push(bit);
        }
    }
    (taken, rest)
}

/// Errors counted in the revealed sample, overall and per basis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Estimate {
    pub size: usize,
    pub errors: usize,
    /// (sample bits, errors) for the rectilinear and the diagonal basis.
    pub by_basis: [(usize, usize); 2],
}

impl Estimate {
    /// Compares the sample bits of both sides. `bases` are the (shared) bases
    /// of the sampled positions.
    pub fn new(alice: &[bool], bob: &[bool], bases: &[Basis]) -> Self {
        let mut estimate = Estimate::default();
        for ((a, b), basis) in alice.iter().zip(bob).zip(bases) {
            let wrong = usize::from(a != b);
            let slot = &mut estimate.by_basis[usize::from(basis.as_bit())];
            slot.0 += 1;
            slot.1 += wrong;
            estimate.size += 1;
            estimate.errors += wrong;
        }
        estimate
    }

    /// The quantum bit error rate estimated from the sample.
    pub fn qber(&self) -> f64 {
        rate(self.errors, self.size)
    }

    /// The error rate among sample bits sent in one basis.
    pub fn qber_in(&self, basis: Basis) -> f64 {
        let (size, errors) = self.by_basis[usize::from(basis.as_bit())];
        rate(errors, size)
    }
}

fn rate(errors: usize, size: usize) -> f64 {
    if size == 0 {
        0.0
    } else {
        errors as f64 / size as f64
    }
}

/// Length of the secret key that privacy amplification may output.
///
/// `n` is the length of the corrected key and `error_rate` the error rate
/// seen on the line. An eavesdropper who causes that error rate can know at
/// most n * h(e) bits about the key (Shor and Preskill); on top she saw every
/// parity of error correction and the verification hash. What is left, less a
/// fixed margin, is the key:
///
/// m = n * (1 - h(e)) - leak_ec - 64 - 64, or 0 if that is negative.
///
/// This is the asymptotic formula. It uses the measured error rate as if it
/// were exact, with no allowance for the statistical error of a finite sample.
pub fn final_key_len(n: usize, error_rate: f64, leak_ec: usize) -> usize {
    let entropy = if error_rate >= 0.5 {
        1.0
    } else {
        binary_entropy(error_rate)
    };
    let secret = (n as f64 * (1.0 - entropy)).floor() as i64;
    let spent = (leak_ec + VERIFY_BITS + MARGIN_BITS) as i64;
    (secret - spent).max(0) as usize
}

/// Packs a 64-bit hash output into a number.
pub fn tag_value(bits: &[bool]) -> u64 {
    bits.iter()
        .enumerate()
        .fold(0, |acc, (i, &bit)| acc | (u64::from(bit) << i))
}

/// XORs `data` with the first bits of `key`: the one-time pad.
/// Returns `None` if the key is shorter than the data.
pub fn one_time_pad(data: &[u8], key: &[bool]) -> Option<Vec<u8>> {
    if key.len() < data.len() * 8 {
        return None;
    }
    Some(
        data.iter()
            .enumerate()
            .map(|(i, &byte)| {
                let pad = (0..8).fold(0u8, |acc, k| acc | (u8::from(key[i * 8 + k]) << k));
                byte ^ pad
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_length_follows_the_formula() {
        assert!((binary_entropy(0.11) - 0.4999).abs() < 1e-3);
        // No errors: only the fixed costs come off.
        assert_eq!(final_key_len(1000, 0.0, 0), 1000 - 128);
        // 5% errors, 400 bits leaked: 1000 * (1 - 0.2864) - 400 - 128 = 185.
        assert_eq!(final_key_len(1000, 0.05, 400), 185);
        // Never negative, and nothing at all at 50% errors.
        assert_eq!(final_key_len(1000, 0.11, 600), 0);
        assert_eq!(final_key_len(10_000, 0.5, 0), 0);
    }

    #[test]
    fn sample_is_split_off_and_the_pad_round_trips() {
        let bits = [true, false, true, true, false];
        let (taken, rest) = split(&bits, &[1, 3]);
        assert_eq!((taken, rest), (vec![false, true], vec![true, true, false]));

        let key: Vec<bool> = (0..40).map(|i| i % 3 == 0).collect();
        let cipher = one_time_pad(b"hello", &key).unwrap();
        assert_ne!(cipher, b"hello");
        assert_eq!(one_time_pad(&cipher, &key).unwrap(), b"hello");
        assert!(one_time_pad(b"hello!", &key).is_none());
    }
}
