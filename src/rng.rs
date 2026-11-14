//! Where random bits come from.
//!
//! The protocol code only sees the [`Random`] trait. The command line tool
//! feeds it from the operating system (`/dev/urandom`); tables, tests and the
//! browser view use [`Xoshiro`] with a seed so that runs can be repeated.
//! [`OsRandom`] is the operating system's generator.

/// A source of random bits.
pub trait Random {
    fn next_u64(&mut self) -> u64;

    /// One fair bit.
    fn bit(&mut self) -> bool {
        self.next_u64() >> 63 == 1
    }

    /// True with probability `p`.
    fn chance(&mut self, p: f64) -> bool {
        // 53 random bits make a uniform number in [0, 1).
        ((self.next_u64() >> 11) as f64) * (1.0 / (1u64 << 53) as f64) < p
    }

    /// A uniform number in `0..n`. `n` must not be zero.
    fn below(&mut self, n: u64) -> u64 {
        // Reject the few values that would make some results more likely.
        let zone = u64::MAX - (u64::MAX % n + 1) % n;
        loop {
            let value = self.next_u64();
            if value <= zone {
                return value % n;
            }
        }
    }

    /// `n` fair bits.
    fn bits(&mut self, n: usize) -> Vec<bool> {
        let mut out = Vec::with_capacity(n);
        while out.len() < n {
            let word = self.next_u64();
            for i in 0..64.min(n - out.len()) {
                out.push((word >> i) & 1 == 1);
            }
        }
        out
    }
}

/// xoshiro256**, seeded through splitmix64. Fast and repeatable; not a
/// cryptographic generator.
#[derive(Debug, Clone)]
pub struct Xoshiro {
    state: [u64; 4],
}

impl Xoshiro {
    pub fn new(seed: u64) -> Self {
        let mut z = seed;
        let mut next = || {
            z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut x = z;
            x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            x ^ (x >> 31)
        };
        Self {
            state: [next(), next(), next(), next()],
        }
    }
}

impl Random for Xoshiro {
    fn next_u64(&mut self) -> u64 {
        let s = &mut self.state;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }
}

/// Random bits from the operating system (`/dev/urandom`), read in blocks.
pub struct OsRandom {
    file: std::fs::File,
    buffer: [u8; 4096],
    used: usize,
}

impl OsRandom {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            file: std::fs::File::open("/dev/urandom")?,
            buffer: [0; 4096],
            used: 4096,
        })
    }
}

impl Random for OsRandom {
    fn next_u64(&mut self) -> u64 {
        use std::io::Read;
        if self.used == self.buffer.len() {
            self.file
                .read_exact(&mut self.buffer)
                .expect("reading /dev/urandom failed");
            self.used = 0;
        }
        let bytes = &self.buffer[self.used..self.used + 8];
        self.used += 8;
        u64::from_le_bytes(bytes.try_into().unwrap())
    }
}

/// A random order of `0..n` (Fisher-Yates).
pub fn permutation(n: usize, rng: &mut dyn Random) -> Vec<u32> {
    let mut order: Vec<u32> = (0..n as u32).collect();
    for i in (1..n).rev() {
        order.swap(i, rng.below(i as u64 + 1) as usize);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_reference_output_and_is_uniform() {
        // First output of xoshiro256** from state {1, 2, 3, 4}, as given by
        // the reference implementation: rotl(2 * 5, 7) * 9.
        let mut rng = Xoshiro {
            state: [1, 2, 3, 4],
        };
        assert_eq!(rng.next_u64(), 11520);

        let mut rng = Xoshiro::new(1);
        let mut seen = [0u32; 6];
        for _ in 0..60_000 {
            seen[rng.below(6) as usize] += 1;
        }
        assert!(
            seen.iter().all(|&c| (9_600..10_400).contains(&c)),
            "{seen:?}"
        );
        let mut sorted = permutation(100, &mut rng);
        sorted.sort_unstable();
        assert!(sorted.iter().enumerate().all(|(i, &v)| i as u32 == v));
    }
}
