//! The messages Alice and Bob exchange, and how they are framed on a stream.
//!
//! A frame is one tag byte, a little-endian u32 payload length, and the
//! payload. Bit strings travel packed, eight to a byte, lowest bit first.
//! Every frame is written with a single `write_all`, so that small frames in
//! the error correction dialogue are not split across packets.

use std::fmt;
use std::io::{self, Read, Write};

use crate::cascade::Query;
use crate::protocol::Settings;

/// Frames larger than this are refused rather than allocated.
pub const MAX_FRAME: usize = 64 << 20;

/// One message of the conversation. See [`crate::session`] for the order.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// Alice announces the protocol settings.
    Hello(Settings),
    /// The simulated quantum channel: one byte of hidden state per photon.
    Photons(Vec<u8>),
    /// The bases one side used, one bit per photon (1 = diagonal).
    Bases(Vec<bool>),
    /// Alice names sample positions of the sifted key and reveals her bits there.
    Sample {
        positions: Vec<u32>,
        bits: Vec<bool>,
    },
    /// Bob's bits at the sample positions.
    Bits(Vec<bool>),
    /// Alice fixes the shuffles for error correction.
    Correct { seed: u64 },
    /// Bob asks for parities.
    Queries(Vec<Query>),
    /// Alice answers them, in order.
    Parities(Vec<bool>),
    /// Bob has finished error correction.
    Corrected { corrected: u32, leaked: u32 },
    /// Alice's verification hash: Toeplitz seed and the 64-bit tag of her key.
    Verify { seed: Vec<bool>, tag: u64 },
    /// Privacy amplification: Toeplitz seed and the length of the final key.
    Amplify { seed: Vec<bool>, len: u32 },
    /// The message, one-time padded with the final key.
    Cipher(Vec<u8>),
    /// Yes or no: Bob's answer to `Verify`, and his receipt for `Cipher`.
    Verdict(bool),
    /// One side stops, saying why.
    Abort(String),
}

/// What can go wrong between the two programs.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    /// The other side sent something that does not fit the protocol.
    Protocol(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) if e.kind() == io::ErrorKind::UnexpectedEof => {
                write!(f, "the other side closed the connection")
            }
            Error::Io(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                write!(f, "the other side stopped answering")
            }
            Error::Io(e) => write!(f, "{e}"),
            Error::Protocol(text) => write!(f, "protocol error: {text}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub fn protocol_error(text: impl Into<String>) -> Error {
    Error::Protocol(text.into())
}

/// Writes one message as one frame.
pub fn send(stream: &mut impl Write, message: &Message) -> Result<(), Error> {
    let mut payload = Vec::new();
    let tag = encode(message, &mut payload);
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(tag);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    stream.write_all(&frame)?;
    stream.flush()?;
    Ok(())
}

/// Reads one frame and decodes it.
pub fn receive(stream: &mut impl Read) -> Result<Message, Error> {
    let mut head = [0u8; 5];
    stream.read_exact(&mut head)?;
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > MAX_FRAME {
        return Err(protocol_error(format!("frame of {len} bytes is too large")));
    }
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload)?;
    decode(head[0], &payload)
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_bits(out: &mut Vec<u8>, bits: &[bool]) {
    put_u32(out, bits.len() as u32);
    for chunk in bits.chunks(8) {
        out.push(
            chunk
                .iter()
                .enumerate()
                .fold(0u8, |acc, (i, &b)| acc | (u8::from(b) << i)),
        );
    }
}

fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    put_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
}

fn encode(message: &Message, out: &mut Vec<u8>) -> u8 {
    match message {
        Message::Hello(s) => {
            out.extend_from_slice(&s.sample_fraction.to_le_bytes());
            out.extend_from_slice(&s.threshold.to_le_bytes());
            1
        }
        Message::Photons(bytes) => {
            put_bytes(out, bytes);
            2
        }
        Message::Bases(bits) => {
            put_bits(out, bits);
            3
        }
        Message::Sample { positions, bits } => {
            put_u32(out, positions.len() as u32);
            for &p in positions {
                put_u32(out, p);
            }
            put_bits(out, bits);
            4
        }
        Message::Bits(bits) => {
            put_bits(out, bits);
            5
        }
        Message::Correct { seed } => {
            out.extend_from_slice(&seed.to_le_bytes());
            6
        }
        Message::Queries(queries) => {
            put_u32(out, queries.len() as u32);
            for q in queries {
                out.push(q.pass);
                put_u32(out, q.start);
                put_u32(out, q.end);
            }
            7
        }
        Message::Parities(bits) => {
            put_bits(out, bits);
            8
        }
        Message::Corrected { corrected, leaked } => {
            put_u32(out, *corrected);
            put_u32(out, *leaked);
            9
        }
        Message::Verify { seed, tag } => {
            put_bits(out, seed);
            out.extend_from_slice(&tag.to_le_bytes());
            10
        }
        Message::Amplify { seed, len } => {
            put_bits(out, seed);
            put_u32(out, *len);
            11
        }
        Message::Cipher(bytes) => {
            put_bytes(out, bytes);
            12
        }
        Message::Verdict(yes) => {
            out.push(u8::from(*yes));
            13
        }
        Message::Abort(reason) => {
            put_bytes(out, reason.as_bytes());
            14
        }
    }
}

