# What the simulation shows and what it does not

It shows the protocol as it runs: the bases, sifting, the error rate an
eavesdropper causes, the cost of error correction in revealed bits, and how
long a key survives at a given error rate. Those numbers come out of running
the steps, not from formulas.

It does not show physics. A photon here is two bits of state that can only be
read through `measure`. Between `qkx alice` and `qkx bob` that state travels
over TCP, and Bob's program is trusted to only measure it; a real photon cannot
be copied, a byte can. Eve and line noise are simulated inside Alice's program
on the photons' way out. There is no photon loss, no detector dark counts, no
multi-photon pulses (so no photon-number-splitting attack), and Eve is limited
to intercept-and-resend. The key length uses the asymptotic formula: it treats
the measured error rate as exact, with no finite-size correction, so it is
optimistic for short keys. The public channel is assumed authentic (Eve can
read it, not change it); nothing here authenticates it. The command line uses
the operating system's random numbers; the demo, the tables and the browser
view use a seeded generator so runs can be repeated.

