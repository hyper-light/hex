//! The guest relay's tests (docs/design/shards-guest.md §3, HEX-GUEST-6 and HEX-GUEST-7; native plan
//! step 5): a host's guest client, through the relay, reaches a stand-in node agent's own socket; its
//! calls and answers cross whole; an exec's streams reach the node as pipe ends with the exec; and the
//! relay's bounds refuse by type: an exec whose stream never comes, and a connection past the bound.

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use super::*;
use hex_api::{BoxId, Class, Exit, Program, Runtime, Stdio};
use hex_client::guest::{Client, Link};
use hex_core::wire::{Header, Hello, Received};
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::fs::FileType;

/// The guest's ports as Unix sockets: the relay's control and stdio listeners, or a stdio socket nobody
/// serves from (`stdio` set apart).
struct Ports {
    control: PathBuf,
    stdio: PathBuf,
}

impl Link for Ports {
    fn dial(&self, port: u32) -> Result<Box<dyn Duplex>, Refusal> {
        let path = match port {
            stream::CONTROL_PORT => &self.control,
            stream::STDIO_PORT => &self.stdio,
            other => panic!("dialed port {other:#x}"),
        };
        Ok(Box::new(UnixStream::connect(path).unwrap()))
    }
}

struct Lab {
    dir: tempfile::TempDir,
    node: OwnedFd,
}

impl Lab {
    /// A stand-in node's listening socket and a relay serving it with `limits`.
    fn new(limits: Limits) -> Self {
        let cache = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache/hex-t7/relay");
        std::fs::create_dir_all(&cache).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("relay-")
            .tempdir_in(&cache)
            .unwrap();
        let node_path = dir.path().join("node.sock");
        let node = rustix::net::socket_with(
            AddressFamily::UNIX,
            SocketType::SEQPACKET,
            SocketFlags::CLOEXEC,
            None,
        )
        .unwrap();
        rustix::net::bind(&node, &SocketAddrUnix::new(&node_path).unwrap()).unwrap();
        rustix::net::listen(&node, 4).unwrap();
        let control = UnixListener::bind(dir.path().join("control.sock")).unwrap();
        let stdio = UnixListener::bind(dir.path().join("stdio.sock")).unwrap();
        let relay = Relay::new(&node_path, limits);
        std::thread::spawn(move || relay.serve(Box::new(control), Box::new(stdio)));
        Self { dir, node }
    }

    fn link(&self) -> Arc<dyn Link> {
        Arc::new(Ports {
            control: self.dir.path().join("control.sock"),
            stdio: self.dir.path().join("stdio.sock"),
        })
    }
}

