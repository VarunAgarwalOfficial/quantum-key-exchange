//! Error correction with Cascade (Brassard and Salvail, 1993).
//!
//! Bob holds a key with a few wrong bits and wants Alice's. In each pass the
//! key is cut into blocks and Alice tells Bob the parity of every block. A
//! block whose parity differs holds an odd number of errors; a binary search,
//! one parity per halving, finds one of them. From the second pass on the bits
//! are shuffled first, so that errors which hid in pairs get separated, and
//! the block size doubles. The name comes from what happens after a
//! correction in a later pass: the same bit sits in a block of every earlier
//! pass, that block's parity now differs, and it can be searched again.
//!
//! Every parity Alice reveals is one bit an eavesdropper learns about the
//! key. [`Outcome::leaked`] counts them; privacy amplification removes that
//! many bits later.
//!
//! This is the original four-pass protocol with the first block size
//! 0.73 / e. It lacks the refinements of later variants (more passes, block
//! parity reuse, tuned block sizes), so it leaks roughly 1.1 to 1.3 times the
//! Shannon limit n * h(e), and a few errors can survive; the verification
//! hash that follows catches those.

use crate::rng::{permutation, Xoshiro};

/// Number of passes.
pub const PASSES: usize = 4;

/// A request for the parity of positions `start..end` in the order of `pass`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Query {
    pub pass: u8,
    pub start: u32,
    pub end: u32,
}

/// The block sizes and shuffles of all passes. Both sides build the same one
/// from the key length, the estimated error rate and a public seed.
#[derive(Debug, Clone)]
pub struct Layout {
    len: usize,
    /// Block size per pass.
    block: [usize; PASSES],
    /// `order[pass][position]` is the key index at that position.
    order: Vec<Vec<u32>>,
    /// `place[pass][index]` is the position of that key index.
    place: Vec<Vec<u32>>,
}

impl Layout {
    pub fn new(len: usize, error_rate: f64, seed: u64) -> Self {
        // With no errors seen in the sample, assume a small rate rather than
        // one giant block.
        let first = (0.73 / error_rate.max(0.002)).ceil() as usize;
        let mut block = [0usize; PASSES];
        let mut order = Vec::with_capacity(PASSES);
        let mut place = Vec::with_capacity(PASSES);
        for (pass, size) in block.iter_mut().enumerate() {
            *size = (first << pass).clamp(1, len.max(1));
            let this: Vec<u32> = if pass == 0 {
                (0..len as u32).collect()
            } else {
                permutation(len, &mut Xoshiro::new(seed.wrapping_add(pass as u64)))
            };
            let mut inverse = vec![0u32; len];
            for (position, &index) in this.iter().enumerate() {
                inverse[index as usize] = position as u32;
            }
            order.push(this);
            place.push(inverse);
        }
        Self {
            len,
            block,
            order,
            place,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// True if `query` names a non-empty range of an existing pass.
    pub fn accepts(&self, query: &Query) -> bool {
        usize::from(query.pass) < PASSES
            && query.start < query.end
            && query.end as usize <= self.len
    }

    /// Parity of `key` over the range of `query`. This is all Alice does.
    pub fn parity(&self, key: &[bool], query: &Query) -> bool {
        self.order[usize::from(query.pass)][query.start as usize..query.end as usize]
            .iter()
            .fold(false, |acc, &index| acc ^ key[index as usize])
    }

    fn blocks(&self, pass: usize) -> usize {
        self.len.div_ceil(self.block[pass])
    }

    fn block_query(&self, pass: usize, block: usize) -> Query {
        Query {
            pass: pass as u8,
            start: (block * self.block[pass]) as u32,
            end: ((block + 1) * self.block[pass]).min(self.len) as u32,
        }
    }
}

/// The side that knows the right key and answers parity questions.
pub trait Oracle {
    type Error;
    fn parities(&mut self, queries: &[Query]) -> Result<Vec<bool>, Self::Error>;
}

/// Alice in the same process: answers from her key.
pub struct Local<'a> {
    pub layout: &'a Layout,
    pub key: &'a [bool],
}

impl Oracle for Local<'_> {
    type Error = std::convert::Infallible;
    fn parities(&mut self, queries: &[Query]) -> Result<Vec<bool>, Self::Error> {
        Ok(queries
            .iter()
            .map(|q| self.layout.parity(self.key, q))
            .collect())
    }
}

