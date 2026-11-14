"""The quantum channel between Alice and Bob, with an optional eavesdropper."""

from __future__ import annotations

import random
from dataclasses import dataclass, field

from qkx.qubit import Basis, Photon, random_basis


@dataclass
class Eve:
    """An intercept-resend eavesdropper.

    She intercepts each photon with probability `fraction`, measures it in a
    random basis and lets it continue. She cannot copy the photon, so the
    photon Bob gets is the one she collapsed.
    """

    fraction: float
    rng: random.Random
    bases: list[Basis | None] = field(default_factory=list)
    bits: list[int | None] = field(default_factory=list)

    def __post_init__(self) -> None:
        if not 0.0 <= self.fraction <= 1.0:
            raise ValueError("the intercept fraction must be between 0 and 1")

    def intercept(self, photon: Photon) -> None:
        if self.rng.random() < self.fraction:
            basis = random_basis(self.rng)
            self.bases.append(basis)
            self.bits.append(photon.measure(basis, self.rng))
        else:
            self.bases.append(None)
            self.bits.append(None)

    @property
    def intercepted(self) -> int:
        return sum(basis is not None for basis in self.bases)


@dataclass
class QuantumChannel:
    """Carries photons from Alice to Bob. Eve, if present, sits in the middle."""

    eve: Eve | None = None

    def transmit(self, photons: list[Photon]) -> list[Photon]:
        if self.eve is not None:
            for photon in photons:
                self.eve.intercept(photon)
        return photons
