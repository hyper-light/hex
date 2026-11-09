//! The wire format's tests (decision 3, HEX-SOCK-1 and HEX-SOCK-2): every request and answer reads
//! back as written; a message of another wire is refused before its body is read, naming both wires, and a
//! hello's head names its sender's build whatever the wire; the wire's identity is its definitions' digest,
//! line endings aside; and hostile bytes, at every truncation, every bit flip and at random, are refused by
//! type and never panic.

use super::*;
use hex_api::{Bounds, BoxOwner, BoxSpec, IoRates, Mount, Network, OciSpec, Program, Rootfs, User};
use proptest::prelude::*;

fn id(s: &str) -> BoxId {
    BoxId::new(s).unwrap()
}

fn bounds() -> Bounds {
    Bounds {
        cpu_weight: Some(100),
        cpu_max_milli: None,
        memory_max: Some(64 << 20),
        memory_high: None,
        io_weight: None,
        io_latency_us: Some(500),
        io_max: Some(IoRates {
            read_bps: Some(1 << 20),
            write_bps: None,
            read_iops: None,
            write_iops: Some(100),
        }),
        pids_max: Some(8),
    }
}

fn program() -> Program {
    Program {
        args: vec!["/usr/bin/true".to_owned(), "x y".to_owned()],
        env: vec![("PATH".to_owned(), "/usr/bin:/bin".to_owned())],
        cwd: Some("/".to_owned()),
        user: Some(User {
            uid: 1000,
            gid: 100,
            groups: vec![10, 20],
        }),
    }
}

fn oci_spec() -> BoxSpec {
    BoxSpec {
        id: id("oci-1"),
        bounds: Bounds::default(),
        class: ClassSpec::Oci(Box::new(OciSpec {
            rootfs: Rootfs::Layers(vec![PathBuf::from("/l/base"), PathBuf::from("/l/app")]),
            init: Program::of(&["sleep", "infinity"]),
            mounts: vec![Mount {
                source: PathBuf::from("/data"),
                destination: "/mnt".to_owned(),
                read_only: true,
            }],
            hostname: Some("box".to_owned()),
            network: Network::Private,
            hosts: vec![("db".to_owned(), "10.0.0.2".to_owned())],
        })),
        owner: None,
    }
}

fn info() -> BoxInfo {
    BoxInfo {
        membership: None,
        id: id("b-1"),
        class: Class::Oci,
        state: BoxState::Running,
        bounds: bounds(),
        adopted: true,
        exit: Some(Exit::Signal(9)),
        abandoned: 2,
    }
}

/// One of each request, with the descriptors each carries.
fn requests() -> Vec<(u64, Request, usize)> {
    let b = id("b-1");
    vec![
        (1, Request::Hello { build: build() }, 0),
        (
            2,
            Request::Call(Call::Create(Box::new(BoxSpec {
                id: b.clone(),
                bounds: bounds(),
                class: ClassSpec::Bundle(PathBuf::from("/run/bundle-sleep")),
                owner: None,
            }))),
            0,
        ),
        (3, Request::Call(Call::Create(Box::new(oci_spec()))), 0),
        (
            4,
            Request::Call(Call::Create(Box::new(BoxSpec {
                id: id("p"),
                bounds: Bounds::default(),
                class: ClassSpec::Process,
                owner: None,
            }))),
            0,
        ),
        (5, Request::Call(Call::Start(b.clone())), 0),
        (
            6,
            Request::Call(Call::Exec {
                id: b.clone(),
                program: program(),
                streams: Streams {
                    stdin: true,
                    stdout: true,
                    stderr: false,
                },
            }),
            2,
        ),
        (
            7,
            Request::Call(Call::Signal {
                id: b.clone(),
                signal: Signal::TERM,
            }),
            0,
        ),
        (8, Request::Call(Call::Inspect(b.clone())), 0),
        (9, Request::Call(Call::Usage(b.clone())), 0),
        (10, Request::Call(Call::List), 0),
        (11, Request::Call(Call::Delete(b)), 0),
        (6, Request::Release, 0),
        (12, Request::Call(Call::Netns), 0),
        (13, Request::Call(Call::Kill { exec: 6 }), 0),
        (14, Request::Taken, 0),
        (
            14,
            Request::Call(Call::ExecStreams {
                id: id("b-1"),
                program: program(),
                streams: Streams {
                    stdin: true,
                    stdout: false,
                    stderr: false,
                },
                ask: Streams {
                    stdin: false,
                    stdout: true,
                    stderr: true,
                },
            }),
            1,
        ),
    ]
}

