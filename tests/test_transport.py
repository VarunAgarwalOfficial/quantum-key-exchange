import pytest

from qkx.transport import HEADER, MAX_FRAME, FrameDecoder, TransportError, encode_frame

MESSAGES = [{"type": "a", "n": 1}, {"type": "b", "text": "ünïcode"}, {"type": "c", "list": [1, 2]}]


def test_framing_handles_split_and_joined_reads():
    stream = b"".join(encode_frame(m) for m in MESSAGES)
    # All frames in one read.
    assert FrameDecoder().feed(stream) == MESSAGES
    # One byte per read.
    decoder = FrameDecoder()
    assert [m for i in range(len(stream)) for m in decoder.feed(stream[i : i + 1])] == MESSAGES
    assert decoder.pending == 0
    # A read that ends in the middle of the second frame.
    decoder = FrameDecoder()
    cut = len(encode_frame(MESSAGES[0])) + 5
    assert decoder.feed(stream[:cut]) == MESSAGES[:1]
    assert decoder.feed(stream[cut:]) == MESSAGES[1:]
    # Garbage is refused.
    with pytest.raises(TransportError, match="limit"):
        FrameDecoder().feed(HEADER.pack(MAX_FRAME + 1))
    with pytest.raises(TransportError, match="JSON"):
        FrameDecoder().feed(HEADER.pack(3) + b"{x}")
