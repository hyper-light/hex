//! T-T7.1.2 on macOS (native plan step 8b): `hex node` killed (SIGKILL) with boxes running, empty, mid-create
//! and mid-delete, then restarted on the same state folder: each box its record says was made is taken back
//! (a running one with its members still running and watched, so a delete ends them), each half-made or
//! half-deleted one is cleaned up (no record, no temp file, no member left), and nothing else is left.
//!
//! Mid-create: the node is held in the record write that marks the box creating by a FIFO at that write's
//! temp path, and killed once `sample(1)` shows its thread there, in `__open` under the store's write (macOS
//! has no wait channel to read). Mid-delete: a delete records the box as deleting before it ends anything, so
//! a node killed past that write leaves the record deleting and the box's members running; this test leaves
//! exactly that (a box made, a member started, the node killed, its record set to deleting), as recovery's
//! own model holds a crash at every write (`hex_core::recovery`).
#![cfg(target_os = "macos")]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hex_api::{Bounds, BoxId, BoxSpec, BoxState, ClassSpec, Program, Refusal, Runtime, Stdio};
use hex_core::record::{BoxRecord, Phase};

/// Longest a test waits for a fact it needs: a bound, so a node that never makes it true fails the test
/// instead of hanging it.
const PATIENCE: Duration = Duration::from_secs(60);

/// A `hex node` of this build on `state`, ended by SIGKILL when dropped (a crash, as the test means one).
struct Node {
    child: std::process::Child,
    socket: PathBuf,
}

impl Node {
    fn start(dir: &Path) -> Self {
        let socket = dir.join("node.sock");
        let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_hex"))
            .arg("node")
            .arg("--socket")
            .arg(&socket)
            .arg("--state")
            .arg(dir.join("state"))
            .env_remove("SHARDS_BIN")
            .env_remove("HEX_GUEST_KERNEL")
            .env_remove("HEX_GUEST_INITRD")
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("start hex node");
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line.trim(), format!("ready {}", socket.display()));
        Self { child, socket }
    }

    fn client(&self) -> hex_client::Client {
        hex_client::Client::connect(&self.socket).expect("connect to hex node")
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// SIGKILL, and reaped.
    fn kill(mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
}

impl Drop for Node {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spec(id: &BoxId) -> BoxSpec {
    BoxSpec {
        route: None,
        allocation: None,
        devices: Vec::new(),
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Process,
        owner: None,
    }
}

fn wait() -> Program {
    Program::of(&[env!("CARGO_BIN_EXE_hex"), "selftest-box", "wait"])
}

fn alive(pid: u32) -> bool {
    hex_runtime::macos::sys::rusage(i32::try_from(pid).unwrap())
        .unwrap()
        .is_some_and(|r| !r.ended)
}

fn wait_for(what: &str, mut fact: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !fact() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::park_timeout(Duration::from_millis(20));
    }
}

/// Whether `sample(1)` finds one of node `pid`'s threads in `__open` under the store's record write, and what
/// it saw of opens and the store, for a failure to name.
fn held_in_a_record_write(pid: u32) -> (bool, String) {
    let out = std::process::Command::new("/usr/bin/sample")
        .arg(pid.to_string())
        .arg("1")
        .arg("-mayDie")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    // Its symbols as `sample` prints them: demangled, or Rust's v0 mangling (`…5Store3put`).
    let held = text
        .lines()
        .skip_while(|l| !(l.contains("Store::put") || l.contains("5Store3put")))
        .any(|l| l.contains("__open"));
    let seen: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("open") || l.contains("Store") || l.contains("store"))
        .take(12)
        .map(str::trim)
        .collect();
    (held, seen.join("\n"))
}

fn record(dir: &Path, id: &BoxId) -> PathBuf {
    hex_node::store::runtime_record(&dir.join("state/@records"), id)
}

fn temp(dir: &Path, id: &BoxId) -> PathBuf {
    hex_node::store::runtime_temp(&dir.join("state/@records"), id)
}

