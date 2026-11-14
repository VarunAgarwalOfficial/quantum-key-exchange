//! Alice and Bob as two threads over a real TCP connection.

use std::net::{TcpListener, TcpStream};
use std::thread;

use qkx::channel::{Channel, Eve, Strategy};
use qkx::protocol::Settings;
use qkx::rng::Xoshiro;
use qkx::session::{self, AliceConfig, Party};
use qkx::simulate::Verdict;

fn exchange(channel: Channel, message: &[u8]) -> (Party, Party) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let bob = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_nodelay(true).unwrap();
        session::bob(&mut stream, &mut Xoshiro::new(2)).unwrap()
    });
    let mut stream = TcpStream::connect(address).unwrap();
    stream.set_nodelay(true).unwrap();
    let config = AliceConfig {
        photons: 20_000,
        settings: Settings::default(),
        channel,
    };
    let alice = session::alice(&mut stream, &config, message, &mut Xoshiro::new(1)).unwrap();
    (alice, bob.join().unwrap())
}

#[test]
fn noisy_line_still_gives_both_the_same_key_and_the_message() {
    let channel = Channel {
        noise: 0.03,
        eve: None,
    };
    let (alice, bob) = exchange(channel, "meet me at noon".as_bytes());
    assert_eq!(alice.verdict, Verdict::Key);
    assert!(bob.correction.corrected > 50, "Cascade had work to do");
    assert_eq!(alice.key, bob.key);
    assert_eq!(alice.final_len, bob.final_len);
    assert!(alice.final_len > 3000);
    assert_eq!(bob.message.as_deref(), Some("meet me at noon".as_bytes()));
}

#[test]
fn eavesdropper_is_caught_before_any_message_leaves() {
    let channel = Channel {
        noise: 0.0,
        eve: Some(Eve {
            strategy: Strategy::Random,
            fraction: 1.0,
        }),
    };
    let (alice, bob) = exchange(channel, b"secret");
    // Intercept and resend in a random basis: a quarter of the sifted bits are wrong.
    assert!(
        (0.22..0.28).contains(&alice.estimate.qber()),
        "{}",
        alice.estimate.qber()
    );
    assert_eq!(alice.estimate, bob.estimate);
    assert_eq!(bob.verdict, Verdict::TooManyErrors);
    assert_eq!((bob.cipher, bob.message), (None, None));
}
