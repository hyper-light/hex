//! The socket's own tests (decision 3, HEX-SOCK-3 to 8 and 10): a live server in this process, over a host
//! whose boxes are the simulator's and whose programs are real processes of this test, so the
//! server's pidfd watch, its bounds and the disconnect rule meet real programs without a cgroup.
//! Hostile bytes go in through a raw socket; the rest through `hex-client`, as a caller sends them.

use super::*;
use std::io::{Read, Write as _};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::PoisonError;
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::net::{AddressFamily, SendFlags, SocketAddrUnix, SocketFlags, SocketType};
use rustix::process::{Pid, Signal};

use hex_api::conformance::PATIENCE;
use hex_api::sim::SimRuntime;
use hex_api::{Bounds, BoxInfo, BoxSpec, ClassSpec, Signal as HexSignal, Usage};
use hex_client::Client;
use hex_core::patience::Patience;
use hex_testkit::waits::{EMPTYING, EXEC, until};

/// Longest a test waits for the server's answer to a call over its socket: twice the slowest recorded answer of the
/// kinds these tests ask (hello, inspect, netns counts, exec), an exec's (`client.answer.exec`, the census's debug
/// arm, as the tests run debug builds: hex_testkit::waits::EXEC), so a server that never answers fails the test
/// instead of hanging it. A program's end takes its own record, the suites' [`PATIENCE`]; each other fact, its own.
const ANSWERED: Duration = EXEC.saturating_mul(2);

/// The server's ending of a program whose client went away: its kill, its reaping and its count in its box, a box's
/// emptying's work, which the server's own wait for the programs it ends takes the record of (decision 9's table:
/// `node.emptying`).
static ABANDONED: Patience = Patience::seeded("test.serve.abandoned", EMPTYING);
/// The I/O thread's letting go of a connection that has closed, and its admitting of one that has connected: no
/// recorded operation makes either, and no census row times them yet, so these wait measuring, for the fact alone, each
/// answer recorded for the census to come, from the suites run under it.
static LET_GO: Patience = Patience::measuring("test.serve.let_go");
static ADMITTED: Patience = Patience::measuring("test.serve.admitted");

/// Shuts a host's inspect until opened: holds calls in flight on purpose. A held call waits for the test to open the
/// gate, the fact it needs, never for a time (CLAUDE.md §4): a test holds it shut across looks of its own, `quiet`'s
/// among them, so a gate that opened itself after a patience would answer a call the test is checking is held. Its
/// bound is the test's end: a test's [`Lab`] opens it as it drops, before the server stops, a failed assertion's
/// unwinding too, so no stop waits on a held call (CLAUDE.md §2).
#[derive(Default)]
struct Gate {
    shut: Mutex<bool>,
    opened: Condvar,
}

impl Gate {
    fn set(&self, shut: bool) {
        *self.shut.lock().unwrap_or_else(PoisonError::into_inner) = shut;
        self.opened.notify_all();
    }

    fn pass(&self) {
        let mut shut = self.shut.lock().unwrap();
        while *shut {
            shut = self.opened.wait(shut).unwrap();
        }
    }
}

/// Boxes in the simulator; programs real processes of this test, started on the request's stdio.
#[derive(Default)]
struct TestHost {
    sim: SimRuntime,
    gate: Gate,
    abandoned: Mutex<Vec<BoxId>>,
}

impl Runtime for TestHost {
    fn classes(&self) -> Vec<hex_api::Class> {
        self.sim.classes()
    }
    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        self.sim.create(spec)
    }
    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.sim.start(id)
    }
    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.sim.inspect(id)?;
        let (first, rest) = program.args.split_first().unwrap();
        let mut cmd = std::process::Command::new(first);
        cmd.args(rest).env_clear().envs(program.env.iter().cloned());
        let null = std::process::Stdio::null;
        cmd.stdin(stdio.stdin.map_or_else(null, std::process::Stdio::from));
        cmd.stdout(stdio.stdout.map_or_else(null, std::process::Stdio::from));
        cmd.stderr(stdio.stderr.map_or_else(null, std::process::Stdio::from));
        let child = cmd.spawn().map_err(|e| Refusal::os("spawning", e))?;
        Ok(Box::new(hex_runtime::linux::proc::PidfdProc::of_child(
            child.id(),
        )?))
    }
    /// The run's box in the simulator, its program a real process of this test as an exec's is.
    fn run(&self, spec: BoxSpec, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        let id = spec.id.clone();
        self.sim.create(spec)?;
        self.exec(&id, program, stdio)
    }
    fn signal(&self, id: &BoxId, signal: HexSignal) -> Result<(), Refusal> {
        self.sim.signal(id, signal)
    }
    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.gate.pass();
        self.sim.inspect(id)
    }
    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.sim.usage(id)
    }
    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        self.sim.list()
    }
    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        self.sim.delete(id)
    }
}