/// T-T7.1.2: a box made and running, and one made and empty, are taken back after the node is killed: the
/// running box's member still runs, the box reads as taken back, and a delete ends the member and leaves no
/// record.
#[test]
fn boxes_made_are_taken_back_after_a_kill() {
    let dir = tempfile::Builder::new().prefix("rec-").tempdir().unwrap();
    let (running, empty) = (BoxId::new("running").unwrap(), BoxId::new("empty").unwrap());
    let node = Node::start(dir.path());
    let client = node.client();
    client.create(spec(&running)).unwrap();
    client.create(spec(&empty)).unwrap();
    let member = client.exec(&running, wait(), Stdio::null()).unwrap();
    let pid = member.pid();
    // Released, as a caller that keeps a box running past its own connection does.
    drop(member);
    drop(client);
    node.kill();
    assert!(alive(pid), "the box's member ended with the node");
    let node = Node::start(dir.path());
    let client = node.client();
    let back = client
        .inspect(&running)
        .expect("the running box is taken back");
    assert!(back.adopted, "{back:?}");
    assert_eq!(back.state, BoxState::Running, "{back:?}");
    let back = client.inspect(&empty).expect("the empty box is taken back");
    assert!(back.adopted, "{back:?}");
    client.delete(&running).unwrap();
    wait_for("the taken-back box's member to end with its delete", || {
        !alive(pid)
    });
    client.delete(&empty).unwrap();
    assert!(!record(dir.path(), &running).exists());
    assert!(!record(dir.path(), &empty).exists());
}

/// T-T7.1.2: a node killed in the record write that marks a box creating leaves a temp file and no box;
/// restarted, it removes the temp file and holds no such box.
#[test]
fn a_box_killed_mid_create_is_cleaned_up() {
    let dir = tempfile::Builder::new().prefix("rec-").tempdir().unwrap();
    let id = BoxId::new("mid-create").unwrap();
    let node = Node::start(dir.path());
    let made_fifo = std::process::Command::new("/usr/bin/mkfifo")
        .arg(temp(dir.path(), &id))
        .status()
        .unwrap();
    assert!(made_fifo.success());
    let socket = node.socket.clone();
    let made = id.clone();
    let _create =
        std::thread::spawn(move || hex_client::Client::connect(&socket)?.create(spec(&made)));
    let pid = node.pid();
    let mut seen = String::new();
    let deadline = Instant::now() + PATIENCE;
    loop {
        let (held, what) = held_in_a_record_write(pid);
        if held {
            break;
        }
        seen = what;
        assert!(
            Instant::now() < deadline,
            "the node wasn't seen held in the record write; sample saw:\n{seen}"
        );
    }
    let _ = seen;
    node.kill();
    let node = Node::start(dir.path());
    let client = node.client();
    assert_eq!(client.inspect(&id), Err(Refusal::NoSuchBox(id.clone())));
    assert!(!temp(dir.path(), &id).exists(), "the temp file was left");
    assert!(!record(dir.path(), &id).exists(), "a record was left");
}

/// T-T7.1.2: a node killed past the write that marks a box deleting leaves the record deleting and the
/// box's member running; restarted, it ends the member and removes the box and its record.
#[test]
fn a_box_killed_mid_delete_is_cleaned_up() {
    let dir = tempfile::Builder::new().prefix("rec-").tempdir().unwrap();
    let id = BoxId::new("mid-delete").unwrap();
    let node = Node::start(dir.path());
    let client = node.client();
    client.create(spec(&id)).unwrap();
    let member = client.exec(&id, wait(), Stdio::null()).unwrap();
    let pid = member.pid();
    drop(member);
    drop(client);
    node.kill();
    let path = record(dir.path(), &id);
    let mut r = BoxRecord::decode(&std::fs::read(&path).unwrap()).unwrap();
    r.phase = Phase::Deleting;
    std::fs::write(&path, r.encode().unwrap()).unwrap();
    assert!(alive(pid));
    let node = Node::start(dir.path());
    let client = node.client();
    assert_eq!(client.inspect(&id), Err(Refusal::NoSuchBox(id.clone())));
    wait_for("the half-deleted box's member to be ended", || !alive(pid));
    assert!(!path.exists(), "the record was left");
}
