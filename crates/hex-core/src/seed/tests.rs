//! The seed format's tests (decision 3, goal 3 plan step 2: HEX-SEED-1 and HEX-SEED-2): every request
//! and answer reads back as written; version 1's bytes are the ones recorded here, so a change to them
//! fails (append-only); and hostile bytes, at every truncation, every bit flip and at random, are
//! refused by type and never panic.

use super::*;
use proptest::prelude::*;

fn full() -> Request {
    Request {
        init_pid: 4242,
        bundle: "/run/hex/oci/box-1".to_owned(),
        cgroup: Some("/sys/fs/cgroup/hex/box-1".to_owned()),
        args: vec!["/usr/bin/env".to_owned(), "x y".to_owned()],
        cwd: Some("/work".to_owned()),
        env: vec!["PATH=/usr/bin".to_owned(), "HOME=/root".to_owned()],
        user: Some((1000, Some(100))),
    }
}

fn minimal() -> Request {
    Request {
        init_pid: 1,
        bundle: "/b".to_owned(),
        args: vec!["true".to_owned()],
        ..Request::default()
    }
}

fn answers() -> Vec<Answer> {
    vec![
        Answer::Started { pid: 4243 },
        Answer::Refused {
            failed: Failed::Start,
            why: "EAGAIN: the container's pids bound".to_owned(),
        },
        Answer::Refused {
            failed: Failed::Request,
            why: String::new(),
        },
    ]
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// HEX-SEED-2: each request and answer reads back as the one written.
#[test]
fn every_message_reads_back_as_written() {
    for want in [full(), minimal()] {
        let bytes = request(&want).unwrap();
        assert_eq!(read_request(&bytes, bytes.len()).unwrap(), want);
    }
    for want in answers() {
        let bytes = answer(&want).unwrap();
        assert_eq!(read_answer(&bytes, bytes.len(), want.fds()).unwrap(), want);
    }
}

/// HEX-SEED-2, append-only: version 1's bytes for these messages are the ones recorded here. A seed
/// and an agent of different builds meet (a node spawns its seeds from the `hex` binary it is given),
/// so a change that alters any of them is a new version, never an edit of this one.
#[test]
fn version_1_messages_are_the_bytes_recorded() {
    let golden: [(&str, Vec<u8>); 4] = [
        (GOLDEN_FULL, request(&full()).unwrap()),
        (GOLDEN_MINIMAL, request(&minimal()).unwrap()),
        (GOLDEN_STARTED, answer(&answers()[0]).unwrap()),
        (GOLDEN_REFUSED, answer(&answers()[1]).unwrap()),
    ];
    for (want, got) in golden {
        assert_eq!(hex(&got), want);
    }
}

const GOLDEN_FULL: &str = "484558530100010092100000120000002f72756e2f6865782f6f63692f626f782d3101180000002f7379732f66732f6367726f75702f6865782f626f782d31020000000c0000002f7573722f62696e2f656e760300000078207901050000002f776f726b020000000d000000504154483d2f7573722f62696e0a000000484f4d453d2f726f6f7401e80300000164000000";
const GOLDEN_MINIMAL: &str =
    "484558530100010001000000020000002f6200010000000400000074727565000000000000";
const GOLDEN_STARTED: &str = "484558530100018093100000";
const GOLDEN_REFUSED: &str = "484558530100028002002200000045414741494e3a2074686520636f6e7461696e65722773207069647320626f756e64";

/// HEX-SEED-1: every truncation of a valid message is refused by type, never a panic.
#[test]
fn every_truncation_is_refused() {
    let bytes = request(&full()).unwrap();
    for n in 0..bytes.len() {
        assert!(read_request(&bytes[..n], n).is_err(), "{n} bytes read");
    }
    for want in answers() {
        let bytes = answer(&want).unwrap();
        for n in 0..bytes.len() {
            assert!(
                read_answer(&bytes[..n], n, want.fds()).is_err(),
                "{n} bytes of {want:?} read"
            );
        }
    }
}

/// HEX-SEED-1: every single bit flipped in a valid message reads as some message or is refused, and
/// never panics; bytes past a message's end are refused.
#[test]
fn bit_flips_never_panic_and_trailing_bytes_are_refused() {
    let bytes = request(&full()).unwrap();
    for bit in 0..bytes.len() * 8 {
        let mut flipped = bytes.clone();
        flipped[bit / 8] ^= 1 << (bit % 8);
        let _ = read_request(&flipped, flipped.len());
    }
    let mut long = bytes;
    long.push(0);
    assert!(matches!(
        read_request(&long, long.len()),
        Err(Fault::Trailing { bytes: 1, .. })
    ));
    let mut long = answer(&answers()[0]).unwrap();
    long.push(0);
    assert!(matches!(
        read_answer(&long, long.len(), 1),
        Err(Fault::Trailing { bytes: 1, .. })
    ));
}

/// HEX-SEED-1: a message that isn't a seed's, of another version or kind, without a program, past the
/// size bound, or an answer with the wrong count of descriptors or an unknown refusal code, is refused
/// by type.
#[test]
fn each_malformed_message_is_refused_naming_what_is_wrong() {
    let good = request(&minimal()).unwrap();
    let with = |at: usize, b: u8| {
        let mut m = good.clone();
        m[at] = b;
        m
    };
    let wire = with(3, b'W');
    assert!(matches!(
        read_request(&wire, wire.len()),
        Err(Fault::Value {
            what: "message",
            ..
        })
    ));
    let v2 = with(4, 2);
    assert!(matches!(
        read_request(&v2, v2.len()),
        Err(Fault::Version { got: 2, .. })
    ));
    let started = with(7, 0x80);
    assert!(matches!(
        read_request(&started, started.len()),
        Err(Fault::Kind { kind: 0x8001, .. })
    ));
    let none = request(&Request {
        args: Vec::new(),
        ..minimal()
    })
    .unwrap();
    assert!(matches!(
        read_request(&none, none.len()),
        Err(Fault::Value { what: "args", .. })
    ));
    assert!(matches!(
        read_request(&good, MAX_REQUEST + 1),
        Err(Fault::Oversized { .. })
    ));
    let huge = Request {
        env: vec!["X".repeat(MAX_REQUEST)],
        ..minimal()
    };
    assert!(matches!(request(&huge), Err(Fault::Oversized { .. })));
    let started = answer(&answers()[0]).unwrap();
    assert!(matches!(
        read_answer(&started, started.len(), 0),
        Err(Fault::Descriptors {
            want: 1,
            got: 0,
            ..
        })
    ));
    let mut code = answer(&answers()[1]).unwrap();
    code[8] = 99;
    assert!(matches!(
        read_answer(&code, code.len(), 0),
        Err(Fault::Value {
            what: "refusal",
            ..
        })
    ));
}

/// A refusal whose reason is past [`MAX_WHY`] is sent with the reason cut at a character's boundary,
/// so a seed can always refuse.
#[test]
fn a_long_reason_is_cut_to_fit_at_a_character() {
    let why = "é".repeat(MAX_WHY);
    let bytes = answer(&Answer::Refused {
        failed: Failed::Seed,
        why,
    })
    .unwrap();
    assert!(bytes.len() <= MAX_ANSWER);
    match read_answer(&bytes, bytes.len(), 0).unwrap() {
        Answer::Refused { why, .. } => {
            assert!(why.len() <= MAX_WHY && why.len() > MAX_WHY - 2);
            assert!(why.chars().all(|c| c == 'é'));
        }
        other => panic!("{other:?}"),
    }
}

proptest! {
    /// HEX-SEED-1: random bytes, as a request or as an answer, never panic.
    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256), fds in 0usize..3) {
        let _ = read_request(&bytes, bytes.len());
        let _ = read_answer(&bytes, bytes.len(), fds);
    }

    /// HEX-SEED-1: random bytes after a valid header and kind never panic either.
    #[test]
    fn random_bodies_never_panic(body in proptest::collection::vec(any::<u8>(), 0..256)) {
        let mut bytes = request(&minimal()).unwrap();
        bytes.truncate(HEADER_BYTES);
        bytes.extend(body);
        let _ = read_request(&bytes, bytes.len());
    }
}
