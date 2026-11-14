"""Alice and Bob as functions over a message pipe.

The conversation, in order:

    Alice -> Bob   photons   the photons (the simulated quantum channel)
    Bob -> Alice   bases     the bases Bob measured in, after measuring
    Alice -> Bob   bases     the bases Alice prepared in, and whether more
                             rounds follow (repeated until enough bits sift)
    Alice -> Bob   sample    random positions of the sifted key and her bits there
    Bob -> Alice   sample    his bits at those positions
    Alice -> Bob   verdict   whether she accepts the error rate
    Bob -> Alice   verdict   whether he does
    Alice -> Bob   confirm   salt and salted hash of her key
    Bob -> Alice   confirm   whether his key hashes to the same value
    Alice -> Bob   message   the one-time-pad ciphertext
    Bob -> Alice   received

Everything after the first line is public: an eavesdropper may read it, and
the protocol assumes she cannot alter it.
"""

from __future__ import annotations

import hmac
import math
import random
import secrets
import threading
from dataclasses import dataclass, field

from qkx import protocol
from qkx.channel import Eve, QuantumChannel
from qkx.qubit import Basis
from qkx.transport import Message, Pipe, decode_photons, encode_photons, memory_pipes

MAX_ROUND_PHOTONS = 100_000


class ProtocolError(Exception):
    """The other party sent something that does not fit the protocol."""


@dataclass(frozen=True)
class Settings:
    sample_size: int = protocol.DEFAULT_SAMPLE_SIZE
    threshold: float = protocol.DEFAULT_THRESHOLD
    photons: int | None = None  # photons per round; None picks it from the message length

    def __post_init__(self) -> None:
        if self.sample_size < 1:
            raise ValueError("the sample must contain at least one bit")
        if not 0.0 <= self.threshold <= 1.0:
            raise ValueError("the threshold must be between 0 and 1")
        if self.photons is not None and not 1 <= self.photons <= MAX_ROUND_PHOTONS:
            raise ValueError(f"photons per round must be between 1 and {MAX_ROUND_PHOTONS}")


@dataclass
class Outcome:
    """What one party saw. `bits` and `bases` are that party's own, for every photon."""

    delivered: bool = False
    aborted_at: str | None = None  # "estimation" or "confirmation"
    reason: str | None = None
    rounds: int = 0
    bits: list[int] = field(default_factory=list)
    bases: list[Basis] = field(default_factory=list)
    kept: list[int] = field(default_factory=list)
    sifted: list[int] = field(default_factory=list)
    sample_size: int = 0
    qber: float | None = None
    key: list[int] = field(default_factory=list)
    ciphertext: bytes | None = None
    plaintext: bytes | None = None

    def _abort(self, stage: str, reason: str) -> Outcome:
        self.aborted_at = stage
        self.reason = reason
        return self

    def _record_round(self, bits: list[int], bases: list[Basis], kept: list[int]) -> None:
        offset = len(self.bits)
        self.rounds += 1
        self.bits += bits
        self.bases += bases
        self.kept += [offset + i for i in kept]
        self.sifted += [bits[i] for i in kept]


def _expect(pipe: Pipe, kind: str) -> Message:
    message = pipe.recv()
    if message.get("type") != kind:
        raise ProtocolError(f"expected a {kind!r} message, got {message.get('type')!r}")
    return message


def _get(message: Message, name: str, kind: type):
    value = message.get(name)
    if type(value) is not kind:
        raise ProtocolError(f"{message.get('type')!r} message has a bad {name!r} field")
    return value


def _bases_text(bases: list[Basis]) -> str:
    return "".join(basis.value for basis in bases)


def _parse_bases(text: str, count: int) -> list[Basis]:
    if len(text) != count or set(text) - {"R", "D"}:
        raise ProtocolError("the list of bases does not match the photons sent")
    return [Basis(letter) for letter in text]


def _bits_text(bits: list[int]) -> str:
    return "".join(map(str, bits))


