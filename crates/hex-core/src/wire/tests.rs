//! The wire format's tests (decision 3, HEX-SOCK-1 and HEX-SOCK-2): every request and answer reads
//! back as written; a message of another wire is refused before its body is read, naming both wires, and a
//! hello's head names its sender's build whatever the wire; the wire's identity is its definitions' digest,
//! line endings aside; and hostile bytes, at every truncation, every bit flip and at random, are refused by
//! type and never panic.

use super::*;
use hex_api::{Bounds, BoxOwner, BoxSpec, IoRates, Mount, Network, OciSpec, Program, Rootfs, User};
use proptest::prelude::*;

fn device_claims() -> Vec<DeviceId> {
    vec![
        DeviceId {
            kind: DeviceKind::Net,
            key: u64::from(u32::MAX),
        },
        DeviceId {
            kind: DeviceKind::Gpu,
            key: 0x1234_5678_abcd,
        },
        DeviceId {
            kind: DeviceKind::Block,
            key: u64::MAX,
        },
        DeviceId {
            kind: DeviceKind::Vfio,
            key: 0,
        },
    ]
}

fn device_spec(devices: Vec<DeviceId>) -> BoxSpec {
    BoxSpec {
        id: id("device-wire"),
        bounds: Bounds::default(),
        class: ClassSpec::Process,
        owner: None,
        devices,
    }
}

fn device_capacity(devices: Vec<DeviceInfo>) -> Capacity {
    Capacity {
        id: "physical-pool".to_owned(),
        total: CapacityResources::default(),
        free: CapacityResources::default(),
        devices,
    }
}

fn device_offer(device: DeviceId) -> DeviceInfo {
    DeviceInfo {
        id: device,
        model: "synthetic-private-model-é".to_owned(),
        size: u64::MAX,
        holder: Some(id("synthetic-private-holder")),
    }
}

// Keep all preceding fields exactly as the real codec writes them; replace only the device tail.
fn with_device_tail(offer: bool, tail: &[u8]) -> Vec<u8> {
    let mut bytes = if offer {
        answer(
            91,
            &Answer::Capacity(Some(device_capacity(Vec::new()))),
            MAX_MESSAGE,
        )
        .unwrap()
    } else {
        request(
            91,
            &Request::Call(Call::Create(Box::new(device_spec(Vec::new())))),
            MAX_MESSAGE,
        )
        .unwrap()
    };
    bytes.truncate(bytes.len() - 4);
    bytes.extend_from_slice(tail);
    bytes
}

fn read_device_tail(offer: bool, tail: &[u8]) -> Fault {
    let bytes = with_device_tail(offer, tail);
    if offer {
        read_answer(&received(&bytes, 0), MAX_MESSAGE).unwrap_err()
    } else {
        read_request(&received(&bytes, 0), MAX_MESSAGE)
            .unwrap_err()
            .fault
    }
}

/// AC-T7.3.1/HEX-SOCK-1: fixed device tags and LE keys; catalog/claim order and shared kinds survive.
#[test]
fn device_claims_and_offers_roundtrip_with_explicit_tags_and_catalog_order() {
    let devices = device_claims();
    let mut encoded = Writer::new(MAX_MESSAGE);
    for device in &devices {
        put_device_id(&mut encoded, device);
    }
    let mut expected = Vec::new();
    for (tag, key) in [
        (3, u64::from(u32::MAX)),
        (0, 0x1234_5678_abcd),
        (2, u64::MAX),
        (1, 0),
    ] {
        expected.push(tag);
        expected.extend_from_slice(&key.to_le_bytes());
    }
    assert_eq!(encoded.done().unwrap(), expected);
    let claimed = Request::Call(Call::Create(Box::new(device_spec(devices.clone()))));
    let bytes = request(91, &claimed, MAX_MESSAGE).unwrap();
    assert_eq!(&bytes[12..14], &kind::CREATE.to_le_bytes());
    assert_eq!(
        read_request(&received(&bytes, 0), MAX_MESSAGE).unwrap().1,
        claimed
    );
    assert_eq!(
        request(
            91,
            &read_request(&received(&bytes, 0), MAX_MESSAGE).unwrap().1,
            MAX_MESSAGE
        )
        .unwrap(),
        bytes
    );
    let mut offers: Vec<_> = devices.into_iter().map(device_offer).collect();
    offers[1].holder = None;
    offers[2].model.clear(); // Opaque metadata has no invented naming grammar.
    let offered = Answer::Capacity(Some(device_capacity(offers)));
    let bytes = answer(91, &offered, MAX_MESSAGE).unwrap();
    assert_eq!(&bytes[12..14], &kind::CAPACITY_ANSWER.to_le_bytes());
    let decoded = read_answer(&received(&bytes, 0), MAX_MESSAGE).unwrap().1;
    assert_eq!(decoded, offered);
    assert_eq!(answer(91, &decoded, MAX_MESSAGE).unwrap(), bytes);
}

