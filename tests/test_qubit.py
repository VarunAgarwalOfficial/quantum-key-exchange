import random

import pytest

from qkx.channel import Eve, QuantumChannel
from qkx.qubit import Basis, Photon

R, D = Basis.RECTILINEAR, Basis.DIAGONAL


@pytest.mark.parametrize("basis", [R, D])
@pytest.mark.parametrize("bit", [0, 1])
def test_same_basis_always_returns_the_bit(bit, basis):
    rng = random.Random(1)
    assert all(Photon(bit, basis).measure(basis, rng) == bit for _ in range(200))


def test_other_basis_is_a_coin_flip():
    rng = random.Random(2)
    ones = sum(Photon(1, R).measure(D, rng) for _ in range(10_000))
    assert 4_800 < ones < 5_200


def test_measurement_collapses_to_the_measured_basis():
    rng = random.Random(3)
    for _ in range(200):
        photon = Photon(1, R)
        first = photon.measure(D, rng)
        # Now a diagonal photon: repeating the measurement cannot change the answer.
        assert all(photon.measure(D, rng) == first for _ in range(5))


def test_wrong_basis_measurement_destroys_the_original_bit():
    rng = random.Random(4)
    flipped = 0
    for _ in range(10_000):
        photon = Photon(1, R)
        photon.measure(D, rng)
        flipped += photon.measure(R, rng) != 1
    assert 4_800 < flipped < 5_200


def test_photon_rejects_non_bits():
    with pytest.raises(ValueError):
        Photon(2, R)


def test_channel_without_eve_leaves_photons_alone():
    rng = random.Random(5)
    photons = [Photon(i % 2, R) for i in range(100)]
    arrived = QuantumChannel().transmit(photons)
    assert [p.measure(R, rng) for p in arrived] == [i % 2 for i in range(100)]


def test_eve_intercepts_about_the_configured_fraction():
    eve = Eve(0.3, random.Random(6))
    QuantumChannel(eve).transmit([Photon(0, R) for _ in range(10_000)])
    assert len(eve.bases) == 10_000
    assert 2_800 < eve.intercepted < 3_200


def test_eve_learns_the_bit_only_when_she_guesses_the_basis():
    eve = Eve(1.0, random.Random(7))
    QuantumChannel(eve).transmit([Photon(1, D) for _ in range(4_000)])
    right_basis = [bit for basis, bit in zip(eve.bases, eve.bits, strict=True) if basis is D]
    wrong_basis = [bit for basis, bit in zip(eve.bases, eve.bits, strict=True) if basis is R]
    assert all(bit == 1 for bit in right_basis)
    assert 0.44 < sum(wrong_basis) / len(wrong_basis) < 0.56


def test_eve_fraction_must_be_a_probability():
    with pytest.raises(ValueError):
        Eve(1.5, random.Random(0))
