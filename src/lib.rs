//! BB84 quantum key distribution with simulated photons, std only.
//!
//! The modules, in the order the protocol uses them:
//!
//! - [`rng`]: the random source trait and a seeded generator
//! - [`photon`]: a photon, two bases, and the measurement rule
//! - [`channel`]: channel noise and the eavesdropper
//! - [`protocol`]: sifting, error estimation and the key length formula
//! - [`cascade`]: error correction by interactive parity checks
//! - [`toeplitz`]: the universal hash used for verification and privacy amplification
//! - [`simulate`]: the whole chain in one process, for tables and the browser view
//! - [`wire`] and [`session`]: Alice and Bob as two programs exchanging messages

pub mod cascade;
pub mod channel;
pub mod photon;
pub mod protocol;
pub mod rng;
pub mod simulate;
pub mod toeplitz;