impl Host for TestHost {
    fn abandoned(&self, id: &BoxId) -> Result<(), Refusal> {
        self.abandoned.lock().unwrap().push(id.clone());
        Ok(())
    }

    fn netns(&self) -> Result<netns::Counts, Refusal> {
        Ok(counts())
    }
}

/// The test host's network namespace counts, each field a different value.
fn counts() -> netns::Counts {
    netns::Counts {
        made_ahead: 5,
        taken: 4,
        made_on_path: 3,
        discarded: 2,
        failed: 15,
        failed_by: netns::FailedBy {
            home: 1,
            unshare: 2,
            open: 3,
            loopback: 4,
            back: 5,
        },
        refused: None,
        refused_on_path: 6,
        last_failed: netns::LastFailed {
            unshare: Some(netns::Reason::new(Some(1), "unshare failed: EPERM")),
            ..netns::LastFailed::default()
        },
    }
}

struct Lab {
    server: Server,
    host: Arc<TestHost>,
    _dir: tempfile::TempDir,
}

impl Lab {
    fn new(limits: Limits) -> Self {
        Self::with(limits, 8)
    }

    /// A lab whose server runs calls on at most `workers` threads.
    fn with(limits: Limits, workers: usize) -> Self {
        let cache = PathBuf::from(std::env::var_os("HOME").unwrap()).join(".cache/hex-t7/serve");
        std::fs::create_dir_all(&cache).unwrap();
        let dir = tempfile::Builder::new()
            .prefix("sock-")
            .tempdir_in(&cache)
            .unwrap();
        let host = Arc::new(TestHost::default());
        host.sim
            .create(BoxSpec {
                id: BoxId::new("b").unwrap(),
                bounds: Bounds::default(),
                class: ClassSpec::Process,
                owner: None,
            })
            .unwrap();
        let server = Server::bind(
            &dir.path().join("node.sock"),
            Arc::clone(&host) as Arc<dyn Host>,
            limits,
            workers,
        )
        .unwrap();
        Self {
            server,
            host,
            _dir: dir,
        }
    }

    fn client(&self) -> Client {
        Client::connect(self.server.path()).unwrap()
    }

    /// A connection that sends what it is given, hostile or not.
    fn raw(&self) -> OwnedFd {
        let fd = rustix::net::socket_with(
            AddressFamily::UNIX,
            SocketType::SEQPACKET,
            SocketFlags::CLOEXEC,
            None,
        )
        .unwrap();
        rustix::net::connect(&fd, &SocketAddrUnix::new(self.server.path()).unwrap()).unwrap();
        fd
    }

    fn connections(&self) -> usize {
        self.server.shared.table.lock().unwrap().conns.len()
    }

    fn watched(&self) -> usize {
        self.server.shared.table.lock().unwrap().watched.len()
    }
}

impl Drop for Lab {
    /// Opens the host's gate before the server stops (this runs before the fields drop), so a call the test left held
    /// is let through and the server's stop never waits on it.
    fn drop(&mut self) {
        self.host.gate.set(false);
    }
}

fn limits() -> Limits {
    Limits {
        connections: 4,
        message_bytes: 4096,
        in_flight: 4,
        programs: 4,
    }
}

fn send_raw(fd: &OwnedFd, bytes: &[u8], fds: &[BorrowedFd<'_>]) {
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(8))];
    let mut control = rustix::net::SendAncillaryBuffer::new(&mut space);
    if !fds.is_empty() {
        assert!(control.push(rustix::net::SendAncillaryMessage::ScmRights(fds)));
    }
    rustix::net::sendmsg(
        fd,
        &[std::io::IoSlice::new(bytes)],
        &mut control,
        SendFlags::NOSIGNAL,
    )
    .unwrap();
}

/// [`ANSWERED`] as poll(2) takes it: its whole seconds and the nanoseconds past them. Its seconds alone would make a
/// bound under a second zero, a poll that looks once and returns.
fn answer_wait() -> Timespec {
    Timespec {
        tv_sec: i64::try_from(ANSWERED.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(ANSWERED.subsec_nanos()),
    }
}

/// The next message the server sends, waiting at most [`ANSWERED`] for it.
fn answer_of(fd: &OwnedFd) -> (Header, Answer) {
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    assert_eq!(
        rustix::event::poll(&mut fds, Some(&answer_wait())).unwrap(),
        1,
        "no answer came"
    );
    let mut buf = vec![0u8; wire::MAX_MESSAGE];
    let n = rustix::net::recv(fd, &mut buf, rustix::net::RecvFlags::empty())
        .unwrap()
        .0;
    wire::read_answer(&received(&buf[..n]), wire::MAX_MESSAGE).unwrap()
}

/// Whether the server sends anything more within a short look: for "nothing more came".
fn quiet(fd: &OwnedFd) -> bool {
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    let ts = Timespec {
        tv_sec: 0,
        tv_nsec: 200_000_000,
    };
    rustix::event::poll(&mut fds, Some(&ts)).unwrap() == 0
}

fn received(bytes: &[u8]) -> Received<'_> {
    Received {
        bytes,
        size: bytes.len(),
        descriptors: 0,
        descriptors_cut: false,
    }
}

