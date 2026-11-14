import random

import pytest

from qkx import protocol
from qkx.channel import Eve, QuantumChannel


def exchange(count, seed, eve_fraction=0.0):
    """Prepare, transmit, measure and sift. Returns both sifted keys."""
    prepared = protocol.prepare(count, random.Random(f"{seed}a"))
    eve = Eve(eve_fraction, random.Random(f"{seed}e")) if eve_fraction else None
    arrived = QuantumChannel(eve).transmit(prepared.photons)
    measured = protocol.measure(arrived, random.Random(f"{seed}b"))
    kept = protocol.sift(prepared.bases, measured.bases)
    return [prepared.bits[i] for i in kept], [measured.bits[i] for i in kept]


def test_prepare_is_reproducible_and_balanced():
    first = protocol.prepare(10_000, random.Random(1))
    again = protocol.prepare(10_000, random.Random(1))
    assert (first.bits, first.bases) == (again.bits, again.bases)
    assert 4_800 < sum(first.bits) < 5_200
    assert 4_800 < sum(b.value == "R" for b in first.bases) < 5_200


def test_sifting_keeps_about_half():
    alice, _ = exchange(20_000, seed=2)
    assert 9_700 < len(alice) < 10_300


def test_sift_returns_matching_positions():
    r, d = protocol.Basis.RECTILINEAR, protocol.Basis.DIAGONAL
    assert protocol.sift([r, r, d, d], [r, d, r, d]) == [0, 3]
    with pytest.raises(ValueError):
        protocol.sift([r], [r, d])


def test_without_eve_the_sifted_keys_are_identical():
    alice, bob = exchange(5_000, seed=3)
    assert alice == bob
    assert protocol.error_rate(alice, bob) == 0.0


def test_full_intercept_gives_a_quarter_errors():
    alice, bob = exchange(40_000, seed=4, eve_fraction=1.0)
    assert 0.235 < protocol.error_rate(alice, bob) < 0.265


def test_partial_intercept_gives_a_quarter_of_the_fraction():
    alice, bob = exchange(80_000, seed=5, eve_fraction=0.4)
    assert 0.09 < protocol.error_rate(alice, bob) < 0.11


def test_photons_needed_is_almost_always_enough():
    wanted = 400
    count = protocol.photons_needed(wanted)
    short = sum(len(exchange(count, seed=s)[0]) < wanted for s in range(300))
    assert short <= 3
    assert protocol.photons_needed(0) == 0


def test_sample_is_random_distinct_and_in_range():
    indices = protocol.choose_sample(1_000, 100, random.Random(6))
    assert len(set(indices)) == 100
    assert indices == sorted(indices)
    assert indices[0] >= 0 and indices[-1] < 1_000
    assert indices != protocol.choose_sample(1_000, 100, random.Random(7))
    with pytest.raises(ValueError):
        protocol.choose_sample(10, 11, random.Random(0))


def test_sampled_bits_never_end_up_in_the_key():
    bits = list(range(20))  # stand-ins so every position is recognisable
    sample, rest = protocol.split_sample(bits, [3, 7, 19])
    assert sample == [3, 7, 19]
    assert sorted(sample + rest) == bits
    assert not set(sample) & set(rest)


@pytest.mark.parametrize("indices", [[1, 1], [-1], [5]])
def test_split_sample_rejects_bad_positions(indices):
    with pytest.raises(ValueError):
        protocol.split_sample([0, 1, 0, 1, 0], indices)


def test_error_rate():
    assert protocol.error_rate([0, 1, 1, 0], [0, 1, 0, 1]) == 0.5
    with pytest.raises(ValueError):
        protocol.error_rate([], [])


def test_bits_to_bytes():
    assert protocol.bits_to_bytes([1, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1, 1, 1]) == b"\x81\x0f"
    assert protocol.bits_to_bytes([]) == b""
    with pytest.raises(ValueError):
        protocol.bits_to_bytes([1, 0, 1])


def test_key_digest_depends_on_every_bit_and_on_the_salt():
    key = [1, 0, 1, 1, 0, 0, 1, 0]
    digest = protocol.key_digest(key, b"salt")
    assert digest == protocol.key_digest(list(key), b"salt")
    assert digest != protocol.key_digest(key, b"pepper")
    for i in range(len(key)):
        changed = key.copy()
        changed[i] ^= 1
        assert protocol.key_digest(changed, b"salt") != digest
    assert protocol.key_digest([0], b"") != protocol.key_digest([0, 0], b"")


@pytest.mark.parametrize("text", ["", "hello", "naïve café", "量子鍵配送", "clef 𝄞"])
def test_one_time_pad_round_trips_bytes(text):
    data = text.encode("utf-8")
    rng = random.Random(8)
    key = [rng.getrandbits(1) for _ in range(8 * len(data))]
    cipher = protocol.one_time_pad(data, key)
    assert len(cipher) == len(data)
    assert protocol.one_time_pad(cipher, key).decode("utf-8") == text


def test_one_time_pad_is_xor_with_the_key_bytes():
    key = [0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0]
    assert protocol.one_time_pad(b"\xff\xff", key) == b"\xf0\x0f"


def test_one_time_pad_refuses_to_reuse_key_bits():
    with pytest.raises(ValueError, match="15 bits"):
        protocol.one_time_pad(b"ab", [1] * 15)


def test_one_time_pad_uses_each_key_bit_for_one_message_bit():
    # Flipping key bit i must flip ciphertext bit i and nothing else.
    data = b"\x00" * 4
    key = [0] * 32
    for i in range(32):
        flipped = key.copy()
        flipped[i] = 1
        cipher = protocol.one_time_pad(data, flipped)
        assert int.from_bytes(cipher, "big") == 1 << (31 - i)
