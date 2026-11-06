//! The guest relay's tests (docs/design/shards-guest.md §3, HEX-GUEST-6 and HEX-GUEST-7; native plan
//! step 5): a host's guest client, through the relay, reaches a stand-in node agent's own socket; its
//! calls and answers cross whole; an exec's streams reach the node as pipe ends with the exec; and the
//! relay's bounds refuse by type: an exec whose stream never comes, and a connection past the bound.
//! And its staging port (§4a, HEX-GUEST-14): a host tree staged lands under the box's staged root as it
//! was, and goes when removed; a stage that names its way out, or comes twice, is refused and leaves
//! nothing behind.

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use super::*;
use hex_api::{BoxId, Class, Exit, Program, Runtime, Stdio};
use hex_client::guest::{Client, Link};
use hex_core::stream::files as hf;
use hex_core::wire::{Header, Hello, Received};
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::fs::FileType;

/// The guest's ports as Unix sockets: the relay's control and stdio listeners, or a stdio socket nobody
/// serves from (`stdio` set apart).
struct Ports {
    control: PathBuf,
    stdio: PathBuf,
    files: PathBuf,
}

impl Link for Ports {
    fn dial(&self, port: u32) -> Result<Box<dyn Duplex>, Refusal> {
        let path = match port {
            stream::CONTROL_PORT => &self.control,
            stream::STDIO_PORT => &self.stdio,
            stream::files::FILES_PORT => &self.files,
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
        let files = UnixListener::bind(dir.path().join("files.sock")).unwrap();
        let staged = dir.path().join("staged");
        std::fs::create_dir(&staged).unwrap();
        let relay = Relay::new(&node_path, &staged, limits);
        std::thread::spawn(move || {
            relay.serve(Box::new(control), Box::new(stdio), Box::new(files))
        });
        Self { dir, node }
    }

    fn link(&self) -> Arc<dyn Link> {
        Arc::new(Ports {
            control: self.dir.path().join("control.sock"),
            stdio: self.dir.path().join("stdio.sock"),
            files: self.dir.path().join("files.sock"),
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

/// Whether a message, or its end, is waiting on `conn` within `ms` (0: now).
fn readable(conn: impl AsFd, ms: i64) -> bool {
    let mut ready = [PollFd::new(&conn, PollFlags::IN)];
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

use std::time::Duration;

/// A host connection that says each time its client writes to it.
struct Told {
    conn: UnixStream,
    wrote: std::sync::mpsc::Sender<()>,
}

impl Read for Told {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.conn.read(buf)
    }
}

impl Write for Told {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.conn.write(buf)?;
        let _ = self.wrote.send(());
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.conn.flush()
    }
}

impl Duplex for Told {
    fn try_clone(&self) -> io::Result<Box<dyn Duplex>> {
        Ok(Box::new(Told {
            conn: self.conn.try_clone()?,
            wrote: self.wrote.clone(),
        }))
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.conn.shutdown(how)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.conn.set_write_timeout(timeout)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.conn.set_read_timeout(timeout)
    }
}

/// The relay's control port, its client's writes told and the connection held for the test to end;
/// stdio to `stdio`.
struct Held {
    control: PathBuf,
    stdio: PathBuf,
    wrote: Mutex<std::sync::mpsc::Sender<()>>,
    held: Mutex<Option<UnixStream>>,
}

impl Link for Held {
    fn dial(&self, port: u32) -> Result<Box<dyn Duplex>, Refusal> {
        if port != stream::CONTROL_PORT {
            return Ok(Box::new(UnixStream::connect(&self.stdio).unwrap()));
        }
        let conn = UnixStream::connect(&self.control).unwrap();
        *self.held.lock().unwrap() = Some(conn.try_clone().unwrap());
        Ok(Box::new(Told {
            conn,
            wrote: self.wrote.lock().unwrap().clone(),
        }))
    }
}

/// An exec of `cat` with stdin alone, through `client`.
fn exec_stdin(client: &Client) -> Result<Box<dyn hex_api::Proc>, Refusal> {
    let (in_r, _in_w) = std::io::pipe().unwrap();
    client.exec(
        &BoxId::new("b").unwrap(),
        Program::of(&["cat"]),
        Stdio {
            stdin: Some(in_r),
            stdout: None,
            stderr: None,
        },
    )
}

/// HEX-GUEST-7 (CLAUDE.md §4: facts, not clocks): on a relay of one set, exec with a stdin whose
/// connection never reaches the relay, then end the host's connection: expect the exec never sent to
/// the node, its set gone with its host (no clock), so a second host's exec takes the one set and runs.
#[test]
fn an_exec_whose_stream_never_comes_waits_for_its_host_and_goes_with_it() {
    let lab = Lab::new(Limits { sets: 1, ..LIMITS });
    // Stdio goes to a socket nobody serves from: it connects, and nothing reads it.
    let nowhere = lab.dir.path().join("nowhere.sock");
    let nowhere_listener = UnixListener::bind(&nowhere).unwrap();
    let (wrote, writes) = std::sync::mpsc::channel();
    let link = Arc::new(Held {
        control: lab.dir.path().join("control.sock"),
        stdio: nowhere,
        wrote: Mutex::new(wrote),
        held: Mutex::new(None),
    });
    let node = lab.node.try_clone().unwrap();
    let agent = std::thread::spawn(move || {
        let first = hello(&node);
        // The host's end reaches the node as its connection's end, with no exec before it.
        let mut buf = [0u8; 64];
        assert_eq!(rustix::io::read(&first, &mut buf).unwrap(), 0);
        let second = hello(&node);
        let (h, r, fds) = next_request(&second);
        assert!(matches!(r, Request::Call(Call::Exec { .. })), "{r:?}");
        assert_eq!(fds.len(), 1);
        answer(&second, h, &Answer::Started { pid: 8 });
        answer(&second, h, &Answer::Exited(Exit::Code(0)));
        second
    });
    let first = Client::connect(Arc::clone(&link) as Arc<dyn Link>).unwrap();
    while writes.try_recv().is_ok() {}
    std::thread::scope(|scope| {
        let waiting = scope.spawn(|| exec_stdin(&first).map(|_| ()));
        // The client dials its stream, then writes its exec: once written, the relay reads it before
        // the end that follows on the same connection.
        let _stream = nowhere_listener.accept().unwrap();
        writes.recv().unwrap();
        let host = link.held.lock().unwrap().take().unwrap();
        host.shutdown(Shutdown::Write).unwrap();
        assert!(matches!(waiting.join().unwrap(), Err(Refusal::Broken(_))));
    });
    let second = Client::connect(lab.link()).unwrap();
    let mut proc = exec_stdin(&second).unwrap();
    assert_eq!(proc.wait_for(Duration::MAX).unwrap(), Some(Exit::Code(0)));
    drop(agent.join().unwrap());
}

/// HEX-GUEST-7: connect a stdio connection that never names itself, then exec with stdout through the
/// relay: expect the exec's stream named and its output read while the silent one waits, and the silent
/// one still open after (it waits on its host, not a clock).
#[test]
fn a_silent_stdio_connection_holds_no_other_exec_up() {
    let lab = Lab::new(LIMITS);
    let silent = UnixStream::connect(lab.dir.path().join("stdio.sock")).unwrap();
    let node = lab.node.try_clone().unwrap();
    let agent = std::thread::spawn(move || {
        let conn = hello(&node);
        let (h, _, fds) = next_request(&conn);
        answer(&conn, h, &Answer::Started { pid: 9 });
        let mut stdout = File::from(fds.into_iter().next().unwrap());
        stdout.write_all(b"named").unwrap();
        drop(stdout);
        answer(&conn, h, &Answer::Exited(Exit::Code(0)));
        conn
    });
    let client = Client::connect(lab.link()).unwrap();
    let (mut out_r, out_w) = std::io::pipe().unwrap();
    let mut proc = client
        .exec(
            &BoxId::new("b").unwrap(),
            Program::of(&["echo"]),
            Stdio {
                stdin: None,
                stdout: Some(out_w),
                stderr: None,
            },
        )
        .unwrap();
    let mut out = String::new();
    out_r.read_to_string(&mut out).unwrap();
    assert_eq!(out, "named");
    assert_eq!(proc.wait_for(Duration::MAX).unwrap(), Some(Exit::Code(0)));
    assert!(!readable(&silent, 0), "the silent connection was closed");
    drop(agent.join().unwrap());
}

/// HEX-GUEST-7 (CLAUDE.md §2): on a relay bounded at two unnamed stdio connections, connect three that
/// never name themselves: expect the third refused and closed (its end is what it reads), the two
/// within the bound still open.
#[test]
fn an_unnamed_connection_past_the_bound_is_refused() {
    let lab = Lab::new(Limits {
        unnamed: 2,
        ..LIMITS
    });
    let path = lab.dir.path().join("stdio.sock");
    let held = [
        UnixStream::connect(&path).unwrap(),
        UnixStream::connect(&path).unwrap(),
    ];
    let mut past = UnixStream::connect(&path).unwrap();
    let mut byte = [0u8; 1];
    assert_eq!(past.read(&mut byte).unwrap(), 0);
    for conn in &held {
        assert!(
            !readable(conn, 0),
            "a connection within the bound was closed"
        );
    }
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

fn staged_lab() -> (Lab, PathBuf) {
    let lab = Lab::new(LIMITS);
    let staged = lab.dir.path().join("staged");
    (lab, staged)
}

/// Sends `bytes` raw on the staging port and returns the relay's answer.
fn stage_raw(lab: &Lab, bytes: &[u8]) -> Answer {
    let mut conn = UnixStream::connect(lab.dir.path().join("files.sock")).unwrap();
    let _ = conn.write_all(bytes);
    conn.shutdown(Shutdown::Write).unwrap();
    let mut back = Vec::new();
    conn.read_to_end(&mut back).unwrap();
    let mut d = Deframer::new(wire::MAX_MESSAGE);
    let mut rest = &back[..];
    let message = loop {
        let (took, whole) = d.take(rest).unwrap();
        rest = &rest[took..];
        if let Some(m) = whole {
            break m;
        }
    };
    let received = Received {
        bytes: &message,
        size: message.len(),
        descriptors: 0,
        descriptors_cut: false,
    };
    wire::read_answer(&received, wire::MAX_MESSAGE).unwrap().1
}

/// HEX-GUEST-14: stage a host tree (a folder, a file with mode 0750, an empty file, a symlink) and a
/// bytes item through the host's packer: expect each under `<staged>/box-1/` as it was, modes kept,
/// the root 0755; then remove it: expect it gone, and a second removal refused as no such box.
#[test]
fn a_staged_tree_lands_as_it_was_and_goes_when_removed() {
    use hex_client::guest::stage::{Item, Source, stage, unstage};
    use std::os::unix::fs::PermissionsExt;
    let (lab, staged) = staged_lab();
    let host = tempfile::tempdir().unwrap();
    let root = host.path().join("rootfs");
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join("bin/sh"), b"#!sh\n").unwrap();
    std::fs::set_permissions(root.join("bin/sh"), std::fs::Permissions::from_mode(0o750)).unwrap();
    std::fs::set_permissions(root.join("bin"), std::fs::Permissions::from_mode(0o555)).unwrap();
    std::fs::write(root.join("empty"), b"").unwrap();
    std::os::unix::fs::symlink("/usr/lib", root.join("lib")).unwrap();
    let id = BoxId::new("box-1").unwrap();
    let items = [
        Item {
            name: "root".to_owned(),
            source: Source::Path(root.clone()),
        },
        Item {
            name: "config.json".to_owned(),
            source: Source::Bytes(b"{}".to_vec()),
        },
    ];
    let link = lab.link();
    stage(link.as_ref(), &id, &items, &hf::LIMITS).unwrap();
    let copy = staged.join("box-1");
    let mode = |p: &str| {
        std::fs::symlink_metadata(copy.join(p))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    };
    assert_eq!(mode(""), 0o755);
    assert_eq!(std::fs::read(copy.join("root/bin/sh")).unwrap(), b"#!sh\n");
    assert_eq!(mode("root/bin/sh"), 0o750);
    assert_eq!(mode("root/bin"), 0o555);
    assert_eq!(std::fs::read(copy.join("root/empty")).unwrap(), b"");
    assert_eq!(
        std::fs::read_link(copy.join("root/lib")).unwrap(),
        Path::new("/usr/lib")
    );
    assert_eq!(std::fs::read(copy.join("config.json")).unwrap(), b"{}");
    // A box id staged already is refused, its copy untouched.
    assert_eq!(
        stage(link.as_ref(), &id, &items, &hf::LIMITS),
        Err(Refusal::Exists(id.clone()))
    );
    assert!(copy.join("root/bin/sh").exists());
    unstage(link.as_ref(), &id).unwrap();
    assert!(!copy.exists());
    assert_eq!(unstage(link.as_ref(), &id), Err(Refusal::NoSuchBox(id)));
    std::fs::set_permissions(root.join("bin"), std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The raw stage of `entries` (path, kind, target) for `b`.
fn raw_stage(entries: &[(&str, hf::What)]) -> Vec<u8> {
    let mut out = hf::open(hf::Op::Stage, "b").unwrap();
    for (path, what) in entries {
        let e = hf::Entry {
            path: (*path).to_owned(),
            mode: 0o755,
            what: what.clone(),
            meta: None,
        };
        out.extend(hf::head(&e, &hf::LIMITS).unwrap());
    }
    out.extend(hf::END);
    out
}

/// HEX-GUEST-14: stage a symlink `a` to `/` and then a directory `a/escaped` under it, and a symlink
/// to the staged folder's parent then a file under it, outside any layer: expect each refused as leading
/// through a symbolic link, nothing made outside the box's root, and the partial copy removed; and the same
/// within a layer: expect each inside the layer's root, as containerd resolves it (§4a).
#[test]
fn a_stage_that_names_its_way_out_through_a_link_is_refused_and_leaves_nothing() {
    let (lab, staged) = staged_lab();
    for (link_to, under) in [("/", hf::What::Dir), ("../..", hf::What::File { size: 0 })] {
        let answer = stage_raw(
            &lab,
            &raw_stage(&[
                (
                    "a",
                    hf::What::Symlink {
                        target: link_to.to_owned(),
                    },
                ),
                ("a/escaped", under),
            ]),
        );
        let Answer::Refused(Refusal::Invalid { why, .. }) = answer else {
            panic!("expected a refusal, got {answer:?}");
        };
        assert!(why.contains("symbolic link"), "{why}");
        assert!(!Path::new("/escaped").exists());
        assert!(!lab.dir.path().join("escaped").exists());
        assert!(!staged.join("b").exists(), "the partial copy was left");
    }
    // In a layer, the same names resolve within the layer's root (§4a, the lead's ruling): inside it.
    for (link_to, under) in [("/", hf::What::Dir), ("../..", hf::What::File { size: 0 })] {
        let answer = stage_raw(
            &lab,
            &raw_stage(&[
                ("r", hf::What::Dir),
                ("r", hf::What::Layer),
                (
                    "r/a",
                    hf::What::Symlink {
                        target: link_to.to_owned(),
                    },
                ),
                ("r/a/escaped", under),
            ]),
        );
        assert_eq!(answer, Answer::Done);
        assert!(!Path::new("/escaped").exists());
        assert!(!lab.dir.path().join("escaped").exists());
        assert!(!staged.join("escaped").exists());
        assert!(
            std::fs::symlink_metadata(staged.join("b/r/escaped")).is_ok(),
            "it lands at the layer's root"
        );
        hex_client::guest::stage::unstage(lab.link().as_ref(), &BoxId::new("b").unwrap()).unwrap();
    }
}

/// HEX-GUEST-14: stage an entry of a device's kind, an entry twice, and an entry under a file: expect
/// each refused by type (the file by its name) and the partial copy removed; and a stream cut before its end head refused as truncated.
#[test]
fn a_hostile_or_cut_stage_is_refused_and_leaves_nothing() {
    let (lab, staged) = staged_lab();
    let mut device = hf::open(hf::Op::Stage, "b").unwrap();
    // A head of kind 8 (none on the stream: a device has none), mode 0644, a one-byte path and no size.
    device.extend([8u8, 0]);
    device.extend(0o644u16.to_le_bytes());
    device.extend(1u16.to_le_bytes());
    device.extend([0u8; 10]);
    device.push(b'd');
    let Answer::Refused(Refusal::Invalid { why, .. }) = stage_raw(&lab, &device) else {
        panic!("expected a refusal");
    };
    assert!(
        why.contains("devices, FIFOs and sockets are never staged"),
        "{why}"
    );
    assert!(!staged.join("b").exists());
    let twice = raw_stage(&[("d", hf::What::Dir), ("d", hf::What::Dir)]);
    let Answer::Refused(Refusal::Invalid { why, .. }) = stage_raw(&lab, &twice) else {
        panic!("expected a refusal");
    };
    assert!(why.contains("came twice"), "{why}");
    assert!(!staged.join("b").exists());
    // A file in the way of a later entry's parent is refused by its name, and the copy removed.
    let through_a_file = raw_stage(&[("f", hf::What::File { size: 0 }), ("f/x", hf::What::Dir)]);
    let Answer::Refused(Refusal::Invalid { why, .. }) = stage_raw(&lab, &through_a_file) else {
        panic!("expected a refusal");
    };
    assert!(why.contains("\"f\", which isn't a directory"), "{why}");
    assert!(!staged.join("b").exists());
    let whole = raw_stage(&[("d", hf::What::Dir)]);
    let Answer::Refused(Refusal::Invalid { why, .. }) = stage_raw(&lab, &whole[..whole.len() - 1])
    else {
        panic!("expected a refusal");
    };
    assert!(why.contains("ended before its end head"), "{why}");
    assert!(!staged.join("b").exists());
}

/// One entry of a test layer: a directory, a file and its bytes, a symbolic link and its text, a hard
/// link and its target.
enum T<'a> {
    Dir(&'a str),
    DirMode(&'a str, u32),
    File(&'a str, &'a [u8]),
    Link(&'a str, &'a str),
    Hard(&'a str, &'a str),
}

/// Writes a layer of `entries` at `path`, each owned by this test's own uid and gid, mtime `mtime`.
fn layer_tar(path: &Path, mtime: u64, entries: &[T]) {
    let mut b = tar::Builder::new(File::create(path).unwrap());
    for e in entries {
        let mut h = tar::Header::new_ustar();
        h.set_uid(u64::from(rustix::process::getuid().as_raw()));
        h.set_gid(u64::from(rustix::process::getgid().as_raw()));
        h.set_mtime(mtime);
        h.set_size(0);
        match e {
            T::Dir(name) | T::DirMode(name, _) => {
                h.set_entry_type(tar::EntryType::Directory);
                h.set_mode(match e {
                    T::DirMode(_, mode) => *mode,
                    _ => 0o755,
                });
                b.append_data(&mut h, format!("{name}/"), std::io::empty())
                    .unwrap();
            }
            T::File(name, data) => {
                h.set_entry_type(tar::EntryType::Regular);
                h.set_mode(0o644);
                h.set_size(data.len() as u64);
                b.append_data(&mut h, name, *data).unwrap();
            }
            T::Link(name, to) => {
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_mode(0o777);
                b.append_link(&mut h, name, to).unwrap();
            }
            T::Hard(name, to) => {
                h.set_entry_type(tar::EntryType::Link);
                h.set_mode(0o644);
                b.append_link(&mut h, name, to).unwrap();
            }
        }
    }
    b.into_inner().unwrap();
}

/// Stages `layers` (each its mtime and entries) as one root through the host's packer and the relay.
fn stage_layers(lab: &Lab, host: &Path, layers: &[(u64, Vec<T>)]) -> Result<(), Refusal> {
    use hex_client::guest::stage::{Item, Source, stage};
    let paths: Vec<PathBuf> = layers
        .iter()
        .enumerate()
        .map(|(i, (mtime, entries))| {
            let path = host.join(format!("layer{i}.tar"));
            layer_tar(&path, *mtime, entries);
            path
        })
        .collect();
    let items = [Item {
        name: "root".to_owned(),
        source: Source::Layers(paths),
    }];
    stage(
        lab.link().as_ref(),
        &BoxId::new("box-1").unwrap(),
        &items,
        &hf::LIMITS,
    )
}

/// HEX-GUEST-14 with the host's packer (the lead's ruling, §4a): stage a tarball root whose entries are
/// written through links it made, end to end through the relay: `a` to `/etc` then `a/x`, and `c` to
/// `../../..` (the lab's folder, from the staged root) then `c/x`, with a sentinel `x` in that folder:
/// expect each where containerd's `fs.RootPath` puts it, inside the root (`etc/x`, `x`), and the sentinel
/// untouched.
#[test]
fn a_tarball_writing_through_its_own_link_lands_inside_its_root() {
    let (lab, staged) = staged_lab();
    let host = tempfile::tempdir().unwrap();
    let sentinel = lab.dir.path().join("x");
    std::fs::write(&sentinel, b"sentinel").unwrap();
    stage_layers(
        &lab,
        host.path(),
        &[(
            1_600_000_000,
            vec![
                T::Dir("etc"),
                T::Link("a", "/etc"),
                T::File("a/x", b"in etc"),
                T::Link("c", "../../.."),
                T::File("c/x", b"at the top"),
            ],
        )],
    )
    .unwrap();
    let root = staged.join("box-1/root");
    assert_eq!(std::fs::read(root.join("etc/x")).unwrap(), b"in etc");
    assert_eq!(std::fs::read(root.join("x")).unwrap(), b"at the top");
    assert_eq!(std::fs::read(&sentinel).unwrap(), b"sentinel");
    assert!(!lab.dir.path().join("staged/x").exists());
    assert_eq!(
        std::fs::read_link(root.join("a")).unwrap(),
        Path::new("/etc"),
        "the link itself is kept as written"
    );
}

/// HEX-GUEST-14 (the lead's ruling after ctr's snapshot, §4a; containerd's overlay applier,
/// `core/diff/apply/apply_linux.go:36-55`, and `mkparent`, `pkg/archive/tar.go:448-503`): stage `bin` as a
/// link to `usr/bin` (mode 0750) in one layer and `bin/tool` in the next: expect `bin` a directory holding
/// `tool`, with `usr/bin`'s owner and mode copied as mkparent copies them, and `usr/bin` without it, as
/// ctr's snapshot of the same layers holds (bench/native/guest-tree, lab:tree).
#[test]
fn a_later_layers_entry_under_an_earlier_layers_link_makes_a_directory_as_overlay_does() {
    use std::os::unix::fs::PermissionsExt;
    let (lab, staged) = staged_lab();
    let host = tempfile::tempdir().unwrap();
    stage_layers(
        &lab,
        host.path(),
        &[
            (
                1_600_000_000,
                vec![
                    T::Dir("usr"),
                    T::DirMode("usr/bin", 0o750),
                    T::Link("bin", "usr/bin"),
                ],
            ),
            (1_600_000_100, vec![T::File("bin/tool", b"tool")]),
        ],
    )
    .unwrap();
    let root = staged.join("box-1/root");
    let bin = std::fs::symlink_metadata(root.join("bin")).unwrap();
    assert!(bin.is_dir(), "bin became a directory");
    assert_eq!(bin.permissions().mode() & 0o7777, 0o750, "usr/bin's mode");
    assert_eq!(std::fs::read(root.join("bin/tool")).unwrap(), b"tool");
    assert!(!root.join("usr/bin/tool").exists(), "usr/bin is unchanged");
}

/// HEX-GUEST-14 (§4a, as ctr's snapshot of lab:file holds): stage a file `p` in one layer and `p/inner`
/// in the next: expect `p` a directory 0755 holding `inner`, as overlay's mkparent makes it.
#[test]
fn a_later_layers_entry_under_an_earlier_layers_file_makes_a_directory_as_overlay_does() {
    use std::os::unix::fs::PermissionsExt;
    let (lab, staged) = staged_lab();
    let host = tempfile::tempdir().unwrap();
    stage_layers(
        &lab,
        host.path(),
        &[
            (1_600_000_000, vec![T::File("p", b"p")]),
            (1_600_000_100, vec![T::File("p/inner", b"inner")]),
        ],
    )
    .unwrap();
    let root = staged.join("box-1/root");
    let p = std::fs::symlink_metadata(root.join("p")).unwrap();
    assert!(p.is_dir());
    assert_eq!(p.permissions().mode() & 0o7777, 0o755);
    assert_eq!(std::fs::read(root.join("p/inner")).unwrap(), b"inner");
}

/// HEX-GUEST-14 (§4a, as ctr fails lab:hardlink's unpack, `link .../q .../h: no such file or
/// directory`): stage a hard link in one layer to a file an earlier layer made: expect the stage
/// refused naming the link and the file, and the partial copy gone. (The relay refuses it likewise
/// where the packer can't tell, a target named through a link: `Stage::entry`.)
#[test]
fn a_hard_link_to_an_earlier_layers_file_is_refused_as_overlay_fails_it() {
    let (lab, staged) = staged_lab();
    let host = tempfile::tempdir().unwrap();
    let got = stage_layers(
        &lab,
        host.path(),
        &[
            (1_600_000_000, vec![T::File("q", b"q")]),
            (1_600_000_100, vec![T::Hard("h", "q")]),
        ],
    );
    let why = got.unwrap_err().to_string();
    // The packer refuses it first, naming the archive's entry `h` and its target `q`.
    assert!(
        why.contains(": h: ") && why.contains("\"q\"") && why.contains("an earlier layer made"),
        "{why}"
    );
    assert!(!staged.join("box-1").exists(), "the partial copy was left");
}

/// HEX-GUEST-14 (the lead's ruling, §4a): stage a hard link whose target is named through the root's own
/// link (`bin` to `/usr/bin`, `usr/bin/perl` to `bin/perl5`): expect it the target's inode; and one named
/// through a link to `../../..`, where a file of that name sits outside the root: expect it refused as
/// containerd's unpack fails it, for the same reason: the target resolves inside the root
/// (`hardlinkRootPath`, `pkg/archive/tar.go:795-807`), where nothing of that name is (`ENOENT`), and the
/// outside file is never linked.
#[test]
fn a_hard_link_through_the_roots_own_link_resolves_inside_it() {
    use std::os::unix::fs::MetadataExt;
    let (lab, staged) = staged_lab();
    let host = tempfile::tempdir().unwrap();
    stage_layers(
        &lab,
        host.path(),
        &[(
            1_600_000_000,
            vec![
                T::Dir("usr"),
                T::Dir("usr/bin"),
                T::File("usr/bin/perl5", b"perl"),
                T::Link("bin", "/usr/bin"),
                T::Hard("usr/bin/perl", "bin/perl5"),
            ],
        )],
    )
    .unwrap();
    let root = staged.join("box-1/root");
    let perl5 = std::fs::metadata(root.join("usr/bin/perl5")).unwrap();
    let perl = std::fs::symlink_metadata(root.join("usr/bin/perl")).unwrap();
    assert_eq!(perl.ino(), perl5.ino());
    hex_client::guest::stage::unstage(lab.link().as_ref(), &BoxId::new("box-1").unwrap()).unwrap();

    let secret = lab.dir.path().join("secret");
    std::fs::write(&secret, b"outside").unwrap();
    let got = stage_layers(
        &lab,
        host.path(),
        &[(
            1_600_000_000,
            vec![T::Link("d", "../../.."), T::Hard("h", "d/secret")],
        )],
    );
    let why = got.unwrap_err().to_string();
    assert!(
        why.contains("which isn't staged") && why.contains("No such file or directory"),
        "{why}"
    );
    assert_eq!(std::fs::metadata(&secret).unwrap().nlink(), 1);
    assert!(!staged.join("box-1").exists(), "the partial copy was left");
}

/// HEX-GUEST-14 (the lead's ruling, §4a; containerd's `mkparent`, `pkg/archive/tar.go:448-503`): stage a
/// layer holding `usr/local/bin/x` with no entry for `usr/local` or `usr/local/bin`, as hand-made and
/// Bazel layers are: expect the file staged and each missing parent made 0755, as containerd makes it.
#[test]
fn an_entry_whose_parents_no_layer_made_gets_them_made_as_containerd_does() {
    use std::os::unix::fs::PermissionsExt;
    let (lab, staged) = staged_lab();
    let host = tempfile::tempdir().unwrap();
    stage_layers(
        &lab,
        host.path(),
        &[(
            1_600_000_000,
            vec![T::Dir("usr"), T::File("usr/local/bin/x", b"x")],
        )],
    )
    .unwrap();
    let root = staged.join("box-1/root");
    assert_eq!(std::fs::read(root.join("usr/local/bin/x")).unwrap(), b"x");
    for made in ["usr/local", "usr/local/bin"] {
        let meta = std::fs::symlink_metadata(root.join(made)).unwrap();
        assert!(meta.is_dir(), "{made}");
        assert_eq!(meta.permissions().mode() & 0o7777, 0o755, "{made}");
    }
}

/// HEX-GUEST-14 (§4a; containerd's `link`, os.Link, `pkg/archive/tar.go:386-394`): stage a hard link to a
/// symbolic link: expect another name for the link itself, as linkat(2) without `AT_SYMLINK_FOLLOW` makes.
#[test]
fn a_hard_link_to_a_symbolic_link_names_the_link_itself() {
    use std::os::unix::fs::MetadataExt;
    let (lab, staged) = staged_lab();
    let host = tempfile::tempdir().unwrap();
    stage_layers(
        &lab,
        host.path(),
        &[(
            1_600_000_000,
            vec![T::Link("sh", "bash"), T::Hard("sh2", "sh")],
        )],
    )
    .unwrap();
    let root = staged.join("box-1/root");
    let sh = std::fs::symlink_metadata(root.join("sh")).unwrap();
    let sh2 = std::fs::symlink_metadata(root.join("sh2")).unwrap();
    assert!(sh2.file_type().is_symlink());
    assert_eq!(sh.ino(), sh2.ino());
}

/// HEX-GUEST-14 (§4a, as ctr fails lab:hardlink's unpack): send the relay, raw, a layer making `r/q` and
/// a later layer linking `r/h` to it: expect the stage refused naming both and the earlier layer, and the
/// partial copy gone, the relay holding the rule where no packer stands before it.
#[test]
fn the_relay_refuses_a_hard_link_to_an_earlier_layers_file() {
    let (lab, staged) = staged_lab();
    let answer = stage_raw(
        &lab,
        &raw_stage(&[
            ("r", hf::What::Dir),
            ("r", hf::What::Layer),
            ("r/q", hf::What::File { size: 0 }),
            ("r", hf::What::Layer),
            (
                "r/h",
                hf::What::Hardlink {
                    target: "r/q".to_owned(),
                },
            ),
        ]),
    );
    let Answer::Refused(Refusal::Invalid { why, .. }) = answer else {
        panic!("expected a refusal, got {answer:?}");
    };
    assert!(
        why.contains("\"r/h\"") && why.contains("\"r/q\"") && why.contains("an earlier layer made"),
        "{why}"
    );
    assert!(!staged.join("b").exists(), "the partial copy was left");
}

/// HEX-GUEST-14 (§4a, containerd's `applyNaive`): stage two layers, the second whiting out `etc/a`,
/// making `d` opaque and adding `d/new`, and replacing a file with a directory, a directory with a file
/// and a link with a directory: expect `etc/a` gone and `etc/b` kept, `d` holding `d/new` alone with the
/// second layer's mtime, and each replaced path of its new kind.
#[test]
fn a_later_layer_whites_out_makes_opaque_and_replaces() {
    use std::os::unix::fs::MetadataExt;
    let (lab, staged) = staged_lab();
    let host = tempfile::tempdir().unwrap();
    stage_layers(
        &lab,
        host.path(),
        &[
            (
                1_600_000_000,
                vec![
                    T::Dir("etc"),
                    T::File("etc/a", b"a"),
                    T::File("etc/b", b"b"),
                    T::Dir("d"),
                    T::File("d/old", b"old"),
                    T::Dir("d/keep"),
                    T::File("d/keep/k", b"k"),
                    T::File("f", b"file"),
                    T::Dir("g"),
                    T::File("g/inner", b"inner"),
                    T::Link("s", "etc"),
                ],
            ),
            (
                1_600_000_100,
                vec![
                    T::File("etc/.wh.a", b""),
                    T::Dir("d"),
                    T::File("d/.wh..wh..opq", b""),
                    T::File("d/new", b"new"),
                    T::Dir("f"),
                    T::File("g", b"now a file"),
                    T::Dir("s"),
                ],
            ),
        ],
    )
    .unwrap();
    let root = staged.join("box-1/root");
    assert!(!root.join("etc/a").exists());
    assert!(
        !root.join("etc/.wh.a").exists(),
        "a whiteout is never a file"
    );
    assert_eq!(std::fs::read(root.join("etc/b")).unwrap(), b"b");
    let mut d: Vec<String> = std::fs::read_dir(root.join("d"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    d.sort();
    assert_eq!(d, ["new"]);
    assert_eq!(
        std::fs::metadata(root.join("d")).unwrap().mtime(),
        1_600_000_100
    );
    assert!(std::fs::symlink_metadata(root.join("f")).unwrap().is_dir());
    assert_eq!(std::fs::read(root.join("g")).unwrap(), b"now a file");
    assert!(std::fs::symlink_metadata(root.join("s")).unwrap().is_dir());
    assert_eq!(
        std::fs::read(root.join("etc/b")).unwrap(),
        b"b",
        "the link's target is untouched when the link is replaced"
    );
}

/// HEX-GUEST-14 (§4a as amended): stage a tarball root, end to end through the host's packer and the relay,
/// whose file is owned by this test's own uid and gid (all a rootless relay may give), has a fractional mtime
/// and a later atime, a `user.` xattr and a setuid mode, with a hard link to it and a directory with its own mtime: expect each
/// on disk as the archive says, the link the same inode, the mode kept though the owner was set after it was
/// written, and the directory's mtime unmoved by its children. Other owners and `security.capability` take
/// the guest's root: HEX-GUEST-21 holds those.
#[test]
fn a_tarball_roots_owners_times_xattrs_and_hard_links_land_on_disk() {
    use hex_client::guest::stage::{Item, Source, stage};
    use std::os::unix::fs::MetadataExt;
    let (lab, staged) = staged_lab();
    let host = tempfile::tempdir().unwrap();
    let tarball = host.path().join("root.tar");
    let (uid, gid) = (
        rustix::process::getuid().as_raw(),
        rustix::process::getgid().as_raw(),
    );
    {
        let mut b = tar::Builder::new(File::create(&tarball).unwrap());
        let mut dir = tar::Header::new_ustar();
        dir.set_entry_type(tar::EntryType::Directory);
        dir.set_mode(0o750);
        dir.set_uid(u64::from(uid));
        dir.set_gid(u64::from(gid));
        dir.set_mtime(1_600_000_000);
        dir.set_size(0);
        b.append_data(&mut dir, "bin/", std::io::empty()).unwrap();
        // A PAX record (`%d key=value\n`, its length counting itself) before the file.
        let mut record = Vec::new();
        for (k, v) in [
            ("mtime", "1700000000.25"),
            ("atime", "1800000000.5"),
            ("SCHILY.xattr.user.hex", "staged"),
        ] {
            let body = format!(" {k}={v}\n");
            let mut n = body.len() + 1;
            while format!("{n}").len() + body.len() != n {
                n += 1;
            }
            record.extend(format!("{n}{body}").into_bytes());
        }
        let mut pax = tar::Header::new_ustar();
        pax.set_entry_type(tar::EntryType::XHeader);
        pax.set_mode(0o644);
        pax.set_size(record.len() as u64);
        b.append_data(&mut pax, "PaxHeaders/perl5", record.as_slice())
            .unwrap();
        let mut file = tar::Header::new_ustar();
        file.set_entry_type(tar::EntryType::Regular);
        file.set_mode(0o4755);
        file.set_uid(u64::from(uid));
        file.set_gid(u64::from(gid));
        file.set_mtime(1_700_000_000);
        file.set_size(4);
        b.append_data(&mut file, "bin/perl5", &b"perl"[..]).unwrap();
        let mut link = tar::Header::new_ustar();
        link.set_entry_type(tar::EntryType::Link);
        link.set_mode(0o4755);
        link.set_size(0);
        b.append_link(&mut link, "bin/perl", "bin/perl5").unwrap();
        b.into_inner().unwrap();
    }
    let id = BoxId::new("box-1").unwrap();
    let items = [Item {
        name: "root".to_owned(),
        source: Source::Tar(tarball),
    }];
    stage(lab.link().as_ref(), &id, &items, &hf::LIMITS).unwrap();
    let root = staged.join("box-1/root");
    let perl5 = std::fs::symlink_metadata(root.join("bin/perl5")).unwrap();
    let perl = std::fs::symlink_metadata(root.join("bin/perl")).unwrap();
    assert_eq!((perl5.uid(), perl5.gid()), (uid, gid));
    assert_eq!(
        perl5.mode() & 0o7777,
        0o4755,
        "setuid kept: the mode follows the owner"
    );
    assert_eq!(
        (perl5.mtime(), perl5.mtime_nsec()),
        (1_700_000_000, 250_000_000)
    );
    assert_eq!(
        (perl5.atime(), perl5.atime_nsec()),
        (1_800_000_000, 500_000_000),
        "its own atime, the later"
    );
    assert_eq!(perl.ino(), perl5.ino(), "the hard link is the same inode");
    assert_eq!(perl5.nlink(), 2);
    let mut value = [0u8; 16];
    let n = rustix::fs::getxattr(root.join("bin/perl5"), "user.hex", &mut value).unwrap();
    assert_eq!(&value[..n], b"staged");
    let bin = std::fs::symlink_metadata(root.join("bin")).unwrap();
    assert_eq!(bin.mode() & 0o7777, 0o750);
    assert_eq!(
        bin.mtime(),
        1_600_000_000,
        "the directory's mtime is set after its children"
    );
}
