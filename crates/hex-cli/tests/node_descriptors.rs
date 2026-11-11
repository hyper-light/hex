//! A node's descriptors (AC-T7.1.1): a node embedded in a process starts within that process's descriptor limit,
//! holding its own descriptors at its pools' bounds, or is refused at its start. The root lab of 2026-11-11 ran a
//! node in a container whose soft limit was 2048 of a 2,097,152 hard one: it ran out at its namespaces' refill
//! (EMFILE) and a create failed, where `hex node` alone had raised its soft limit to its hard one.
//!
//! It needs a cgroup v2 subtree the test owns, with cpu, io, memory and pids delegated, to start nodes in. Where this
//! binary's own cgroup is delegated to it alone (HEX_DELEGATED's runner), it uses it; where it is shared, it re-runs
//! itself once in a transient `systemd-run --user --scope -p Delegate=yes`; where neither is possible it records the
//! skip rather than pass falsely.
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

use hex_node::Node;
use hex_node::netns::{FilesBound, Pools, open_descriptors};
use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

const TEST: &str = "a_node_starting_under_a_low_descriptor_limit_holds_its_own_or_refuses_typed";
/// Set in the copy of this binary re-run to start a node under a lowered descriptor limit: `soft` lowers the soft
/// limit alone, `hard` the hard one with it.
const MODE_ENV: &str = "HEX_NODE_DESCRIPTORS_MODE";
const PASS: &str = "low descriptor limit: the node's start held its own or refused";
/// The root lab's container's soft limit (the fedora image's), under which an embedded node ran out mid-refill.
const LOW: u64 = 2048;

/// AC-T7.1.1. Do: start a node in a process whose soft descriptor limit is 2048 and its hard one as given; then in one
/// whose hard limit is 2048 too. Expect: the first node's start raises the soft limit to the hard one its sizes count
/// on, which holds its own descriptors at its pools' bounds; the second's start is refused by type, naming the hard
/// limit, never failing calls at random later.
#[test]
fn a_node_starting_under_a_low_descriptor_limit_holds_its_own_or_refuses_typed() {
    if let Some(mode) = std::env::var_os(MODE_ENV) {
        start_under_a_low_limit(&mode.to_string_lossy());
        return;
    }
    let cache = cache_dir();
    let parent = match prepare_cgroup(&cache) {
        Ok(own) => own,
        Err(direct) if std::env::var_os("HEX_DELEGATED").is_none() && have_systemd() => {
            let status = Command::new("systemd-run")
                .args(["--user", "--scope", "--quiet", "-p", "Delegate=yes"])
                .arg(std::env::current_exe().expect("test binary path"))
                .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
                .env("HEX_DELEGATED", "1")
                .status()
                .expect("run the test in a delegated scope");
            assert!(status.success(), "the delegated re-run failed ({direct})");
            return;
        }
        Err(direct) => {
            hex_testkit::skip(
                TEST,
                "cgroup v2 delegation",
                &format!("own cgroup isn't ours to subdivide ({direct}); run on a cgroup-v2 host"),
            )
            .expect("recording the skip");
            return;
        }
    };
    for mode in ["soft", "hard"] {
        // Each copy alone in a fresh cgroup of its own, written before exec, as its node subdivides it.
        let own = parent.join(format!("hex-nofile-{mode}-{}", std::process::id()));
        std::fs::create_dir(&own).expect("a fresh cgroup for the copy");
        let output = Command::new("/bin/sh")
            .args([
                "-e",
                "-c",
                r#"printf '%s' "$$" > "$1/cgroup.procs"; shift; exec "$@""#,
                "hex-nofile",
            ])
            .arg(&own)
            .arg(std::env::current_exe().expect("test binary path"))
            .args([TEST, "--exact", "--nocapture", "--test-threads=1"])
            .env(MODE_ENV, mode)
            .output()
            .expect("run the copy under a lowered descriptor limit");
        let log = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprint!("{log}");
        assert!(output.status.success(), "{mode}: the copy failed");
        assert!(
            log.lines().any(|line| line == PASS),
            "{mode}: the copy did not start its node"
        );
    }
}

/// The copy's half: lower this process's descriptor limit as `mode` says, start a node in this copy's own cgroup, and
/// hold it to what its start must do.
fn start_under_a_low_limit(mode: &str) {
    let hard = getrlimit(Resource::Nofile)
        .maximum
        .expect("a finite hard descriptor limit");
    let low = LOW.min(hard);
    let maximum = match mode {
        "soft" => hard,
        "hard" => low,
        other => panic!("unknown mode {other}"),
    };
    setrlimit(
        Resource::Nofile,
        Rlimit {
            current: Some(low),
            maximum: Some(maximum),
        },
    )
    .expect("lowering the descriptor limit");
    let cache = cache_dir();
    let own = prepare_cgroup(&cache).expect("this copy's own delegated cgroup");
    let state = tempfile::Builder::new()
        .prefix("nofile-")
        .tempdir_in(&cache)
        .expect("state dir");
    let needed = FilesBound::needed(
        Pools::rootless(hex_node::sizes::Sizes::now().expect("this host's sizes")),
        open_descriptors(&getrlimit(Resource::Nofile)).expect("this process's descriptors"),
    );
    assert!(
        needed.own > low,
        "the case needs a node whose own descriptors exceed {low}: {needed:?}"
    );
    let opened = Node::open(&own.join("hex-boxes-nofile"), state.path(), state.path());
    let limit = getrlimit(Resource::Nofile);
    if mode == "soft" {
        assert!(opened.is_ok(), "the node's start: {:?}", opened.err());
        assert_eq!(
            limit.current,
            Some(hard),
            "the start raised its soft limit to the hard one"
        );
        assert!(
            limit.current.is_some_and(|soft| soft >= needed.own),
            "{limit:?} holds the node's own {} descriptors",
            needed.own
        );
    } else {
        match opened {
            Err(hex_api::Refusal::Platform { why, .. }) => assert!(
                why.contains(&format!("hard limit of {low}")),
                "the refusal names the hard limit: {why}"
            ),
            Err(other) => panic!("refused, but not for its descriptors: {other}"),
            Ok(_) => panic!(
                "a node whose own {} descriptors exceed the hard limit {low} started",
                needed.own
            ),
        }
    }
    eprintln!("{PASS}");
}

fn have_systemd() -> bool {
    Command::new("systemd-run")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
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

/// Moves this process into a leaf of its own cgroup and confirms the cgroup can hand every controller down (a box root
/// opens with all of them). Returns the own cgroup dir; `Err` when the cgroup isn't ours to subdivide.
fn prepare_cgroup(disk: &Path) -> Result<PathBuf, String> {
    let own = own_cgroup();
    let leaf = own.join("hex-agent");
    std::fs::create_dir_all(&leaf).map_err(|e| format!("making a leaf for the agent: {e}"))?;
    std::fs::write(leaf.join("cgroup.procs"), format!("{}", std::process::id()))
        .map_err(|e| format!("moving self into a leaf: {e}"))?;
    hex_runtime::linux::cgroup::BoxRoot::open(
        &own.join("hex-probe"),
        hex_runtime::linux::disks::under(disk).unwrap_or_default(),
    )
    .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir(own.join("hex-probe"));
    Ok(own)
}

fn cache_dir() -> PathBuf {
    let cache = PathBuf::from(std::env::var_os("HOME").expect("HOME")).join(".cache/hex-t7/nofile");
    std::fs::create_dir_all(&cache).expect("cache dir");
    cache
}
