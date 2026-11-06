//! HEX-GUEST-19 (native plan step 5, docs/design/shards-guest.md §6): the `oci` class's conformance suite,
//! every case, through a real Shards guest booted by `shards_api::Shards` from this host, the path a macOS
//! or Windows node takes: `hex_runtime::guest::WithGuest` boots one guest, stages each box's root into it
//! and forwards every call to the Linux node agent inside. "Boot the guest, run every case, expect each to
//! pass and the guest booted once."
//!
//! It needs a guest, so it runs only where one is given, and says SKIP and why elsewhere:
//! - `HEX_GUEST_KERNEL`: the guest's kernel (Shards' pinned 6.18; an arm64 `Image` or x86_64 `vmlinux`);
//! - `HEX_GUEST_INITRD`: its initramfs, whose `/init` is a static Linux `hex` (as PID 1, `hex` is the guest's init);
//! - `SHARDS_BIN`: the `shards` binary (on macOS the signed build, where it is installed);
//! - `HEX_GUEST_LINUX_HEX`: a static Linux `hex` for the guest's architecture, put at `/hex` in each
//!   box's root so the suite's programs (`hex selftest-box`) run there;
//! - optionally `HEX_GUEST_CPUS` and `HEX_GUEST_MEMORY_MIB` (else 2 and 1024, this test's own) and
//!   `HEX_GUEST_SHARES` (else the OS's default roots).
//!
//! Run: `cargo test -p hex-cli --test guest_real -- --nocapture` with those set.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Network, OciSpec, Program, Rootfs, Runtime};
use hex_runtime::guest::{GuestConfig, GuestRuntime, shares};

/// `oci` boxes whose root holds the Linux `hex` at `/hex` alone, their programs its selftest acts.
struct OciBoxes(PathBuf);

fn act(act: &Act) -> Program {
    let mut args = vec!["/hex".to_owned(), "selftest-box".to_owned()];
    args.extend(match act {
        Act::Say(words) => vec!["say".to_owned(), (*words).to_owned()],
        Act::Fail(code) => vec!["fail".to_owned(), code.to_string()],
        Act::Wait => vec!["wait".to_owned()],
        Act::Hog(bytes) => vec!["hog".to_owned(), bytes.to_string()],
        Act::Spawn(n) => vec!["spawn".to_owned(), n.to_string()],
        Act::Echo => vec!["echo".to_owned()],
    });
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    Program::of(&args)
}

impl OciBoxes {
    fn box_spec(&self, id: BoxId, bounds: Bounds, init: &Act) -> BoxSpec {
        BoxSpec {
            id,
            bounds,
            class: ClassSpec::Oci(Box::new(OciSpec {
                rootfs: Rootfs::Dir(self.0.clone()),
                init: act(init),
                mounts: Vec::new(),
                hostname: None,
                network: Network::Private,
                hosts: Vec::new(),
            })),
        }
    }
}

impl Fixture for OciBoxes {
    fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec {
        self.box_spec(id, bounds, &Act::Wait)
    }

    fn init(&self, id: BoxId, act: &Act) -> BoxSpec {
        self.box_spec(id, Bounds::default(), act)
    }

    fn program(&self, a: &Act) -> Program {
        act(a)
    }
}

fn given(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

#[test]
fn the_oci_class_holds_its_contract_through_a_real_shards_guest() {
    let needed = [
        "HEX_GUEST_KERNEL",
        "HEX_GUEST_INITRD",
        "SHARDS_BIN",
        "HEX_GUEST_LINUX_HEX",
    ];
    let missing: Vec<&str> = needed
        .iter()
        .copied()
        .filter(|n| std::env::var_os(n).is_none())
        .collect();
    if !missing.is_empty() {
        eprintln!("SKIP: no Shards guest given ({} unset)", missing.join(", "));
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let rootfs = root.path().join("rootfs");
    std::fs::create_dir_all(&rootfs).unwrap();
    std::fs::copy(given("HEX_GUEST_LINUX_HEX").unwrap(), rootfs.join("hex")).unwrap();
    make_executable(&rootfs.join("hex"));
    let number =
        |name: &str, default: u64| std::env::var(name).map_or(default, |v| v.parse().unwrap());
    let config = GuestConfig {
        binary: given("SHARDS_BIN").unwrap(),
        kernel: given("HEX_GUEST_KERNEL").unwrap(),
        initrd: given("HEX_GUEST_INITRD").unwrap(),
        vcpus: u32::try_from(number("HEX_GUEST_CPUS", 2)).unwrap(),
        memory_mib: number("HEX_GUEST_MEMORY_MIB", 1024),
        shares: shares::from_env().unwrap(),
        staged: hex_client::guest::stage::STAGED.to_owned(),
    };
    let guests = Arc::new(shards_api::Shards::new());
    let node = GuestRuntime::new(guests, config);
    let failures = conformance::run(&node, &OciBoxes(rootfs));
    assert!(
        failures.is_empty(),
        "oci boxes through a real Shards guest: {failures:#?}"
    );
    assert!(node.list().unwrap().is_empty(), "a box is left");
}

#[cfg(unix)]
fn make_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// NTFS holds no mode: the packer stages every file 0755 from Windows.
#[cfg(not(unix))]
fn make_executable(_path: &Path) {}
