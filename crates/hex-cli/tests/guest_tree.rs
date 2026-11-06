//! HEX-GUEST-21 (native plan step 5, docs/design/shards-guest.md §4a): each image's own layers, staged as a
//! tarball root, hold exactly what containerd's unpack of the same layers holds. "Stage each image's layers,
//! walk the staged root with `hex selftest-box tree`, expect every entry as ctr's snapshot has it": its kind,
//! mode, owner, mtime, link text, hard-link group, xattrs and sha256. The stated differences, and no others:
//! - atime (the walker doesn't write it: reading a file moves it);
//! - the root directory itself (the walker doesn't list it: its owner and times are whoever made it);
//! - the mtime of directories mkparent made or made a child in (both sides stamp them when the unpack runs):
//!   a directory's mtime may differ only where both sides' are from their own unpack, at or after its start;
//! - security.selinux labels the host assigned, which the image's layers don't carry.
//!
//! The reference is `bench/native/guest-tree/reference.py`'s: each image's layer blobs byte-exact from
//! containerd's content store and its snapshot's walk (`<image>/layers.txt`, `layer<i>.tar.gz`,
//! `reference.tree`), with `RECORD.txt` saying how each was made. Given as:
//! - `HEX_GUEST21_REFERENCE`: that folder;
//! - `HEX_GUEST21_SINCE`: unix seconds no later than the reference's unpack began (its RECORD).
//!
//! Hex's side, one of:
//! - a real guest (`HEX_GUEST_KERNEL`, `HEX_GUEST_INITRD`, `SHARDS_BIN`, as HEX-GUEST-19): the layers are
//!   staged into the guest's layer store over its staging port, and a `process` box in the guest running
//!   the guest's own `hex` walks them through the overlay the node mounts for a box of that image
//!   (`/init selftest-box tree --layers`), so fidelity is proved through the guest's real overlayfs;
//! - on Linux, as root, `HEX_GUEST21_LOCAL=1`: staged by the relay in this process, walked through a real
//!   overlay mount by this build's `hex` (the check before a guest run).
//!
//! `HEX_GUEST21_OUT`, when set, gets each image's Hex-side walk (`<image>.tree`) for the record. Run:
//! `cargo test -p hex-cli --test guest_tree -- --nocapture` with those set; elsewhere it says SKIP and why.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use hex_api::BoxId;
use hex_client::guest::stage::{Item, Source};

/// One image of the reference: its name, its layers in order, and ctr's walk.
struct Image {
    name: String,
    layers: Vec<PathBuf>,
    reference: String,
}

fn images(dir: &Path) -> Vec<Image> {
    let mut all: Vec<Image> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.join("reference.tree").exists() && p.join("layers.txt").exists())
        .map(|p| Image {
            name: p.file_name().unwrap().to_str().unwrap().to_owned(),
            layers: std::fs::read_to_string(p.join("layers.txt"))
                .unwrap()
                .lines()
                .filter(|l| !l.is_empty())
                .map(|l| p.join(l))
                .collect(),
            reference: std::fs::read_to_string(p.join("reference.tree")).unwrap(),
        })
        .collect();
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all
}

/// Whether any of `layers` carries a security.selinux xattr of its own (a PAX `SCHILY.xattr.` or
/// `LIBARCHIVE.xattr.` key): then the image's labels are compared, else the host's are left out.
fn carries_selinux(layers: &[PathBuf]) -> bool {
    layers.iter().any(|layer| {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(std::fs::File::open(layer).unwrap())
            .read_to_end(&mut bytes)
            .unwrap();
        bytes
            .windows(b"xattr.security.selinux".len())
            .any(|w| w == b"xattr.security.selinux")
    })
}

/// A manifest's entries by path: kind, mode, uid, gid, mtime, link, group, xattrs, sha256.
fn entries(text: &str) -> BTreeMap<String, Vec<String>> {
    text.lines()
        .map(|line| {
            let mut fields: Vec<String> = line.split('\t').map(str::to_owned).collect();
            let path = fields.remove(0);
            assert_eq!(fields.len(), 9, "a manifest line: {line}");
            (path, fields)
        })
        .collect()
}

/// `xattrs` without security.selinux, when the host assigned it.
fn without_selinux(xattrs: &str) -> String {
    let kept: Vec<&str> = xattrs
        .split(',')
        .filter(|x| *x != "-" && !x.starts_with("security.selinux="))
        .collect();
    if kept.is_empty() {
        "-".to_owned()
    } else {
        kept.join(",")
    }
}

