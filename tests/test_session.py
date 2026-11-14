import random
import socket
import threading

from qkx.channel import Eve, QuantumChannel
from qkx.session import Settings, run_alice, run_bob, run_local
from qkx.transport import SocketPipe


def test_clean_run_gives_equal_keys_and_delivers_the_message():
    for text in ("", "meet me at noon", "naïve café 量子鍵配送 𝄞"):
        message = text.encode("utf-8")
        alice, bob, _ = run_local(message, seed=1)
        assert alice.delivered and bob.delivered
        assert alice.qber == bob.qber == 0.0
        assert alice.key == bob.key
        assert len(alice.key) == 8 * len(message)
        assert len(alice.key) + alice.sample_size <= len(alice.sifted)
        assert bob.plaintext.decode("utf-8") == text
        assert alice.ciphertext == bob.ciphertext
        assert not message or alice.ciphertext != message


def test_full_eavesdropping_gives_a_quarter_errors_and_aborts():
    settings = Settings(sample_size=20_000)
    alice, _, _ = run_local(b"", eve_fraction=1.0, seed=2, settings=settings)
    assert 0.235 < alice.qber < 0.265

    # With the default sample, both stop and nothing is sent.
    for seed in range(10):
        alice, bob, eve = run_local(b"secret", eve_fraction=1.0, seed=seed)
        assert eve.intercepted == len(alice.bits)
        assert alice.aborted_at == bob.aborted_at == "estimation"
        assert alice.ciphertext is None and bob.plaintext is None


def test_partial_eavesdropping_never_yields_garbage():
    message = b"sixteen byte msg"
    stages = set()
    for fraction in (0.02, 0.2):
        for seed in range(40):
            alice, bob, _ = run_local(message, eve_fraction=fraction, seed=seed)
            assert alice.aborted_at == bob.aborted_at
            stages.add(alice.aborted_at)
            if alice.delivered:
                assert alice.key == bob.key and bob.plaintext == message
            else:
                assert alice.ciphertext is None and bob.plaintext is None
    # Some runs slip through with equal keys; the rest are caught by the hash.
    assert {None, "confirmation"} <= stages


def test_end_to_end_over_a_localhost_socket():
    message = "über die Leitung: 量子 𝄞".encode()
    results = {}
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as server:
        server.bind(("127.0.0.1", 0))
        server.listen(1)

        def bob():
            conn, _ = server.accept()
            with conn:
                conn.settimeout(10)
                results["bob"] = run_bob(SocketPipe(conn), rng=random.Random(2))

        thread = threading.Thread(target=bob, daemon=True)
        thread.start()
        with socket.create_connection(server.getsockname(), timeout=10) as conn:
            alice = run_alice(SocketPipe(conn), message, rng=random.Random(1))
        thread.join(timeout=10)
    assert alice.delivered and results["bob"].delivered
    assert results["bob"].plaintext == message
    assert alice.key == results["bob"].key

    # The same wire, with Eve on it.
    eve = Eve(1.0, random.Random(3))
    a, b = socket.socketpair()
    with a, b:
        a.settimeout(10)
        b.settimeout(10)
        thread = threading.Thread(
            target=lambda: results.update(bob=run_bob(SocketPipe(b), rng=random.Random(2))),
            daemon=True,
        )
        thread.start()
        alice = run_alice(
            SocketPipe(a), b"secret", rng=random.Random(1), channel=QuantumChannel(eve)
        )
        thread.join(timeout=10)
    assert alice.aborted_at == results["bob"].aborted_at == "estimation"
    assert results["bob"].plaintext is None