/// What a run of Cascade did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outcome {
    /// Bits of the key that were flipped.
    pub corrected: usize,
    /// Parity bits Alice revealed.
    pub leaked: usize,
}

/// Corrects `key` in place towards the key the oracle holds.
pub fn correct<O: Oracle>(
    key: &mut [bool],
    layout: &Layout,
    oracle: &mut O,
) -> Result<Outcome, O::Error> {
    let mut outcome = Outcome::default();
    if key.is_empty() {
        return Ok(outcome);
    }
    // odd[pass][block]: Bob's parity of that block differs from Alice's.
    let mut odd: Vec<Vec<bool>> = Vec::with_capacity(PASSES);
    for pass in 0..PASSES {
        let queries: Vec<Query> = (0..layout.blocks(pass))
            .map(|block| layout.block_query(pass, block))
            .collect();
        let answers = oracle.parities(&queries)?;
        outcome.leaked += queries.len();
        let mut pending: Vec<(usize, usize)> = Vec::new();
        let flags: Vec<bool> = queries
            .iter()
            .zip(&answers)
            .map(|(query, &theirs)| layout.parity(key, query) != theirs)
            .collect();
        pending.extend(
            flags
                .iter()
                .enumerate()
                .filter(|(_, &flag)| flag)
                .map(|(block, _)| (pass, block)),
        );
        odd.push(flags);

        while let Some((p, block)) = pending.pop() {
            if !odd[p][block] {
                continue;
            }
            // Binary search for one wrong bit inside the block.
            let Query {
                mut start, mut end, ..
            } = layout.block_query(p, block);
            while end - start > 1 {
                let half = Query {
                    pass: p as u8,
                    start,
                    end: start + (end - start) / 2,
                };
                let theirs = oracle.parities(&[half])?[0];
                outcome.leaked += 1;
                if layout.parity(key, &half) != theirs {
                    end = half.end;
                } else {
                    start = half.end;
                }
            }
            let index = layout.order[p][start as usize] as usize;
            key[index] = !key[index];
            outcome.corrected += 1;
            // The flipped bit changes the parity of its block in every pass so far.
            for (q, flags) in odd.iter_mut().enumerate() {
                let other = layout.place[q][index] as usize / layout.block[q];
                flags[other] = !flags[other];
                if flags[other] {
                    pending.push((q, other));
                }
            }
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::binary_entropy;
    use crate::rng::Random;

    #[test]
    fn corrects_five_percent_errors_and_counts_the_leak() {
        let mut rng = Xoshiro::new(11);
        let n = 20_000;
        let alice = rng.bits(n);
        let mut bob = alice.clone();
        let mut flipped = 0;
        for bit in bob.iter_mut() {
            if rng.chance(0.05) {
                *bit = !*bit;
                flipped += 1;
            }
        }
        let layout = Layout::new(n, 0.05, 99);
        let mut oracle = Local {
            layout: &layout,
            key: &alice,
        };
        let outcome = correct(&mut bob, &layout, &mut oracle).unwrap();
        assert_eq!(bob, alice);
        assert_eq!(outcome.corrected, flipped);
        // Between the Shannon limit and 1.4 times it.
        let shannon = n as f64 * binary_entropy(flipped as f64 / n as f64);
        let efficiency = outcome.leaked as f64 / shannon;
        assert!((1.0..1.4).contains(&efficiency), "f = {efficiency}");
    }
}