/// The seconds of a manifest's `seconds.nanoseconds` mtime.
fn secs(mtime: &str) -> i64 {
    mtime.split('.').next().unwrap().parse().unwrap()
}

/// What the comparison found: every difference past the stated ones, and how often each stated one was
/// used (non-vacuity counters: a directory mtime both sides stamped, a host-assigned label left out).
#[derive(Debug, Default)]
struct Found {
    diffs: Vec<String>,
    stamped: usize,
    labels: usize,
}

/// Every difference between ctr's walk and Hex's, past the stated ones.
fn differences(reference: &str, hex: &str, selinux: bool, since_ref: i64, since_hex: i64) -> Found {
    let (want, got) = (entries(reference), entries(hex));
    let mut found = Found::default();
    let out = &mut found.diffs;
    for path in want
        .keys()
        .chain(got.keys().filter(|k| !want.contains_key(*k)))
    {
        let (Some(w), Some(g)) = (want.get(path), got.get(path)) else {
            out.push(format!(
                "{path}: only in {}",
                if want.contains_key(path) {
                    "ctr's"
                } else {
                    "Hex's"
                }
            ));
            continue;
        };
        const NAMES: [&str; 9] = [
            "kind", "mode", "uid", "gid", "mtime", "link", "group", "xattrs", "sha256",
        ];
        for (i, name) in NAMES.iter().enumerate() {
            let (mut a, mut b) = (w[i].clone(), g[i].clone());
            if *name == "xattrs" && !selinux {
                let (a2, b2) = (without_selinux(&a), without_selinux(&b));
                found.labels += usize::from(a2 != a) + usize::from(b2 != b);
                (a, b) = (a2, b2);
            }
            if *name == "mtime" && w[0] == "d" && secs(&a) >= since_ref && secs(&b) >= since_hex {
                found.stamped += 1;
                continue;
            }
            if a != b {
                out.push(format!("{path}: {name} ctr {a} Hex {b}"));
            }
        }
    }
    found
}

fn now() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
}

/// The image's layers, each named by its digest (its bytes' sha256), as the host's node stages them (§4a's
/// layer store).
fn layers(image: &Image) -> Vec<hex_client::guest::stage::Layer> {
    image
        .layers
        .iter()
        .map(|path| hex_client::guest::stage::Layer {
            digest: Some(
                <sha2::Sha256 as sha2::Digest>::digest(std::fs::read(path).unwrap()).into(),
            ),
            path: path.clone(),
        })
        .collect()
}

fn items(image: &Image) -> Vec<Item> {
    vec![Item {
        name: "root".to_owned(),
        source: Source::Layers(layers(image)),
    }]
}

/// Box `id`'s layers under `staged`, lowest first, `:`-joined, each its `layers/<i>` (a link to the store's
/// folder, or the box's own): what the walker mounts as the overlay the guest's node mounts for the box.
fn box_layers(staged: &str, id: &BoxId, image: &Image) -> String {
    (0..image.layers.len())
        .map(|i| hex_client::guest::stage::box_layer_path(staged, id, i))
        .collect::<Vec<_>>()
        .join(":")
}

/// Hex's side: stages and walks each image, returning its walk.
trait Side {
    fn walk(&self, id: &BoxId, image: &Image) -> String;
}

#[cfg(target_os = "linux")]
mod local {
    use std::path::PathBuf;
    use std::sync::Arc;

    use hex_api::{BoxId, Refusal};
    use hex_client::guest::{Duplex, Link, stage};
    use hex_core::stream::files::LIMITS;

    use super::{Image, Side, box_layers, items};

    /// The relay in this process: each dial a socket pair whose far end it serves.
    struct InProcess(PathBuf);

    impl Link for InProcess {
        fn dial(&self, _port: u32) -> Result<Box<dyn Duplex>, Refusal> {
            let (near, far) = std::os::unix::net::UnixStream::pair()
                .map_err(|e| Refusal::os("pairing a staging socket", e))?;
            let staged = self.0.clone();
            std::thread::spawn(move || {
                hex_node::relay::files::serve(Box::new(far), &staged, &LIMITS)
            });
            Ok(Box::new(near))
        }
    }

    pub struct Local {
        pub dir: tempfile::TempDir,
    }

