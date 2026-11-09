//! `hex node` on Windows (native plan step 8; decision 3 of docs/design/runtime-architecture.md, AC-T7.1.1):
//! the node runs as a process of its own, and this test reaches it only through its AF_UNIX socket, as kiln
//! will. Each test starts a node on a folder of its own and kills it when done.
//! - HEX-SOCK-9: create a `process` box, exec a program in it that exits 7, see its end come over the
//!   socket, delete the box;
//! - HEX-SOCK-6, the disconnect rule: a caller that goes away while its exec'd program runs has it killed,
//!   reaped and counted in its box (`abandoned`), and one it released runs on;
//! - HEX-SOCK-3: hostile bytes to the live node (a frame past the bound, a frame of garbage) end that
//!   connection alone, and the node serves the next;
//! - an exec that asks for a stream is refused by name until the stdio transport's decision is recorded.
#![cfg(windows)]
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
use std::process::Command;
use std::time::{Duration, Instant};

use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Exit, Program, Refusal, Runtime, Stdio};
use hex_core::stream::{self, Deframer};
use hex_core::wire::{self, Answer, Call, Received, Request, Streams};

/// Longest a test waits for a fact it needs: a bound, so a node that never makes it true fails the test
/// instead of hanging it.
const PATIENCE: Duration = Duration::from_secs(60);

/// A `hex node` of this build on a folder of its own, ended when dropped, and its keeper with it: the keeper
/// outlives a killed node by design (it holds the node's boxes for the next one), so a node this test started
/// leaves no keeper, and no box the keeper held, behind it.
struct Node {
    child: std::process::Child,
    socket: PathBuf,
    /// The keeper's state folder (`state\keeper`), whose `keeper.owner` names it.
    keeping: PathBuf,
}

impl Node {
    fn start(dir: &Path) -> Self {
        let socket = dir.join("node.sock");
        let mut child = Command::new(env!("CARGO_BIN_EXE_hex"))
            .arg("node")
            .arg("--socket")
            .arg(&socket)
            .arg("--state")
            .arg(dir.join("state"))
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("start hex node");
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(line.trim(), format!("ready {}", socket.display()));
        Self {
            child,
            socket,
            keeping: dir.join("state").join("keeper"),
        }
    }

    fn client(&self) -> hex_client::Client {
        hex_client::Client::connect(&self.socket).expect("connect to hex node")
    }
}

impl Drop for Node {
    /// Kills the node, then asks the keeper its `keeper.owner` names to stop, as a node going down cleanly
    /// does; the keeper's going ends what it held (kill-on-close).
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let Ok(text) = std::fs::read_to_string(self.keeping.join("keeper.owner")) else {
            return;
        };
        let mut words = text.split_whitespace();
        if let (Some(Ok(pid)), Some(Ok(created))) = (
            words.next().map(str::parse::<u32>),
            words.next().map(str::parse::<u64>),
        ) && let Ok(keeper) =
            hex_runtime::windows::keeper::NodeKeeper::open(pid, created, &self.keeping)
        {
            keeper.stop();
        }
    }
}

fn spec(id: &BoxId) -> BoxSpec {
    BoxSpec {
        id: id.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Process,
    }
}

fn act(args: &[&str]) -> Program {
    let mut all = vec![env!("CARGO_BIN_EXE_hex"), "selftest-box"];
    all.extend_from_slice(args);
    Program::of(&all)
}

/// Whether process `pid` is still running, by asking `tasklist` (pure std): its output names the pid only
/// while it runs.
fn alive(pid: u32) -> bool {
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .output()
        .expect("tasklist");
    String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
}

fn wait_for(what: &str, mut fact: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !fact() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::park_timeout(Duration::from_millis(20));
    }
}

fn dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("hexnode-")
        .tempdir()
        .unwrap()
}

