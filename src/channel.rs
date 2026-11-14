//! What happens to a photon between Alice and Bob: an eavesdropper, then noise.

use crate::photon::{Basis, Photon};
use crate::rng::Random;

/// How Eve chooses the basis she measures in (intercept and resend).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Always the rectilinear basis.
    Rectilinear,
    /// Always the diagonal basis.
    Diagonal,
    /// A fresh random basis for every photon.
    Random,
}

impl Strategy {
    pub fn parse(name: &str) -> Option<Option<Self>> {
        match name {
            "none" | "off" => Some(None),
            "rect" | "rectilinear" | "+" => Some(Some(Strategy::Rectilinear)),
            "diag" | "diagonal" | "x" => Some(Some(Strategy::Diagonal)),
            "random" => Some(Some(Strategy::Random)),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Strategy::Rectilinear => "fixed +",
            Strategy::Diagonal => "fixed x",
            Strategy::Random => "random",
        }
    }
}

/// An eavesdropper who measures a fraction of the photons and sends on what she saw.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Eve {
    pub strategy: Strategy,
    /// Probability that she intercepts a given photon, 0 to 1.
    pub fraction: f64,
}

/// The line between Alice and Bob.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Channel {
    /// Probability that a photon arrives with its polarisation turned by 90
    /// degrees. Without Eve this is exactly the error rate of the sifted key.
    pub noise: f64,
    pub eve: Option<Eve>,
}

/// What Eve did to one photon: the basis she used and the bit she read.
pub type Tap = Option<(Basis, bool)>;

impl Channel {
    /// Carries one photon across. Returns what arrives and what Eve saw.
    pub fn carry(&self, mut photon: Photon, rng: &mut dyn Random) -> (Photon, Tap) {
        let mut tap = None;
        if let Some(eve) = self.eve {
            if rng.chance(eve.fraction) {
                let basis = match eve.strategy {
                    Strategy::Rectilinear => Basis::Rectilinear,
                    Strategy::Diagonal => Basis::Diagonal,
                    Strategy::Random => Basis::random(rng),
                };
                // Measuring is all she can do; the photon she passes on is
                // the one her measurement left behind.
                tap = Some((basis, photon.measure(basis, rng)));
            }
        }
        if rng.chance(self.noise) {
            photon.flip();
        }
        (photon, tap)
    }
}