fn request(id: u64, call: Call) -> Vec<u8> {
    wire::request(id, &Request::Call(call), wire::MAX_MESSAGE).unwrap()
}

/// A hello of this wire and this build, as request `id`.
fn hello_of_this_wire(id: u64) -> Vec<u8> {
    let hello = Request::Hello {
        build: wire::build(),
    };
    wire::request(id, &hello, wire::MAX_MESSAGE).unwrap()
}

/// `bytes`, a message of this wire, as the wire whose identity is `theirs` would send it: the identity is the
/// header's bytes 4 to 12 in every build.
fn of_wire(mut bytes: Vec<u8>, theirs: u64) -> Vec<u8> {
    bytes[4..12].copy_from_slice(&theirs.to_le_bytes());
    bytes
}

fn b() -> BoxId {
    BoxId::new("b").unwrap()
}

/// Waits for `fact`, the server's, on `wait`, its own record's patience or measuring (hex_testkit::waits).
fn wait_until(wait: &Patience, what: &str, fact: impl FnMut() -> bool) {
    until(wait, what, fact).unwrap();
}

fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
        s.rsplit_once(')')
            .is_some_and(|(_, rest)| !rest.trim_start().starts_with('Z'))
    })
}

struct Hostile<'a>(&'a str, Vec<u8>, Vec<BorrowedFd<'a>>, u64, &'a str);

/// HEX-SOCK-3: hostile messages to a live server (truncated, empty, not Hex's, an unknown kind, an answer sent
/// as a request, trailing bytes, oversized, descriptors where none belong and more than a message takes) are each
/// refused by type, to the request their header names, and the connection is served after each; another
/// connection is served too. A message of another wire is refused by its own test
/// (`a_client_of_another_wire_is_refused_at_its_hello`).
#[test]
fn hostile_messages_are_refused_and_the_connection_is_served_after() {
    let lab = Lab::new(limits());
    let fd = lab.raw();
    let hello = hello_of_this_wire(9);
    let with = |at: usize, two: [u8; 2]| {
        let mut m = hello.clone();
        m[at..at + 2].copy_from_slice(&two);
        m
    };
    let mut trailing = request(5, Call::Inspect(b()));
    trailing.push(0);
    let mut oversized = request(6, Call::Inspect(b()));
    oversized.resize(4097, 0);
    let (pipe_r, _pipe_w) = std::io::pipe().unwrap();
    let exec_stdout = request(
        8,
        Call::Exec {
            id: b(),
            program: Program::of(&["true"]),
            streams: wire::Streams {
                stdout: true,
                ..wire::Streams::default()
            },
        },
    );
    let p = pipe_r.as_fd();
    // Each: what it is, its bytes, the descriptors sent with it, the id its refusal goes to, and
    // what the refusal says.
    let cases: Vec<Hostile<'_>> = vec![
        Hostile("empty", Vec::new(), vec![], 0, "short of"),
        Hostile(
            "truncated header",
            b"HEXW\x01\x00".to_vec(),
            vec![],
            0,
            "short of",
        ),
        Hostile(
            "not Hex's",
            b"GET / HTTP/1.1\r\n\r\n".to_vec(),
            vec![],
            0,
            "HEXW",
        ),
        Hostile(
            "unknown kind",
            with(12, [0x0f, 0]),
            vec![],
            9,
            "kind 0x000f isn't a request this wire defines",
        ),
        Hostile(
            "an answer kind",
            with(12, [0x03, 0x80]),
            vec![],
            9,
            "kind 0x8003",
        ),
        Hostile(
            "trailing bytes",
            trailing,
            vec![],
            5,
            "follow what this wire defines",
        ),
        Hostile(
            "oversized",
            oversized,
            vec![],
            6,
            "4097 bytes is over the 4096-byte bound",
        ),
        Hostile(
            "a descriptor on an inspect",
            request(7, Call::Inspect(b())),
            vec![p],
            7,
            "an inspect takes 0",
        ),
        Hostile(
            "no descriptor on an exec naming stdout",
            exec_stdout.clone(),
            vec![],
            8,
            "an exec takes 1",
        ),
        Hostile(
            "more descriptors than a message takes",
            exec_stdout,
            vec![p, p, p, p],
            8,
            "more descriptors than the 3",
        ),
    ];
    for Hostile(name, bytes, fds, id, says) in cases {
        send_raw(&fd, &bytes, &fds);
        match answer_of(&fd) {
            (
                h,
                Answer::Refused(Refusal::Invalid {
                    what: "message",
                    why,
                }),
            ) => {
                assert_eq!(h.id, id, "{name}: answered to {}", h.id);
                assert!(why.contains(says), "{name}: {why}");
            }
            other => panic!("{name}: {other:?}"),
        }
        send_raw(&fd, &hello, &[]);
        assert!(
            matches!(answer_of(&fd), (Header { id: 9, .. }, Answer::Hello(_))),
            "{name}: the connection wasn't served after"
        );
    }
    assert!(quiet(&fd), "an answer came that no request asked for");
    let other = lab.raw();
    send_raw(&other, &hello, &[]);
    assert!(matches!(answer_of(&other), (_, Answer::Hello(_))));
}