def _parse_bits(text: str, count: int) -> list[int]:
    if len(text) != count or set(text) - {"0", "1"}:
        raise ProtocolError("the sample bits do not match the sample positions")
    return [int(letter) for letter in text]


def _estimation_reason(qber: float, threshold: float) -> str:
    return (
        f"error rate {qber:.1%} is above the {threshold:.1%} limit, "
        "so the photons were disturbed on the way"
    )


CONFIRMATION_REASON = "the two keys differ, so errors slipped past the sample"


def run_alice(
    pipe: Pipe,
    message: bytes,
    *,
    rng: random.Random | None = None,
    channel: QuantumChannel | None = None,
    settings: Settings | None = None,
) -> Outcome:
    """Agree a key with Bob and, only if it is safe, send `message` under it."""
    rng = rng or secrets.SystemRandom()
    channel = channel or QuantumChannel()
    settings = settings or Settings()
    out = Outcome(sample_size=settings.sample_size)

    key_bits = 8 * len(message)
    needed = key_bits + settings.sample_size
    per_round = settings.photons or min(protocol.photons_needed(needed), MAX_ROUND_PHOTONS)
    max_rounds = 10 * math.ceil(2 * needed / per_round) + 20

    # 1-3. Send photons, hear Bob's bases, reveal ours, keep the matches.
    while len(out.sifted) < needed:
        if out.rounds >= max_rounds:
            raise ProtocolError("too few bases matched after many rounds")
        prepared = protocol.prepare(per_round, rng)
        arrived = channel.transmit(prepared.photons)
        pipe.send({"type": "photons", "states": encode_photons(arrived)})
        reply = _expect(pipe, "bases")
        bob_bases = _parse_bases(_get(reply, "bases", str), per_round)
        out._record_round(prepared.bits, prepared.bases, protocol.sift(prepared.bases, bob_bases))
        done = len(out.sifted) >= needed
        pipe.send({"type": "bases", "bases": _bases_text(prepared.bases), "done": done})

    # 4. Sacrifice a random sample to estimate the error rate.
    indices = protocol.choose_sample(len(out.sifted), settings.sample_size, rng)
    sample, rest = protocol.split_sample(out.sifted, indices)
    pipe.send({"type": "sample", "indices": indices, "bits": _bits_text(sample)})
    reply = _expect(pipe, "sample")
    bob_sample = _parse_bits(_get(reply, "bits", str), len(indices))
    out.qber = protocol.error_rate(sample, bob_sample)
    accept = out.qber <= settings.threshold
    pipe.send({"type": "verdict", "ok": accept})
    bob_accepts = _get(_expect(pipe, "verdict"), "ok", bool)
    if not accept:
        return out._abort("estimation", _estimation_reason(out.qber, settings.threshold))
    if not bob_accepts:
        return out._abort("estimation", "Bob rejected the error rate")

    # 5. Confirm both hold the same key before trusting it with the message.
    out.key = rest[:key_bits]
    salt = rng.getrandbits(128).to_bytes(16, "big")
    digest = protocol.key_digest(out.key, salt)
    pipe.send({"type": "confirm", "bits": key_bits, "salt": salt.hex(), "digest": digest})
    if not _get(_expect(pipe, "confirm"), "ok", bool):
        return out._abort("confirmation", CONFIRMATION_REASON)

    # 6. One-time pad.
    out.ciphertext = protocol.one_time_pad(message, out.key)
    pipe.send({"type": "message", "ciphertext": out.ciphertext.hex()})
    _expect(pipe, "received")
    out.plaintext = message
    out.delivered = True
    return out


