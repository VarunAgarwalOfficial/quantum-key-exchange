"""The `qkx` command: demo, bob, alice, stats."""

from __future__ import annotations

import argparse
import os
import socket
import sys

from qkx import protocol
from qkx.channel import Eve, QuantumChannel
from qkx.session import Outcome, ProtocolError, Settings, party_rng, run_alice, run_bob, run_local
from qkx.transport import SocketPipe, TransportError

TABLE_WIDTH = 32
EXIT_OK, EXIT_ERROR, EXIT_ABORTED = 0, 1, 2


def _fraction(text: str) -> float:
    value = float(text)
    if not 0.0 <= value <= 1.0:
        raise argparse.ArgumentTypeError("must be between 0 and 1")
    return value


def _positive(text: str) -> int:
    value = int(text)
    if value < 1:
        raise argparse.ArgumentTypeError("must be at least 1")
    return value


def _row(label: str, cells: list[str]) -> str:
    return f"  {label:<12}{' '.join(cells)}"


def photon_table(alice: Outcome, bob: Outcome, eve: Eve | None) -> list[str]:
    """The first few photons, one column each."""
    width = min(TABLE_WIDTH, len(alice.bits))
    kept = set(alice.kept)
    rows = [
        _row("Alice bit", [str(bit) for bit in alice.bits[:width]]),
        _row("Alice basis", [basis.value for basis in alice.bases[:width]]),
    ]
    if eve is not None:
        rows.append(_row("Eve basis", [b.value if b else "." for b in eve.bases[:width]]))
    rows += [
        _row("Bob basis", [basis.value for basis in bob.bases[:width]]),
        _row("Bob bit", [str(bit) for bit in bob.bits[:width]]),
        _row("kept", ["*" if i in kept else "." for i in range(width)]),
    ]
    wrong = ["x" if i in kept and alice.bits[i] != bob.bits[i] else "." for i in range(width)]
    if "x" in wrong:
        rows.append(_row("error", wrong))
    return rows


def summary(out: Outcome, threshold: float) -> list[str]:
    """The steps after the photons, as either party saw them."""
    photons = len(out.bits)
    lines = [
        f"  photons      {photons} in {out.rounds} round{'s' if out.rounds != 1 else ''}",
        f"  sifted       {len(out.sifted)} bits ({len(out.sifted) / photons:.0%} of the photons)",
    ]
    if out.qber is not None:
        errors = round(out.qber * out.sample_size)
        lines += [
            f"  sample       {out.sample_size} bits revealed and thrown away",
            f"  error rate   {out.qber:.1%} ({errors} of {out.sample_size} differ), "
            f"limit {threshold:.0%}",
        ]
    if out.aborted_at == "estimation":
        lines.append(f"  verdict      ABORT: {out.reason}")
    elif out.aborted_at == "confirmation":
        lines += ["  verdict      error rate accepted", f"  key check    ABORT: {out.reason}"]
    else:
        lines += [
            "  verdict      error rate accepted",
            f"  key          {len(out.key)} bits, hashes match",
        ]
    return lines


def _shorten(text: str, limit: int = 64) -> str:
    if not text:
        return "(empty)"
    return text if len(text) <= limit else f"{text[:limit]}... ({len(text) // 2} bytes)"


def _text(data: bytes) -> str:
    return data.decode("utf-8", errors="replace")


def cmd_demo(args: argparse.Namespace) -> int:
    settings = Settings(sample_size=args.sample, threshold=args.threshold, photons=args.photons)
    message = args.message.encode("utf-8")
    alice, bob, eve = run_local(message, eve_fraction=args.eve, seed=args.seed, settings=settings)

    shown = min(TABLE_WIDTH, len(alice.bits))
    print(f"Alice wants to send {len(message)} bytes, so she needs {8 * len(message)} key bits.")
    print()
    print(f"1. Photons (first {shown} of {len(alice.bits)}). R and D are the two filters.")
    print("\n".join(photon_table(alice, bob, eve)))
    if eve is not None:
        print(f"  Eve intercepted {eve.intercepted} of {len(eve.bases)} photons.")
    print()
    print("2. Sifting, sampling and checking")
    print("\n".join(summary(alice, settings.threshold)))
    print()
    if not alice.delivered:
        print("3. Message")
        print("  not sent. No part of it left Alice.")
        return EXIT_ABORTED
    print("3. Message")
    print(f"  ciphertext   {_shorten(alice.ciphertext.hex())}")
    print(f"  Bob reads    {_text(bob.plaintext)}")
    return EXIT_OK


def cmd_bob(args: argparse.Namespace) -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as server:
        server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        server.bind((args.host, args.port))
        server.listen(1)
        print(f"Bob is listening on {args.host}:{server.getsockname()[1]}", flush=True)
        conn, peer = server.accept()
    with conn:
        conn.settimeout(args.timeout)
        print(f"Alice connected from {peer[0]}:{peer[1]}", flush=True)
        out = run_bob(SocketPipe(conn), threshold=args.threshold)
    print("\n".join(summary(out, args.threshold)))
    if not out.delivered:
        print("  message      none received")
        return EXIT_ABORTED
    print(f"  ciphertext   {_shorten(out.ciphertext.hex())}")
    print(f"  message      {_text(out.plaintext)}")
    return EXIT_OK