/// HEX-SOCK-10: the node's network namespace counts cross the socket as its host holds them: asked
/// through `hex-client`, and on a raw connection, answered with its request's id.
#[test]
fn the_network_namespace_counts_cross_the_socket() {
    let lab = Lab::new(limits());
    assert_eq!(lab.client().netns().unwrap(), counts());
    let fd = lab.raw();
    send_raw(&fd, &request(4, Call::Netns), &[]);
    let (h, answer) = answer_of(&fd);
    assert_eq!((h.id, answer), (4, Answer::Netns(counts())));
}

/// One wire (the lead's word of 2026-11-09, which drops the append-only rule): a client of another wire is refused at
/// its hello, and nothing it sends is read as this wire's. Do: on a raw connection, a hello as a client of another wire
/// sends it (its identity and its build, in the hello's head every build lays out alike); on another, greeted in this
/// wire, a delete of box b as the other wire would send it; then inspect b through `hex-client`, of this wire. Expect:
/// each answered with the node's own hello, of this wire and this build, to its id, which a client of another wire
/// refuses naming both (hex-client's own tests), then its connection's end, nothing more sent; b kept; the inspect
/// answered.
#[test]
fn a_client_of_another_wire_is_refused_at_its_hello() {
    let lab = Lab::new(limits());
    let theirs = !wire::IDENTITY;
    let hello = Request::Hello {
        build: "hex 9.9.9 (another wire)".to_owned(),
    };
    let their_hello = of_wire(wire::request(9, &hello, wire::MAX_MESSAGE).unwrap(), theirs);
    let their_delete = of_wire(request(2, Call::Delete(b())), theirs);
    let refused = lab.raw();
    send_raw(&refused, &their_hello, &[]);
    let greeted = lab.raw();
    send_raw(&greeted, &hello_of_this_wire(1), &[]);
    assert!(matches!(answer_of(&greeted).1, Answer::Hello(_)));
    send_raw(&greeted, &their_delete, &[]);
    for (fd, id) in [(&refused, 9), (&greeted, 2)] {
        match answer_of(fd) {
            (h, Answer::Hello(told)) if h.id == id => assert_eq!(told.build, wire::build()),
            other => panic!("the node's own hello to {id}, not {other:?}"),
        }
        let mut after = [0u8; 64];
        let mut readable = [PollFd::new(fd, PollFlags::IN)];
        assert_eq!(
            rustix::event::poll(&mut readable, Some(&answer_wait())).unwrap(),
            1,
            "the node kept the connection open"
        );
        assert_eq!(
            rustix::net::recv(fd, &mut after, rustix::net::RecvFlags::empty())
                .unwrap()
                .0,
            0,
            "the node sent more than its hello"
        );
    }
    wait_until(&LET_GO, "the refused connections to close", || {
        lab.connections() == 0
    });
    assert_eq!(lab.host.sim.inspect(&b()).unwrap().id, b());
    assert_eq!(lab.client().inspect(&b()).unwrap().id, b());
}

/// HEX-SOCK-4: connections past the bound are refused by type, naming it, without disturbing the
/// connections the server holds; once one closes, the next is served.
#[test]
fn connections_past_the_bound_are_refused_naming_it() {
    let lab = Lab::new(Limits {
        connections: 2,
        ..limits()
    });
    let (a, c) = (lab.client(), lab.client());
    assert_eq!(
        Client::connect(lab.server.path()).err(),
        Some(Refusal::Exhausted {
            what: CONNECTIONS,
            bound: 2
        })
    );
    assert_eq!(a.inspect(&b()).unwrap().id, b());
    assert_eq!(c.list().unwrap().len(), 1);
    drop(a);
    wait_until(&LET_GO, "the closed connection to be let go", || {
        lab.connections() == 1
    });
    let d = lab.client();
    assert_eq!(d.inspect(&b()).unwrap().id, b());
    assert_eq!(c.inspect(&b()).unwrap().id, b());
}