/// HEX-SOCK-9 (AC-T7.1.1): do create a box, exec a program that exits 7 and delete the box over a live
/// node's socket; expect the program's end, code 7, to come over the socket, and the box gone after.
#[test]
fn a_box_is_made_run_and_deleted_through_the_nodes_socket() {
    let dir = dir();
    let node = Node::start(dir.path());
    let client = node.client();
    let id = BoxId::new("sock-9").unwrap();
    client.create(spec(&id)).unwrap();
    let mut p = client
        .exec(&id, act(&["fail", "7"]), Stdio::null())
        .unwrap();
    let exit = p.wait_for(PATIENCE).unwrap();
    assert_eq!(exit, Some(Exit::Code(7)));
    client.delete(&id).unwrap();
    assert_eq!(client.inspect(&id), Err(Refusal::NoSuchBox(id)));
}

/// A caller of the node's socket in frames of its own (`hex_core::stream`), so a test can drop the
/// connection while programs it started run.
struct Raw {
    socket: hex_afunix::Socket,
    deframer: Deframer,
    next: u64,
}

impl Raw {
    fn connect(node: &Node) -> Self {
        Self {
            socket: hex_afunix::Socket::connect(&node.socket).unwrap(),
            deframer: Deframer::new(wire::MAX_MESSAGE),
            next: 0,
        }
    }

    /// Sends `call` and waits for its answer.
    fn call(&mut self, call: Call) -> Answer {
        self.next += 1;
        let id = self.next;
        let bytes =
            wire::request(wire::VERSION, id, &Request::Call(call), wire::MAX_MESSAGE).unwrap();
        let framed = stream::frame(&bytes, wire::MAX_MESSAGE).unwrap();
        let mut sent = 0;
        while sent < framed.len() {
            sent += self.socket.send(&framed[sent..]).unwrap();
        }
        let mut buf = vec![0u8; stream::MAX_FRAME];
        loop {
            let n = self.socket.recv(&mut buf).unwrap();
            assert!(
                n > 0,
                "the node closed the connection before answering {id}"
            );
            let mut input = &buf[..n];
            while !input.is_empty() {
                let (took, message) = self.deframer.take(input).unwrap();
                input = &input[took..];
                let Some(message) = message else { continue };
                let received = Received {
                    bytes: &message,
                    size: message.len(),
                    descriptors: 0,
                    descriptors_cut: false,
                };
                let (h, answer) = wire::read_answer(&received, wire::MAX_MESSAGE).unwrap();
                if h.id == id {
                    return answer;
                }
            }
        }
    }

    fn exec(&mut self, id: &BoxId, args: &[&str]) -> u32 {
        match self.call(Call::Exec {
            id: id.clone(),
            program: act(args),
            streams: Streams::default(),
        }) {
            Answer::Started { pid } => pid,
            other => panic!("exec in {id}: {other:?}"),
        }
    }
}

/// HEX-SOCK-6 (AC-T7.1.1), the disconnect rule: do exec a waiting program over one connection and a second
/// through a client that releases it, then drop the first connection; expect its program killed and counted
/// in its box as abandoned, and the released one running on.
#[test]
fn a_gone_callers_programs_are_killed_and_counted_and_a_released_one_runs_on() {
    let dir = dir();
    let node = Node::start(dir.path());
    let client = node.client();
    let id = BoxId::new("sock-6").unwrap();
    client.create(spec(&id)).unwrap();
    let released = client.exec(&id, act(&["wait"]), Stdio::null()).unwrap();
    let released_pid = released.pid();
    // Dropped: its client sends a release, and the program runs on unwatched.
    drop(released);
    let mut caller = Raw::connect(&node);
    let watched = caller.exec(&id, &["wait"]);
    assert!(alive(watched), "the watched program isn't running");
    drop(caller);
    wait_for("the gone caller's program to be killed", || !alive(watched));
    wait_for("its box to count it abandoned", || {
        client.inspect(&id).is_ok_and(|b| b.abandoned == 1)
    });
    assert!(alive(released_pid), "the released program was ended");
    client.delete(&id).unwrap();
    wait_for("the delete to end the released program", || {
        !alive(released_pid)
    });
}