/// AC-T7.3.1: malformed claims and offers refuse on both sides, rather than narrowing an identity.
#[test]
fn device_codecs_refuse_invalid_keys_and_duplicate_ids_without_private_values() {
    for device in [
        DeviceId {
            kind: DeviceKind::Gpu,
            key: 1 << 48,
        },
        DeviceId {
            kind: DeviceKind::Vfio,
            key: 1 << 32,
        },
        DeviceId {
            kind: DeviceKind::Net,
            key: 0,
        },
        DeviceId {
            kind: DeviceKind::Net,
            key: 1 << 32,
        },
    ] {
        let claimed = Request::Call(Call::Create(Box::new(device_spec(vec![device]))));
        assert!(matches!(
            request(91, &claimed, MAX_MESSAGE),
            Err(Fault::Value {
                what: "devices",
                ..
            })
        ));
        let offered = Answer::Capacity(Some(device_capacity(vec![device_offer(device)])));
        assert!(matches!(
            answer(91, &offered, MAX_MESSAGE),
            Err(Fault::Value {
                what: "capacity",
                ..
            })
        ));
        for offer in [false, true] {
            let mut tail = Writer::new(MAX_MESSAGE);
            tail.u32(1);
            put_device_id(&mut tail, &device);
            if offer {
                tail.string("synthetic-private-model").unwrap();
                tail.u64(1);
                tail.u8(0);
            }
            let error = read_device_tail(offer, &tail.done().unwrap());
            assert!(matches!(
                error,
                Fault::Value {
                    what: "devices.id",
                    ..
                }
            ));
            assert!(!error.to_string().contains("synthetic-private"));
            assert!(!error.to_string().contains(&device.key.to_string()));
        }
    }
    let device = device_claims()[0];
    let claimed = Request::Call(Call::Create(Box::new(device_spec(vec![device, device]))));
    assert!(request(91, &claimed, MAX_MESSAGE).is_err());
    let offered = Answer::Capacity(Some(device_capacity(vec![device_offer(device); 2])));
    assert!(answer(91, &offered, MAX_MESSAGE).is_err());
    for offer in [false, true] {
        let mut tail = Writer::new(MAX_MESSAGE);
        tail.u32(2);
        for _ in 0..2 {
            put_device_id(&mut tail, &device);
            if offer {
                tail.string("synthetic-private-model").unwrap();
                tail.u64(1);
                tail.u8(0);
            }
        }
        let error = read_device_tail(offer, &tail.done().unwrap());
        assert!(matches!(error, Fault::Value { ref why, .. } if why.contains("duplicate")));
        assert!(!error.to_string().contains("synthetic-private"));
    }
}