/// HEX-SOCK-5: requests past a connection's in-flight bound are refused by type, naming it, while the
/// ones within it are carried out: every request gets exactly one answer, and none is dropped.
#[test]
fn requests_past_the_in_flight_bound_are_refused_and_none_is_lost() {
    let lab = Lab::new(Limits {
        in_flight: 2,
        ..limits()
    });
    let fd = lab.raw();
    lab.host.gate.set(true);
    for id in 1..=3 {
        send_raw(&fd, &request(id, Call::Inspect(b())), &[]);
    }
    assert_eq!(
        answer_of(&fd),
        (
            Header {
                kind: 0x8007,
                id: 3
            },
            Answer::Refused(Refusal::Exhausted {
                what: IN_FLIGHT,
                bound: 2
            })
        )
    );
    assert!(
        quiet(&fd),
        "a held request was answered while the gate was shut"
    );
    lab.host.gate.set(false);
    let mut ids: Vec<u64> = (0..2)
        .map(|_| match answer_of(&fd) {
            (h, Answer::Info(info)) if info.id == b() => h.id,
            other => panic!("{other:?}"),
        })
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, [1, 2]);
    send_raw(&fd, &request(4, Call::Inspect(b())), &[]);
    assert!(matches!(
        answer_of(&fd),
        (Header { id: 4, .. }, Answer::Info(_))
    ));
    assert!(quiet(&fd), "more answers than requests");
}

/// HEX-SOCK-7: execs past the programs a connection watches are refused by type, naming the bound;
/// a program's end gives its place back before the end is reported.
#[test]
fn execs_past_the_programs_bound_are_refused_naming_it() {
    let lab = Lab::new(Limits {
        programs: 1,
        ..limits()
    });
    let client = lab.client();
    let mut first = client
        .exec(&b(), Program::of(&["sleep", "1000"]), Stdio::null())
        .unwrap();
    assert_eq!(
        client
            .exec(&b(), Program::of(&["true"]), Stdio::null())
            .err(),
        Some(Refusal::Exhausted {
            what: PROGRAMS,
            bound: 1
        })
    );
    rustix::process::kill_process(
        Pid::from_raw(i32::try_from(first.pid()).unwrap()).unwrap(),
        Signal::KILL,
    )
    .unwrap();
    assert_eq!(first.wait_for(PATIENCE).unwrap(), Some(Exit::Signal(9)));
    let mut next = client
        .exec(&b(), Program::of(&["true"]), Stdio::null())
        .unwrap();
    assert_eq!(next.wait_for(PATIENCE).unwrap(), Some(Exit::Code(0)));
}

