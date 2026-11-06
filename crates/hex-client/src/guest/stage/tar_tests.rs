//! The packer's tarball roots (docs/design/shards-guest.md §4a, HEX-GUEST-15; native plan step 5): a root
//! given as a `.tar` or `.tar.gz` packs to its own entries, Linux modes and symlink texts exact; and
//! hostile archives are refused by name before anything past the fault is sent: absolute names, `..`,
//! names and counts past the bounds, device, FIFO and hard-link entries, an unknown entry type, an entry
//! before its directory, a gzip stream whose trailer lies, an archive cut short. A socket has no tar entry
//! type, so an unknown type stands for it.

use std::collections::BTreeMap;

use super::tests::{id, unpack};
use super::*;
use hex_core::stream::files::LIMITS;

/// A raw tar header for `name`, of `kind` (its typeflag byte), `mode`, `size`, linking to `link`.
fn header(name: &[u8], kind: u8, mode: u32, size: u64, link: &[u8]) -> [u8; 512] {
    let mut h = [0u8; 512];
    h[..name.len()].copy_from_slice(name);
    let octal = |field: &mut [u8], v: u64| {
        let s = format!("{:0width$o}\0", v, width = field.len() - 1);
        field.copy_from_slice(s.as_bytes());
    };
    octal(&mut h[100..108], u64::from(mode));
    octal(&mut h[108..116], 0);
    octal(&mut h[116..124], 0);
    octal(&mut h[124..136], size);
    octal(&mut h[136..148], 0);
    h[156] = kind;
    h[157..157 + link.len()].copy_from_slice(link);
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
    let s = format!("{sum:06o}\0 ");
    h[148..156].copy_from_slice(s.as_bytes());
    h
}