def cmd_alice(args: argparse.Namespace) -> int:
    settings = Settings(sample_size=args.sample, threshold=args.threshold, photons=args.photons)
    eve = Eve(args.eve, party_rng(None, "eve")) if args.eve > 0 else None
    with socket.create_connection((args.host, args.port), timeout=args.timeout) as conn:
        print(f"Alice connected to Bob at {args.host}:{args.port}", flush=True)
        out = run_alice(
            SocketPipe(conn),
            args.message.encode("utf-8"),
            channel=QuantumChannel(eve),
            settings=settings,
        )
    if eve is not None:
        print(f"  eavesdropper intercepted {eve.intercepted} of {len(eve.bases)} photons")
    print("\n".join(summary(out, settings.threshold)))
    if not out.delivered:
        print("  message      not sent")
        return EXIT_ABORTED
    print(f"  ciphertext   {_shorten(out.ciphertext.hex())}")
    print("  message      sent, Bob confirmed receipt")
    return EXIT_OK


def cmd_stats(args: argparse.Namespace) -> int:
    settings = Settings(sample_size=args.sample, threshold=args.threshold)
    message = bytes(args.bytes)
    bar_width = 30
    print(
        f"{args.trials} trials per row, {8 * args.bytes} bit key, "
        f"{args.sample} bit sample, limit {args.threshold:.0%}"
    )
    print("Eve    QBER     QBER    stopped by  stopped by")
    print("takes  expected mean    sample      key check   delivered  detected")
    for step in range(11):
        fraction = step / 10
        by_sample = by_check = delivered = 0
        total_qber = 0.0
        for trial in range(args.trials):
            seed = None if args.seed is None else args.seed * 1_000_003 + step * 10_007 + trial
            alice, bob, _ = run_local(message, eve_fraction=fraction, seed=seed, settings=settings)
            total_qber += alice.qber
            by_sample += alice.aborted_at == "estimation"
            by_check += alice.aborted_at == "confirmation"
            if alice.delivered:
                if bob.plaintext != message:
                    raise AssertionError("a wrong message was delivered")
                delivered += 1
        detected = (by_sample + by_check) / args.trials
        print(
            f"{fraction:4.0%}   {fraction / 4:6.1%}  {total_qber / args.trials:6.1%}"
            f"   {by_sample / args.trials:6.0%}      {by_check / args.trials:6.0%}"
            f"      {delivered / args.trials:6.0%}     {'#' * round(detected * bar_width)}"
            f" {detected:.0%}",
            flush=True,
        )
    return EXIT_OK


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="qkx",
        description="BB84 quantum key exchange between Alice and Bob, simulated classically. "
        "A teaching tool: it secures nothing.",
    )
    commands = parser.add_subparsers(dest="command", required=True)

    def add_protocol_options(sub: argparse.ArgumentParser, alice: bool = True) -> None:
        sub.add_argument(
            "--threshold",
            type=_fraction,
            default=protocol.DEFAULT_THRESHOLD,
            help="abort above this error rate (default %(default)s)",
        )
        if alice:
            sub.add_argument(
                "--sample",
                type=_positive,
                default=protocol.DEFAULT_SAMPLE_SIZE,
                help="sifted bits to reveal for the error estimate (default %(default)s)",
            )

    def add_alice_options(sub: argparse.ArgumentParser) -> None:
        sub.add_argument("message", help="the text to send")
        sub.add_argument(
            "--eve",
            type=_fraction,
            default=0.0,
            metavar="FRACTION",
            help="put an eavesdropper on the line who intercepts this fraction of photons",
        )
        sub.add_argument(
            "--photons",
            type=_positive,
            default=None,
            metavar="N",
            help="photons per round (default: chosen from the message length)",
        )

    def add_network_options(sub: argparse.ArgumentParser) -> None:
        sub.add_argument(
            "--host",
            default=os.environ.get("QKX_HOST", "127.0.0.1"),
            help="default: $QKX_HOST or 127.0.0.1",
        )
        sub.add_argument(
            "--port",
            type=int,
            default=os.environ.get("QKX_PORT", "5050"),
            help="default: $QKX_PORT or 5050",
        )
        sub.add_argument(
            "--timeout", type=float, default=30.0, help="seconds to wait for the other party"
        )

    demo = commands.add_parser("demo", help="run Alice and Bob here and show every step")
    add_alice_options(demo)
    add_protocol_options(demo)
    demo.add_argument("--seed", type=int, default=None, help="make the run repeatable")
    demo.set_defaults(run=cmd_demo)

    bob = commands.add_parser("bob", help="listen for Alice, receive one message, exit")
    add_network_options(bob)
    add_protocol_options(bob, alice=False)
    bob.set_defaults(run=cmd_bob)

    alice = commands.add_parser("alice", help="connect to Bob and send a message")
    add_alice_options(alice)
    add_network_options(alice)
    add_protocol_options(alice)
    alice.set_defaults(run=cmd_alice)

    stats = commands.add_parser("stats", help="how often an eavesdropper is caught")
    add_protocol_options(stats)
    stats.add_argument("--trials", type=_positive, default=200, help="runs per row")
    stats.add_argument("--bytes", type=_positive, default=16, help="message length in bytes")
    stats.add_argument("--seed", type=int, default=None, help="make the run repeatable")
    stats.set_defaults(run=cmd_stats)
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.run(args)
    except (TransportError, ProtocolError, OSError, ValueError) as error:
        print(f"qkx: {error}", file=sys.stderr)
        return EXIT_ERROR
    except KeyboardInterrupt:
        print("qkx: interrupted", file=sys.stderr)
        return 130


if __name__ == "__main__":
    sys.exit(main())