/// HEX-SOCK-8: an exec's stdio crosses as descriptors: what the program writes reaches the pipes the
/// client holds, what the client writes reaches it, and its status comes back; a descriptor that
/// isn't the right end of a pipe is refused by name.
#[test]
fn stdio_crosses_as_descriptors_and_only_pipes_are_taken() {
    let lab = Lab::new(limits());
    let client = lab.client();
    let (mut out_r, out_w) = std::io::pipe().unwrap();
    let (mut err_r, err_w) = std::io::pipe().unwrap();
    let mut p = client
        .exec(
            &b(),
            Program::of(&["sh", "-c", "echo hi; echo oops >&2; exit 3"]),
            Stdio {
                stdin: None,
                stdout: Some(out_w),
                stderr: Some(err_w),
                ..Stdio::null()
            },
        )
        .unwrap();
    let (mut out, mut err) = (String::new(), String::new());
    out_r.read_to_string(&mut out).unwrap();
    err_r.read_to_string(&mut err).unwrap();
    assert_eq!((out.as_str(), err.as_str()), ("hi\n", "oops\n"));
    assert_eq!(p.wait_for(PATIENCE).unwrap(), Some(Exit::Code(3)));
    let (in_r, mut in_w) = std::io::pipe().unwrap();
    let (mut out_r, out_w) = std::io::pipe().unwrap();
    let mut cat = client
        .exec(
            &b(),
            Program::of(&["cat"]),
            Stdio {
                stdin: Some(in_r),
                stdout: Some(out_w),
                stderr: None,
                ..Stdio::null()
            },
        )
        .unwrap();
    in_w.write_all(b"round trip\n").unwrap();
    drop(in_w);
    let mut back = String::new();
    out_r.read_to_string(&mut back).unwrap();
    assert_eq!(back, "round trip\n");
    assert_eq!(cat.wait_for(PATIENCE).unwrap(), Some(Exit::Code(0)));

    let fd = lab.raw();
    let devnull = std::fs::File::open("/dev/null").unwrap();
    let (read_end, _write_end) = std::io::pipe().unwrap();
    let exec = request(
        1,
        Call::Exec {
            id: b(),
            program: Program::of(&["true"]),
            streams: wire::Streams {
                stdout: true,
                ..wire::Streams::default()
            },
        },
    );
    for (fd_given, says) in [
        (devnull.as_fd(), "stdout isn't a pipe"),
        (read_end.as_fd(), "stdout is a pipe's read end"),
    ] {
        send_raw(&fd, &exec, &[fd_given]);
        match answer_of(&fd) {
            (Header { id: 1, .. }, Answer::Refused(Refusal::Invalid { what: "stdio", why })) => {
                assert!(why.contains(says), "{why}");
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(lab.watched(), 0, "a refused exec started a program");
}

/// HEX-SOCK-6, the disconnect rule: a client that goes away while its program runs (its connection
/// closes, no release sent, as when its process dies) has the program sent KILL, reaped, and
/// counted in its box; none is left running unwatched.
#[test]
fn a_program_whose_client_goes_away_is_killed_reaped_and_counted() {
    let lab = Lab::new(limits());
    let fd = lab.raw();
    send_raw(
        &fd,
        &request(
            1,
            Call::Exec {
                id: b(),
                program: Program::of(&["sleep", "1000"]),
                streams: wire::Streams::default(),
            },
        ),
        &[],
    );
    let pid = match answer_of(&fd) {
        (Header { id: 1, .. }, Answer::Started { pid }) => pid,
        other => panic!("{other:?}"),
    };
    assert!(alive(pid));
    drop(fd);
    wait_until(
        &ABANDONED,
        "the abandoned program to be counted in its box",
        || lab.host.abandoned.lock().unwrap().as_slice() == [b()],
    );
    // The count is the last of the server's ending of the program, after its kill and reaping, so counted, it is gone:
    // the fact is made within that ending, whose record bounds the wait.
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "the program wasn't reaped"
    );
    assert_eq!((lab.connections(), lab.watched()), (0, 0));
}

/// HEX-SOCK-6: a program whose `Proc` the client dropped was released: it runs on after the client
/// goes, uncounted, as dropping an in-process `Proc` lets it.
#[test]
fn a_released_program_runs_on_after_its_client_goes() {
    let lab = Lab::new(limits());
    let client = lab.client();
    let proc = client
        .exec(&b(), Program::of(&["sleep", "1000"]), Stdio::null())
        .unwrap();
    let pid = proc.pid();
    drop(proc);
    drop(client);
    wait_until(&LET_GO, "the connection to close", || {
        lab.connections() == 0
    });
    assert_eq!(lab.watched(), 0);
    assert!(alive(pid), "a released program was ended");
    assert!(lab.host.abandoned.lock().unwrap().is_empty());
    let pid = Pid::from_raw(i32::try_from(pid).unwrap()).unwrap();
    rustix::process::kill_process(pid, Signal::KILL).unwrap();
    rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::empty()).unwrap();
}

/// The socket at a path: one an ended agent left is replaced, one a live agent serves is refused, and
/// a file that isn't a socket is refused, never removed.
#[test]
fn a_socket_path_is_taken_only_from_an_ended_agent() {
    let lab = Lab::new(limits());
    let path = lab.server.path().to_owned();
    let host = Arc::clone(&lab.host) as Arc<dyn Host>;
    assert!(matches!(
        Server::bind(&path, Arc::clone(&host), limits(), 1),
        Err(Refusal::Invalid { what: "socket path", why }) if why.contains("served by a node agent already")
    ));
    let file = path.with_file_name("not-a-socket");
    std::fs::write(&file, b"keep").unwrap();
    assert!(matches!(
        Server::bind(&file, Arc::clone(&host), limits(), 1),
        Err(Refusal::Invalid { what: "socket path", why }) if why.contains("isn't a socket")
    ));
    assert_eq!(std::fs::read(&file).unwrap(), b"keep");
    // A socket no agent listens on: bound but never listened, as one an agent killed mid-start leaves.
    let stale = path.with_file_name("stale.sock");
    let left = rustix::net::socket_with(
        AddressFamily::UNIX,
        SocketType::SEQPACKET,
        SocketFlags::CLOEXEC,
        None,
    )
    .unwrap();
    rustix::net::bind(&left, &SocketAddrUnix::new(&stale).unwrap()).unwrap();
    drop(left);
    let again = Server::bind(&stale, host, limits(), 1).unwrap();
    assert!(Client::connect(again.path()).is_ok());
}

/// A list longer than one message comes whole, in order, over the socket.
#[test]
fn a_list_longer_than_one_message_comes_whole() {
    let lab = Lab::new(limits());
    for n in 0..200 {
        lab.host
            .sim
            .create(BoxSpec {
                id: BoxId::new(&format!("many-{n:04}")).unwrap(),
                bounds: Bounds::default(),
                class: ClassSpec::Process,
                owner: None,
            })
            .unwrap();
    }
    let listed = lab.client().list().unwrap();
    assert_eq!(listed.len(), 201);
    assert_eq!(listed, lab.host.sim.list().unwrap());
}

/// HEX-SOCK-6: a client that closes with an answer unread leaves the server a reset, reported once
/// and before what it sent; the server still reads what it sent, so a release sent just before it
/// went lets its program run on rather than be ended as abandoned.
#[test]
fn a_release_sent_before_a_reset_is_honoured() {
    let lab = Lab::new(limits());
    let fd = lab.raw();
    send_raw(
        &fd,
        &request(
            1,
            Call::Exec {
                id: b(),
                program: Program::of(&["sleep", "1000"]),
                streams: wire::Streams::default(),
            },
        ),
        &[],
    );
    let pid = match answer_of(&fd) {
        (Header { id: 1, .. }, Answer::Started { pid }) => pid,
        other => panic!("{other:?}"),
    };
    // An answer left unread at close is what makes the kernel report a reset to the server.
    send_raw(&fd, &request(2, Call::Inspect(b())), &[]);
    let mut readable = [PollFd::new(&fd, PollFlags::IN)];
    assert_eq!(
        rustix::event::poll(&mut readable, Some(&answer_wait())).unwrap(),
        1
    );
    {
        // The I/O thread takes the table on every event, so while it is held here the release and
        // the close both land before the server reads either: the reset comes first.
        let _held = lab.server.shared.table.lock().unwrap();
        send_raw(
            &fd,
            &wire::request(1, &Request::Release, wire::MAX_MESSAGE).unwrap(),
            &[],
        );
        drop(fd);
    }
    wait_until(&LET_GO, "the connection to close", || {
        lab.connections() == 0
    });
    assert_eq!(lab.watched(), 0);
    assert!(alive(pid), "the released program was ended");
    assert!(lab.host.abandoned.lock().unwrap().is_empty());
    let pid = Pid::from_raw(i32::try_from(pid).unwrap()).unwrap();
    rustix::process::kill_process(pid, Signal::KILL).unwrap();
    rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::empty()).unwrap();
}

