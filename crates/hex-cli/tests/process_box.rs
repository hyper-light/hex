//! The `process` class against the runtime conformance suite with real cgroups and real processes
//! (AC-T7.1.1, T-T7.1.2). The programs are `hex selftest-box` acts, so the real box is held to the
//! same contract as the simulator in `hex-api`.
//!
//! It needs a cgroup v2 subtree the test owns, with cpu, io, memory and pids delegated. Where this
//! process's own cgroup is private and delegated (a cgroupns-root CI pod), the test uses it
//! directly. Where it is shared (a developer shell, a kiln runner's service cgroup), the test
//! re-runs itself once inside a transient `systemd-run --user --scope -p Delegate=yes`, which gives
//! it a private subtree. Where neither is possible, it returns early with a printed reason rather
//! than a false pass. Run it on a Linux host with cgroup v2 (fedora); the recorded run is CP-T7.1's
//! evidence.
#![cfg(target_os = "linux")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::path::PathBuf;
use std::process::Command;

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Program};
use hex_node::Node;

struct BinFixture {
    exe: String,
}

impl Fixture for BinFixture {
    fn spec(&self, id: BoxId, bounds: Bounds) -> BoxSpec {
        BoxSpec {
            id,
            bounds,
            class: ClassSpec::Process,
        }
    }

    fn program(&self, act: &Act) -> Program {
        let mut args = vec![self.exe.clone(), "selftest-box".to_owned()];
        match act {
            Act::Say(words) => args.extend(["say".to_owned(), (*words).to_owned()]),
            Act::Fail(code) => args.extend(["fail".to_owned(), code.to_string()]),
            Act::Wait => args.push("wait".to_owned()),
            Act::Hog(bytes) => args.extend(["hog".to_owned(), bytes.to_string()]),
            Act::Spawn(n) => args.extend(["spawn".to_owned(), n.to_string()]),
            Act::Echo => args.push("echo".to_owned()),
        }
        Program::of(&args.iter().map(String::as_str).collect::<Vec<_>>())
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

/// Opens a node in a box root under this process's own (private) cgroup, moving this process into a
/// leaf first so the cgroup can hand its controllers down. `Err` when the cgroup isn't ours to
/// subdivide (shared with other processes, or controllers not delegated).
fn open_here(state: &std::path::Path) -> Result<Node, String> {
    let own = own_cgroup();
    let leaf = own.join("hex-agent");
    std::fs::create_dir_all(&leaf).map_err(|e| format!("making a leaf for the agent: {e}"))?;
    std::fs::write(leaf.join("cgroup.procs"), format!("{}", std::process::id()))
        .map_err(|e| format!("moving self into a leaf: {e}"))?;
    Node::open(&own.join("hex-boxes"), state, state).map_err(|e| e.to_string())
}

#[test]
fn process_boxes_hold_the_runtime_contract() {
    let exe = env!("CARGO_BIN_EXE_hex").to_owned();
    // Labs go under ~/.cache, never /tmp; a real disk also lets the io-bound discovery resolve to a
    // block device.
    let cache =
        PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cache/hex-t7/boxtest");
    std::fs::create_dir_all(&cache).expect("cache dir");
    let state = tempfile::Builder::new()
        .prefix("conf-")
        .tempdir_in(&cache)
        .expect("state dir");

    match open_here(state.path()) {
        Ok(node) => {
            let failures = conformance::run(&node, &BinFixture { exe });
            assert!(failures.is_empty(), "conformance failures: {failures:#?}");
        }
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
            let test_bin = std::env::current_exe().expect("test binary path");
            let status = Command::new("systemd-run")
                .args(["--user", "--scope", "--quiet", "-p", "Delegate=yes"])
                .arg(&test_bin)
                .args([
                    "process_boxes_hold_the_runtime_contract",
                    "--exact",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("HEX_DELEGATED", "1")
                .status()
                .expect("run the test in a delegated scope");
            assert!(status.success(), "the delegated re-run failed");
        }
        Err(direct) => {
            eprintln!("SKIP: no usable cgroup v2 delegation ({direct}); run on a cgroup-v2 host");
        }
    }
}