/// The next request on the node's side of a connection, with the descriptors that came with it.
fn next_request(conn: &OwnedFd) -> (Header, Request, Vec<OwnedFd>) {
    let mut buf = vec![0u8; wire::MAX_MESSAGE];
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let msg = rustix::net::recvmsg(
        conn,
        &mut [IoSliceMut::new(&mut buf)],
        &mut control,
        RecvFlags::CMSG_CLOEXEC,
    )
    .unwrap();
    let fds: Vec<OwnedFd> = control
        .drain()
        .flat_map(|m| match m {
            RecvAncillaryMessage::ScmRights(fds) => fds.collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect();
    let received = Received {
        bytes: &buf[..msg.bytes],
        size: msg.bytes,
        descriptors: fds.len(),
        descriptors_cut: false,
    };
    let (h, r) = wire::read_request(&received, wire::MAX_MESSAGE).unwrap();
    (h, r, fds)
}

fn answer(conn: &OwnedFd, h: Header, answer: &Answer) {
    let bytes = wire::answer(h.version, h.id, answer, wire::MAX_MESSAGE).unwrap();
    rustix::net::send(conn, &bytes, SendFlags::NOSIGNAL).unwrap();
}

/// Accepts a connection and answers its hello.
fn hello(node: &OwnedFd) -> OwnedFd {
    let conn = rustix::net::accept_with(node, SocketFlags::CLOEXEC).unwrap();
    let (h, r, _) = next_request(&conn);
    assert_eq!(r, Request::Hello);
    answer(
        &conn,
        h,
        &Answer::Hello(Hello {
            version: wire::VERSION,
            classes: vec![Class::Oci],
            limits: crate::serve::LIMITS,
        }),
    );
    conn
}

/// Whether a message is waiting on `conn` within `ms`.
fn readable(conn: &OwnedFd, ms: i64) -> bool {
    let mut ready = [PollFd::new(conn, PollFlags::IN)];
    let ts = Timespec {
        tv_sec: 0,
        tv_nsec: ms * 1_000_000,
    };
    rustix::event::poll(&mut ready, Some(&ts)).unwrap() == 1
}

/// HEX-GUEST-6: connect through the relay and inspect a box the node doesn't hold: expect the node's
/// classes and its refusal, typed, as the node's own socket gives them.
#[test]
fn a_hosts_calls_reach_the_node_and_its_answers_come_back() {
    let lab = Lab::new(LIMITS);
    let node = lab.node.try_clone().unwrap();
    let agent = std::thread::spawn(move || {
        let conn = hello(&node);
        let (h, r, fds) = next_request(&conn);
        assert!(fds.is_empty());
        let Request::Call(Call::Inspect(id)) = r else {
            panic!("expected an inspect, got {r:?}")
        };
        answer(&conn, h, &Answer::Refused(Refusal::NoSuchBox(id)));
    });
    let client = Client::connect(lab.link()).unwrap();
    assert_eq!(client.classes(), vec![Class::Oci]);
    let b = BoxId::new("b").unwrap();
    assert_eq!(client.inspect(&b), Err(Refusal::NoSuchBox(b)));
    agent.join().unwrap();
}

/// HEX-GUEST-6: exec a program with stdin and stdout through the relay: expect the node to get the exec
/// with two descriptors, each a pipe's end the program can use, the bytes the host writes to stdin read
/// from the one, the bytes written to the other read back by the host to its end, and the end as the
/// node sent it.
#[test]
fn an_execs_streams_reach_the_node_as_pipe_ends() {
    let lab = Lab::new(LIMITS);
    let node = lab.node.try_clone().unwrap();
    let agent = std::thread::spawn(move || {
        let conn = hello(&node);
        let (h, r, fds) = next_request(&conn);
        assert!(matches!(r, Request::Call(Call::Exec { .. })), "{r:?}");
        assert_eq!(fds.len(), 2);
        for fd in &fds {
            let st = rustix::fs::fstat(fd).unwrap();
            assert_eq!(FileType::from_raw_mode(st.st_mode), FileType::Fifo);
        }
        answer(&conn, h, &Answer::Started { pid: 7 });
        let mut fds = fds.into_iter();
        let mut stdin = File::from(fds.next().unwrap());
        let mut stdout = File::from(fds.next().unwrap());
        let mut got = String::new();
        stdin.read_to_string(&mut got).unwrap();
        stdout
            .write_all(format!("pong to {got}").as_bytes())
            .unwrap();
        drop(stdout);
        answer(&conn, h, &Answer::Exited(Exit::Code(3)));
        conn
    });
    let client = Client::connect(lab.link()).unwrap();
    let (in_r, mut in_w) = std::io::pipe().unwrap();
    let (mut out_r, out_w) = std::io::pipe().unwrap();
    let mut proc = client
        .exec(
            &BoxId::new("b").unwrap(),
            Program::of(&["cat"]),
            Stdio {
                stdin: Some(in_r),
                stdout: Some(out_w),
                stderr: None,
            },
        )
        .unwrap();
    assert_eq!(proc.pid(), 7);
    in_w.write_all(b"ping").unwrap();
    drop(in_w);
    let mut out = String::new();
    out_r.read_to_string(&mut out).unwrap();
    assert_eq!(out, "pong to ping");
    assert_eq!(
        proc.wait_for(Duration::from_secs(30)).unwrap(),
        Some(Exit::Code(3))
    );
    drop(agent.join().unwrap());
}

/// HEX-GUEST-7: exec with a stdin whose connection never reaches the relay: expect the exec refused by
/// type once the relay's patience ends, naming its streams, and the node never asked to run it.
#[test]
fn an_exec_whose_stream_never_comes_is_refused_and_never_reaches_the_node() {
    let lab = Lab::new(Limits {
        patience: Duration::from_millis(300),
        ..LIMITS
    });
    // Stdio goes to a socket nobody serves from: it connects, and nothing reads it.
    let nowhere = lab.dir.path().join("nowhere.sock");
    let _nowhere = UnixListener::bind(&nowhere).unwrap();
    let link = Arc::new(Ports {
        control: lab.dir.path().join("control.sock"),
        stdio: nowhere,
    });
    let node = lab.node.try_clone().unwrap();
    let agent = std::thread::spawn(move || {
        let conn = hello(&node);
        // Nothing more comes: the exec was refused at the relay.
        assert!(!readable(&conn, 900));
        conn
    });
    let client = Client::connect(link).unwrap();
    let (in_r, _in_w) = std::io::pipe().unwrap();
    let got = client.exec(
        &BoxId::new("b").unwrap(),
        Program::of(&["cat"]),
        Stdio {
            stdin: Some(in_r),
            stdout: None,
            stderr: None,
        },
    );
    let Err(Refusal::Invalid { what: "stdio", why }) = got.map(|_| ()) else {
        panic!("expected the exec's stdio refused");
    };
    assert!(why.contains("0 of the exec's 1 streams"), "{why}");
    drop(agent.join().unwrap());
}

/// HEX-GUEST-7: connect twice to a relay bounded at one connection: expect the second refused by type,
/// naming the bound, as the node's own socket refuses one past its own.
#[test]
fn a_connection_past_the_bound_is_refused() {
    let lab = Lab::new(Limits {
        connections: 1,
        ..LIMITS
    });
    let node = lab.node.try_clone().unwrap();
    let agent = std::thread::spawn(move || hello(&node));
    let _first = Client::connect(lab.link()).unwrap();
    let held = agent.join().unwrap();
    assert_eq!(
        Client::connect(lab.link()).err(),
        Some(Refusal::Exhausted {
            what: crate::serve::CONNECTIONS,
            bound: 1,
        })
    );
    drop(held);
}
