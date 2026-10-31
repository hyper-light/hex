//! Goal 5's extension (AC-T7.1.1, T-T7.1.4): a rootless `oci` box on the host's network must not be
//! able to change another box's cgroup, nor read the host cgroup tree. Such a box binds the host's
//! `/sys` (it can't mount a sysfs of its own in the host's network namespace), which carries the
//! host's cgroup filesystem beneath the box's own (crates/hex-core/src/bundle.rs, `default_mounts`). A
//! box holding `CAP_SYS_ADMIN` could unmount its own cgroup mounts off and reach that tree.
//!
//! "do X, expect Y" (CLAUDE.md §6), on a rootless node:
//!   - create a box from the bundle the node renders for a rootless host-network box, with
//!     `CAP_SYS_ADMIN` added to its capability sets: expect the create refused by name (the spec's
//!     check refuses a container in a user namespace holding `CAP_SYS_ADMIN` without a network
//!     namespace of its own, on Ada's call, 2026-10-31);
//!   - in a plain rootless host-network box, read its mount table: expect the host cgroup filesystem
//!     its `/sys` bind carries, beneath its own, mounted read-only (the bind's remount makes the
//!     carried cgroup filesystems read-only, which a recursive bind's read-only otherwise leaves to
//!     its top), so a box that ever reached it couldn't change it.
//!
//! Rootless where the caller isn't root, in a delegated cgroup v2 subtree, as oci_box.rs; where that
//! isn't possible it prints SKIP and why rather than pass falsely. The refused box goes through the
//! bundle path (`ClassSpec::Bundle`) because an `OciSpec` carries no capabilities.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use hex_api::{
    Bounds, BoxId, BoxSpec, ClassSpec, Exit, Mount, Network, OciSpec, Program, Rootfs, Runtime,
    Stdio,
};
use hex_core::bundle::{Owner, Place, render};
use hex_node::{Node, OCI_BOXES};

const TEST: &str = "a_rootless_host_network_box_cannot_change_another_boxs_cgroup";
/// Bounded wait for the box's program that reads its mount table: one that never ends fails rather
/// than hangs (CLAUDE.md §4).
const PATIENCE: Duration = Duration::from_secs(30);

#[test]
fn a_rootless_host_network_box_cannot_change_another_boxs_cgroup() {
    let exe = env!("CARGO_BIN_EXE_hex").to_owned();
    let cache = cache_dir();
    match prepare_cgroup(&cache) {
        Ok(own) => exercise(&own, &cache, &exe),
        Err(direct) if std::env::var_os("HEX_DELEGATED").is_none() => {
            let has_systemd = Command::new("systemd-run")
                .arg("--version")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !has_systemd {
                eprintln!(
                    "SKIP: own cgroup isn't ours to subdivide ({direct}) and no systemd --user for a private scope"
                );
                return;
            }
            let status = Command::new("systemd-run")
                .args(["--user", "--scope", "--quiet", "-p", "Delegate=yes"])
                .arg(std::env::current_exe().expect("test binary path"))
                .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
                .env("HEX_DELEGATED", "1")
                .status()
                .expect("run the test in a delegated scope");
            assert!(status.success(), "the delegated re-run failed");
        }
        Err(direct) => {
            eprintln!("SKIP: no usable cgroup v2 delegation ({direct}); run on a cgroup-v2 host")
        }
    }
}

