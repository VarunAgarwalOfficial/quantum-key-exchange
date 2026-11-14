//! A simulated photon and the one rule of quantum mechanics this program uses.

use crate::rng::Random;

/// The two polarisation bases of BB84.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// Horizontal and vertical, written `+`.
    Rectilinear,
    /// The two diagonals, written `x`.
    Diagonal,
}

impl Basis {
    pub fn random(rng: &mut dyn Random) -> Self {
        Self::from_bit(rng.bit())
    }

    pub fn from_bit(bit: bool) -> Self {
        if bit {
            Basis::Diagonal
        } else {
            Basis::Rectilinear
        }
    }

    pub fn as_bit(self) -> bool {
        self == Basis::Diagonal
    }

    pub fn symbol(self) -> char {
        match self {
            Basis::Rectilinear => '+',
            Basis::Diagonal => 'x',
        }
    }
}

/// One photon: polarised along one of four directions.
///
/// The fields are private. The only way to learn anything is [`Photon::measure`],
/// and measuring in the other basis destroys what was there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Photon {
    basis: Basis,
    bit: bool,
}

impl Photon {
    /// A photon carrying `bit`, polarised in `basis`.
    pub fn prepare(bit: bool, basis: Basis) -> Self {
        Self { basis, bit }
    }

    /// Measures in `basis`. In the photon's own basis this returns its bit.
    /// In the other basis the answer is a coin flip, and afterwards the
    /// photon is polarised the way the measurement says.
    pub fn measure(&mut self, basis: Basis, rng: &mut dyn Random) -> bool {
        if basis != self.basis {
            self.basis = basis;
            self.bit = rng.bit();
        }
        self.bit
    }

    /// Channel noise: the polarisation is turned by 90 degrees, which flips
    /// the bit in the photon's own basis.
    pub(crate) fn flip(&mut self) {
        self.bit = !self.bit;
    }

    /// The photon's hidden state as one byte, for the simulated channel
    /// between two programs. See the README: a real photon cannot be copied
    /// like this, and the receiving program is trusted to only measure it.
    pub fn to_wire(self) -> u8 {
        u8::from(self.bit) | (u8::from(self.basis.as_bit()) << 1)
    }

    pub fn from_wire(byte: u8) -> Option<Self> {
        (byte < 4).then(|| Self {
            basis: Basis::from_bit(byte & 2 != 0),
            bit: byte & 1 != 0,
        })
    }
}