    impl Side for Local {
        fn walk(&self, id: &BoxId, image: &Image) -> String {
            let link: Arc<dyn Link> = Arc::new(InProcess(self.dir.path().to_owned()));
            stage::stage(link.as_ref(), id, &items(image), &LIMITS)
                .unwrap_or_else(|why| panic!("{}: staging: {why}", image.name));
            let staged = self.dir.path().to_str().unwrap();
            let out = std::process::Command::new(env!("CARGO_BIN_EXE_hex"))
                .args(["selftest-box", "tree", "--layers"])
                .arg(box_layers(staged, id, image))
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}: walk: {}",
                image.name,
                String::from_utf8_lossy(&out.stderr)
            );
            stage::unstage(link.as_ref(), id).unwrap();
            String::from_utf8(out.stdout).unwrap()
        }
    }
}

/// A real guest: the layers staged over its staging port, walked by a `process` box in it.
struct Guest {
    link: std::sync::Arc<dyn hex_client::guest::Link>,
    client: hex_client::guest::Client,
    _guest: std::sync::Arc<dyn shards_api::Guest>,
}

impl Side for Guest {
    fn walk(&self, id: &BoxId, image: &Image) -> String {
        use hex_api::{Bounds, BoxSpec, ClassSpec, Exit, Program, Runtime, Stdio};
        use hex_client::guest::stage;
        use hex_core::stream::files::LIMITS;
        stage::stage(self.link.as_ref(), id, &items(image), &LIMITS)
            .unwrap_or_else(|why| panic!("{}: staging: {why}", image.name));
        let walker = BoxId::new(&format!("{}-walk", id.as_str())).unwrap();
        let _ = self.client.delete(&walker);
        self.client
            .create(BoxSpec {
                id: walker.clone(),
                bounds: Bounds::default(),
                class: ClassSpec::Process,
            })
            .unwrap();
        let layers = box_layers(stage::STAGED, id, image);
        let (mut r, w) = std::io::pipe().unwrap();
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            r.read_to_string(&mut text).unwrap();
            text
        });
        let mut p = self
            .client
            .exec(
                &walker,
                Program::of(&["/init", "selftest-box", "tree", "--layers", &layers]),
                Stdio {
                    stdout: Some(w),
                    ..Stdio::null()
                },
            )
            .unwrap();
        // The walk's output ends when it does (the fact); its end after that is recorded on every run, the
        // distribution its bound is to come from (the lead's rule for learned waits). Until that record
        // exists, it waits within the node's own answer patience.
        let text = reader.join().unwrap();
        let after = std::time::Instant::now();
        let exit = p
            .wait_for(hex_client::ANSWER_PATIENCE)
            .unwrap()
            .expect("the walk ended");
        eprintln!(
            "{}: the walk ended {} us after its output did",
            image.name,
            after.elapsed().as_micros()
        );
        self.client.delete(&walker).unwrap();
        stage::unstage(self.link.as_ref(), id).unwrap();
        assert_eq!(exit, Exit::Code(0), "{}: the walk", image.name);
        text
    }
}