fn exercise(own: &Path, cache: &Path, exe: &str) {
    let uid = host_id("-u");
    let gid = host_id("-g");
    eprintln!(
        "oci_cgroupfs_box: {} (uid {uid}), kernel {}",
        if uid != 0 { "rootless" } else { "as root" },
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .unwrap_or_default()
            .trim()
    );
    if uid == 0 {
        // As root a box shares the host user namespace and gets a fresh sysfs, not a bind of the
        // host's, so there is no host cgroup filesystem beneath its own: nothing to guard here.
        eprintln!("SKIP: this guard is for a rootless node; as root no host cgroup fs is bound in");
        return;
    }
    if !userns_available() {
        eprintln!("SKIP: unprivileged user namespaces aren't available here");
        return;
    }

    let work = tempfile::Builder::new()
        .prefix("ocicgfs-")
        .tempdir_in(cache)
        .expect("work dir");
    let out = work.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let plain_rootfs = work.path().join("rootfs-plain");
    build_rootfs(&plain_rootfs);
    let attacker_rootfs = work.path().join("rootfs-attacker");
    build_rootfs(&attacker_rootfs);

    let box_root = own.join("hex-oci-cgfs");
    let state = work.path().join("state");
    std::fs::create_dir_all(&state).unwrap();
    let node = Node::open(&box_root, &state, &state)
        .expect("open a node")
        .with_oci_helper(PathBuf::from(exe));

    // A box holding CAP_SYS_ADMIN without a network namespace of its own: the bundle the node renders
    // for a rootless host-network box, with CAP_SYS_ADMIN added to its capability sets. Its create is
    // refused by name, and nothing of it is left.
    let bundle_dir = work.path().join("attacker-bundle");
    std::fs::create_dir_all(&bundle_dir).unwrap();
    render_attacker_bundle(&bundle_dir, &attacker_rootfs, Owner::Rootless { uid, gid });
    let attacker = BoxId::new("attacker").unwrap();
    let made = node.create(BoxSpec {
        id: attacker.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Bundle(bundle_dir),
    });
    let _attacker_cleanup = Deleting(&node, attacker.clone());
    let why = match made {
        Ok(_) => panic!("a rootless host-network box holding CAP_SYS_ADMIN was made"),
        Err(e) => e.to_string(),
    };
    assert!(
        why.contains("CAP_SYS_ADMIN") && why.contains("network namespace"),
        "the box was refused, but not by name: {why}"
    );
    assert!(
        !box_root.join(OCI_BOXES).join(attacker.as_str()).exists(),
        "the refused box left its cgroup"
    );

    // A plain rootless host-network box reads its mount table into /out.
    let plain = BoxId::new("plain").unwrap();
    node.create(BoxSpec {
        id: plain.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Oci(Box::new(OciSpec {
            rootfs: Rootfs::Dir(plain_rootfs.clone()),
            init: Program::of(&["/usr/bin/sleep", "600"]),
            mounts: vec![bind("/usr", "/usr", true), bind_path(&out, "/out", false)],
            hostname: None,
            network: Network::Host,
            hosts: Vec::new(),
        })),
    })
    .expect("create the plain box");
    let _plain_cleanup = Deleting(&node, plain.clone());
    node.start(&plain).expect("start the plain box");
    let mut reader = Program::of(&[
        "/usr/bin/sh",
        "-c",
        "cat /proc/self/mountinfo > /out/mountinfo",
    ]);
    reader
        .env
        .push(("PATH".to_owned(), "/usr/bin:/bin".to_owned()));
    let ended = node
        .exec(&plain, reader, Stdio::null())
        .expect("exec the mount table's reader")
        .wait_for(PATIENCE)
        .expect("wait for the reader")
        .expect("the reader ended within its patience");
    assert_eq!(ended, Exit::Code(0), "the reader failed");
    let mountinfo = std::fs::read_to_string(out.join("mountinfo")).expect("the box's mount table");

    // The cgroup filesystems at /sys/fs/cgroup, in the order they were mounted: the host's, carried by
    // the /sys bind, then the box's own over it (proc_pid_mountinfo(5): field 5 the mount point, field
    // 6 its options, the filesystem type after " - ").
    let cgroups: Vec<(&str, &str)> = mountinfo
        .lines()
        .filter_map(|line| {
            let (left, right) = line.split_once(" - ")?;
            let fields: Vec<&str> = left.split(' ').collect();
            let kind = right.split(' ').next()?;
            (matches!(kind, "cgroup2" | "cgroup") && fields.get(4) == Some(&"/sys/fs/cgroup"))
                .then(|| (*fields.get(4).unwrap_or(&""), *fields.get(5).unwrap_or(&"")))
        })
        .collect();
    assert!(
        cgroups.len() >= 2,
        "the host's cgroup filesystem and the box's own are both at /sys/fs/cgroup:\n{mountinfo}"
    );
    for (point, options) in &cgroups[..cgroups.len() - 1] {
        assert!(
            options.split(',').any(|o| o == "ro"),
            "the host cgroup filesystem the box's /sys bind carries at {point} is writable ({options}):\n{mountinfo}"
        );
    }

    node.delete(&plain).expect("delete the plain box");
}