def run_bob(
    pipe: Pipe,
    *,
    rng: random.Random | None = None,
    threshold: float = protocol.DEFAULT_THRESHOLD,
) -> Outcome:
    """Measure Alice's photons, agree a key with her and decrypt her message."""
    rng = rng or secrets.SystemRandom()
    out = Outcome()

    # 1-3. Measure in random bases, announce them, hear Alice's, keep the matches.
    done = False
    while not done:
        photons = decode_photons(_get(_expect(pipe, "photons"), "states", str))
        measured = protocol.measure(photons, rng)
        pipe.send({"type": "bases", "bases": _bases_text(measured.bases)})
        reply = _expect(pipe, "bases")
        alice_bases = _parse_bases(_get(reply, "bases", str), len(photons))
        out._record_round(measured.bits, measured.bases, protocol.sift(alice_bases, measured.bases))
        done = _get(reply, "done", bool)

    # 4. Reveal the sampled bits and judge the error rate ourselves.
    request = _expect(pipe, "sample")
    indices = _get(request, "indices", list)
    try:
        sample, rest = protocol.split_sample(out.sifted, indices)
        alice_sample = _parse_bits(_get(request, "bits", str), len(indices))
        out.qber = protocol.error_rate(alice_sample, sample)
    except (ValueError, TypeError) as error:
        raise ProtocolError(f"bad sample request: {error}") from error
    out.sample_size = len(indices)
    pipe.send({"type": "sample", "bits": _bits_text(sample)})
    alice_accepts = _get(_expect(pipe, "verdict"), "ok", bool)
    accept = out.qber <= threshold
    pipe.send({"type": "verdict", "ok": accept})
    if not accept:
        return out._abort("estimation", _estimation_reason(out.qber, threshold))
    if not alice_accepts:
        return out._abort("estimation", "Alice rejected the error rate")

    # 5. Check our key against the hash of hers.
    request = _expect(pipe, "confirm")
    key_bits = _get(request, "bits", int)
    if not 0 <= key_bits <= len(rest) or key_bits % 8:
        raise ProtocolError("Alice asked for a key we do not have")
    try:
        salt = bytes.fromhex(_get(request, "salt", str))
    except ValueError as error:
        raise ProtocolError("bad salt") from error
    out.key = rest[:key_bits]
    digest = protocol.key_digest(out.key, salt)
    same = hmac.compare_digest(digest, _get(request, "digest", str))
    pipe.send({"type": "confirm", "ok": same})
    if not same:
        return out._abort("confirmation", CONFIRMATION_REASON)

    # 6. One-time pad. The ciphertext must use the whole key, no more, no less.
    try:
        out.ciphertext = bytes.fromhex(_get(_expect(pipe, "message"), "ciphertext", str))
    except ValueError as error:
        raise ProtocolError("bad ciphertext") from error
    if 8 * len(out.ciphertext) != key_bits:
        raise ProtocolError("the ciphertext does not match the agreed key length")
    out.plaintext = protocol.one_time_pad(out.ciphertext, out.key)
    pipe.send({"type": "received"})
    out.delivered = True
    return out


def party_rng(seed: int | None, name: str) -> random.Random:
    """An independent generator per party: seeded for replays, system randomness otherwise."""
    return secrets.SystemRandom() if seed is None else random.Random(f"{seed}/{name}")


def run_local(
    message: bytes,
    *,
    eve_fraction: float = 0.0,
    seed: int | None = None,
    settings: Settings | None = None,
) -> tuple[Outcome, Outcome, Eve | None]:
    """Run Alice and Bob in this process over an in-memory pipe."""
    settings = settings or Settings()
    eve = Eve(eve_fraction, party_rng(seed, "eve")) if eve_fraction > 0 else None
    alice_pipe, bob_pipe = memory_pipes()
    result: list[Outcome | BaseException] = []

    def bob() -> None:
        try:
            result.append(
                run_bob(bob_pipe, rng=party_rng(seed, "bob"), threshold=settings.threshold)
            )
        except BaseException as error:
            result.append(error)

    thread = threading.Thread(target=bob, daemon=True)
    thread.start()
    alice = run_alice(
        alice_pipe,
        message,
        rng=party_rng(seed, "alice"),
        channel=QuantumChannel(eve),
        settings=settings,
    )
    thread.join(timeout=60)
    if not result:
        raise ProtocolError("Bob did not finish")
    if isinstance(result[0], BaseException):
        raise result[0]
    return alice, result[0], eve
