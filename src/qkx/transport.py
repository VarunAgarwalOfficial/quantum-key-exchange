"""Message pipes between Alice and Bob.

Every message is a JSON object sent as one frame: a 4 byte big-endian length
followed by that many bytes of UTF-8 JSON.

In a real system there are two channels: an optical one for photons and an
ordinary one for talking. Here both are the same pipe, and the "quantum
channel" is simulated: the photon frame carries each photon's hidden state,
and the receiver is trusted to rebuild the photons and touch them only
through `Photon.measure`.
"""

from __future__ import annotations

import json
import queue
import socket
import struct
from collections import deque
from typing import Any, Protocol

from qkx.qubit import Photon

Message = dict[str, Any]

HEADER = struct.Struct(">I")
MAX_FRAME = 4 * 1024 * 1024


class TransportError(Exception):
    """The pipe broke or carried something that is not a valid frame."""


class Pipe(Protocol):
    def send(self, message: Message) -> None: ...

    def recv(self) -> Message: ...


def encode_frame(message: Message) -> bytes:
    body = json.dumps(message, separators=(",", ":")).encode("utf-8")
    if len(body) > MAX_FRAME:
        raise TransportError(f"frame of {len(body)} bytes is over the {MAX_FRAME} byte limit")
    return HEADER.pack(len(body)) + body


class FrameDecoder:
    """Turns a byte stream, in whatever pieces it arrives, back into messages."""

    def __init__(self) -> None:
        self._buffer = bytearray()

    def feed(self, data: bytes) -> list[Message]:
        self._buffer += data
        messages: list[Message] = []
        while len(self._buffer) >= HEADER.size:
            (length,) = HEADER.unpack_from(self._buffer)
            if length > MAX_FRAME:
                raise TransportError(f"frame of {length} bytes is over the {MAX_FRAME} byte limit")
            end = HEADER.size + length
            if len(self._buffer) < end:
                break
            body = bytes(self._buffer[HEADER.size : end])
            del self._buffer[:end]
            try:
                message = json.loads(body)
            except ValueError as error:
                raise TransportError("frame is not valid JSON") from error
            if not isinstance(message, dict):
                raise TransportError("frame is not a JSON object")
            messages.append(message)
        return messages

    @property
    def pending(self) -> int:
        """Bytes received that do not yet make a whole frame."""
        return len(self._buffer)


class SocketPipe:
    """Framed JSON messages over a connected socket."""

    def __init__(self, sock: socket.socket) -> None:
        self._sock = sock
        self._decoder = FrameDecoder()
        self._ready: deque[Message] = deque()

    def send(self, message: Message) -> None:
        try:
            self._sock.sendall(encode_frame(message))
        except OSError as error:
            raise TransportError(f"could not send: {error}") from error

    def recv(self) -> Message:
        while not self._ready:
            try:
                data = self._sock.recv(65536)
            except OSError as error:
                raise TransportError(f"could not receive: {error}") from error
            if not data:
                where = "in the middle of a frame" if self._decoder.pending else "by the peer"
                raise TransportError(f"connection closed {where}")
            self._ready.extend(self._decoder.feed(data))
        return self._ready.popleft()


class QueuePipe:
    """One end of an in-memory pipe, for running both parties in one process."""

    def __init__(self, inbox: queue.Queue[bytes], outbox: queue.Queue[bytes], timeout: float):
        self._inbox = inbox
        self._outbox = outbox
        self._timeout = timeout
        self._decoder = FrameDecoder()

    def send(self, message: Message) -> None:
        self._outbox.put(encode_frame(message))

    def recv(self) -> Message:
        try:
            data = self._inbox.get(timeout=self._timeout)
        except queue.Empty as error:
            raise TransportError("timed out waiting for the other party") from error
        (message,) = self._decoder.feed(data)
        return message


def memory_pipes(timeout: float = 30.0) -> tuple[QueuePipe, QueuePipe]:
    """Two connected in-memory pipe ends."""
    a_to_b: queue.Queue[bytes] = queue.Queue()
    b_to_a: queue.Queue[bytes] = queue.Queue()
    return QueuePipe(b_to_a, a_to_b, timeout), QueuePipe(a_to_b, b_to_a, timeout)


def encode_photons(photons: list[Photon]) -> str:
    """Simulated photons as text, one letter each. See the module docstring."""
    return "".join(photon._wire_state() for photon in photons)


def decode_photons(text: str) -> list[Photon]:
    try:
        return [Photon._from_wire(letter) for letter in text]
    except ValueError as error:
        raise TransportError("photon frame contains something that is not a photon") from error