/// Renders the rootless host-network bundle the node would render for the attacker's spec, adds
/// CAP_SYS_ADMIN to its capability sets, and writes `config.json` and `hosts` into `bundle_dir`.
fn render_attacker_bundle(bundle_dir: &Path, rootfs: &Path, owner: Owner) {
    let spec = OciSpec {
        rootfs: Rootfs::Dir(rootfs.to_path_buf()),
        init: Program::of(&["/usr/bin/sleep", "600"]),
        mounts: vec![bind("/usr", "/usr", true)],
        hostname: None,
        network: Network::Host,
        hosts: Vec::new(),
    };
    let rendered = render(
        &spec,
        &Place {
            dir: bundle_dir,
            owner,
            masked: &[],
        },
    )
    .expect("render the attacker bundle");
    // Add CAP_SYS_ADMIN to each capability set. The rendered config lists Docker's 14 defaults, which
    // end in CAP_AUDIT_WRITE and leave out CAP_SYS_ADMIN; each of the bounding, effective and
    // permitted sets ends in that one, so extending it there adds the capability to all three.
    let config = String::from_utf8(rendered.config).expect("the rendered config is UTF-8");
    assert_eq!(
        config.matches("\"CAP_AUDIT_WRITE\"").count(),
        3,
        "three capability sets to extend with CAP_SYS_ADMIN: {config}"
    );
    let config = config.replace(
        "\"CAP_AUDIT_WRITE\"",
        "\"CAP_AUDIT_WRITE\", \"CAP_SYS_ADMIN\"",
    );
    std::fs::write(bundle_dir.join("config.json"), config).expect("write config.json");
    std::fs::write(bundle_dir.join("hosts"), &rendered.hosts).expect("write hosts");
}

fn bind(source: &str, destination: &str, read_only: bool) -> Mount {
    bind_path(Path::new(source), destination, read_only)
}

fn bind_path(source: &Path, destination: &str, read_only: bool) -> Mount {
    Mount {
        source: source.to_path_buf(),
        destination: destination.to_owned(),
        read_only,
    }
}

/// Deletes its box when dropped, so a check that fails leaves no container running.
struct Deleting<'a>(&'a Node, BoxId);

impl Drop for Deleting<'_> {
    fn drop(&mut self) {
        let _ = self.0.delete(&self.1);
    }
}

fn userns_available() -> bool {
    Command::new("unshare")
        .args(["-U", "true"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn host_id(flag: &str) -> u32 {
    let out = Command::new("id").arg(flag).output().expect("id");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("id is a number")
}

/// A rootfs of the host's `/usr`, with the host's top-level links into it (`/bin`, `/lib64`, …) so
/// the loader and `/bin/sh` resolve as on the host, and the directories the box's mounts land on.
fn build_rootfs(rootfs: &Path) {
    for d in ["proc", "dev", "sys", "tmp", "usr", "out"] {
        std::fs::create_dir_all(rootfs.join(d)).unwrap();
    }
    for link in ["bin", "sbin", "lib", "lib64"] {
        if let Ok(target) = std::fs::read_link(Path::new("/").join(link)) {
            let target = target.to_string_lossy().trim_start_matches('/').to_owned();
            std::os::unix::fs::symlink(target, rootfs.join(link)).unwrap();
        }
    }
}

/// This process's cgroup directory under /sys/fs/cgroup.
fn own_cgroup() -> PathBuf {
    let own = std::fs::read_to_string("/proc/self/cgroup").expect("/proc/self/cgroup");
    let rel = own
        .lines()
        .find_map(|l| l.strip_prefix("0::"))
        .expect("a 0:: cgroup line")
        .trim_start_matches('/');
    PathBuf::from("/sys/fs/cgroup").join(rel)
}

/// Moves this process into a leaf of its own cgroup and confirms every controller is delegated, as
/// oci_box.rs does; returns the own cgroup dir, or why it isn't ours to subdivide.
fn prepare_cgroup(disk: &Path) -> Result<PathBuf, String> {
    let own = own_cgroup();
    let leaf = own.join("hex-agent");
    std::fs::create_dir_all(&leaf).map_err(|e| format!("making a leaf for the agent: {e}"))?;
    std::fs::write(leaf.join("cgroup.procs"), format!("{}", std::process::id()))
        .map_err(|e| format!("moving self into a leaf: {e}"))?;
    let probe = hex_runtime::linux::cgroup::BoxRoot::open(
        &own.join("hex-probe"),
        hex_runtime::linux::disks::under(disk).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    let missing: Vec<&str> = hex_core::cgroup::CONTROLLERS
        .into_iter()
        .filter(|c| !probe.controllers().contains(*c))
        .collect();
    let _ = std::fs::remove_dir(own.join("hex-probe"));
    if missing.is_empty() {
        Ok(own)
    } else {
        Err(format!(
            "{} can't hand down the {} controllers",
            own.display(),
            missing.join(", ")
        ))
    }
}

fn cache_dir() -> PathBuf {
    let cache =
        PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cache/hex-t7/ocicgfs");
    std::fs::create_dir_all(&cache).expect("cache dir");
    cache
}
