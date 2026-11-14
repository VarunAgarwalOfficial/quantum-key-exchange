import random
import socket

import pytest

from qkx.qubit import Basis, Photon
from qkx.transport import (
    HEADER,
    MAX_FRAME,
    FrameDecoder,
    SocketPipe,
    TransportError,
    decode_photons,
    encode_frame,
    encode_photons,
    memory_pipes,
)

MESSAGES = [{"type": "a", "n": 1}, {"type": "b", "text": "ünïcode"}, {"type": "c", "list": [1, 2]}]


def test_frame_is_length_prefixed_json():
    frame = encode_frame({"type": "x"})
    assert frame[:4] == (len(frame) - 4).to_bytes(4, "big")
    assert frame[4:] == b'{"type":"x"}'


def test_decoder_handles_concatenated_frames():
    stream = b"".join(encode_frame(m) for m in MESSAGES)
    assert FrameDecoder().feed(stream) == MESSAGES


def test_decoder_handles_one_byte_at_a_time():
    stream = b"".join(encode_frame(m) for m in MESSAGES)
    decoder = FrameDecoder()
    got = [m for i in range(len(stream)) for m in decoder.feed(stream[i : i + 1])]
    assert got == MESSAGES
    assert decoder.pending == 0


@pytest.mark.parametrize("seed", range(20))
def test_decoder_handles_arbitrary_splits(seed):
    rng = random.Random(seed)
    stream = b"".join(encode_frame(m) for m in MESSAGES * 3)
    decoder, got, start = FrameDecoder(), [], 0
    while start < len(stream):
        size = rng.randint(1, 40)
        got += decoder.feed(stream[start : start + size])
        start += size
    assert got == MESSAGES * 3


def test_decoder_rejects_oversized_and_malformed_frames():
    with pytest.raises(TransportError, match="limit"):
        FrameDecoder().feed(HEADER.pack(MAX_FRAME + 1))
    with pytest.raises(TransportError, match="JSON"):
        FrameDecoder().feed(HEADER.pack(3) + b"{x}")
    with pytest.raises(TransportError, match="object"):
        FrameDecoder().feed(HEADER.pack(2) + b"[]")


def test_socket_pipe_round_trip_and_split_writes():
    a, b = socket.socketpair()
    with a, b:
        b.settimeout(5)
        pipe = SocketPipe(b)
        SocketPipe(a).send(MESSAGES[0])
        assert pipe.recv() == MESSAGES[0]
        # Two frames in one write, then one frame over three writes.
        a.sendall(encode_frame(MESSAGES[1]) + encode_frame(MESSAGES[2]))
        assert [pipe.recv(), pipe.recv()] == MESSAGES[1:]
        frame = encode_frame(MESSAGES[0])
        for part in (frame[:2], frame[2:7], frame[7:]):
            a.sendall(part)
        assert pipe.recv() == MESSAGES[0]


def test_socket_pipe_reports_a_closed_connection():
    a, b = socket.socketpair()
    with b:
        a.close()
        with pytest.raises(TransportError, match="closed by the peer"):
            SocketPipe(b).recv()
    a, b = socket.socketpair()
    with b:
        a.sendall(encode_frame(MESSAGES[0])[:-3])
        a.close()
        with pytest.raises(TransportError, match="middle of a frame"):
            SocketPipe(b).recv()


def test_memory_pipes_are_connected_both_ways():
    left, right = memory_pipes(timeout=1)
    left.send({"type": "ping"})
    right.send({"type": "pong"})
    assert right.recv() == {"type": "ping"}
    assert left.recv() == {"type": "pong"}
    with pytest.raises(TransportError, match="timed out"):
        memory_pipes(timeout=0.01)[0].recv()


def test_photons_survive_the_wire_unmeasured():
    rng = random.Random(1)
    r, d = Basis.RECTILINEAR, Basis.DIAGONAL
    states = [(1, r), (0, r), (1, d), (0, d)]
    text = encode_photons([Photon(bit, basis) for bit, basis in states])
    assert text == "RrDd"
    arrived = decode_photons(text)
    assert [p.measure(basis, rng) for p, (_, basis) in zip(arrived, states, strict=True)] == [
        bit for bit, _ in states
    ]
    with pytest.raises(TransportError):
        decode_photons("RxD")
