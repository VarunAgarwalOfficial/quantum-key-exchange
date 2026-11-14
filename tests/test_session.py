import random
import socket
import threading

import pytest

from qkx.channel import Eve, QuantumChannel
from qkx.session import ProtocolError, Settings, run_alice, run_bob, run_local
from qkx.transport import SocketPipe, memory_pipes


@pytest.mark.parametrize("text", ["", "x", "meet me at noon", "naïve café 𝄞", "量子鍵配送" * 40])
def test_message_round_trips_without_eve(text):
    message = text.encode("utf-8")
    alice, bob, eve = run_local(message, seed=1)
    assert eve is None
    assert alice.delivered and bob.delivered
    assert alice.qber == bob.qber == 0.0
    assert alice.key == bob.key
    assert alice.sifted == bob.sifted
    assert bob.plaintext.decode("utf-8") == text
    assert bob.ciphertext == alice.ciphertext


def test_ciphertext_is_not_the_message():
    alice, _, _ = run_local(b"attack at dawn, attack at dawn", seed=2)
    assert alice.ciphertext != b"attack at dawn, attack at dawn"
    assert len(alice.ciphertext) == 30


def test_same_seed_gives_the_same_run():
    first = run_local(b"again", seed=3)[0]
    again = run_local(b"again", seed=3)[0]
    other = run_local(b"again", seed=4)[0]
    assert (first.bits, first.key, first.ciphertext) == (again.bits, again.key, again.ciphertext)
    assert first.bits != other.bits


def test_key_is_exactly_message_sized_and_disjoint_from_the_sample():
    message = b"twelve bytes"
    alice, bob, _ = run_local(message, seed=5)
    assert len(alice.key) == len(bob.key) == 8 * len(message)
    # Sample and key together never need more bits than were sifted.
    assert len(alice.key) + alice.sample_size <= len(alice.sifted)


def test_extra_rounds_run_when_a_round_is_too_small():
    alice, bob, _ = run_local(b"hi", seed=6, settings=Settings(photons=50))
    assert alice.rounds == bob.rounds > 5
    assert len(alice.bits) == 50 * alice.rounds
    assert bob.plaintext == b"hi"


@pytest.mark.parametrize("seed", range(25))
def test_full_eavesdropping_is_caught_and_nothing_is_sent(seed):
    alice, bob, eve = run_local(b"secret", eve_fraction=1.0, seed=seed)
    assert eve.intercepted == len(alice.bits)
    for party in (alice, bob):
        assert not party.delivered
        assert party.aborted_at == "estimation"
        assert party.ciphertext is None
        assert 0.14 < party.qber < 0.36
    assert bob.plaintext is None


def test_full_eavesdropping_error_rate_is_a_quarter_with_a_large_sample():
    settings = Settings(sample_size=20_000)
    alice, _, _ = run_local(b"", eve_fraction=1.0, seed=7, settings=settings)
    assert 0.235 < alice.qber < 0.265


@pytest.mark.parametrize("fraction", [0.02, 0.1, 0.2, 0.4])
def test_partial_eavesdropping_never_yields_garbage(fraction):
    message = b"sixteen byte msg"
    stages = set()
    for seed in range(60):
        alice, bob, _ = run_local(message, eve_fraction=fraction, seed=seed)
        assert alice.delivered == bob.delivered
        assert alice.aborted_at == bob.aborted_at
        stages.add(alice.aborted_at)
        if alice.delivered:
            assert alice.key == bob.key
            assert bob.plaintext == message
        else:
            assert alice.ciphertext is None and bob.plaintext is None
            if alice.aborted_at == "confirmation":
                assert alice.qber <= 0.11
                assert alice.key != bob.key
    # Below the threshold on average, so the hash check has to do real work.
    assert "confirmation" in stages


def test_a_light_touch_can_slip_through_but_only_with_equal_keys():
    outcomes = [run_local(b"ab", eve_fraction=0.02, seed=s) for s in range(40)]
    delivered = [(a, b) for a, b, _ in outcomes if a.delivered]
    assert delivered, "with 2% interception and a 16 bit key, most runs go through"
    assert all(a.key == b.key and b.plaintext == b"ab" for a, b in delivered)


def test_stricter_threshold_on_either_side_aborts_both():
    settings = Settings(threshold=0.0)
    alice, bob, _ = run_local(b"m", eve_fraction=0.5, seed=8, settings=settings)
    assert alice.aborted_at == bob.aborted_at == "estimation"


def test_settings_are_validated():
    for bad in ({"sample_size": 0}, {"threshold": 1.5}, {"photons": 0}):
        with pytest.raises(ValueError):
            Settings(**bad)


def run_alice_against(script):
    """Run Alice against a hand-written peer. `script(pipe)` plays Bob."""
    alice_pipe, peer_pipe = memory_pipes(timeout=5)
    thread = threading.Thread(target=script, args=(peer_pipe,), daemon=True)
    thread.start()
    try:
        return run_alice(alice_pipe, b"hi", rng=random.Random(1))
    finally:
        thread.join(timeout=5)


def test_alice_rejects_a_peer_that_breaks_the_protocol():
    def wrong_type(pipe):
        pipe.recv()
        pipe.send({"type": "hello"})

    def wrong_length(pipe):
        pipe.recv()
        pipe.send({"type": "bases", "bases": "RD"})

    for script in (wrong_type, wrong_length):
        with pytest.raises(ProtocolError):
            run_alice_against(script)


def test_bob_rejects_a_bad_sample_request():
    alice_pipe, bob_pipe = memory_pipes(timeout=5)
    alice_pipe.send({"type": "photons", "states": "RrDd"})
    alice_pipe.send({"type": "bases", "bases": "RRDD", "done": True})
    alice_pipe.send({"type": "sample", "indices": [0, 0, 99], "bits": "000"})
    with pytest.raises(ProtocolError, match="bad sample"):
        run_bob(bob_pipe, rng=random.Random(1))


def over_tcp(message, eve_fraction=0.0, seed=1):
    """Alice and Bob in two threads over a real localhost TCP connection."""
    results = {}
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as server:
        server.bind(("127.0.0.1", 0))
        server.listen(1)
        port = server.getsockname()[1]

        def bob():
            conn, _ = server.accept()
            with conn:
                conn.settimeout(10)
                results["bob"] = run_bob(SocketPipe(conn), rng=random.Random(seed + 1))

        thread = threading.Thread(target=bob, daemon=True)
        thread.start()
        with socket.create_connection(("127.0.0.1", port), timeout=10) as conn:
            eve = Eve(eve_fraction, random.Random(seed + 2)) if eve_fraction else None
            results["alice"] = run_alice(
                SocketPipe(conn), message, rng=random.Random(seed), channel=QuantumChannel(eve)
            )
        thread.join(timeout=10)
    assert not thread.is_alive()
    return results["alice"], results["bob"]


def test_end_to_end_over_tcp():
    message = "über die Leitung: 量子 ✓".encode()
    alice, bob = over_tcp(message)
    assert alice.delivered and bob.delivered
    assert bob.plaintext == message
    assert alice.key == bob.key


def test_end_to_end_over_tcp_with_a_long_message():
    message = bytes(range(256)) * 40
    alice, bob = over_tcp(message)
    assert alice.rounds == bob.rounds >= 2
    assert bob.plaintext == message


def test_end_to_end_over_tcp_with_eve():
    alice, bob = over_tcp(b"secret", eve_fraction=1.0)
    assert alice.aborted_at == bob.aborted_at == "estimation"
    assert bob.plaintext is None and alice.ciphertext is None