/// The node agent's network namespace counts, each field a different value: makes failed at every step, the pool
/// refused for both capabilities, the latest reason kept at two steps, one with an errno and one without.
fn counts() -> netns::Counts {
    netns::Counts {
        made_ahead: 1,
        taken: 2,
        made_on_path: 3,
        discarded: 4,
        failed: u64::MAX,
        failed_by: netns::FailedBy {
            home: 1,
            unshare: 2,
            open: 3,
            loopback: 4,
            back: 5,
        },
        refused: netns::Missing::of(false, false),
        refused_on_path: 6,
        last_failed: netns::LastFailed {
            unshare: Some(netns::Reason::new(Some(1), "EPERM")),
            loopback: Some(netns::Reason::new(None, "not an ack")),
            ..netns::LastFailed::default()
        },
    }
}

fn refusals() -> Vec<Refusal> {
    vec![
        Refusal::Invalid {
            what: "cpu_weight",
            why: "0 is outside 1..=10000".to_owned(),
        },
        Refusal::NoSuchBox(id("gone")),
        Refusal::Exists(id("here")),
        Refusal::NotRunning(id("b-1"), "start"),
        Refusal::Platform {
            what: "the node agent's socket",
            platform: "macos",
            why: "no SOCK_SEQPACKET".to_owned(),
        },
        Refusal::Full(4096),
        Refusal::Exhausted {
            what: "requests in flight on one connection",
            bound: 64,
        },
        Refusal::Os {
            op: "starting a program in a box",
            why: "ENOENT".to_owned(),
        },
        Refusal::Broken("a lock was poisoned".to_owned()),
    ]
}

fn answers() -> Vec<(u64, Answer)> {
    let mut out = vec![
        (
            1,
            Answer::Hello(Hello {
                build: build(),
                classes: vec![Class::Process, Class::Oci],
                limits: Limits {
                    connections: 64,
                    message_bytes: MAX_MESSAGE,
                    in_flight: 1,
                    programs: 1,
                },
                budgets: Budgets {
                    in_flight: 64,
                    programs: 1024,
                },
            }),
        ),
        (2, Answer::Info(info())),
        (3, Answer::Done),
        (6, Answer::Started { pid: 4242 }),
        (
            9,
            Answer::Usage(Usage {
                memory_bytes: 1,
                memory_peak_bytes: 2,
                oom_kills: 3,
                pids: 4,
                pids_refused: 5,
                cpu_usec: 6,
                io_read_bytes: 7,
                io_write_bytes: u64::MAX,
            }),
        ),
        (
            10,
            Answer::Boxes {
                infos: vec![
                    info(),
                    BoxInfo {
                        membership: None,
                        id: id("p"),
                        class: Class::Process,
                        state: BoxState::Empty,
                        bounds: Bounds::default(),
                        adopted: false,
                        exit: None,
                        abandoned: 0,
                    },
                ],
                more: true,
            },
        ),
        (
            10,
            Answer::Boxes {
                infos: Vec::new(),
                more: false,
            },
        ),
        (12, Answer::Netns(counts())),
        (
            14,
            Answer::Given {
                pid: 4243,
                given: Streams::default(),
            },
        ),
    ];
    out.extend(refusals().into_iter().map(|r| (11, Answer::Refused(r))));
    for exit in [
        Exit::Code(0),
        Exit::Code(-1),
        Exit::Signal(9),
        Exit::Unknown,
    ] {
        out.push((6, Answer::Exited(exit)));
    }
    out
}

fn received(bytes: &[u8], descriptors: usize) -> Received<'_> {
    Received {
        bytes,
        size: bytes.len(),
        descriptors,
        descriptors_cut: false,
    }
}

/// HEX-SOCK-2: each request reads back as the one written, with its header.
#[test]
fn every_request_reads_back_as_written() {
    for (n, req, fds) in requests() {
        let bytes = request(n, &req, MAX_MESSAGE).unwrap();
        let (h, back) = read_request(&received(&bytes, fds), MAX_MESSAGE).unwrap();
        assert_eq!((h.id, &back), (n, &req));
    }
}

/// HEX-SOCK-2: each answer reads back as the one written.
#[test]
fn every_answer_reads_back_as_written() {
    for (n, ans) in answers() {
        let bytes = answer(n, &ans, MAX_MESSAGE).unwrap();
        let (h, back) = read_answer(&received(&bytes, 0), MAX_MESSAGE).unwrap();
        assert_eq!((h.id, &back), (n, &ans));
    }
}

/// HEX-SOCK-2: a given answer carries a descriptor for each stream it names, and is refused, naming the count,
/// with any other.
#[test]
fn a_given_answer_carries_a_descriptor_for_each_stream_it_names() {
    let given = Answer::Given {
        pid: 4243,
        given: Streams {
            stdin: false,
            stdout: true,
            stderr: true,
        },
    };
    let bytes = answer(14, &given, MAX_MESSAGE).unwrap();
    assert_eq!(
        read_answer(&received(&bytes, 2), MAX_MESSAGE).map(|(_, a)| a),
        Ok(given.clone())
    );
    for fds in [0, 1, 3] {
        assert_eq!(
            read_answer(&received(&bytes, fds), MAX_MESSAGE),
            Err(Fault::Descriptors {
                what: "a given answer",
                want: 2,
                got: fds
            })
        );
    }
}