/// An archive of `entries` (each a header and its data), ended with its two zero blocks.
fn archive(entries: &[([u8; 512], &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    for (h, data) in entries {
        out.extend_from_slice(h);
        out.extend_from_slice(data);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.extend_from_slice(&[0u8; 1024]);
    out
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    gz.write_all(bytes).unwrap();
    gz.finish().unwrap()
}

/// Packs `bytes` as the tarball `file` (its name says whether it's gzip) under the item `root`.
fn pack_tar(file: &str, bytes: &[u8], limits: &Limits) -> Result<Vec<u8>, Refusal> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(file);
    std::fs::write(&path, bytes).unwrap();
    let items = [Item {
        name: "root".to_owned(),
        source: Source::Tar(path),
    }];
    let mut out = Vec::new();
    pack(&mut out, &id(), &items, limits).map(|()| out)
}

fn refused(file: &str, bytes: &[u8]) -> String {
    pack_tar(file, bytes, &LIMITS).unwrap_err().to_string()
}

fn sample() -> Vec<u8> {
    archive(&[
        (header(b"./", b'5', 0o755, 0, b""), b""),
        (header(b"./bin/", b'5', 0o755, 0, b""), b""),
        (header(b"./bin/busybox", b'0', 0o4755, 5, b""), b"#!bb\n"),
        (
            header(b"./bin/ls", b'2', 0o777, 0, b"/usr/bin/busybox"),
            b"",
        ),
        (header(b"./etc/", b'5', 0o700, 0, b""), b""),
        (header(b"./etc/empty", b'0', 0o600, 0, b""), b""),
    ])
}

/// HEX-GUEST-15: pack a root given as a tarball, plain and gzip: expect its entries under the item's name,
/// each with the archive's own mode, the file's bytes, and the symlink's text exactly as written.
#[test]
fn a_tarball_root_packs_to_its_own_entries_modes_and_link_texts_exact() {
    for (file, bytes) in [("root.tar", sample()), ("root.tar.gz", gzip(&sample()))] {
        let entries = unpack(&pack_tar(file, &bytes, &LIMITS).unwrap());
        let by: BTreeMap<&str, &(Entry, Vec<u8>)> =
            entries.iter().map(|e| (e.0.path.as_str(), e)).collect();
        assert_eq!(by["root"].0.what, What::Dir, "{file}");
        assert_eq!(by["root/bin/busybox"].0.mode, 0o4755, "{file}");
        assert_eq!(by["root/bin/busybox"].1, b"#!bb\n", "{file}");
        assert_eq!(
            by["root/bin/ls"].0.what,
            What::Symlink {
                target: "/usr/bin/busybox".to_owned()
            },
            "{file}"
        );
        assert_eq!(by["root/etc"].0.mode, 0o700, "{file}");
        assert_eq!(
            by["root/etc/empty"].0.what,
            What::File { size: 0 },
            "{file}"
        );
        assert_eq!(entries.len(), 6, "{file}");
    }
}

/// HEX-GUEST-15: pack archives naming their way out (an absolute name, a `..` component) and one whose
/// entry comes before its directory: expect each refused naming the archive's entry and why.
#[test]
fn a_tarball_naming_its_way_out_or_out_of_order_is_refused_by_name() {
    for (name, why) in [
        (&b"/etc/passwd"[..], "is absolute"),
        (b"a/../../etc/passwd", "has a '..' component"),
        (b"nodir/file", "comes before its directory"),
    ] {
        let bytes = archive(&[(header(name, b'0', 0o644, 1, b""), b"x")]);
        let got = refused("r.tar", &bytes);
        assert!(got.contains(why), "{name:?}: {got}");
    }
}

/// HEX-GUEST-15: pack archives holding a character device, a block device, a FIFO, a hard link and an entry
/// of a type tar doesn't define (as a socket would be, which tar can't carry): expect each refused by name.
#[test]
fn a_tarball_with_a_device_fifo_hard_link_or_unknown_kind_is_refused_by_name() {
    for (kind, why) in [
        (b'3', "a device node is never staged"),
        (b'4', "a device node is never staged"),
        (b'6', "a FIFO is never staged"),
        (b'1', "a hard link is never staged"),
        (b'Z', "an entry of a kind staging carries not"),
    ] {
        let bytes = archive(&[(header(b"x", kind, 0o644, 0, b"y"), b"")]);
        let got = refused("r.tar", &bytes);
        assert!(got.contains(why), "{}: {got}", kind as char);
    }
}

/// HEX-GUEST-15: pack a tarball past the entries bound, past the bytes bound, and with a name past the path
/// bound: expect each refused naming the bound, before the entry past it is sent.
#[test]
fn a_tarball_past_its_bounds_is_refused() {
    let three = archive(&[
        (header(b"a", b'0', 0o644, 4, b""), b"1234"),
        (header(b"b", b'0', 0o644, 4, b""), b"1234"),
        (header(b"c", b'0', 0o644, 4, b""), b"1234"),
    ]);
    let few = Limits {
        entries: 3,
        ..LIMITS
    };
    assert_eq!(
        pack_tar("r.tar", &three, &few),
        Err(Refusal::Exhausted {
            what: "entries in one stage",
            bound: 3
        })
    );
    let small = Limits { bytes: 7, ..LIMITS };
    assert_eq!(
        pack_tar("r.tar", &three, &small),
        Err(Refusal::Exhausted {
            what: "file bytes in one stage",
            bound: 7
        })
    );
    let short = Limits { path: 8, ..LIMITS };
    let long = archive(&[(header(b"abcdefghij", b'0', 0o644, 0, b""), b"")]);
    assert!(pack_tar("r.tar", &long, &short).is_err());
}

/// HEX-GUEST-15: pack a gzip tarball whose trailer lies (its length, then its CRC), an archive cut in the
/// middle of a file, and one cut before its end-of-archive block: expect each refused, never staged as whole.
#[test]
fn a_lying_or_truncated_tarball_is_refused() {
    let good = gzip(&sample());
    let n = good.len();
    for at in [n - 1, n - 5] {
        let mut lies = good.clone();
        lies[at] ^= 0x01;
        let got = refused("r.tar.gz", &lies);
        assert!(got.contains("r.tar.gz"), "trailer byte {at}: {got}");
    }
    let plain = sample();
    let mid_file = &plain[..512 * 3 + 2];
    assert!(refused("r.tar", mid_file).contains("r.tar"));
    let no_end = &plain[..plain.len() - 1024];
    let got = refused("r.tar", no_end);
    assert!(got.contains("end-of-archive"), "{got}");
    let cut_gz = &good[..n / 2];
    assert!(refused("r.tar.gz", cut_gz).contains("r.tar.gz"));
}
