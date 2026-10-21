//! The `process` class with real cgroups and real processes: the runtime conformance suite
//! (AC-T7.1.1) and end-to-end crash recovery (T-T7.1.2). The programs are `hex selftest-box` acts,
//! so the real box is held to the same contract as the simulator in `hex-api`.
//!
//! It needs a cgroup v2 subtree the test owns, with cpu, io, memory and pids delegated. Where this
//! process's own cgroup is already private and delegated (a cgroupns-root CI pod), the test uses it
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

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use hex_api::conformance::{self, Act, Fixture};
use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Program, Runtime, Signal, Stdio};
use hex_node::Node;

struct BinFixture {
    exe: String,
}

impl BinFixture {
    fn wait(&self) -> Program {
        Program::of(&[&self.exe, "selftest-box", "wait"])
    }
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

/// Moves this process into a leaf of its own cgroup and confirms the cgroup can hand its controllers
/// down (a box root opens). Returns the own cgroup dir on success; `Err` when the cgroup isn't ours
/// to subdivide (shared with other processes, or controllers not delegated).
fn prepare_cgroup(disk: &Path) -> Result<PathBuf, String> {
    let own = own_cgroup();
    let leaf = own.join("hex-agent");
    std::fs::create_dir_all(&leaf).map_err(|e| format!("making a leaf for the agent: {e}"))?;
    std::fs::write(leaf.join("cgroup.procs"), format!("{}", std::process::id()))
        .map_err(|e| format!("moving self into a leaf: {e}"))?;
    // A probe root proves the controllers are delegated before the real roots are opened.
    hex_runtime::linux::cgroup::BoxRoot::open(
        &own.join("hex-probe"),
        hex_runtime::linux::disks::under(disk).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir(own.join("hex-probe"));
    Ok(own)
}

fn cache_dir() -> PathBuf {
    let cache =
        PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cache/hex-t7/boxtest");
    std::fs::create_dir_all(&cache).expect("cache dir");
    cache
}

#[test]
fn process_boxes_hold_the_contract_and_recover() {
    let exe = env!("CARGO_BIN_EXE_hex").to_owned();
    let cache = cache_dir();

    match prepare_cgroup(&cache) {
        Ok(own) => {
            exercise_conformance(&own, &cache, &exe);
            exercise_recovery(&own, &cache, &exe);
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
                    "process_boxes_hold_the_contract_and_recover",
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
            eprintln!("SKIP: no usable cgroup v2 delegation ({direct}); run on a cgroup-v2 host")
        }
    }
}

/// AC-T7.1.1: the `process` class holds the runtime contract on real cgroups and processes.
fn exercise_conformance(own: &Path, cache: &Path, exe: &str) {
    let state = tempfile::Builder::new()
        .prefix("conf-")
        .tempdir_in(cache)
        .expect("state dir");
    let node =
        Node::open(&own.join("hex-boxes-conf"), state.path(), state.path()).expect("open a node");
    let failures = conformance::run(
        &node,
        &BinFixture {
            exe: exe.to_owned(),
        },
    );
    assert!(failures.is_empty(), "conformance failures: {failures:#?}");
}

/// T-T7.1.2: a node agent restart adopts a box still running and scrubs every leftover, leaking none.
fn exercise_recovery(own: &Path, cache: &Path, exe: &str) {
    let state = tempfile::Builder::new()
        .prefix("rec-")
        .tempdir_in(cache)
        .expect("state dir");
    let box_root = own.join("hex-boxes-rec");
    let fx = BinFixture {
        exe: exe.to_owned(),
    };

    // A box with a program still running when the agent "restarts".
    let node = Node::open(&box_root, state.path(), state.path()).expect("node");
    let live = BoxId::new("live").unwrap();
    node.create(fx.spec(live.clone(), Bounds::default()))
        .expect("create live");
    let mut proc = node
        .exec(&live, fx.wait(), Stdio::null())
        .expect("exec wait");
    wait_until(
        || {
            node.inspect(&live)
                .map(|i| i.state == hex_api::BoxState::Running)
                .unwrap_or(false)
        },
        "live box runs",
    );

    // A leftover cgroup with no record (a crash after the cgroup was made, before the record).
    std::fs::create_dir_all(box_root.join("orphan")).expect("orphan cgroup");
    // A leftover record mid-create (a crash after the record, with its cgroup made).
    let midcreate = BoxId::new("midcreate").unwrap();
    let rec = hex_core::record::BoxRecord::new(
        fx.spec(midcreate.clone(), Bounds::default()),
        hex_core::record::Phase::Creating,
        vec![],
    );
    std::fs::write(state.path().join("midcreate.json"), rec.encode().unwrap())
        .expect("midcreate record");
    std::fs::create_dir_all(box_root.join("midcreate")).expect("midcreate cgroup");

    // The restart: a fresh node on the same directories recovers.
    let restarted = Node::open(&box_root, state.path(), state.path()).expect("restart");
    let summary = restarted.recover().expect("recover");

    assert!(
        summary.adopted.contains(&live),
        "live not adopted: {summary:?}"
    );
    let removed: Vec<&BoxId> = summary.removed.iter().map(|(id, _)| id).collect();
    assert!(
        removed.contains(&&orphan_id()),
        "orphan not removed: {summary:?}"
    );
    assert!(
        removed.contains(&&midcreate),
        "midcreate not removed: {summary:?}"
    );

    // The adopted box is still running; the leftovers are gone, leaking nothing.
    assert_eq!(
        restarted.inspect(&live).unwrap().state,
        hex_api::BoxState::Running
    );
    assert!(!box_root.join("orphan").exists(), "orphan cgroup leaked");
    assert!(
        !box_root.join("midcreate").exists(),
        "midcreate cgroup leaked"
    );
    assert!(
        !state.path().join("midcreate.json").exists(),
        "midcreate record leaked"
    );

    // Clean up: ending the live box ends its program.
    restarted.delete(&live).expect("delete live");
    assert_eq!(
        proc.try_wait().expect("wait"),
        Some(hex_api::Exit::Signal(Signal::KILL.number()))
    );
    assert!(
        restarted.list().unwrap().is_empty(),
        "boxes left after cleanup"
    );
}

fn orphan_id() -> BoxId {
    BoxId::new("orphan").unwrap()
}

fn wait_until(mut cond: impl FnMut() -> bool, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        std::thread::yield_now();
    }
    panic!("timed out waiting for: {what}");
}