/// Native plan step 8, guest stdio (HEX-GUEST-41): a client's word that it took a given answer's ends, by its exec's
/// id, is a header alone, kind 0x000e beside the given answer's; read back as itself; and it carries no descriptor,
/// one that comes with it refused, naming the count.
#[test]
fn a_word_that_a_given_answers_ends_were_taken_is_a_header_alone() {
    let taken = request(14, &Request::Taken, MAX_MESSAGE).unwrap();
    assert_eq!(taken, header(0x000e, 14));
    assert_eq!(
        read_request(&received(&taken, 0), MAX_MESSAGE),
        Ok((
            Header {
                kind: 0x000e,
                id: 14
            },
            Request::Taken
        ))
    );
    assert_eq!(
        read_request(&received(&taken, 1), MAX_MESSAGE).map_err(|bad| bad.fault),
        Err(Fault::Descriptors {
            what: "a taken",
            want: 0,
            got: 1
        })
    );
}

/// Every message's header: its magic, this side's wire identity, its kind and its id.
fn header(kind: u16, id: u64) -> Vec<u8> {
    let mut b = MAGIC.to_vec();
    b.extend_from_slice(&IDENTITY.to_le_bytes());
    b.extend_from_slice(&kind.to_le_bytes());
    b.extend_from_slice(&id.to_le_bytes());
    b
}

/// `bytes` as a message of the wire whose identity is `other`.
fn of_wire(mut bytes: Vec<u8>, other: u64) -> Vec<u8> {
    bytes[4..12].copy_from_slice(&other.to_le_bytes());
    bytes
}

/// One wire (the lead's word of 2026-11-09): a message's header is its magic, its sender's wire identity, its kind
/// and its id, in that order and at those bytes, and a hello's body begins with its sender's build. Do: write a hello
/// and its answer. Expect: each starts with the header, then the build as a string, the layout every build keeps.
#[test]
fn a_hellos_head_is_the_header_then_its_senders_build() {
    let b = build();
    let mut built = u32::try_from(b.len()).unwrap().to_le_bytes().to_vec();
    built.extend_from_slice(b.as_bytes());
    let hello = request(5, &Request::Hello { build: b.clone() }, MAX_MESSAGE).unwrap();
    assert_eq!(hello.len(), HEADER_BYTES + built.len());
    assert_eq!(&hello[..HEADER_BYTES], &header(0x0001, 5)[..]);
    assert_eq!(&hello[HEADER_BYTES..], &built[..]);
    let answered = answer(5, &answers()[0].1, MAX_MESSAGE).unwrap();
    assert_eq!(&answered[..HEADER_BYTES], &header(0x8001, 5)[..]);
    assert_eq!(
        &answered[HEADER_BYTES..HEADER_BYTES + built.len()],
        &built[..]
    );
}

/// One wire (the lead's word of 2026-11-09): a message of another wire is refused before its body is read, naming
/// both wires, and a hello's or a hello's answer's sender's build with them. Do: read a hello, a hello's answer, an
/// inspect and a done, each stamped with another identity. Expect: each refused as of another wire, naming that
/// identity; the hello's and the answer's naming their sender's build too; and as a refusal, `Invalid` of "wire"
/// naming both identities and both builds.
#[test]
fn a_message_of_another_wire_is_refused_naming_both_wires() {
    let other = IDENTITY ^ 1;
    let theirs = "hex 0.0.0 (another commit)".to_owned();
    let hello = of_wire(
        request(
            5,
            &Request::Hello {
                build: theirs.clone(),
            },
            MAX_MESSAGE,
        )
        .unwrap(),
        other,
    );
    let got = read_request(&received(&hello, 0), MAX_MESSAGE).unwrap_err();
    assert_eq!(
        (got.id, &got.fault),
        (
            5,
            &Fault::Wire {
                theirs: other,
                build: Some(theirs.clone())
            }
        )
    );
    let answered = of_wire(
        answer(
            5,
            &Answer::Hello(Hello {
                build: theirs.clone(),
                ..match &answers()[0].1 {
                    Answer::Hello(h) => h.clone(),
                    other => panic!("{other:?}"),
                }
            }),
            MAX_MESSAGE,
        )
        .unwrap(),
        other,
    );
    let fault = read_answer(&received(&answered, 0), MAX_MESSAGE).unwrap_err();
    assert_eq!(
        fault,
        Fault::Wire {
            theirs: other,
            build: Some(theirs.clone())
        }
    );
    let refusal = Refusal::from(fault);
    let Refusal::Invalid { what: "wire", why } = &refusal else {
        panic!("{refusal:?}")
    };
    for named in [
        format!("{other:016x}"),
        format!("{IDENTITY:016x}"),
        theirs.clone(),
        build(),
    ] {
        assert!(why.contains(&named), "{why} names no {named}");
    }
    let inspect = of_wire(request(8, &requests()[7].1, MAX_MESSAGE).unwrap(), other);
    assert_eq!(
        read_request(&received(&inspect, 0), MAX_MESSAGE)
            .unwrap_err()
            .fault,
        Fault::Wire {
            theirs: other,
            build: None
        }
    );
    let done = of_wire(answer(3, &Answer::Done, MAX_MESSAGE).unwrap(), other);
    assert_eq!(
        read_answer(&received(&done, 0), MAX_MESSAGE),
        Err(Fault::Wire {
            theirs: other,
            build: None
        })
    );
}