/// Reads fields from a payload, failing cleanly when it runs short.
struct Reader<'a> {
    data: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if self.data.len() < n {
            return Err(protocol_error("message ends early"));
        }
        let (head, rest) = self.data.split_at(n);
        self.data = rest;
        Ok(head)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> Result<f64, Error> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn bytes(&mut self) -> Result<Vec<u8>, Error> {
        let n = self.u32()? as usize;
        Ok(self.take(n)?.to_vec())
    }

    fn bits(&mut self) -> Result<Vec<bool>, Error> {
        let n = self.u32()? as usize;
        let packed = self.take(n.div_ceil(8))?;
        Ok((0..n).map(|i| packed[i / 8] >> (i % 8) & 1 == 1).collect())
    }

    /// A count of fixed-size items, checked against what is left.
    fn count(&mut self, item: usize) -> Result<usize, Error> {
        let n = self.u32()? as usize;
        if n.saturating_mul(item) > self.data.len() {
            return Err(protocol_error("message ends early"));
        }
        Ok(n)
    }
}

fn decode(tag: u8, payload: &[u8]) -> Result<Message, Error> {
    let mut r = Reader { data: payload };
    let message = match tag {
        1 => Message::Hello(Settings {
            sample_fraction: r.f64()?,
            threshold: r.f64()?,
        }),
        2 => Message::Photons(r.bytes()?),
        3 => Message::Bases(r.bits()?),
        4 => {
            let n = r.count(4)?;
            let positions = (0..n).map(|_| r.u32()).collect::<Result<_, _>>()?;
            Message::Sample {
                positions,
                bits: r.bits()?,
            }
        }
        5 => Message::Bits(r.bits()?),
        6 => Message::Correct { seed: r.u64()? },
        7 => {
            let n = r.count(9)?;
            let mut queries = Vec::with_capacity(n);
            for _ in 0..n {
                queries.push(Query {
                    pass: r.u8()?,
                    start: r.u32()?,
                    end: r.u32()?,
                });
            }
            Message::Queries(queries)
        }
        8 => Message::Parities(r.bits()?),
        9 => Message::Corrected {
            corrected: r.u32()?,
            leaked: r.u32()?,
        },
        10 => Message::Verify {
            seed: r.bits()?,
            tag: r.u64()?,
        },
        11 => Message::Amplify {
            seed: r.bits()?,
            len: r.u32()?,
        },
        12 => Message::Cipher(r.bytes()?),
        13 => Message::Verdict(r.u8()? != 0),
        14 => Message::Abort(String::from_utf8_lossy(&r.bytes()?).into_owned()),
        other => return Err(protocol_error(format!("unknown message type {other}"))),
    };
    if !r.data.is_empty() {
        return Err(protocol_error("message has trailing bytes"));
    }
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_garbage_is_refused() {
        let messages = [
            Message::Hello(Settings::default()),
            Message::Bases(vec![
                true, false, true, true, false, false, true, false, true,
            ]),
            Message::Sample {
                positions: vec![3, 70_000],
                bits: vec![true, false],
            },
            Message::Queries(vec![Query {
                pass: 2,
                start: 5,
                end: 9,
            }]),
            Message::Abort("too many errors".into()),
        ];
        let mut stream = Vec::new();
        for m in &messages {
            send(&mut stream, m).unwrap();
        }
        let mut reader = stream.as_slice();
        for m in &messages {
            assert_eq!(&receive(&mut reader).unwrap(), m);
        }
        // A claimed count far beyond the payload, an unknown tag, a cut frame.
        let lying = [7u8, 4, 0, 0, 0, 0xff, 0xff, 0xff, 0x0f];
        assert!(matches!(receive(&mut &lying[..]), Err(Error::Protocol(_))));
        assert!(matches!(
            receive(&mut &[99u8, 0, 0, 0, 0][..]),
            Err(Error::Protocol(_))
        ));
        assert!(matches!(receive(&mut &stream[..7]), Err(Error::Io(_))));
    }
}
