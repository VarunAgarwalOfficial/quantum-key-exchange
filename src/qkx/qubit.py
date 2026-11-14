"""A simulated polarised photon.

The whole quantum rule this project needs lives in `Photon.measure`:
measuring in the basis the photon was prepared in returns its bit, measuring
in the other basis returns a coin flip, and either way the photon is left in
the state that was just measured.
"""

from __future__ import annotations

import random
from enum import Enum


class Basis(Enum):
    """The two polarisation filters."""

    RECTILINEAR = "R"
    DIAGONAL = "D"


BASES = (Basis.RECTILINEAR, Basis.DIAGONAL)


def random_basis(rng: random.Random) -> Basis:
    return BASES[rng.getrandbits(1)]


class Photon:
    """One photon carrying one bit in one basis.

    The state is private. Code that plays by the rules only calls `measure`.
    """

    __slots__ = ("_basis", "_bit")

    def __init__(self, bit: int, basis: Basis) -> None:
        if bit not in (0, 1):
            raise ValueError("a photon carries a single bit, 0 or 1")
        self._bit = bit
        self._basis = basis

    def measure(self, basis: Basis, rng: random.Random) -> int:
        """Measure in `basis` and collapse the photon to the result."""
        if basis is not self._basis:
            self._bit = rng.getrandbits(1)
            self._basis = basis
        return self._bit

    # The two methods below exist only so the transport can carry a simulated
    # photon between processes. A real photon has no such thing: nobody can
    # read its state without measuring it.

    def _wire_state(self) -> str:
        """The basis letter; upper case means bit 1, lower case bit 0."""
        letter = self._basis.value
        return letter if self._bit else letter.lower()

    @classmethod
    def _from_wire(cls, letter: str) -> Photon:
        return cls(int(letter.isupper()), Basis(letter.upper()))
