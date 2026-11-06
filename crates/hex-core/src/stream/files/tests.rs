//! The staging stream's tests (docs/design/shards-guest.md §4a, HEX-GUEST-9; native plan step 5): a
//! stage's entries read back as written however the stream cuts them; hostile names, kinds, modes and
//! fields are refused by type; and every count past its bound is refused before its bytes are held.

use super::*;
use proptest::prelude::*;

fn entry(path: &str, what: What) -> Entry {
    Entry {
        path: path.to_owned(),
        mode: 0o755,
        what,
    }
}

/// A stage of `entries`, a file's bytes after its head as `bytes` gives them.
fn stage(id: &str, entries: &[(Entry, Vec<u8>)]) -> Vec<u8> {
    let mut out = open(Op::Stage, id).unwrap();
    for (e, bytes) in entries {
        out.extend(head(e, &LIMITS).unwrap());
        out.extend(bytes);
    }
    out.extend(END);
    out
}

/// What an unpacker reads from `input`, fed `cut` bytes at a time: its steps, a file's bytes gathered
/// into the entry before them, or the first refusal.
fn read(limits: Limits, input: &[u8], cut: usize) -> Result<Vec<(Step, Vec<u8>)>, Refused> {
    let mut u = Unpack::new(limits);
    let mut out: Vec<(Step, Vec<u8>)> = Vec::new();
    for mut piece in input.chunks(cut.max(1)) {
        while !piece.is_empty() {
            let (took, step) = u.take(piece)?;
            assert!(took > 0, "take made no progress");
            match step {
                Some(Step::Data { .. }) => out.last_mut().unwrap().1.extend(&piece[..took]),
                Some(step) => out.push((step, Vec::new())),
                None => {}
            }
            piece = &piece[took..];
        }
    }
    u.finish()?;
    Ok(out)
}

fn sample() -> Vec<(Entry, Vec<u8>)> {
    vec![
        (entry("root", What::Dir), vec![]),
        (entry("root/bin", What::Dir), vec![]),
        (
            entry("root/bin/sh", What::File { size: 5 }),
            b"#!sh\n".to_vec(),
        ),
        (entry("root/empty", What::File { size: 0 }), vec![]),
        (
            entry(
                "root/lib",
                What::Symlink {
                    target: "/usr/lib".to_owned(),
                },
            ),
            vec![],
        ),
    ]
}

/// HEX-GUEST-9: stage a directory tree with a file, an empty file and a symlink, read it back fed a
/// byte at a time and whole: expect the opening, each entry with its bytes, and the end, the same both
/// ways.
#[test]
fn a_stage_reads_back_as_written_however_it_is_cut() {
    let entries = sample();
    let bytes = stage("box-1", &entries);
    let mut want = vec![(
        Step::Open {
            op: Op::Stage,
            id: "box-1".to_owned(),
        },
        vec![],
    )];
    want.extend(entries.into_iter().map(|(e, b)| (Step::Entry(e), b)));
    want.push((Step::End, vec![]));
    for cut in [1, 3, 16, 17, bytes.len()] {
        assert_eq!(read(LIMITS, &bytes, cut).unwrap(), want, "cut {cut}");
    }
}

/// HEX-GUEST-9: a remove is its opening alone: expect it read back, and a byte after it refused as
/// trailing.
#[test]
fn a_remove_is_its_opening_alone() {
    let bytes = open(Op::Remove, "box-1").unwrap();
    assert_eq!(
        read(LIMITS, &bytes, 1).unwrap(),
        vec![(
            Step::Open {
                op: Op::Remove,
                id: "box-1".to_owned()
            },
            vec![]
        )]
    );
    let mut more = bytes;
    more.push(0);
    assert_eq!(read(LIMITS, &more, 64), Err(Refused::Trailing(1)));
}