/// HEX-SOCK-3 (AC-T7.1.1): do send the live node a frame past its bound on one connection and a frame of
/// garbage on another; expect each connection ended by the node and the node still serving a third.
#[test]
fn hostile_frames_end_their_connection_and_the_node_serves_the_next() {
    let dir = dir();
    let node = Node::start(dir.path());
    for hostile in [
        // A prefix declaring a body past the format's largest message.
        u32::MAX.to_le_bytes().to_vec(),
        // A whole frame of bytes no request is.
        [16u32.to_le_bytes().to_vec(), vec![0xee; 16]].concat(),
    ] {
        let raw = hex_afunix::Socket::connect(&node.socket).unwrap();
        assert_eq!(raw.send(&hostile).unwrap(), hostile.len());
        // The node answers what it can (a refusal) and closes: the read reaches the end, within PATIENCE.
        // Read on a thread of its own, so a node that keeps the connection fails the test by name instead
        // of holding it (gates at 8907b97a, 2026-11-09, held here).
        let (ended, end) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 1 << 16];
            while let Ok(n) = raw.recv(&mut buf) {
                if n == 0 {
                    break;
                }
            }
            let _ = ended.send(());
        });
        assert!(
            end.recv_timeout(PATIENCE).is_ok(),
            "the node kept the connection {PATIENCE:?} after a hostile frame"
        );
    }
    let client = node.client();
    let id = BoxId::new("sock-3").unwrap();
    client.create(spec(&id)).unwrap();
    client.delete(&id).unwrap();
}

/// HEX-RT-16 through the socket (native plan step 8): do exec two waiting programs in one box and kill one
/// through its `Proc`; expect its end, read over the socket, to be KILL's (Signal(9)), the other still
/// running, and a second kill accepted.
#[test]
fn a_program_killed_through_the_socket_ends_as_kill_and_its_box_runs_on() {
    let dir = dir();
    let node = Node::start(dir.path());
    let client = node.client();
    let id = BoxId::new("rt-16").unwrap();
    client.create(spec(&id)).unwrap();
    let mut killed = client.exec(&id, act(&["wait"]), Stdio::null()).unwrap();
    let other = client.exec(&id, act(&["wait"]), Stdio::null()).unwrap();
    killed.kill().unwrap();
    let exit = killed.wait_for(PATIENCE).unwrap();
    assert_eq!(exit, Some(Exit::Signal(9)), "a killed program's end");
    assert!(
        alive(other.pid()),
        "the box's other program ended with the kill"
    );
    killed.kill().unwrap();
    client.delete(&id).unwrap();
}

/// Native plan step 8: do exec a program with its stdout a pipe through the node; expect the exec refused
/// by name (the platform, and why), the box untouched and the connection still served.
#[test]
fn an_exec_with_a_stream_is_refused_by_name_until_the_stdio_transport_is_decided() {
    let dir = dir();
    let node = Node::start(dir.path());
    let client = node.client();
    let id = BoxId::new("stdio").unwrap();
    client.create(spec(&id)).unwrap();
    let (_read, write) = std::io::pipe().unwrap();
    let stdio = Stdio {
        stdin: None,
        stdout: Some(write),
        stderr: None,
        ..Stdio::null()
    };
    match client.exec(&id, act(&["say", "hi"]), stdio) {
        Err(Refusal::Platform { what, .. }) => {
            assert_eq!(what, "an exec's stdio through the node agent's socket");
        }
        Err(other) => panic!("an exec with a stream was refused, but not by name: {other:?}"),
        Ok(_) => panic!("an exec with a stream wasn't refused"),
    }
    client.delete(&id).unwrap();
}