/// HEX-SOCK-5, the lead's call of 2026-11-06 (AC-T7.1.3): calls in flight are one budget for the node, not a share
/// for each connection; the node's clients are its own control plane, so none is fair to another. Do: two
/// connections under a node-wide bound of 2, the host's gate shut, one call on the first and two on the second.
/// Expect: the third call, whichever connection it is on, refused naming the bound; the two within it answered once
/// the gate opens.
#[test]
fn calls_in_flight_are_bounded_for_the_node_not_each_connection() {
    let lab = Lab::new(Limits {
        in_flight: 2,
        ..limits()
    });
    let (one, two) = (lab.raw(), lab.raw());
    lab.host.gate.set(true);
    send_raw(&one, &request(1, Call::Inspect(b())), &[]);
    send_raw(&two, &request(2, Call::Inspect(b())), &[]);
    send_raw(&two, &request(3, Call::Inspect(b())), &[]);
    assert_eq!(
        answer_of(&two).1,
        Answer::Refused(Refusal::Exhausted {
            what: IN_FLIGHT,
            bound: 2
        })
    );
    lab.host.gate.set(false);
    assert!(matches!(
        answer_of(&one),
        (Header { id: 1, .. }, Answer::Info(_))
    ));
    assert!(matches!(
        answer_of(&two),
        (Header { id: 2, .. }, Answer::Info(_))
    ));
}

/// HEX-SOCK-7 and the lead's ruling of 2026-11-06 (AC-T7.1.3): the programs watched are one budget for the node, and
/// each admitted connection keeps the one its hello promised. Do: two connections under a node-wide budget of 3, an
/// exec on the first until refused, then one on the second. Expect: the first starts two, its third refused naming
/// the node's budget, since the second connection's one is kept for it; the second's starts.
#[test]
fn programs_watched_are_bounded_for_the_node_and_each_connection_keeps_its_one() {
    let lab = Lab::new(Limits {
        programs: 3,
        ..limits()
    });
    let (a, c) = (lab.client(), lab.client());
    let mut held = Vec::new();
    for _ in 0..2 {
        held.push(
            a.exec(&b(), Program::of(&["sleep", "1000"]), Stdio::null())
                .unwrap(),
        );
    }
    assert_eq!(
        a.exec(&b(), Program::of(&["true"]), Stdio::null()).err(),
        Some(Refusal::Exhausted {
            what: PROGRAMS,
            bound: 3
        })
    );
    let mut theirs = c.exec(&b(), Program::of(&["true"]), Stdio::null()).unwrap();
    assert_eq!(theirs.wait_for(PATIENCE).unwrap(), Some(Exit::Code(0)));
    for mut p in held {
        rustix::process::kill_process(
            Pid::from_raw(i32::try_from(p.pid()).unwrap()).unwrap(),
            Signal::KILL,
        )
        .unwrap();
        assert_eq!(p.wait_for(PATIENCE).unwrap(), Some(Exit::Signal(9)));
    }
}

