//! Toeplitz hashing: a universal family of linear maps over GF(2).
//!
//! A Toeplitz matrix is constant along its diagonals, so an m-by-n matrix is
//! described by n + m - 1 seed bits. Output bit i is the parity of the key
//! bits selected by row i. For two different keys the outputs collide with
//! probability 2^-m over the choice of seed, which is what the leftover hash
//! lemma needs for privacy amplification.

/// Packs bits into 64-bit words, lowest bit first, with one spare zero word.
fn pack(bits: impl Iterator<Item = bool>) -> Vec<u64> {
    let mut words = vec![0u64];
    for (i, bit) in bits.enumerate() {
        if i / 64 + 1 >= words.len() {
            words.push(0);
        }
        words[i / 64] |= u64::from(bit) << (i % 64);
    }
    words.push(0);
    words
}

/// Seed length for hashing `key_len` bits down to `out_len` bits.
pub fn seed_len(key_len: usize, out_len: usize) -> usize {
    if out_len == 0 {
        0
    } else {
        key_len + out_len - 1
    }
}

/// Hashes `key` to `out_len` bits with the Toeplitz matrix given by `seed`.
///
/// Row i of the matrix is `seed[i..i + n]` reversed, so that entry (i, j) is
/// `seed[i + n - 1 - j]`: constant along diagonals. `seed` must hold at least
/// [`seed_len`] bits.
pub fn hash(key: &[bool], seed: &[bool], out_len: usize) -> Vec<bool> {
    let n = key.len();
    assert!(seed.len() >= seed_len(n, out_len), "seed too short");
    let reversed = pack(key.iter().rev().copied());
    let seed = pack(seed.iter().copied());
    let words = n.div_ceil(64);
    (0..out_len)
        .map(|i| {
            let (word, shift) = (i / 64, i % 64);
            let mut acc = 0u64;
            for (w, &k) in reversed.iter().take(words).enumerate() {
                let low = seed[word + w] >> shift;
                let high = if shift == 0 {
                    0
                } else {
                    seed.get(word + w + 1).copied().unwrap_or(0) << (64 - shift)
                };
                acc ^= k & (low | high);
            }
            acc.count_ones() % 2 == 1
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::{Random, Xoshiro};

    /// The definition, bit by bit.
    fn slow(key: &[bool], seed: &[bool], out_len: usize) -> Vec<bool> {
        let n = key.len();
        (0..out_len)
            .map(|i| (0..n).fold(false, |acc, j| acc ^ (key[j] & seed[i + n - 1 - j])))
            .collect()
    }

    #[test]
    fn matches_the_definition_and_is_linear() {
        let mut rng = Xoshiro::new(3);
        for (n, m) in [(1, 1), (63, 64), (64, 10), (200, 130), (131, 131)] {
            let a = rng.bits(n);
            let b = rng.bits(n);
            let seed = rng.bits(seed_len(n, m));
            assert_eq!(hash(&a, &seed, m), slow(&a, &seed, m), "{n} to {m}");
            // H(a xor b) = H(a) xor H(b).
            let sum: Vec<bool> = a.iter().zip(&b).map(|(x, y)| x ^ y).collect();
            let expected: Vec<bool> = hash(&a, &seed, m)
                .iter()
                .zip(hash(&b, &seed, m))
                .map(|(x, y)| x ^ y)
                .collect();
            assert_eq!(hash(&sum, &seed, m), expected);
        }
        assert!(hash(&[true, false], &[], 0).is_empty());
    }
}
