import random

import pytest

from qkx import protocol


def test_sifting_keeps_matching_bases_about_half():
    r, d = protocol.Basis.RECTILINEAR, protocol.Basis.DIAGONAL
    assert protocol.sift([r, r, d, d], [r, d, r, d]) == [0, 3]

    prepared = protocol.prepare(20_000, random.Random(1))
    measured = protocol.measure(prepared.photons, random.Random(2))
    kept = protocol.sift(prepared.bases, measured.bases)
    assert 9_700 < len(kept) < 10_300
    assert all(prepared.bits[i] == measured.bits[i] for i in kept)


def test_one_time_pad_never_reuses_key_bits():
    # A short key is an error, not something to cycle.
    with pytest.raises(ValueError, match="15 bits"):
        protocol.one_time_pad(b"ab", [1] * 15)
    # Key bit i affects ciphertext bit i and nothing else.
    for i in range(32):
        key = [0] * 32
        key[i] = 1
        cipher = protocol.one_time_pad(b"\x00" * 4, key)
        assert int.from_bytes(cipher, "big") == 1 << (31 - i)
    # Bits revealed in the sample are removed from the key.
    sample, rest = protocol.split_sample(list(range(20)), [3, 7, 19])
    assert sample == [3, 7, 19]
    assert not set(sample) & set(rest)
    assert sorted(sample + rest) == list(range(20))