/// One wire (the lead's word of 2026-11-09): the identity is FNV-1a of the wire definitions' source, this module's
/// and the stream module's, with no number to bump, and a checkout's line endings don't split it. Do: digest the two
/// files as read now, then with every line ended `\r\n`. Expect: both [`IDENTITY`]; and a source with one byte more
/// another identity.
#[test]
fn the_wire_identity_is_its_definitions_digest_line_endings_aside() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let wire = std::fs::read(dir.join("wire.rs")).unwrap();
    let stream = std::fs::read(dir.join("stream.rs")).unwrap();
    assert_eq!(identity(&[&wire, &stream]), IDENTITY);
    let crlf = |bytes: &[u8]| -> Vec<u8> {
        let mut out = Vec::new();
        for &b in bytes.iter().filter(|&&b| b != b'\r') {
            if b == b'\n' {
                out.push(b'\r');
            }
            out.push(b);
        }
        out
    };
    assert_eq!(identity(&[&crlf(&wire), &crlf(&stream)]), IDENTITY);
    let mut more = wire.clone();
    more.push(b' ');
    assert_ne!(identity(&[&more, &stream]), IDENTITY);
}

/// HEX-SOCK-2: a list longer than one message carries is split into messages within the bound, each
/// but the last marked `more`, and the parts read back as the whole list, in order.
#[test]
fn a_long_list_is_split_into_messages_within_the_bound() {
    let infos: Vec<BoxInfo> = (0..5_000)
        .map(|n| BoxInfo {
            membership: None,
            id: id(&format!("box-{n:06}")),
            ..info()
        })
        .collect();
    let parts = boxes(10, &infos, MAX_MESSAGE).unwrap();
    assert!(parts.len() > 1, "5,000 boxes fit one message");
    let mut back = Vec::new();
    for (n, bytes) in parts.iter().enumerate() {
        assert!(
            bytes.len() <= MAX_MESSAGE,
            "part {n} is {} bytes",
            bytes.len()
        );
        match read_answer(&received(bytes, 0), MAX_MESSAGE).unwrap() {
            (Header { id: 10, .. }, Answer::Boxes { infos, more }) => {
                assert_eq!(more, n + 1 < parts.len(), "part {n}'s more");
                back.extend(infos);
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(back, infos);
    let one = boxes(10, &[], MAX_MESSAGE).unwrap();
    assert_eq!(one.len(), 1);
    assert_eq!(
        read_answer(&received(&one[0], 0), MAX_MESSAGE).unwrap().1,
        Answer::Boxes {
            infos: Vec::new(),
            more: false
        }
    );
}

/// HEX-SOCK-1: every truncation of every request and answer is refused, by type, never a panic.
#[test]
fn every_truncation_is_refused() {
    for (n, req, fds) in requests() {
        let bytes = request(n, &req, MAX_MESSAGE).unwrap();
        for cut in 0..bytes.len() {
            let got = read_request(&received(&bytes[..cut], fds), MAX_MESSAGE);
            assert!(got.is_err(), "{req:?} cut at {cut} read as {got:?}");
        }
    }
    for (n, ans) in answers() {
        let bytes = answer(n, &ans, MAX_MESSAGE).unwrap();
        for cut in 0..bytes.len() {
            let got = read_answer(&received(&bytes[..cut], 0), MAX_MESSAGE);
            assert!(got.is_err(), "{ans:?} cut at {cut} read as {got:?}");
        }
    }
}

/// HEX-SOCK-1: a message whose header is cut, or isn't Hex's, is refused with id 0 (there is no
/// request to answer by its own); past the header, the refusal carries the request's id.
#[test]
fn a_refusal_goes_to_the_request_its_header_names() {
    let got = read_request(&received(b"HEXW\x01\x00", 0), MAX_MESSAGE).unwrap_err();
    assert_eq!(got.id, 0);
    assert!(matches!(got.fault, Fault::Truncated { .. }), "{got:?}");
    let got = read_request(&received(b"GET / HTTP/1.1\r\n\r\n", 0), MAX_MESSAGE).unwrap_err();
    assert_eq!((got.id, got.fault), (0, Fault::Magic));
    let mut bytes = request(77, &requests()[4].1, MAX_MESSAGE).unwrap();
    bytes.push(0);
    let got = read_request(&received(&bytes, 0), MAX_MESSAGE).unwrap_err();
    assert_eq!(got.id, 77);
    assert_eq!(
        got.fault,
        Fault::Leftover {
            bytes: 1,
            kind: 0x0003,
        }
    );
}

/// HEX-SOCK-1: a message over the bound is refused naming it, by the size the kernel reports even
/// when the receiver's buffer cut it, and with the request's id.
#[test]
fn an_oversized_message_is_refused_naming_the_bound() {
    let bytes = request(9, &requests()[8].1, MAX_MESSAGE).unwrap();
    let cut = Received {
        bytes: &bytes,
        size: MAX_MESSAGE + 1,
        descriptors: 0,
        descriptors_cut: false,
    };
    let got = read_request(&cut, MAX_MESSAGE).unwrap_err();
    assert_eq!(got.id, 9);
    assert_eq!(
        got.fault,
        Fault::Oversized {
            bytes: MAX_MESSAGE + 1,
            bound: MAX_MESSAGE
        }
    );
    assert_eq!(
        Refusal::from(got.fault),
        Refusal::Invalid {
            what: "message",
            why: format!(
                "{} bytes is over the {MAX_MESSAGE}-byte bound on one message",
                MAX_MESSAGE + 1
            ),
        }
    );
    // A writer refuses before it sends: a program past the bound is never copied whole.
    let huge = Call::Exec {
        id: id("b"),
        program: Program::of(&[&"x".repeat(MAX_MESSAGE)]),
        streams: Streams::default(),
    };
    assert!(matches!(
        request(1, &Request::Call(huge), MAX_MESSAGE),
        Err(Fault::Oversized {
            bound: MAX_MESSAGE,
            ..
        })
    ));
}

/// HEX-SOCK-1: a kind the wire doesn't define is refused by type, a request's kinds as an answer and an
/// answer's as a request too.
#[test]
fn unknown_kinds_are_refused() {
    let hello = request(5, &Request::Hello { build: build() }, MAX_MESSAGE).unwrap();
    for kind in [0u16, 0x000f, 0x7fff, 0x8001, 0x8009, 0xffff] {
        let mut bytes = hello.clone();
        bytes[12..14].copy_from_slice(&kind.to_le_bytes());
        let got = read_request(&received(&bytes, 0), MAX_MESSAGE).unwrap_err();
        assert!(
            matches!(got.fault, Fault::Undefined { kind: k, expected: "a request" } if k == kind),
            "{got:?}"
        );
    }
    for kind in [0u16, 0x0001, 0x000b, 0x8000, 0x800b, 0xffff] {
        let mut bytes = answer(5, &Answer::Done, MAX_MESSAGE).unwrap();
        bytes[12..14].copy_from_slice(&kind.to_le_bytes());
        assert!(matches!(
            read_answer(&received(&bytes, 0), MAX_MESSAGE),
            Err(Fault::Undefined {
                expected: "an answer",
                ..
            })
        ));
    }
}

/// HEX-SOCK-1: descriptors travel only with an exec, one for each stream it names; any other count,
/// or more than the three a message takes, is refused by type.
#[test]
fn descriptors_where_none_belong_are_refused() {
    let create = request(2, &requests()[1].1, MAX_MESSAGE).unwrap();
    let got = read_request(&received(&create, 1), MAX_MESSAGE).unwrap_err();
    assert_eq!(
        (got.id, got.fault),
        (
            2,
            Fault::Descriptors {
                what: "a create",
                want: 0,
                got: 1
            }
        )
    );
    let exec = request(6, &requests()[5].1, MAX_MESSAGE).unwrap();
    for fds in [0, 1, 3] {
        let got = read_request(&received(&exec, fds), MAX_MESSAGE).unwrap_err();
        assert_eq!(
            got.fault,
            Fault::Descriptors {
                what: "an exec",
                want: 2,
                got: fds
            }
        );
    }
    let cut = Received {
        bytes: &exec,
        size: exec.len(),
        descriptors: 3,
        descriptors_cut: true,
    };
    assert_eq!(
        read_request(&cut, MAX_MESSAGE).unwrap_err().fault,
        Fault::DescriptorsCut
    );
    let done = answer(3, &Answer::Done, MAX_MESSAGE).unwrap();
    assert!(matches!(
        read_answer(&received(&done, 1), MAX_MESSAGE),
        Err(Fault::Descriptors {
            want: 0,
            got: 1,
            ..
        })
    ));
}

/// HEX-SOCK-1: a body whose values break the seam's own rules is refused by name: an id that is a
/// path, a signal outside 1..=64, a boolean or option byte that is neither, streams past stderr, text
/// that isn't UTF-8, and a list claiming more items than bytes are left.
#[test]
fn hostile_values_are_refused_by_name() {
    let head = |kind: u16| header(kind, 42);
    let string = |s: &[u8]| {
        let mut b = u32::try_from(s.len()).unwrap().to_le_bytes().to_vec();
        b.extend_from_slice(s);
        b
    };
    let value = |bytes: &[u8], fds| match read_request(&received(bytes, fds), MAX_MESSAGE) {
        Err(Bad {
            id: 42,
            fault: Fault::Value { what, .. },
            ..
        }) => what,
        other => panic!("{other:?}"),
    };
    let mut start = head(0x0003);
    start.extend(string(b"../etc"));
    assert_eq!(value(&start, 0), "box id");
    let mut start = head(0x0003);
    start.extend(string(b"\xff\xfe"));
    assert_eq!(value(&start, 0), "box id");
    for n in [0u8, 65, 255] {
        let mut signal = head(0x0005);
        signal.extend(string(b"b"));
        signal.push(n);
        assert_eq!(value(&signal, 0), "signal");
    }
    let mut exec = head(0x0004);
    exec.extend(string(b"b"));
    exec.extend([0, 0, 0, 0, 0, 0, 0, 0, 7]); // no args, no env, then an option byte of 7
    assert_eq!(value(&exec, 0), "option");
    let mut exec = head(0x0004);
    exec.extend(string(b"b"));
    exec.extend([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0b1000]);
    assert_eq!(value(&exec, 0), "streams");
    let mut create = head(0x0002);
    create.extend(string(b"b"));
    create.extend([0; 8]);
    create.push(9); // no such class spec
    assert_eq!(value(&create, 0), "class spec");
    // A list claiming four billion items in a short message is refused before room is made for them.
    let mut exec = head(0x0004);
    exec.extend(string(b"b"));
    exec.extend(u32::MAX.to_le_bytes());
    assert!(matches!(
        read_request(&received(&exec, 0), MAX_MESSAGE)
            .unwrap_err()
            .fault,
        Fault::Truncated { .. }
    ));
    let mut info = answer(2, &Answer::Info(info()), MAX_MESSAGE).unwrap();
    let adopted = info.len() - 1 - 8 - 6; // abandoned's 8, the exit's 5 and option byte, then adopted
    info[adopted] = 2;
    assert!(matches!(
        read_answer(&received(&info, 0), MAX_MESSAGE),
        Err(Fault::Value {
            what: "adopted",
            ..
        })
    ));
}

/// The refusal names a decoded refusal carries are kept once, so the same name is the same string
/// however often it comes, and a name past the bound is refused, never kept.
#[test]
fn refusal_names_are_kept_once_and_bounded() {
    let bytes = answer(1, &Answer::Refused(refusals()[0].clone()), MAX_MESSAGE).unwrap();
    let what = |bytes: &[u8]| match read_answer(&received(bytes, 0), MAX_MESSAGE).unwrap().1 {
        Answer::Refused(Refusal::Invalid { what, .. }) => what,
        other => panic!("{other:?}"),
    };
    let (a, b) = (what(&bytes), what(&bytes));
    assert_eq!(a, "cpu_weight");
    assert!(std::ptr::eq(a, b), "a name is kept once");
    let long = Refusal::Invalid {
        what: "x",
        why: String::new(),
    };
    let mut bytes = answer(1, &Answer::Refused(long), MAX_MESSAGE).unwrap();
    // Swap the one-byte name for one longer than the bound.
    let at = HEADER_BYTES + 1;
    let name = "n".repeat(NAME_BYTES + 1);
    let mut body = u32::try_from(name.len()).unwrap().to_le_bytes().to_vec();
    body.extend_from_slice(name.as_bytes());
    bytes.splice(at..at + 5, body);
    assert!(matches!(
        read_answer(&received(&bytes, 0), MAX_MESSAGE),
        Err(Fault::Value {
            what: "refusal name",
            ..
        })
    ));
}

fn any_id() -> impl Strategy<Value = BoxId> {
    "[A-Za-z0-9_+-][A-Za-z0-9_+.-]{0,15}".prop_map(|s| BoxId::new(&s).unwrap())
}

fn any_text() -> impl Strategy<Value = String> {
    "\\PC{0,12}"
}

/// Any path: on Unix any bytes but NUL, which needn't be UTF-8; elsewhere any text, which a path
/// there is.
#[cfg(unix)]
fn any_path() -> impl Strategy<Value = PathBuf> {
    proptest::collection::vec(any::<u8>().prop_filter("no NUL", |b| *b != 0), 0..24).prop_map(|b| {
        use std::os::unix::ffi::OsStrExt;
        PathBuf::from(std::ffi::OsStr::from_bytes(&b))
    })
}

#[cfg(not(unix))]
fn any_path() -> impl Strategy<Value = PathBuf> {
    "[^\0]{0,24}".prop_map(PathBuf::from)
}

fn any_bounds() -> impl Strategy<Value = Bounds> {
    (
        (
            any::<Option<u16>>(),
            any::<Option<u32>>(),
            any::<Option<u64>>(),
            any::<Option<u64>>(),
        ),
        (
            any::<Option<u16>>(),
            any::<Option<u32>>(),
            any::<Option<(Option<u64>, Option<u64>, Option<u64>, Option<u64>)>>(),
            any::<Option<u32>>(),
        ),
    )
        .prop_map(|((cw, cm, mm, mh), (iw, il, io, pm))| Bounds {
            cpu_weight: cw,
            cpu_max_milli: cm,
            memory_max: mm,
            memory_high: mh,
            io_weight: iw,
            io_latency_us: il,
            io_max: io.map(|(a, b, c, d)| IoRates {
                read_bps: a,
                write_bps: b,
                read_iops: c,
                write_iops: d,
            }),
            pids_max: pm,
        })
}

fn any_program() -> impl Strategy<Value = Program> {
    (
        proptest::collection::vec(any_text(), 0..4),
        proptest::collection::vec((any_text(), any_text()), 0..3),
        proptest::option::of(any_text()),
        proptest::option::of((
            any::<u32>(),
            any::<u32>(),
            proptest::collection::vec(any::<u32>(), 0..3),
        )),
    )
        .prop_map(|(args, env, cwd, user)| Program {
            args,
            env,
            cwd,
            user: user.map(|(uid, gid, groups)| User { uid, gid, groups }),
        })
}

fn any_spec() -> impl Strategy<Value = BoxSpec> {
    let oci = (
        prop_oneof![
            any_path().prop_map(Rootfs::Dir),
            proptest::collection::vec(any_path(), 0..3).prop_map(Rootfs::Layers),
        ],
        any_program(),
        proptest::collection::vec((any_path(), any_text(), any::<bool>()), 0..3),
        proptest::option::of(any_text()),
        any::<bool>(),
        proptest::collection::vec((any_text(), any_text()), 0..3),
    )
        .prop_map(|(rootfs, init, mounts, hostname, host, hosts)| {
            ClassSpec::Oci(Box::new(OciSpec {
                rootfs,
                init,
                mounts: mounts
                    .into_iter()
                    .map(|(source, destination, read_only)| Mount {
                        source,
                        destination,
                        read_only,
                    })
                    .collect(),
                hostname,
                network: if host {
                    Network::Host
                } else {
                    Network::Private
                },
                hosts,
            }))
        });
    let class = prop_oneof![
        Just(ClassSpec::Process),
        any_path().prop_map(ClassSpec::Bundle),
        oci,
    ];
    let owner = proptest::option::of(("\\PC{0,16}", any::<bool>()));
    (any_id(), any_bounds(), class, owner).prop_map(|(id, bounds, class, owner)| BoxSpec {
        id,
        bounds,
        class,
        owner: owner.map(|(name, reuse)| BoxOwner { name, reuse }),
    })
}

fn any_exit() -> impl Strategy<Value = Exit> {
    prop_oneof![
        any::<i32>().prop_map(Exit::Code),
        any::<i32>().prop_map(Exit::Signal),
        Just(Exit::Unknown),
    ]
}

fn any_info() -> impl Strategy<Value = BoxInfo> {
    (
        any_id(),
        any::<bool>(),
        0u8..3,
        any_bounds(),
        any::<bool>(),
        proptest::option::of(any_exit()),
        any::<u64>(),
    )
        .prop_map(
            |(id, oci, state, bounds, adopted, exit, abandoned)| BoxInfo {
                membership: None,
                id,
                class: if oci { Class::Oci } else { Class::Process },
                state: match state {
                    0 => BoxState::Empty,
                    1 => BoxState::Running,
                    _ => BoxState::Created,
                },
                bounds,
                adopted,
                exit,
                abandoned,
            },
        )
}

/// The names a generated refusal carries, from a fixed few: the names this process keeps are
/// bounded, and random ones would spend that bound across cases.
const SOME_NAMES: [&str; 4] = ["cpu_weight", "start", "connections", "an op"];

fn any_refusal() -> impl Strategy<Value = Refusal> {
    let name = proptest::sample::select(&SOME_NAMES[..]);
    prop_oneof![
        (name.clone(), any_text()).prop_map(|(what, why)| Refusal::Invalid { what, why }),
        any_id().prop_map(Refusal::NoSuchBox),
        any_id().prop_map(Refusal::Exists),
        (any_id(), name.clone()).prop_map(|(id, op)| Refusal::NotRunning(id, op)),
        (name.clone(), name.clone(), any_text()).prop_map(|(what, platform, why)| {
            Refusal::Platform {
                what,
                platform,
                why,
            }
        }),
        any::<u32>().prop_map(|n| Refusal::Full(usize::try_from(n).unwrap())),
        (name.clone(), any::<u32>()).prop_map(|(what, bound)| Refusal::Exhausted {
            what,
            bound: usize::try_from(bound).unwrap()
        }),
        (name, any_text()).prop_map(|(op, why)| Refusal::Os { op, why }),
        any_text().prop_map(Refusal::Broken),
    ]
}

proptest! {
    /// HEX-SOCK-2: any create, and any exec, reads back as written.
    #[test]
    fn any_create_or_exec_reads_back(spec in any_spec(), program in any_program(), id in any_id(), n in any::<u64>(), bits in 0u8..8) {
        let req = Request::Call(Call::Create(Box::new(spec)));
        let bytes = request(n, &req, MAX_MESSAGE).unwrap();
        prop_assert_eq!(read_request(&received(&bytes, 0), MAX_MESSAGE).unwrap(), (Header { kind: 0x0002, id: n }, req));
        let streams = Streams::of(bits).unwrap();
        let req = Request::Call(Call::Exec { id, program, streams });
        let bytes = request(n, &req, MAX_MESSAGE).unwrap();
        prop_assert_eq!(read_request(&received(&bytes, streams.count()), MAX_MESSAGE).unwrap().1, req);
    }

    /// HEX-SOCK-2: any box info, list, refusal and network namespace counts read back as written.
    #[test]
    fn any_answer_reads_back(infos in proptest::collection::vec(any_info(), 0..4), refusal in any_refusal(), exit in any_exit(), n in any::<[u64; 11]>(), bits in 0u8..4, errno in proptest::option::of(any::<i32>()), text in "[a-zé ]{0,300}") {
        let counts = netns::Counts {
            made_ahead: n[0], taken: n[1], made_on_path: n[2], discarded: n[3], failed: n[4],
            failed_by: netns::FailedBy { home: n[5], unshare: n[6], open: n[7], loopback: n[8], back: n[9] },
            refused: netns::Missing::of(bits & 1 == 0, bits & 2 == 0),
            refused_on_path: n[10],
            last_failed: netns::LastFailed {
                open: Some(netns::Reason::new(errno, &text)),
                ..netns::LastFailed::default()
            },
        };
        for ans in [Answer::Boxes { infos, more: false }, Answer::Refused(refusal), Answer::Exited(exit), Answer::Netns(counts)] {
            let bytes = answer(7, &ans, MAX_MESSAGE).unwrap();
            prop_assert_eq!(read_answer(&received(&bytes, 0), MAX_MESSAGE).unwrap().1, ans);
        }
    }

    /// HEX-SOCK-1: any bytes are a message or a typed refusal, never a panic, as a request or an
    /// answer, with any count of descriptors.
    #[test]
    fn any_bytes_are_a_message_or_refused(bytes in proptest::collection::vec(any::<u8>(), 0..512), fds in 0usize..5) {
        let _ = read_request(&received(&bytes, fds), MAX_MESSAGE);
        let _ = read_answer(&received(&bytes, fds), MAX_MESSAGE);
    }

    /// HEX-SOCK-1: bodies of random bytes behind a valid header, so every kind's decoder meets them, are a
    /// message or a typed refusal, never a panic.
    #[test]
    fn any_body_behind_a_header_is_read_or_refused(kind in prop_oneof![0u16..0x0010, 0x8000u16..0x800c], body in proptest::collection::vec(any::<u8>(), 0..256), fds in 0usize..4) {
        let mut bytes = header(kind, 3);
        bytes.extend_from_slice(&body);
        let _ = read_request(&received(&bytes, fds), MAX_MESSAGE);
        let _ = read_answer(&received(&bytes, fds), MAX_MESSAGE);
    }
}

/// HEX-SOCK-1: every single-bit flip of every request and answer is a message or a typed refusal,
/// never a panic.
#[test]
fn every_bit_flip_is_read_or_refused() {
    let messages: Vec<(Vec<u8>, usize)> = requests()
        .into_iter()
        .map(|(n, r, fds)| (request(n, &r, MAX_MESSAGE).unwrap(), fds))
        .chain(
            answers()
                .into_iter()
                .map(|(n, a)| (answer(n, &a, MAX_MESSAGE).unwrap(), 0)),
        )
        .collect();
    for (bytes, fds) in messages {
        for at in 0..bytes.len() {
            for bit in 0..8 {
                let mut flipped = bytes.clone();
                flipped[at] ^= 1 << bit;
                let _ = read_request(&received(&flipped, fds), MAX_MESSAGE);
                let _ = read_answer(&received(&flipped, fds), MAX_MESSAGE);
            }
        }
    }
}

/// HEX-GUEST-1 (docs/design/shards-guest.md §3): an exec naming stdin and stdout, sent with no
/// descriptors: expect it read whole where its stdio travels apart (a stream transport), and refused by
/// type where descriptors ride with the message.
#[test]
fn an_exec_whose_stdio_travels_apart_carries_no_descriptors() {
    let exec = Request::Call(Call::Exec {
        id: id("b"),
        program: Program::of(&["cat"]),
        streams: Streams {
            stdin: true,
            stdout: true,
            stderr: false,
        },
    });
    let bytes = request(7, &exec, MAX_MESSAGE).unwrap();
    let (h, read) = read_request_apart(&bytes, MAX_MESSAGE).unwrap();
    assert_eq!((h.id, read), (7, exec));
    assert!(matches!(
        read_request(&received(&bytes, 0), MAX_MESSAGE),
        Err(Bad {
            fault: Fault::Descriptors {
                want: 2,
                got: 0,
                ..
            },
            ..
        })
    ));
}