/// AC-T7.3.1/HEX-SOCK-2: reject hostile counts before reserving or visiting list elements.
#[test]
fn device_lists_refuse_oversized_and_truncated_counts_before_visiting_elements() {
    for offer in [false, true] {
        for count in [u32::try_from(MAX_DEVICES + 1).unwrap(), u32::MAX] {
            assert!(matches!(
                read_device_tail(offer, &count.to_le_bytes()),
                Fault::Value {
                    what: "devices",
                    ..
                }
            ));
            let bytes = count.to_le_bytes();
            let mut reader = Reader::new(&bytes);
            let mut visits = 0;
            assert!(
                reader
                    .device_list::<DeviceId>(if offer { 22 } else { 9 }, |_| {
                        visits += 1;
                        panic!("oversized count visited an element")
                    })
                    .is_err()
            );
            assert_eq!(visits, 0);
            assert_eq!(reader.at(), 4);
        }
        for count in [1, u32::try_from(MAX_DEVICES).unwrap()] {
            assert!(matches!(
                read_device_tail(offer, &count.to_le_bytes()),
                Fault::Truncated { .. }
            ));
        }
        let oversized: Vec<_> = (0..=MAX_DEVICES)
            .map(|key| DeviceId {
                kind: DeviceKind::Block,
                key: key as u64,
            })
            .collect();
        let error = if offer {
            answer(
                91,
                &Answer::Capacity(Some(device_capacity(
                    oversized.into_iter().map(device_offer).collect(),
                ))),
                MAX_MESSAGE,
            )
            .unwrap_err()
        } else {
            request(
                91,
                &Request::Call(Call::Create(Box::new(device_spec(oversized)))),
                MAX_MESSAGE,
            )
            .unwrap_err()
        };
        assert!(matches!(error, Fault::Value { .. }));
    }
    // A valid API list can exceed this transport's independent byte bound; the writer refuses early.
    let claimed = Request::Call(Call::Create(Box::new(device_spec(
        (0..MAX_DEVICES)
            .map(|key| DeviceId {
                kind: DeviceKind::Block,
                key: key as u64,
            })
            .collect(),
    ))));
    assert!(matches!(
        request(91, &claimed, MAX_MESSAGE),
        Err(Fault::Oversized {
            bound: MAX_MESSAGE,
            ..
        })
    ));
}

/// AC-T7.3.1/HEX-SOCK-2: unknown tags/invalid holder data are refusals, never default grants or echoes.
#[test]
fn device_unknown_tags_and_invalid_metadata_diagnostics_are_redacted() {
    for offer in [false, true] {
        for tag in [4, u8::MAX] {
            let mut tail = Writer::new(MAX_MESSAGE);
            tail.u32(1);
            tail.u8(tag);
            tail.u64(1);
            if offer {
                tail.string("synthetic-private-model").unwrap();
                tail.u64(1);
                tail.u8(0);
            }
            let error = read_device_tail(offer, &tail.done().unwrap());
            assert!(matches!(
                error,
                Fault::Value {
                    what: "devices.kind",
                    ..
                }
            ));
            assert!(!error.to_string().contains("synthetic-private"));
        }
    }
    let device = device_claims()[0];
    let mut tail = Writer::new(MAX_MESSAGE);
    tail.u32(1);
    put_device_id(&mut tail, &device);
    tail.string("synthetic-private-model").unwrap();
    tail.u64(1);
    tail.u8(1);
    tail.string("../synthetic-private-holder").unwrap();
    let error = read_device_tail(true, &tail.done().unwrap());
    assert!(matches!(
        error,
        Fault::Value {
            what: "devices.holder",
            ..
        }
    ));
    assert!(!error.to_string().contains("synthetic-private"));
    let mut tail = Writer::new(MAX_MESSAGE);
    tail.u32(1);
    put_device_id(&mut tail, &device);
    tail.bytes(&[0xff]).unwrap();
    tail.u64(1);
    tail.u8(0);
    assert!(matches!(
        read_device_tail(true, &tail.done().unwrap()),
        Fault::Value {
            what: "devices.model",
            ..
        }
    ));
}

