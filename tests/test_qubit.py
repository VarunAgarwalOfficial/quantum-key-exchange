import random

from qkx.qubit import Basis, Photon

R, D = Basis.RECTILINEAR, Basis.DIAGONAL


def test_measurement_rule():
    rng = random.Random(1)
    # Same basis: always the bit.
    for bit in (0, 1):
        for basis in (R, D):
            assert all(Photon(bit, basis).measure(basis, rng) == bit for _ in range(100))
    # Other basis: a coin flip.
    ones = sum(Photon(1, R).measure(D, rng) for _ in range(10_000))
    assert 4_800 < ones < 5_200
    # Collapse: the photon now answers consistently in the measured basis,
    # and its original bit is gone.
    flipped = 0
    for _ in range(10_000):
        photon = Photon(1, R)
        first = photon.measure(D, rng)
        assert photon.measure(D, rng) == first
        flipped += photon.measure(R, rng) != 1
    assert 4_800 < flipped < 5_200