/// HEX-SOCK-5, the lead's ruling of 2026-11-06: a client is told what the node keeps for each connection, beside the
/// node's budgets, and is never refused within what it keeps, however much of the budgets other connections hold. Do:
/// a hello; then, under a node-wide budget of 3 calls held at the gate, one connection taking all it can before
/// another's call. Expect: the hello told one call and one program a connection and the node's budgets beside; the
/// first connection refused past two calls, naming the node's budget; the other's call admitted and answered.
#[test]
fn a_client_within_what_it_was_told_is_never_refused() {
    let lab = Lab::new(Limits {
        in_flight: 3,
        ..limits()
    });
    let told = {
        let fd = lab.raw();
        send_raw(&fd, &hello_of_this_wire(9), &[]);
        match answer_of(&fd).1 {
            Answer::Hello(h) => h,
            other => panic!("{other:?}"),
        }
    };
    assert_eq!((told.limits.in_flight, told.limits.programs), (1, 1));
    assert_eq!(
        told.budgets,
        wire::Budgets {
            in_flight: 3,
            programs: limits().programs
        }
    );
    // Each connection holds its one call from its admission, so the hello's is let go first, and both of these are
    // admitted before either asks.
    wait_until(&LET_GO, "the hello's connection to close", || {
        lab.connections() == 0
    });
    let (greedy, other) = (lab.raw(), lab.raw());
    wait_until(&ADMITTED, "both connections to be admitted", || {
        lab.connections() == 2
    });
    lab.host.gate.set(true);
    for id in 1..=3 {
        send_raw(&greedy, &request(id, Call::Inspect(b())), &[]);
    }
    assert_eq!(
        answer_of(&greedy).1,
        Answer::Refused(Refusal::Exhausted {
            what: IN_FLIGHT,
            bound: 3
        })
    );
    send_raw(&other, &request(7, Call::Inspect(b())), &[]);
    lab.host.gate.set(false);
    assert!(matches!(
        answer_of(&other),
        (Header { id: 7, .. }, Answer::Info(_))
    ));
    for _ in 0..2 {
        assert!(matches!(answer_of(&greedy).1, Answer::Info(_)));
    }
}

/// The init budget and the socket's programs-watched bound are a pidfd per box the node may hold, within its hard
/// `RLIMIT_NOFILE` read at run time (getrlimit(2)), never a figure copied from systemd's default (Ada's 2026-11-06
/// constants rule; CLAUDE.md §2). Do: read the node's sizes and its socket's limits on this host. Expect: the
/// programs bound is the least of the boxes' bound and the hard limit (`/proc/sys/fs/nr_open` where unlimited).
#[test]
fn the_programs_bound_is_the_boxes_within_the_hard_descriptor_limit() {
    let hard = rustix::process::getrlimit(rustix::process::Resource::Nofile)
        .maximum
        .or_else(|| {
            std::fs::read_to_string("/proc/sys/fs/nr_open")
                .ok()
                .and_then(|t| t.trim().parse().ok())
        })
        .unwrap_or(u64::MAX);
    let sizes = crate::sizes::Sizes::now().unwrap();
    assert_eq!(
        u64::try_from(super::limits(&sizes).programs).unwrap(),
        hard.min(u64::try_from(hex_runtime::MAX_BOXES).unwrap())
    );
}

/// The waits census's suites (run 5, waits-census-20261109T184054Z): `node_socket`'s contract test failed in 15 of 53
/// loops on calls answered past their patience, a start's among them while a hook held another. Two calls the I/O
/// thread reads in one wake were one worker's: it started a worker only when none was idle, and the one it notified
/// for the first is idle still until it wakes, so the second waited for the first to be done. Do: under calls in flight
/// bounded at 2 on 2 workers, an exec answered, its worker then idle; then, the host's gate shut, an inspect and a
/// netns call sent one after the other on the same connection. Expect: the netns call answered while the inspect is
/// held at the gate; the inspect once it opens.
#[test]
fn two_calls_read_in_one_wake_each_get_a_worker() {
    let lab = Lab::with(
        Limits {
            in_flight: 2,
            ..limits()
        },
        2,
    );
    let fd = lab.raw();
    let (stdin_r, stdin_w) = std::io::pipe().unwrap();
    send_raw(
        &fd,
        &request(
            1,
            Call::Exec {
                id: b(),
                program: Program::of(&["cat"]),
                streams: wire::Streams {
                    stdin: true,
                    ..wire::Streams::default()
                },
            },
        ),
        &[stdin_r.as_fd()],
    );
    drop(stdin_r);
    assert!(matches!(
        answer_of(&fd),
        (Header { id: 1, .. }, Answer::Started { .. })
    ));
    lab.host.gate.set(true);
    send_raw(&fd, &request(2, Call::Inspect(b())), &[]);
    send_raw(&fd, &request(3, Call::Netns), &[]);
    match answer_of(&fd) {
        (Header { id: 3, .. }, Answer::Netns(c)) => assert_eq!(c, counts()),
        other => panic!("{other:?}"),
    }
    lab.host.gate.set(false);
    assert!(matches!(
        answer_of(&fd),
        (Header { id: 2, .. }, Answer::Info(_))
    ));
    // The `cat` ends with its stdin, and its end comes.
    drop(stdin_w);
    assert!(matches!(
        answer_of(&fd),
        (Header { id: 1, .. }, Answer::Exited(Exit::Code(0)))
    ));
}
