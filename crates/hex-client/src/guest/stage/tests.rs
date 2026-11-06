//! The host packer's tests (docs/design/shards-guest.md §4a, HEX-GUEST-11; native plan step 5): a host
//! tree packs to the entries it holds, each parent before its children, files with their bytes; what is
//! never staged (a device node, a socket) is refused by its path; and the stage's bounds are refused
//! before anything past them is sent.

use std::collections::BTreeMap;

use super::*;
use hex_core::stream::files::{LIMITS, Step, Unpack};

fn id() -> BoxId {
    BoxId::new("box-1").unwrap()
}

/// What an unpacker reads from a stage: each entry by its path, a file's bytes beside it, in order.
fn unpack(bytes: &[u8]) -> Vec<(Entry, Vec<u8>)> {
    let mut u = Unpack::new(LIMITS);
    let mut out: Vec<(Entry, Vec<u8>)> = Vec::new();
    let mut rest = bytes;
    while !rest.is_empty() {
        let (took, step) = u.take(rest).unwrap();
        match step {
            Some(Step::Entry(e)) => out.push((e, Vec::new())),
            Some(Step::Data { .. }) => out.last_mut().unwrap().1.extend(&rest[..took]),
            Some(Step::Open { id, .. }) => assert_eq!(id, "box-1"),
            _ => {}
        }
        rest = &rest[took..];
    }
    u.finish().unwrap();
    out
}

/// HEX-GUEST-11: pack a folder holding a subfolder, a file, an empty file and (on Unix) a symlink, and
/// a bytes item: expect each as an entry under its item's name, the file's bytes after it, every
/// parent before its children.
#[test]
fn a_host_tree_packs_to_its_entries_parents_first() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("rootfs");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join("bin/sh"), b"#!sh\n").unwrap();
    std::fs::write(root.join("empty"), b"").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("/usr/lib", root.join("lib")).unwrap();
    let items = [
        Item {
            name: "root".to_owned(),
            source: Source::Path(root),
        },
        Item {
            name: "config.json".to_owned(),
            source: Source::Bytes(b"{}".to_vec()),
        },
    ];
    let mut out = Vec::new();
    pack(&mut out, &id(), &items, &LIMITS).unwrap();
    let entries = unpack(&out);
    let order: Vec<&str> = entries.iter().map(|(e, _)| e.path.as_str()).collect();
    for (i, path) in order.iter().enumerate() {
        if let Some((parent, _)) = path.rsplit_once('/') {
            assert!(order[..i].contains(&parent), "{path} came before {parent}");
        }
    }
    let by: BTreeMap<&str, &(Entry, Vec<u8>)> =
        entries.iter().map(|e| (e.0.path.as_str(), e)).collect();
    assert_eq!(by["root"].0.what, What::Dir);
    assert_eq!(by["root/bin"].0.what, What::Dir);
    assert_eq!(by["root/bin/sh"].0.what, What::File { size: 5 });
    assert_eq!(by["root/bin/sh"].1, b"#!sh\n");
    assert_eq!(by["root/empty"].0.what, What::File { size: 0 });
    assert_eq!(by["config.json"].1, b"{}");
    assert_eq!(by["config.json"].0.mode, 0o644);
    #[cfg(unix)]
    assert_eq!(
        by["root/lib"].0.what,
        What::Symlink {
            target: "/usr/lib".to_owned()
        }
    );
    assert_eq!(entries.len(), if cfg!(unix) { 6 } else { 5 });
}

/// HEX-GUEST-11: pack a root holding a device node and one holding a socket: expect each refused
/// naming the path and its kind, nothing staged of it.
#[cfg(unix)]
#[test]
fn a_device_node_or_socket_is_refused_by_its_path() {
    let item = |p: &Path| Item {
        name: "root".to_owned(),
        source: Source::Path(p.to_owned()),
    };
    let why = pack(&mut Vec::new(), &id(), &[item(Path::new("/dev/null"))], &LIMITS).unwrap_err();
    assert!(
        why.to_string().contains("/dev/null: a device node is never staged"),
        "{why}"
    );
    let dir = tempfile::tempdir().unwrap();
    let _sock = std::os::unix::net::UnixListener::bind(dir.path().join("s.sock")).unwrap();
    let why = pack(&mut Vec::new(), &id(), &[item(dir.path())], &LIMITS).unwrap_err();
    assert!(
        why.to_string().contains("s.sock: a socket is never staged"),
        "{why}"
    );
}

/// HEX-GUEST-11: pack past the entries bound and past the bytes bound: expect each refused as
/// exhausted, naming the bound, before the entry past it is sent.
#[test]
fn a_stage_past_its_bounds_is_refused_before_it_is_sent() {
    let dir = tempfile::tempdir().unwrap();
    for n in ["a", "b", "c"] {
        std::fs::write(dir.path().join(n), b"12345").unwrap();
    }
    let items = [Item {
        name: "root".to_owned(),
        source: Source::Path(dir.path().to_owned()),
    }];
    let few = Limits {
        entries: 3,
        ..LIMITS
    };
    let mut out = Vec::new();
    assert_eq!(
        pack(&mut out, &id(), &items, &few),
        Err(Refusal::Exhausted {
            what: "entries in one stage",
            bound: 3
        })
    );
    let small = Limits {
        bytes: 9,
        ..LIMITS
    };
    assert_eq!(
        pack(&mut Vec::new(), &id(), &items, &small),
        Err(Refusal::Exhausted {
            what: "file bytes in one stage",
            bound: 9
        })
    );
}
