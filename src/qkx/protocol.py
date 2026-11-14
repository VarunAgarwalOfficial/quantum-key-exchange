"""The steps of BB84 as pure functions. No sockets, no printing."""

from __future__ import annotations

import hashlib
import math
import random
from dataclasses import dataclass

from qkx.qubit import Basis, Photon, random_basis

# Intercepting every photon causes about 25% errors in the sifted key.
# 11% is the error rate usually quoted as the most BB84 can tolerate, and
# it sits far enough below 25% that a full attack is not missed by chance.
DEFAULT_THRESHOLD = 0.11
DEFAULT_SAMPLE_SIZE = 256


@dataclass(frozen=True)
class Prepared:
    bits: list[int]
    bases: list[Basis]
    photons: list[Photon]


@dataclass(frozen=True)
class Measured:
    bases: list[Basis]
    bits: list[int]


def prepare(count: int, rng: random.Random) -> Prepared:
    """Alice: pick a random bit and a random basis for each photon."""
    bits = [rng.getrandbits(1) for _ in range(count)]
    bases = [random_basis(rng) for _ in range(count)]
    photons = [Photon(bit, basis) for bit, basis in zip(bits, bases, strict=True)]
    return Prepared(bits, bases, photons)


def measure(photons: list[Photon], rng: random.Random) -> Measured:
    """Bob: measure each photon in a random basis."""
    bases = [random_basis(rng) for _ in photons]
    bits = [photon.measure(basis, rng) for photon, basis in zip(photons, bases, strict=True)]
    return Measured(bases, bits)


def sift(alice_bases: list[Basis], bob_bases: list[Basis]) -> list[int]:
    """Positions where both used the same basis. Only these can form a key."""
    pairs = zip(alice_bases, bob_bases, strict=True)
    return [i for i, (a, b) in enumerate(pairs) if a is b]


def photons_needed(sifted_bits: int) -> int:
    """How many photons to send so that about `sifted_bits` survive sifting.

    Sifting keeps half on average; the extra term is three standard
    deviations so one round is almost always enough.
    """
    if sifted_bits <= 0:
        return 0
    return 2 * sifted_bits + math.ceil(3 * math.sqrt(2 * sifted_bits))


def choose_sample(sifted_length: int, sample_size: int, rng: random.Random) -> list[int]:
    """Random positions of the sifted key to reveal and compare."""
    if sample_size > sifted_length:
        raise ValueError("the sample cannot be larger than the sifted key")
    return sorted(rng.sample(range(sifted_length), sample_size))


def split_sample(bits: list[int], indices: list[int]) -> tuple[list[int], list[int]]:
    """Split a sifted key into the revealed sample and the bits that stay secret."""
    chosen = set(indices)
    if len(chosen) != len(indices) or any(not 0 <= i < len(bits) for i in chosen):
        raise ValueError("sample positions must be distinct and inside the sifted key")
    sample = [bits[i] for i in indices]
    rest = [bit for i, bit in enumerate(bits) if i not in chosen]
    return sample, rest


def error_rate(alice_sample: list[int], bob_sample: list[int]) -> float:
    """The quantum bit error rate (QBER) of the revealed sample."""
    if not alice_sample:
        raise ValueError("cannot estimate an error rate from an empty sample")
    pairs = zip(alice_sample, bob_sample, strict=True)
    return sum(a != b for a, b in pairs) / len(alice_sample)


def bits_to_bytes(bits: list[int]) -> bytes:
    """Pack bits into bytes, first bit highest. The length must be a multiple of 8."""
    if len(bits) % 8:
        raise ValueError("the number of bits must be a multiple of 8")
    out = bytearray()
    for start in range(0, len(bits), 8):
        value = 0
        for bit in bits[start : start + 8]:
            value = (value << 1) | bit
        out.append(value)
    return bytes(out)


def key_digest(key: list[int], salt: bytes) -> str:
    """A salted hash of the key, compared in public to confirm both keys match."""
    text = "".join(map(str, key)).encode("ascii")
    return hashlib.sha256(salt + b"|" + text).hexdigest()


def one_time_pad(data: bytes, key: list[int]) -> bytes:
    """XOR `data` with exactly as many key bits as it has bits.

    The key is never cycled or stretched. A key that is too short is an error.
    Encrypting and decrypting are the same operation.
    """
    needed = 8 * len(data)
    if len(key) < needed:
        raise ValueError(f"the key has {len(key)} bits but the message needs {needed}")
    pad = bits_to_bytes(key[:needed])
    return bytes(d ^ k for d, k in zip(data, pad, strict=True))
