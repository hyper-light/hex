//! The socket's own tests (decision 3, HEX-SOCK-3 to 8 and 10): a live server in this process, over a host
//! whose boxes are the simulator's and whose programs are real processes of this test, so the
//! server's pidfd watch, its bounds and the disconnect rule meet real programs without a cgroup.
//! Hostile bytes go in through a raw socket; the rest through `hex-client`, as a caller sends them.

use super::*;
use std::io::{Read, Write as _};
use std::os::fd::{AsFd, BorrowedFd};

use hex_api::sim::SimRuntime;
use hex_api::{Bounds, BoxInfo, BoxSpec, ClassSpec, Signal as HexSignal, Usage};
use hex_client::Client;

/// Longest a test waits for a fact it needs: a bound, so a server that never makes it true fails the
/// test instead of hanging it.
const PATIENCE: Duration = Duration::from_secs(30);

/// Shuts a host's inspect until opened: holds calls in flight on purpose.
#[derive(Default)]
struct Gate {
    shut: Mutex<bool>,
    opened: Condvar,
}

impl Gate {
    fn set(&self, shut: bool) {
        *self.shut.lock().unwrap() = shut;
        self.opened.notify_all();
    }

    fn pass(&self) {
        let deadline = Instant::now() + PATIENCE;
        let mut shut = self.shut.lock().unwrap();
        while *shut && Instant::now() < deadline {
            shut = self.opened.wait_timeout(shut, PATIENCE).unwrap().0;
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

    fn netns(&self) -> netns::Counts {
        COUNTS
    }
}

/// The test host's network namespace counts, each field a different value.
const COUNTS: netns::Counts = netns::Counts {
    made_ahead: 5,
    taken: 4,
    made_on_path: 3,
    discarded: 2,
    failed: 1,
};

struct Lab {
    server: Server,
    host: Arc<TestHost>,
    _dir: tempfile::TempDir,
}

impl Lab {
    fn new(limits: Limits) -> Self {
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
            })
            .unwrap();
        let server = Server::bind(
            &dir.path().join("node.sock"),
            Arc::clone(&host) as Arc<dyn Host>,
            limits,
            8,
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

/// The next message the server sends, waiting at most [`PATIENCE`] for it.
fn answer_of(fd: &OwnedFd) -> (Header, Answer) {
    let mut fds = [PollFd::new(fd, PollFlags::IN)];
    let ts = Timespec {
        tv_sec: i64::try_from(PATIENCE.as_secs()).unwrap(),
        tv_nsec: 0,
    };
    assert_eq!(
        rustix::event::poll(&mut fds, Some(&ts)).unwrap(),
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
    wire::request(wire::VERSION, id, &Request::Call(call), wire::MAX_MESSAGE).unwrap()
}

fn b() -> BoxId {
    BoxId::new("b").unwrap()
}

fn wait_until(what: &str, mut fact: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !fact() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::park_timeout(Duration::from_millis(5));
    }
}

fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
        s.rsplit_once(')')
            .is_some_and(|(_, rest)| !rest.trim_start().starts_with('Z'))
    })
}

struct Hostile<'a>(&'a str, Vec<u8>, Vec<BorrowedFd<'a>>, u64, &'a str);

/// HEX-SOCK-3: hostile messages to a live server (truncated, empty, not Hex's, an unknown version or
/// kind, an answer sent as a request, trailing bytes, oversized, descriptors where none belong and
/// more than a message takes) are each refused by type, to the request their header names, and the
/// connection is served after each; another connection is served too.
#[test]
fn hostile_messages_are_refused_and_the_connection_is_served_after() {
    let lab = Lab::new(limits());
    let fd = lab.raw();
    let hello = wire::request(wire::FIRST, 9, &Request::Hello, wire::MAX_MESSAGE).unwrap();
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
        Hostile("version 3", with(4, [3, 0]), vec![], 9, "version 3"),
        Hostile("unknown kind", with(6, [0x0c, 0]), vec![], 9, "kind 0x000c"),
        Hostile(
            "a version 2 kind in version 1",
            with(6, [0x0b, 0]),
            vec![],
            9,
            "kind 0x000b isn't a request version 1 defines",
        ),
        Hostile(
            "an answer kind",
            with(6, [0x03, 0x80]),
            vec![],
            9,
            "kind 0x8003",
        ),
        Hostile(
            "trailing bytes",
            trailing,
            vec![],
            5,
            "follow what version 2 defines",
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
/// through `hex-client`, and on a raw connection in version 2, answered in its request's version with
/// its id.
#[test]
fn the_network_namespace_counts_cross_the_socket() {
    let lab = Lab::new(limits());
    assert_eq!(lab.client().netns().unwrap(), COUNTS);
    let fd = lab.raw();
    send_raw(&fd, &request(4, Call::Netns), &[]);
    let (h, answer) = answer_of(&fd);
    assert_eq!(
        (h.version, h.id, answer),
        (wire::VERSION, 4, Answer::Netns(COUNTS))
    );
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
    wait_until("the closed connection to be let go", || {
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
                version: wire::VERSION,
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
    wait_until("the abandoned program to be counted in its box", || {
        lab.host.abandoned.lock().unwrap().as_slice() == [b()]
    });
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
    wait_until("the connection to close", || lab.connections() == 0);
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
    let ts = Timespec {
        tv_sec: i64::try_from(PATIENCE.as_secs()).unwrap(),
        tv_nsec: 0,
    };
    assert_eq!(rustix::event::poll(&mut readable, Some(&ts)).unwrap(), 1);
    {
        // The I/O thread takes the table on every event, so while it is held here the release and
        // the close both land before the server reads either: the reset comes first.
        let _held = lab.server.shared.table.lock().unwrap();
        send_raw(
            &fd,
            &wire::request(wire::VERSION, 1, &Request::Release, wire::MAX_MESSAGE).unwrap(),
            &[],
        );
        drop(fd);
    }
    wait_until("the connection to close", || lab.connections() == 0);
    assert_eq!(lab.watched(), 0);
    assert!(alive(pid), "the released program was ended");
    assert!(lab.host.abandoned.lock().unwrap().is_empty());
    let pid = Pid::from_raw(i32::try_from(pid).unwrap()).unwrap();
    rustix::process::kill_process(pid, Signal::KILL).unwrap();
    rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::empty()).unwrap();
}
