//! The seed format's tests (decision 3, goal 3 plan step 2: HEX-SEED-1 and HEX-SEED-2; decision 5, goal 5
//! plan step 4: HEX-SEED-3): every request and answer reads back as written; version 1's bytes are the
//! ones recorded here, so a change to them fails (append-only); hostile bytes, at every truncation,
//! every bit flip and at random, are refused by type and never panic; and a create reads as one only
//! with the descriptors it says it carries.

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

fn creates() -> Vec<Create> {
    vec![
        Create {
            bundle: "/srv/bundles/box-1".to_owned(),
            dir: "/run/hex-node/0123456789abcdef-box-1".to_owned(),
            cgroup: Some("/sys/fs/cgroup/hex/0123456789abcdef-box-1".to_owned()),
            network: true,
        },
        Create {
            bundle: "/b".to_owned(),
            dir: "/d".to_owned(),
            cgroup: None,
            network: false,
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

/// HEX-SEED-3: a create reads back as the one written, through [`read_create`] and [`read_message`] with
/// the descriptors it carries; an exec reads through [`read_message`] as through [`read_request`]; and the
/// create's bytes are the ones recorded here, the exec's beside them unchanged.
#[test]
fn a_create_reads_back_as_written_and_its_bytes_are_recorded() {
    for want in creates() {
        let bytes = create(&want).unwrap();
        assert_eq!(read_create(&bytes, bytes.len()).unwrap(), want);
        assert_eq!(
            read_message(&bytes, bytes.len(), want.fds()).unwrap(),
            Message::Create(want.clone())
        );
    }
    let exec = request(&full()).unwrap();
    assert_eq!(
        read_message(&exec, exec.len(), REQUEST_FDS).unwrap(),
        Message::Exec(full())
    );
    assert_eq!(hex(&create(&creates()[0]).unwrap()), GOLDEN_CREATE);
    assert_eq!(
        creates()[0].fds(),
        CREATE_FDS + 1,
        "network adds the made-ahead netns"
    );
    assert_eq!(creates()[1].fds(), CREATE_FDS);
    let unable = answer(&Answer::Refused {
        failed: Failed::Unable,
        why: "clone3: ENOSYS".to_owned(),
    })
    .unwrap();
    assert_eq!(
        read_answer(&unable, unable.len(), 0).unwrap(),
        Answer::Refused {
            failed: Failed::Unable,
            why: "clone3: ENOSYS".to_owned()
        }
    );
}

/// HEX-SEED-3: a create or an exec that came with another count of descriptors than it says it carries,
/// a create that names no bundle or no state directory, one whose `network` byte is neither 0 nor 1,
/// and a message of an answer's kind are each refused by type; every truncation of a create is refused,
/// and its bit flips never panic.
#[test]
fn a_malformed_create_is_refused_naming_what_is_wrong() {
    let with_network = create(&creates()[0]).unwrap();
    for got in [0, 1, 2, 4] {
        assert!(
            matches!(
                read_message(&with_network, with_network.len(), got),
                Err(Fault::Descriptors { want: 3, .. })
            ),
            "{got} descriptors"
        );
    }
    let exec = request(&minimal()).unwrap();
    assert!(matches!(
        read_message(&exec, exec.len(), 2),
        Err(Fault::Descriptors {
            want: 4,
            got: 2,
            ..
        })
    ));
    for nameless in [
        Create {
            bundle: String::new(),
            ..creates()[1].clone()
        },
        Create {
            dir: String::new(),
            ..creates()[1].clone()
        },
    ] {
        let bytes = create(&nameless).unwrap();
        assert!(matches!(
            read_create(&bytes, bytes.len()),
            Err(Fault::Value { what: "create", .. })
        ));
    }
    let mut flag = create(&creates()[1]).unwrap();
    let last = flag.len() - 1;
    flag[last] = 2;
    assert!(matches!(
        read_create(&flag, flag.len()),
        Err(Fault::Value {
            what: "network",
            ..
        })
    ));
    let started = answer(&answers()[0]).unwrap();
    assert!(matches!(
        read_message(&started, started.len(), 1),
        Err(Fault::Kind { kind: 0x8001, .. })
    ));
    assert!(matches!(
        read_create(&exec, exec.len()),
        Err(Fault::Kind { kind: 0x0001, .. })
    ));
    assert!(matches!(
        read_create(&with_network, MAX_REQUEST + 1),
        Err(Fault::Oversized { .. })
    ));
    for n in 0..with_network.len() {
        assert!(
            read_create(&with_network[..n], n).is_err(),
            "{n} bytes read"
        );
    }
    for bit in 0..with_network.len() * 8 {
        let mut flipped = with_network.clone();
        flipped[bit / 8] ^= 1 << (bit % 8);
        let _ = read_message(&flipped, flipped.len(), 3);
    }
}

const GOLDEN_CREATE: &str = "4845585301000200120000002f7372762f62756e646c65732f626f782d31240000002f72756e2f6865782d6e6f64652f303132333435363738396162636465662d626f782d3101290000002f7379732f66732f6367726f75702f6865782f303132333435363738396162636465662d626f782d3101";

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
    /// HEX-SEED-1 and HEX-SEED-3: random bytes, as a request of either kind or as an answer, never panic.
    #[test]
    fn random_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256), fds in 0usize..5) {
        let _ = read_request(&bytes, bytes.len());
        let _ = read_message(&bytes, bytes.len(), fds);
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

/// HEX-RT-14 through a seed (CLAUDE.md §3): a seed refuses a program past its exec bound as that, so the
/// agent refuses it as `Invalid` "program", not as a failed start. Do: write a program refusal. Expect: it
/// reads back as written, its code is 5, appended after version 1's four (append-only), and the four keep
/// theirs.
#[test]
fn a_program_refusal_reads_back_with_a_code_of_its_own() {
    let want = Answer::Refused {
        failed: Failed::Program,
        why: "a program of 300000 bytes".to_owned(),
    };
    let bytes = answer(&want).unwrap();
    assert_eq!(read_answer(&bytes, bytes.len(), 0).unwrap(), want);
    assert_eq!(&bytes[HEADER_BYTES..HEADER_BYTES + 2], &5u16.to_le_bytes());
    for (failed, code) in [
        (Failed::Request, 1u16),
        (Failed::Start, 2),
        (Failed::Seed, 3),
        (Failed::Unable, 4),
        (Failed::Program, 5),
    ] {
        assert_eq!(failed.code(), code);
        assert_eq!(Failed::of(code).unwrap(), failed);
    }
}