/// HEX-GUEST-9: end the stream before its end head, in a head and in a file: expect each refused as
/// truncated, never taken as a whole stage.
#[test]
fn a_stage_cut_short_is_refused_as_truncated() {
    let bytes = stage("b", &sample());
    for cut in [
        3,
        OPEN_BYTES + 1,
        OPEN_BYTES + 5,
        bytes.len() - HEAD_BYTES,
        bytes.len() - 1,
    ] {
        assert!(
            matches!(read(LIMITS, &bytes[..cut], 7), Err(Refused::Truncated(_))),
            "cut at {cut}"
        );
    }
}

/// The stage of one entry whose head is written raw: `kind`, `mode`, `path` and `target`, `size`.
fn raw(kind: u8, pad: u8, mode: u16, path: &[u8], target: &[u8], size: u64) -> Vec<u8> {
    let mut out = open(Op::Stage, "b").unwrap();
    out.push(kind);
    out.push(pad);
    out.extend(mode.to_le_bytes());
    out.extend(u16::try_from(path.len()).unwrap().to_le_bytes());
    out.extend(u16::try_from(target.len()).unwrap().to_le_bytes());
    out.extend(size.to_le_bytes());
    out.extend(path);
    out.extend(target);
    out
}

fn refused_name(path: &[u8]) -> Refused {
    read(LIMITS, &raw(1, 0, 0o755, path, b"", 0), 64).unwrap_err()
}

/// HEX-GUEST-9: send names that would leave the staged root, or aren't names: expect each refused as
/// [`Refused::Name`] naming the path and why, before anything is made.
#[test]
fn hostile_names_are_refused_by_name() {
    for (path, why) in [
        (&b"/etc/passwd"[..], "it is absolute"),
        (b"..", "it has a '..' component"),
        (b"a/../../etc", "it has a '..' component"),
        (b"a/./b", "it has a '.' component"),
        (b"a//b", "it has an empty component"),
        (b"a/", "it has an empty component"),
        (b"", "it is empty"),
        (b"a\0b", "it holds a NUL"),
        (b"\xff\xfe", "it isn't UTF-8"),
    ] {
        match refused_name(path) {
            Refused::Name { why: got, .. } => assert_eq!(got, why, "{path:?}"),
            other => panic!("{path:?}: {other:?}"),
        }
    }
    let long = "a".repeat(NAME_MAX + 1);
    assert!(matches!(
        refused_name(long.as_bytes()),
        Refused::Name { .. }
    ));
}

/// HEX-GUEST-9: send a device node's, a FIFO's and a socket's kind (there is none on the stream), a
/// mode with type bits, a pad byte set, a directory with a size, a file with a target, and a symlink
/// with no target: expect each refused by its type.
#[test]
fn hostile_kinds_modes_and_fields_are_refused_by_type() {
    for kind in [4, 5, 6, 0xff] {
        assert_eq!(
            read(LIMITS, &raw(kind, 0, 0o644, b"dev/null", b"", 0), 64),
            Err(Refused::Kind(kind))
        );
    }
    assert!(matches!(
        read(LIMITS, &raw(2, 0, 0o10_644, b"f", b"", 0), 64),
        Err(Refused::Mode { .. })
    ));
    assert_eq!(
        read(LIMITS, &raw(1, 9, 0o755, b"d", b"", 0), 64),
        Err(Refused::Pad(9))
    );
    assert!(matches!(
        read(LIMITS, &raw(1, 0, 0o755, b"d", b"", 1), 64),
        Err(Refused::Field { field: "size", .. })
    ));
    assert!(matches!(
        read(LIMITS, &raw(2, 0, 0o644, b"f", b"x", 0), 64),
        Err(Refused::Field {
            field: "target",
            ..
        })
    ));
    assert!(matches!(
        read(LIMITS, &raw(3, 0, 0o777, b"l", b"", 0), 64),
        Err(Refused::Target { .. })
    ));
    let mut end = open(Op::Stage, "b").unwrap();
    let mut bad_end = END;
    bad_end[8] = 1;
    end.extend(bad_end);
    assert!(matches!(read(LIMITS, &end, 64), Err(Refused::Field { .. })));
}