/// AC-T7.4.1: capacity travels as a typed call/answer, including native host fallback.
#[test]
fn runtime_capacity_roundtrip_and_invalid_room_refused() {
    let request = Request::Call(Call::Capacity(Class::Oci));
    let bytes = super::request(42, &request, MAX_MESSAGE).unwrap();
    assert_eq!(
        read_request(&received(&bytes, 0), MAX_MESSAGE).unwrap().1,
        request
    );
    let capacity = hex_api::Capacity {
        devices: Vec::new(),
        id: "guest:123:456".into(),
        total: hex_api::CapacityResources {
            cpu_milli: 2_000,
            mem_bytes: 632 << 20,
            disk_bytes: 8 << 30,
        },
        free: hex_api::CapacityResources {
            cpu_milli: 2_000,
            mem_bytes: 600 << 20,
            disk_bytes: 7 << 30,
        },
    };
    for value in [None, Some(capacity.clone())] {
        let expected = Answer::Capacity(value);
        let bytes = answer(42, &expected, MAX_MESSAGE).unwrap();
        assert_eq!(
            read_answer(&received(&bytes, 0), MAX_MESSAGE).unwrap().1,
            expected
        );
    }
    let mut malformed = answer(42, &Answer::Capacity(Some(capacity.clone())), MAX_MESSAGE).unwrap();
    let free_memory = malformed.len() - 20; // The trailing empty device list contributes four bytes.
    malformed[free_memory..free_memory + 8]
        .copy_from_slice(&(capacity.total.mem_bytes + 1).to_le_bytes());
    assert!(read_answer(&received(&malformed, 0), MAX_MESSAGE).is_err());
    let mut bad = capacity;
    bad.free.mem_bytes = bad.total.mem_bytes + 1;
    assert!(answer(42, &Answer::Capacity(Some(bad)), MAX_MESSAGE).is_err());
}

/// AC-T7.4.1/HEX-SOCK-1: the chosen backend and exact host claims cross the socket together.
#[test]
fn device_backend_identity_survives_create_capacity_and_inspect() {
    let ClassSpec::Oci(workload) = oci_spec().class else {
        panic!("the fixture is an OCI workload");
    };
    for (class, tag, spec_tag, form) in [
        (Class::Oci, 1, 1, ClassSpec::Oci(workload.clone())),
        (
            Class::Firecracker,
            2,
            3,
            ClassSpec::Firecracker(workload.clone()),
        ),
        (Class::Shards, 3, 4, ClassSpec::Shards(workload.clone())),
    ] {
        let spec = BoxSpec {
            class: form,
            devices: device_claims(),
            ..oci_spec()
        };
        let create = Request::Call(Call::Create(Box::new(spec.clone())));
        let bytes = request(93, &create, MAX_MESSAGE).unwrap();
        let decoded = read_request(&received(&bytes, 0), MAX_MESSAGE).unwrap().1;
        assert_eq!(decoded, create);
        let mut reader = Reader::new(&bytes[HEADER_BYTES..]);
        assert_eq!(box_id(&mut reader).unwrap(), spec.id);
        assert_eq!(super::bounds(&mut reader).unwrap(), spec.bounds);
        assert_eq!(reader.u8().unwrap(), spec_tag);

        let capacity = Request::Call(Call::Capacity(class));
        let bytes = request(94, &capacity, MAX_MESSAGE).unwrap();
        assert_eq!(&bytes[HEADER_BYTES..], &[tag]);
        assert_eq!(
            read_request(&received(&bytes, 0), MAX_MESSAGE).unwrap().1,
            capacity
        );
        let expected = Answer::Info(BoxInfo { class, ..info() });
        let bytes = answer(95, &expected, MAX_MESSAGE).unwrap();
        assert_eq!(
            read_answer(&received(&bytes, 0), MAX_MESSAGE).unwrap().1,
            expected
        );
    }
    for tag in [4, u8::MAX] {
        let mut bytes =
            request(94, &Request::Call(Call::Capacity(Class::Oci)), MAX_MESSAGE).unwrap();
        bytes[HEADER_BYTES] = tag;
        assert!(matches!(
            read_request(&received(&bytes, 0), MAX_MESSAGE)
                .unwrap_err()
                .fault,
            Fault::Value { what: "class", .. }
        ));
    }
}

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
        devices: Vec::new(),
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
                devices: Vec::new(),
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
                devices: Vec::new(),
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
        (16, Request::StreamsReady, 0),
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
        (15, run(), 2),
    ]
}

