# quantum-key-exchange

Two programs, Alice and Bob, agree on a secret key using the BB84 quantum key
distribution protocol, check whether anyone listened, and then send a message
under that key as a one-time pad. The photons are simulated, so this teaches
the protocol and secures nothing.

## The idea

Alice sends Bob a stream of "photons". Each one carries a random bit, encoded
with one of two filters picked at random: rectilinear (R) or diagonal (D). Bob
does not know which filter Alice used, so he also picks one at random for each
photon. If he picks the same filter he reads her bit; if he picks the other
one he gets a coin flip. Afterwards they tell each other, in public, which
filters they used (not what they read) and keep only the positions where the
filters matched. Those bits are the key. The nice part of the original was
that you could watch this happen between two terminals, and that the whole
quantum rule came down to one `if` statement. This rebuild keeps both and adds
the piece that makes BB84 worth doing: an eavesdropper, and the check that
catches her.

## How it works

| Step | What happens | Channel |
| --- | --- | --- |
| 1. Prepare | Alice picks a random bit and a random basis (R or D) per photon and sends the photons. | quantum |
| 2. Measure | Bob measures each photon in a random basis of his own. | |
| 3. Sift | Bob announces his bases, then Alice hers. They keep positions where the bases match, about half. | public |
| 4. Estimate | Alice names a random sample of the sifted bits. Both reveal those bits, count the differences (the error rate, QBER), and throw the sample away. Above the limit, both stop. | public |
| 5. Confirm | Alice sends a salted SHA-256 of the remaining key. If Bob's key hashes differently, both stop. | public |
| 6. Encrypt | Alice XORs the message bytes with the key and sends the result. Bob XORs again. | public |

**Why the wrong basis gives a coin flip.** A photon polarised for one filter
has no definite answer for the other one: it passes or not with probability
one half, and afterwards it is polarised the way the filter that measured it
says. That is all of the physics in this program. In the code it is
`Photon.measure` in `src/qkx/qubit.py`: same basis returns the bit, different
basis replaces the bit with a random one and changes the photon's basis.

**Why an eavesdropper shows up as 25% errors.** Eve cannot copy a photon, so
she has to measure it and pass it on. She does not know Alice's basis either,
so she guesses wrong half the time. A wrong guess leaves the photon in Eve's
basis, and when Bob then measures in Alice's basis (the only cases that
survive sifting) he gets a coin flip, which is wrong half the time. One half
times one half: a quarter of the sifted bits differ. If Eve intercepts only a
fraction f of the photons, the error rate is f/4.

**Catching her.** Each sampled bit that Eve intercepted exposes her with
probability 1/4, so she survives n such bits with probability (3/4)^n. For
n = 256 that is about 10^-32. The program does not ask for zero errors; it
stops when the sampled error rate is above 11%. That number is the error rate
usually quoted as the most BB84 can tolerate in theory, and here it simply
serves as a limit well below 25%.

**What the sample misses.** An Eve who intercepts only a few photons causes an
error rate below the limit and passes step 4. Her errors are still in the key,
so step 5 compares hashes, and a single differing bit stops the exchange
before any message is sent. The outcome is always one of: stopped at step 4,
stopped at step 5, or the exact message delivered. `qkx stats` shows how the
three split as Eve intercepts more.

**The one-time pad.** The key has exactly as many bits as the message, each
key bit is used once, and the sampled bits are never part of the key. The
number of photons is chosen from the message length: 8 bits per byte plus the
sample, doubled because sifting keeps half, plus a margin. If too few bases
match, another round of photons is sent.

### What is simplified

- The photons are simulated. In `qkx demo` they are Python objects whose state
  can only be reached by measuring. Between `qkx alice` and `qkx bob` the
  hidden state of each photon travels over the TCP connection, and Bob's
  program is trusted to touch it only through `measure`. A real photon needs
  no such trust. With `qkx alice --eve`, the eavesdropper is simulated inside
  Alice's process, on the photons' way out.
- The channel has no noise, so every error is Eve's. Real links have noise,
  and real BB84 follows the estimate with error correction and privacy
  amplification. Without error correction, any leftover error ends the
  exchange at step 5 instead of being repaired. Without privacy amplification,
  an Eve who intercepts a few photons and happens to cause no errors keeps
  what she learned about those bits, and the published hash also says a little
  about the key.
- The public channel is assumed authentic: Eve can read it but not change it.
  Nothing here enforces that.

## Run

Requires Python 3.12 or newer and [uv](https://docs.astral.sh/uv/). No
runtime dependencies.

    uv sync
    uv run qkx demo "meet me at noon"
    uv run qkx demo "meet me at noon" --eve 1.0
    uv run qkx demo "meet me at noon" --eve 0.2 --seed 7
    uv run qkx stats

`demo` runs both parties in one process and prints each step. `--eve F` puts
an eavesdropper on the line who intercepts a fraction F of the photons.
`--seed N` makes a run repeatable. `--photons N` sets the photons per round.
The exit status is 0 if the message was delivered and 2 if the exchange was
stopped.

Two terminals:

    uv run qkx bob                       # terminal 1: listens, prints the message, exits
    uv run qkx alice "meet me at noon"   # terminal 2

Both take `--host` and `--port`, or read `QKX_HOST` and `QKX_PORT` from the
environment (default 127.0.0.1:5050). See `.env.example`; Python does not load
that file by itself, so either export the variables or use
`uv run --env-file .env qkx bob`.

## Layout

    src/qkx/qubit.py      a photon and the measurement rule
    src/qkx/channel.py    the quantum channel and Eve
    src/qkx/protocol.py   each BB84 step as a pure function
    src/qkx/transport.py  length-prefixed JSON frames over a socket or in memory
    src/qkx/session.py    Alice and Bob as functions over a message pipe
    src/qkx/cli.py        the qkx command

## Test

    uv run pytest
    uvx ruff check
    uvx ruff format --check

## Credit

Based on a university course project (CSE3501) by Varun Agarwal, 2022.