fn given(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

/// The bytes of `layers` unpacked: their tar streams' lengths, at least what the staged root's files hold.
fn unpacked(layers: &[PathBuf]) -> u64 {
    layers
        .iter()
        .map(|l| {
            std::io::copy(
                &mut flate2::read::GzDecoder::new(std::fs::File::open(l).unwrap()),
                &mut std::io::sink(),
            )
            .unwrap()
        })
        .sum()
}

/// A guest of Shards' least size (one vCPU, `min_memory_mib`), and memory for the largest image staged in
/// it: the guest's staged folder is a tmpfs, which holds half its memory by default (tmpfs(5)), so twice
/// that image's unpacked bytes more.
fn guest(images: &[Image]) -> Option<Guest> {
    use shards_api::Guests;
    let (kernel, initrd, binary) = (
        given("HEX_GUEST_KERNEL")?,
        given("HEX_GUEST_INITRD")?,
        given("SHARDS_BIN")?,
    );
    let config = hex_runtime::guest::GuestConfig {
        binary,
        kernel,
        initrd,
        vcpus: 1,
        memory_mib: {
            let largest = images
                .iter()
                .map(|i| unpacked(&i.layers))
                .max()
                .unwrap_or(0);
            let mib = (2 * largest).div_ceil(1 << 20);
            (shards_api::min_memory_mib(std::env::consts::ARCH) + mib).next_multiple_of(2)
        },
        shares: Vec::new(),
        staged: hex_client::guest::stage::STAGED.to_owned(),
    };
    let guest: std::sync::Arc<dyn shards_api::Guest> = shards_api::Shards::new()
        .boot(&config.spec())
        .unwrap()
        .into();
    let link: std::sync::Arc<dyn hex_client::guest::Link> =
        std::sync::Arc::new(hex_client::guest::shards::ShardsLink::new(
            guest.clone(),
            2 * hex_runtime::guest::recorded_boot()
                .expect("this host class's boot-to-ready record"),
        ));
    let client = hex_client::guest::Client::connect(link.clone()).unwrap();
    Some(Guest {
        link,
        client,
        _guest: guest,
    })
}

#[test]
fn each_images_staged_root_holds_what_containerds_unpack_holds() {
    let (Some(reference), Some(since)) = (
        given("HEX_GUEST21_REFERENCE"),
        std::env::var("HEX_GUEST21_SINCE").ok(),
    ) else {
        eprintln!("SKIP: no reference given (HEX_GUEST21_REFERENCE, HEX_GUEST21_SINCE unset)");
        return;
    };
    let since_ref: i64 = since.parse().unwrap();
    let since_hex = now();
    let side: Box<dyn Side> = if std::env::var_os("HEX_GUEST21_LOCAL").is_some() {
        #[cfg(target_os = "linux")]
        {
            Box::new(local::Local {
                dir: tempfile::tempdir().unwrap(),
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            eprintln!("SKIP: HEX_GUEST21_LOCAL stages in-process, on Linux alone");
            return;
        }
    } else {
        match guest(&images(&reference)) {
            Some(g) => Box::new(g),
            None => {
                eprintln!(
                    "SKIP: no Shards guest given (HEX_GUEST_KERNEL, HEX_GUEST_INITRD, SHARDS_BIN) \
                     and not HEX_GUEST21_LOCAL"
                );
                return;
            }
        }
    };
    let all = images(&reference);
    assert!(!all.is_empty(), "no images under {}", reference.display());
    let mut failed = Vec::new();
    for (i, image) in all.iter().enumerate() {
        let id = BoxId::new(&format!("g21-{i}")).unwrap();
        let hex = side.walk(&id, image);
        if let Some(out) = given("HEX_GUEST21_OUT") {
            std::fs::create_dir_all(&out).unwrap();
            std::fs::write(out.join(format!("{}.tree", image.name)), &hex).unwrap();
        }
        let selinux = carries_selinux(&image.layers);
        let found = differences(&image.reference, &hex, selinux, since_ref, since_hex);
        let diffs = found.diffs;
        eprintln!(
            "{}: {} entries in ctr's, {} in Hex's, {} differences past the stated ones; stated: {} \
             directory mtimes both sides stamped, {} host-assigned labels left out{}",
            image.name,
            image.reference.lines().count(),
            hex.lines().count(),
            diffs.len(),
            found.stamped,
            found.labels,
            if selinux {
                " (its layers carry labels: compared)"
            } else {
                ""
            }
        );
        for d in &diffs {
            eprintln!("  {d}");
        }
        if !diffs.is_empty() {
            failed.push(image.name.clone());
        }
    }
    assert!(
        failed.is_empty(),
        "images that differ from ctr's: {failed:?}"
    );
}

/// HEX-GUEST-21's comparison itself: two walks that differ in one file's mode, in a directory's mtime from
/// a header, in a host-assigned label and in a mkparent directory's stamp: expect the mode and the header
/// mtime named, the label and the stamp passed as stated.
#[test]
fn the_comparison_names_every_difference_but_the_stated_ones() {
    let line = |p: &str, k: &str, mode: &str, mtime: &str, x: &str| {
        format!("\"{p}\"\t{k}\t{mode}\t0\t0\t{mtime}\t-\t-\t{x}\t-\n")
    };
    let reference = [
        line("a", "f", "0644", "100.000000000", "security.selinux=00"),
        line("d", "d", "0755", "200.000000000", "-"),
        line("m", "d", "0755", "5000.000000000", "-"),
    ]
    .concat();
    let hex = [
        line("a", "f", "0600", "100.000000000", "-"),
        line("d", "d", "0755", "201.000000000", "-"),
        line("m", "d", "0755", "9000.000000000", "-"),
    ]
    .concat();
    let found = differences(&reference, &hex, false, 4000, 8000);
    assert_eq!((found.stamped, found.labels), (1, 1));
    assert_eq!(
        found.diffs,
        [
            "\"a\": mode ctr 0644 Hex 0600",
            "\"d\": mtime ctr 200.000000000 Hex 201.000000000"
        ]
    );
    // An image whose layers carry labels has them compared.
    assert_eq!(
        differences(&reference, &hex, true, 4000, 8000).diffs.len(),
        3
    );
}