/// A run (runtime design goal 7) of a `process` box carrying stdout and stderr and asking for stdin.
fn run() -> Request {
    Request::Call(Call::Run {
        spec: Box::new(BoxSpec {
            devices: Vec::new(),
            id: id("r"),
            bounds: bounds(),
            class: ClassSpec::Process,
            owner: None,
        }),
        program: program(),
        streams: Streams {
            stdin: false,
            stdout: true,
            stderr: true,
        },
        ask: Streams {
            stdin: true,
            stdout: false,
            stderr: false,
        },
    })
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
    out.push((
        16,
        Answer::Offered(StreamOffer {
            pid: 42,
            started: u64::MAX,
            vsock: PathBuf::from("guest/vsock"),
            token: 7,
            exec: 19,
            streams: Streams {
                stdin: true,
                stdout: true,
                stderr: false,
            },
        }),
    ));
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
    let paths = [
        "wire.rs",
        "stream.rs",
        "schedule.rs",
        "../../hex-sim/src/peer.rs",
        "../../hex-sim/src/model.rs",
        "../../hex-api/src/spec.rs",
        "../../hex-api/src/id.rs",
        "../../hex-api/src/bounds.rs",
        "../../hex-api/src/devices.rs",
        "../../hex-api/src/runtime.rs",
    ];
    let mut sources: Vec<Vec<u8>> = paths
        .iter()
        .map(|p| std::fs::read(dir.join(p)).unwrap())
        .collect();
    let digest =
        |sources: &[Vec<u8>]| identity(&sources.iter().map(Vec::as_slice).collect::<Vec<_>>());
    assert_eq!(digest(&sources), IDENTITY);
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
    assert_eq!(
        digest(&sources.iter().map(|s| crlf(s)).collect::<Vec<_>>()),
        IDENTITY
    );
    for changed in 0..sources.len() {
        sources[changed].push(b' ');
        assert_ne!(digest(&sources), IDENTITY);
        sources[changed].pop();
    }
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

/// HEX-SOCK-2 (runtime design goal 7, decision 7): a run is one request, the wire's kind after every other
/// request's (0x000f): a create's spec, then an exec's program, the streams it carries and those it asks for, as an
/// exec asking for streams has them. Do: write one carrying stdout and stderr and asking for stdin; read it with two
/// descriptors, with none, one and three, and apart from its stdio. Expect: read back as written with two, and whole
/// apart from its stdio (a stream transport's, whose streams travel on connections of their own); refused naming
/// the count otherwise.
#[test]
fn a_run_is_a_create_and_an_exec_in_one_request() {
    let run = run();
    let bytes = request(15, &run, MAX_MESSAGE).unwrap();
    assert_eq!(&bytes[12..14], &0x000fu16.to_le_bytes());
    assert_eq!(
        read_request(&received(&bytes, 2), MAX_MESSAGE).unwrap(),
        (
            Header {
                kind: 0x000f,
                id: 15
            },
            run.clone()
        )
    );
    for fds in [0, 1, 3] {
        assert_eq!(
            read_request(&received(&bytes, fds), MAX_MESSAGE)
                .unwrap_err()
                .fault,
            Fault::Descriptors {
                what: "a run",
                want: 2,
                got: fds
            }
        );
    }
    assert_eq!(read_request_apart(&bytes, MAX_MESSAGE).unwrap().1, run);
}

/// HEX-SOCK-1: a kind the wire doesn't define is refused by type, a request's kinds as an answer and an
/// answer's as a request too.
#[test]
fn unknown_kinds_are_refused() {
    let hello = request(5, &Request::Hello { build: build() }, MAX_MESSAGE).unwrap();
    for kind in [0u16, 0x0013, 0x7fff, 0x8001, 0x8009, 0xffff] {
        let mut bytes = hello.clone();
        bytes[12..14].copy_from_slice(&kind.to_le_bytes());
        let got = read_request(&received(&bytes, 0), MAX_MESSAGE).unwrap_err();
        assert!(
            matches!(got.fault, Fault::Undefined { kind: k, expected: "a request" } if k == kind),
            "{got:?}"
        );
    }
    for kind in [0u16, 0x0001, 0x000b, 0x8000, 0xffff] {
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
            Box::new(OciSpec {
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
            })
        });
    let workload = (0u8..3, oci).prop_map(|(backend, spec)| match backend {
        0 => ClassSpec::Oci(spec),
        1 => ClassSpec::Firecracker(spec),
        _ => ClassSpec::Shards(spec),
    });
    let class = prop_oneof![
        Just(ClassSpec::Process),
        any_path().prop_map(ClassSpec::Bundle),
        workload,
    ];
    let owner = proptest::option::of(("\\PC{0,16}", any::<bool>()));
    (any_id(), any_bounds(), class, owner).prop_map(|(id, bounds, class, owner)| BoxSpec {
        devices: Vec::new(),
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
        0u8..4,
        0u8..3,
        any_bounds(),
        any::<bool>(),
        proptest::option::of(any_exit()),
        any::<u64>(),
    )
        .prop_map(
            |(id, backend, state, bounds, adopted, exit, abandoned)| BoxInfo {
                membership: None,
                id,
                class: match backend {
                    0 => Class::Process,
                    1 => Class::Oci,
                    2 => Class::Firecracker,
                    _ => Class::Shards,
                },
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
    /// HEX-SOCK-2: any create, any exec and any run (runtime design goal 7) read back as written.
    #[test]
    fn any_create_or_exec_reads_back(spec in any_spec(), program in any_program(), id in any_id(), n in any::<u64>(), bits in 0u8..8) {
        let req = Request::Call(Call::Create(Box::new(spec.clone())));
        let bytes = request(n, &req, MAX_MESSAGE).unwrap();
        prop_assert_eq!(read_request(&received(&bytes, 0), MAX_MESSAGE).unwrap(), (Header { kind: 0x0002, id: n }, req));
        let streams = Streams::of(bits).unwrap();
        let req = Request::Call(Call::Exec { id, program: program.clone(), streams });
        let bytes = request(n, &req, MAX_MESSAGE).unwrap();
        prop_assert_eq!(read_request(&received(&bytes, streams.count()), MAX_MESSAGE).unwrap().1, req);
        let ask = Streams::of(!bits & 0b111).unwrap();
        let req = Request::Call(Call::Run { spec: Box::new(spec), program, streams, ask });
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
    fn any_body_behind_a_header_is_read_or_refused(kind in prop_oneof![0u16..0x0013, 0x8000u16..0x800e], body in proptest::collection::vec(any::<u8>(), 0..256), fds in 0usize..4) {
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

/// CP-T7.4: scheduling shares descriptor validation and message admission with Runtime calls.
#[test]
fn scheduler_control_cannot_carry_a_descriptor_or_exceed_the_wire_bound() {
    let request_value = Request::Call(Call::Schedule(Box::new(crate::schedule::Command::Retry)));
    let bytes = request(47, &request_value, MAX_MESSAGE).unwrap();
    assert_eq!(
        read_request_apart(&bytes, MAX_MESSAGE).unwrap().1,
        request_value
    );
    let received = Received {
        bytes: &bytes,
        size: bytes.len(),
        descriptors: 1,
        descriptors_cut: false,
    };
    assert!(matches!(
        read_request(&received, MAX_MESSAGE).unwrap_err().fault,
        Fault::Descriptors { .. }
    ));
    assert!(request(47, &request_value, HEADER_BYTES).is_err());
}