/// HEX-GUEST-9: open with the wrong magic, version, op or pad, and with an empty or over-long id:
/// expect each refused by its type.
#[test]
fn hostile_openings_are_refused_by_type() {
    let good = open(Op::Stage, "b").unwrap();
    let with = |at: usize, byte: u8| {
        let mut b = good.clone();
        b[at] = byte;
        read(LIMITS, &b, 64).unwrap_err()
    };
    assert_eq!(with(0, b'X'), Refused::Magic);
    assert_eq!(with(4, 2), Refused::Version { got: 2 });
    assert_eq!(with(6, 3), Refused::Op(3));
    assert_eq!(with(7, 1), Refused::Pad(1));
    let mut empty = good.clone();
    empty.truncate(OPEN_BYTES);
    empty[8] = 0;
    empty[9] = 0;
    assert!(matches!(read(LIMITS, &empty, 64), Err(Refused::Id(_))));
    let long = open(Op::Stage, &"a".repeat(129)).unwrap();
    assert!(matches!(read(LIMITS, &long, 64), Err(Refused::Id(_))));
}

/// HEX-GUEST-9: stage one entry past the entries bound, a file past the bytes bound, and a path and a
/// target past the path bound: expect each refused as over its bound from its head, before its path
/// or bytes are held.
#[test]
fn every_count_past_its_bound_is_refused_from_its_head() {
    let tight = Limits {
        entries: 2,
        bytes: 10,
        path: 8,
    };
    let three: Vec<(Entry, Vec<u8>)> = ["a", "b", "c"]
        .iter()
        .map(|p| (entry(p, What::Dir), vec![]))
        .collect();
    assert_eq!(
        read(tight, &stage("b", &three), 64),
        Err(Refused::Over {
            what: "the stage's entries",
            bound: 2
        })
    );
    // The head alone, its 11 bytes never sent: refused as soon as the head is whole.
    let big = raw(2, 0, 0o644, b"f", b"", 11);
    let head_only = &big[..OPEN_BYTES + 1 + HEAD_BYTES];
    assert_eq!(
        read(tight, head_only, 64),
        Err(Refused::Over {
            what: "the stage's file bytes",
            bound: 10
        })
    );
    let long_path = raw(1, 0, 0o755, b"abcdefghi", b"", 0);
    assert_eq!(
        read(tight, &long_path[..OPEN_BYTES + 1 + HEAD_BYTES], 64),
        Err(Refused::Over {
            what: "a path",
            bound: 8
        })
    );
    let long_target = raw(3, 0, 0o777, b"l", b"/abcdefgh", 0);
    assert_eq!(
        read(tight, &long_target[..OPEN_BYTES + 1 + HEAD_BYTES], 64),
        Err(Refused::Over {
            what: "a symbolic link's target",
            bound: 8
        })
    );
    // The packer refuses what the unpacker would.
    assert!(head(&entry("abcdefghi", What::Dir), &tight).is_err());
    assert!(head(&entry("../x", What::Dir), &LIMITS).is_err());
}

proptest! {
    /// HEX-GUEST-9: feed any bytes after a stage's opening: expect a refusal, steps, or a wait for
    /// more, never a panic, and never a name that leaves the root.
    #[test]
    fn any_bytes_are_read_or_refused(bytes in proptest::collection::vec(any::<u8>(), 0..512)) {
        let mut input = open(Op::Stage, "b").unwrap();
        input.extend(bytes);
        let mut u = Unpack::new(LIMITS);
        let mut rest = &input[..];
        while !rest.is_empty() {
            match u.take(rest) {
                Ok((took, step)) => {
                    if let Some(Step::Entry(e)) = step {
                        prop_assert!(check_path(&e.path, &LIMITS).is_ok());
                    }
                    rest = &rest[took..];
                }
                Err(_) => break,
            }
        }
    }
}
