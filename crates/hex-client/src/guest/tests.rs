//! The guest client's tests (docs/design/shards-guest.md §3, HEX-GUEST-4 and HEX-GUEST-5; native plan
//! step 5, serving HEX-RT-1..13 through the seam): against a stand-in agent on loopback, the client says
//! hello with its build, then calls, framed, in its one wire; an exec's stdin and stdout
//! ride connections of their own, named for it, and its end comes back; and an agent that sends bytes
//! that aren't a message, or closes mid-message, refuses every waiting call by type.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use super::*;
use hex_api::{BoxId, Class, Exit, Program, Runtime};
use hex_core::wire::{Answer, Call, Header, Limits, Request};

/// The guest's ports, as two loopback listeners.
struct Loopback {
    control: SocketAddr,
    stdio: SocketAddr,
}

impl Link for Loopback {
    fn dial(&self, port: u32) -> Result<Box<dyn Duplex>, Refusal> {
        let addr = match port {
            stream::CONTROL_PORT => self.control,
            stream::STDIO_PORT => self.stdio,
            other => panic!("dialed port {other:#x}"),
        };
        Ok(Box::new(TcpStream::connect(addr).unwrap()))
    }
}

fn listeners() -> (TcpListener, TcpListener, Arc<dyn Link>) {
    let control = TcpListener::bind("127.0.0.1:0").unwrap();
    let stdio = TcpListener::bind("127.0.0.1:0").unwrap();
    let link = Arc::new(Loopback {
        control: control.local_addr().unwrap(),
        stdio: stdio.local_addr().unwrap(),
    });
    (control, stdio, link)
}

/// The next framed request on `conn`, waiting on the fact: its bytes, or its end, which fails the test.
fn next_request(conn: &mut TcpStream) -> (Header, Request) {
    let mut d = Deframer::new(wire::MAX_MESSAGE);
    let mut byte = [0u8; 1];
    loop {
        conn.read_exact(&mut byte).unwrap();
        if let (_, Some(m)) = d.take(&byte).unwrap() {
            return wire::read_request_apart(&m, wire::MAX_MESSAGE).unwrap();
        }
    }
}

fn send_answer(conn: &mut TcpStream, id: u64, answer: &Answer) {
    let bytes = wire::answer(id, answer, wire::MAX_MESSAGE).unwrap();
    conn.write_all(&stream::frame(&bytes, wire::MAX_MESSAGE).unwrap())
        .unwrap();
}

/// The relay's token for every stand-in connection.
const TOKEN: u64 = 0x7465_7374;

/// The stand-in agent's hello, of this wire and this build.
fn agents_hello() -> wire::Hello {
    wire::Hello {
        build: wire::build(),
        classes: vec![Class::Oci],
        // The stand-in's own limits, inputs rather than tuning: past what any test here makes.
        limits: Limits {
            connections: 1,
            message_bytes: wire::MAX_MESSAGE,
            in_flight: 4,
            programs: 4,
        },
        budgets: wire::Budgets {
            in_flight: 4,
            programs: 4,
        },
    }
}

fn hello(conn: &mut TcpStream) -> Header {
    conn.write_all(&TOKEN.to_le_bytes()).unwrap();
    let (h, request) = next_request(conn);
    assert_eq!(
        request,
        Request::Hello {
            build: wire::build()
        }
    );
    send_answer(conn, h.id, &Answer::Hello(agents_hello()));
    h
}

/// HEX-GUEST-4: connect, then inspect a box the agent doesn't hold: expect the hello and the inspect, each
/// framed, the classes the agent named, and its refusal returned typed.
#[test]
fn a_call_goes_framed_and_its_refusal_comes_back_typed() {
    let (control, _stdio, link) = listeners();
    let agent = std::thread::spawn(move || {
        let (mut conn, _) = control.accept().unwrap();
        let first = hello(&mut conn);
        let (h, request) = next_request(&mut conn);
        let Request::Call(Call::Inspect(id)) = request else {
            panic!("expected an inspect, got {request:?}");
        };
        send_answer(&mut conn, h.id, &Answer::Refused(Refusal::NoSuchBox(id)));
        [first.kind, h.kind]
    });
    let client = Client::connect(link).unwrap();
    assert_eq!(client.classes(), vec![Class::Oci]);
    let b = BoxId::new("b").unwrap();
    assert_eq!(client.inspect(&b), Err(Refusal::NoSuchBox(b)));
    assert_eq!(agent.join().unwrap(), [0x0001, 0x0006]);
}

/// One wire (the lead's word of 2026-11-09, which drops the append-only rule), over the guest's stream: a client
/// refuses an agent of another wire at the hello, typed, naming both wires' identities and both builds, and sends it
/// nothing more; an agent of its own wire it serves. Do: connect to a stand-in agent that answers the hello, framed,
/// as an agent of another wire would (its own identity and build, in the hello's head every build lays out alike);
/// then connect again, answered in this wire, and inspect a box. Expect: the first connect refused as
/// `Invalid { what: "wire" }` naming both, its connection ended with nothing more sent; the second greeted and its
/// inspect answered.
#[test]
fn an_agent_of_another_wire_is_refused_at_the_hello() {
    let (control, _stdio, link) = listeners();
    let theirs = !wire::IDENTITY;
    let their_build = "hex 9.9.9 (another wire)";
    let agent = std::thread::spawn(move || {
        let (mut conn, _) = control.accept().unwrap();
        conn.write_all(&TOKEN.to_le_bytes()).unwrap();
        let (h, request) = next_request(&mut conn);
        assert!(matches!(request, Request::Hello { .. }), "{request:?}");
        let mut bytes = wire::answer(
            h.id,
            &Answer::Hello(wire::Hello {
                build: their_build.to_owned(),
                ..agents_hello()
            }),
            wire::MAX_MESSAGE,
        )
        .unwrap();
        bytes[4..12].copy_from_slice(&theirs.to_le_bytes());
        conn.write_all(&stream::frame(&bytes, wire::MAX_MESSAGE).unwrap())
            .unwrap();
        // Nothing more comes: the client's end of the connection, read as its close.
        let mut rest = Vec::new();
        conn.read_to_end(&mut rest).unwrap();
        let (mut conn, _) = control.accept().unwrap();
        hello(&mut conn);
        let (h, request) = next_request(&mut conn);
        let Request::Call(Call::Inspect(id)) = request else {
            panic!("an inspect, not {request:?}")
        };
        send_answer(&mut conn, h.id, &Answer::Refused(Refusal::NoSuchBox(id)));
        (rest, conn)
    });
    let refused = Client::connect(Arc::clone(&link)).err();
    let Some(Refusal::Invalid { what: "wire", why }) = &refused else {
        panic!("refused for its wire, not {refused:?}")
    };
    for named in [
        format!("{theirs:016x}"),
        format!("{:016x}", wire::IDENTITY),
        their_build.to_owned(),
        wire::build(),
    ] {
        assert!(why.contains(&named), "{why:?} names {named:?}");
    }
    let client = Client::connect(link).unwrap();
    let b = BoxId::new("b").unwrap();
    assert_eq!(client.inspect(&b), Err(Refusal::NoSuchBox(b.clone())));
    let (rest, conn) = agent.join().unwrap();
    assert!(
        rest.is_empty(),
        "the refused client sent {} bytes after its hello",
        rest.len()
    );
    drop(conn);
}

/// HEX-GUEST-4: exec a program with stdin and stdout whose agent echoes one into the other: expect two
/// stdio connections named for the exec's id, the bytes written to stdin read back from stdout to its
/// end, and the program's end as the agent sent it.
#[test]
fn an_execs_stdio_rides_named_connections_and_its_end_comes_back() {
    let (control, stdio, link) = listeners();
    let agent = std::thread::spawn(move || {
        let (mut conn, _) = control.accept().unwrap();
        hello(&mut conn);
        let (h, request) = next_request(&mut conn);
        let Request::Call(Call::Exec { streams, .. }) = request else {
            panic!("expected an exec, got {request:?}");
        };
        assert!(streams.stdin && streams.stdout && !streams.stderr);
        let mut named = std::collections::BTreeMap::new();
        for _ in 0..2 {
            let (mut s, _) = stdio.accept().unwrap();
            let mut name = [0u8; stream::NAME_BYTES];
            s.read_exact(&mut name).unwrap();
            let n = stream::read_name(&name).unwrap();
            assert_eq!(
                n.exec,
                Exec {
                    conn: TOKEN,
                    id: h.id
                },
                "a stream names its connection's token and its exec's request id"
            );
            named.insert(n.stream, s);
        }
        send_answer(&mut conn, h.id, &Answer::Started { pid: 42 });
        let mut stdin = named.remove(&stream::Stdio::In).unwrap();
        let mut stdout = named.remove(&stream::Stdio::Out).unwrap();
        std::io::copy(&mut stdin, &mut stdout).unwrap();
        drop(stdout);
        send_answer(&mut conn, h.id, &Answer::Exited(Exit::Code(0)));
        // Held open until the client has its end, as an agent's connection is.
        let _ = next_request(&mut conn);
    });
    let client = Client::connect(link).unwrap();
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
                ..Stdio::null()
            },
        )
        .unwrap();
    assert_eq!(proc.pid(), 42);
    in_w.write_all(b"hello through the guest\n").unwrap();
    drop(in_w);
    let mut out = String::new();
    out_r.read_to_string(&mut out).unwrap();
    assert_eq!(out, "hello through the guest\n");
    assert_eq!(proc.wait_for(Duration::MAX).unwrap(), Some(Exit::Code(0)));
    drop(proc);
    let _ = client.inspect(&BoxId::new("b").unwrap());
    agent.join().unwrap();
}

/// HEX-GUEST-5: have the agent announce a frame past the wire's bound while a call waits: expect the
/// call refused by type, naming bytes that aren't a message, before any body is read.
#[test]
fn an_agent_announcing_an_oversized_frame_is_refused() {
    let (control, _stdio, link) = listeners();
    let agent = std::thread::spawn(move || {
        let (mut conn, _) = control.accept().unwrap();
        hello(&mut conn);
        let _ = next_request(&mut conn);
        let past = u32::try_from(wire::MAX_MESSAGE + 1).unwrap();
        conn.write_all(&past.to_be_bytes()).unwrap();
        conn
    });
    let client = Client::connect(link).unwrap();
    let got = client.inspect(&BoxId::new("b").unwrap());
    let Err(Refusal::Broken(why)) = got else {
        panic!("expected a broken connection, got {got:?}");
    };
    assert!(why.contains("aren't a message"), "{why}");
    drop(agent.join().unwrap());
}

/// HEX-GUEST-5: have the agent close the connection partway through an answer: expect the waiting call
/// refused by type, naming the cut message.
#[test]
fn an_agent_closing_mid_message_is_refused() {
    let (control, _stdio, link) = listeners();
    let agent = std::thread::spawn(move || {
        let (mut conn, _) = control.accept().unwrap();
        hello(&mut conn);
        let (h, _) = next_request(&mut conn);
        let bytes = wire::answer(h.id, &Answer::Done, wire::MAX_MESSAGE).unwrap();
        let framed = stream::frame(&bytes, wire::MAX_MESSAGE).unwrap();
        conn.write_all(&framed[..framed.len() - 1]).unwrap();
    });
    let client = Client::connect(link).unwrap();
    let got = client.start(&BoxId::new("b").unwrap());
    agent.join().unwrap();
    let Err(Refusal::Broken(why)) = got else {
        panic!("expected a broken connection, got {got:?}");
    };
    assert!(why.contains("middle of a message"), "{why}");
}

/// How a connection's sends are bounded: by a timeout on its socket, or by the client's core, which writes it without
/// waiting and waits on it within its call's learned wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bound {
    Socket(Option<Duration>),
    Core,
}

/// Each send bound set on a connection, by the port it was dialed on.
type SendBounds = Arc<std::sync::Mutex<Vec<(u32, Bound)>>>;

/// A connection that records each send bound set on it, by the port it was dialed on.
struct Recorded {
    conn: TcpStream,
    port: u32,
    bounds: SendBounds,
}

impl Read for Recorded {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.conn.read(buf)
    }
}

impl Write for Recorded {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.conn.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.conn.flush()
    }
}

impl Duplex for Recorded {
    fn try_clone(&self) -> std::io::Result<Box<dyn Duplex>> {
        Ok(Box::new(Recorded {
            conn: self.conn.try_clone()?,
            port: self.port,
            bounds: Arc::clone(&self.bounds),
        }))
    }

    fn shutdown(&self, how: Shutdown) -> std::io::Result<()> {
        self.conn.shutdown(how)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.bounds
            .lock()
            .unwrap()
            .push((self.port, Bound::Socket(timeout)));
        self.conn.set_write_timeout(timeout)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.conn.set_read_timeout(timeout)
    }

    fn handle(&self) -> super::Handle<'_> {
        Duplex::handle(&self.conn)
    }

    fn set_nonblocking(&self, on: bool) -> std::io::Result<()> {
        if on {
            self.bounds.lock().unwrap().push((self.port, Bound::Core));
        }
        self.conn.set_nonblocking(on)
    }
}

struct Recording {
    control: SocketAddr,
    stdio: SocketAddr,
    bounds: SendBounds,
}

impl Link for Recording {
    fn dial(&self, port: u32) -> Result<Box<dyn Duplex>, Refusal> {
        let addr = if port == stream::CONTROL_PORT {
            self.control
        } else {
            self.stdio
        };
        Ok(Box::new(Recorded {
            conn: TcpStream::connect(addr).unwrap(),
            port,
            bounds: Arc::clone(&self.bounds),
        }))
    }
}

/// HEX-GUEST-32 (§3, CLAUDE.md §8): exec a program with stdin through the guest: expect its stdio
/// connection given no send bound, as a local exec's stdin is the caller's own pipe, which waits on the
/// program as long as it runs; only the control connection is bounded, as the local client's socket is: written
/// without waiting and waited on by the client's core, within its call's learned wait.
#[test]
fn a_stdio_connection_waits_on_its_program_as_a_local_pipe_does() {
    let (control, stdio) = (
        TcpListener::bind("127.0.0.1:0").unwrap(),
        TcpListener::bind("127.0.0.1:0").unwrap(),
    );
    let bounds = Arc::new(std::sync::Mutex::new(Vec::new()));
    let link = Arc::new(Recording {
        control: control.local_addr().unwrap(),
        stdio: stdio.local_addr().unwrap(),
        bounds: Arc::clone(&bounds),
    });
    let agent = std::thread::spawn(move || {
        let (mut conn, _) = control.accept().unwrap();
        hello(&mut conn);
        let (h, _) = next_request(&mut conn);
        let (s, _) = stdio.accept().unwrap();
        send_answer(&mut conn, h.id, &Answer::Started { pid: 7 });
        send_answer(&mut conn, h.id, &Answer::Exited(Exit::Code(0)));
        let _ = next_request(&mut conn);
        drop(s);
    });
    let client = Client::connect(link).unwrap();
    let (in_r, in_w) = std::io::pipe().unwrap();
    let mut proc = client
        .exec(
            &BoxId::new("b").unwrap(),
            Program::of(&["cat"]),
            Stdio {
                stdin: Some(in_r),
                stdout: None,
                stderr: None,
                ..Stdio::null()
            },
        )
        .unwrap();
    assert_eq!(proc.wait_for(Duration::MAX).unwrap(), Some(Exit::Code(0)));
    drop((proc, in_w));
    let _ = client.inspect(&BoxId::new("b").unwrap());
    agent.join().unwrap();
    let bounds = bounds.lock().unwrap().clone();
    assert!(
        bounds.iter().all(|(port, _)| *port == stream::CONTROL_PORT),
        "{bounds:?}"
    );
    assert!(
        bounds.contains(&(stream::CONTROL_PORT, Bound::Core)),
        "{bounds:?}"
    );
}

/// A stream over loopback, as Shards' is over its device.
struct Looped(TcpStream);

impl Read for Looped {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for Looped {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl shards_api::Stream for Looped {
    fn try_clone(&self) -> std::io::Result<Box<dyn shards_api::Stream>> {
        Ok(Box::new(Looped(self.0.try_clone()?)))
    }

    fn handle(&self) -> shards_api::Handle<'_> {
        #[cfg(unix)]
        return std::os::fd::AsFd::as_fd(&self.0);
        #[cfg(windows)]
        std::os::windows::io::AsSocket::as_socket(&self.0)
    }

    fn set_nonblocking(&self, on: bool) -> std::io::Result<()> {
        self.0.set_nonblocking(on)
    }

    fn shutdown(&self, how: Shutdown) -> std::io::Result<()> {
        self.0.shutdown(how)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.0.set_read_timeout(timeout)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> std::io::Result<()> {
        self.0.set_write_timeout(timeout)
    }

    /// Not asked of a guest's stream by Hex: refused, as loopback's are std's to keep.
    fn buffer_sizes(&self) -> std::io::Result<(usize, usize)> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "a test's loopback stream reports no buffer sizes",
        ))
    }
}

/// A guest that records the patience each dial is given and answers every one on loopback.
struct Patient {
    listener: TcpListener,
    given: std::sync::Mutex<Vec<Duration>>,
}

impl shards_api::Guest for Patient {
    fn connect(
        &self,
        _port: u32,
        patience: Duration,
    ) -> Result<Box<dyn shards_api::Stream>, shards_api::Refusal> {
        self.given.lock().unwrap().push(patience);
        let conn = TcpStream::connect(self.listener.local_addr().unwrap()).unwrap();
        Ok(Box::new(Looped(conn)))
    }

    fn stop(&self) -> Result<(), shards_api::Refusal> {
        Ok(())
    }

    fn wait_for(&self, _: Duration) -> Result<Option<shards_api::End>, shards_api::Refusal> {
        Ok(None)
    }

    fn identity(&self) -> Result<shards_api::Identity, shards_api::Refusal> {
        Err(shards_api::Refusal::Unsupported {
            what: "identity",
            why: "a loopback stand-in names no VM".into(),
        })
    }
}

/// HEX-GUEST-33 (§2, CLAUDE.md §4): dial a booted guest three times: expect the first dial given the
/// caller's bound on the boot, and each later one a single try (`Duration::ZERO`), as the guest's relay
/// already listens and a refusal then is its end, not its boot.
#[test]
fn the_first_dial_waits_for_the_guest_and_later_ones_try_once() {
    let guest = Arc::new(Patient {
        listener: TcpListener::bind("127.0.0.1:0").unwrap(),
        given: std::sync::Mutex::new(Vec::new()),
    });
    let first = Duration::from_millis(354);
    let link = shards::ShardsLink::new(guest.clone(), first);
    for port in [stream::CONTROL_PORT, stream::STDIO_PORT, stream::STDIO_PORT] {
        link.dial(port).unwrap();
    }
    assert_eq!(
        *guest.given.lock().unwrap(),
        [first, Duration::ZERO, Duration::ZERO]
    );
}

/// HEX-RT-17, HEX-GUEST-41: ask an actual Shards simulator for stdin and stdout, then use their raw descriptors as
/// the caller's files. Expect blocking ends that echo to EOF, while the control connection remains non-blocking.
/// Shards waits around its non-blocking kernel sockets in its Stream wrapper; that wrapper is gone at handoff.
#[test]
fn asked_shards_streams_wait_as_the_callers_files_do() {
    use shards_api::Guests;

    let guests = shards_api::sim::SimGuests::new(|host| {
        host.listen(stream::CONTROL_PORT);
        host.listen(stream::STDIO_PORT);
        let (port, mut control) = host.accept().unwrap();
        assert_eq!(port, stream::CONTROL_PORT);
        control.write_all(&TOKEN.to_le_bytes()).unwrap();
        let mut named = std::collections::BTreeMap::new();
        for _ in 0..2 {
            let (port, mut conn) = host.accept().unwrap();
            assert_eq!(port, stream::STDIO_PORT);
            let mut name = [0u8; stream::NAME_BYTES];
            conn.read_exact(&mut name).unwrap();
            let named_stream = stream::read_name(&name).unwrap();
            assert_eq!(named_stream.exec, Exec { conn: TOKEN, id: 1 });
            named.insert(named_stream.stream, conn);
        }
        let mut stdin = named.remove(&stream::Stdio::In).unwrap();
        let mut stdout = named.remove(&stream::Stdio::Out).unwrap();
        std::io::copy(&mut stdin, &mut stdout).unwrap();
        shards_api::End::PowerOff
    });
    let guest = guests
        .boot(&shards_api::GuestSpec {
            kernel: "kernel".into(),
            init: shards_api::Init::Executable("init".into()),
            cmdline: String::new(),
            vcpus: 1,
            gpu: false,
            gpu_devices: Vec::new(),
            memory_mib: shards_api::min_memory_mib(std::env::consts::ARCH),
            disks: vec![],
            shares: vec![],
            ports: vec![stream::CONTROL_PORT, stream::STDIO_PORT],
            binary: None,
        })
        .unwrap();
    let transport = Stream::dial(Arc::new(shards::ShardsLink::new(
        Arc::from(guest),
        Duration::MAX,
    )))
    .unwrap();
    let given = transport
        .asks(
            1,
            hex_api::Ask {
                stdin: true,
                stdout: true,
                stderr: false,
            },
        )
        .unwrap();
    let (stdin, stdout) = (given.stdin.unwrap(), given.stdout.unwrap());
    #[cfg(unix)]
    for fd in [&stdin, &stdout] {
        assert!(
            !rustix::fs::fcntl_getfl(fd)
                .unwrap()
                .contains(rustix::fs::OFlags::NONBLOCK),
            "a raw given end must wait without Shards' Stream wrapper"
        );
    }
    #[cfg(unix)]
    assert!(
        rustix::fs::fcntl_getfl(transport.handle())
            .unwrap()
            .contains(rustix::fs::OFlags::NONBLOCK)
    );
    #[cfg(windows)]
    {
        assert!(stdin.socket().is_some());
        assert!(stdout.socket().is_some());
        // A retained transfer copy closing must not shut down the caller's socket stream.
        let retained: hex_api::End = stdin.socket().unwrap().try_clone_to_owned().unwrap().into();
        drop(retained);
        let empty = hex_afunix::receive_socket(transport.handle(), &mut [0]).unwrap_err();
        assert_eq!(empty.kind(), std::io::ErrorKind::WouldBlock);
    }
    let mut stdin = hex_api::OwnedStream::from(stdin);
    stdin.write_all(b"round trip\n").unwrap();
    drop(stdin);
    let mut output = String::new();
    hex_api::OwnedStream::from(stdout)
        .read_to_string(&mut output)
        .unwrap();
    assert_eq!(output, "round trip\n");
}

/// Native plan step 8 (1), CLAUDE.md §2: the event form's writes queue behind a backlog bounded in bytes.
/// To a peer that never reads, frames go while the socket and the backlog have room; the one that wouldn't
/// fit is refused whole, `Exhausted` naming the backlog and its bound, nothing past the bound is ever
/// held, and the loop is told once, when bytes first wait.
#[test]
fn the_event_forms_writes_are_bounded_and_refused_whole_past_the_bound() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let mut writer = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    // Held, never read.
    let (_peer, _) = listener.accept().unwrap();
    writer.set_nonblocking(true).unwrap();
    let told = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let telling = Arc::clone(&told);
    let mut backlog = Backlog {
        bytes: VecDeque::new(),
        bound: 4096,
        wants_room: Arc::new(move || {
            telling.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }),
    };
    let frame = vec![7u8; 1000];
    let mut refused = None;
    // Bounded: 20 MB, past any loopback socket's buffers, so the backlog fills well before it ends.
    for _ in 0..20_000 {
        match backlog.send(&mut writer, &frame) {
            Ok(()) => assert!(backlog.bytes.len() <= 4096, "{} held", backlog.bytes.len()),
            Err(Sent::Refused(why)) => {
                refused = Some(why);
                break;
            }
            Err(Sent::Gone | Sent::Full(_)) => panic!("the peer went, or the send waited"),
        }
    }
    assert_eq!(
        refused,
        Some(Refusal::Exhausted {
            what: BACKLOG,
            bound: 4096
        })
    );
    assert!(backlog.bytes.len() <= 4096);
    assert_eq!(told.load(std::sync::atomic::Ordering::SeqCst), 1);
}

/// Native plan step 8 (1): the event form's pump, its connection ended by the agent, says `Ended`, and
/// every call waiting through it hears why, as a typed refusal; a call begun after is refused at once,
/// never left waiting.
#[cfg(unix)]
#[test]
fn a_pumps_end_reaches_every_call_waiting_through_it() {
    let (control, _stdio, link) = listeners();
    let agent = std::thread::spawn(move || {
        let (mut conn, _) = control.accept().unwrap();
        hello(&mut conn);
        // Reads the inspect, and closes without answering it.
        let _ = next_request(&mut conn);
    });
    let (client, mut pump) = Client::connect_evented(link, Arc::new(|| {})).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    client
        .begin(
            Call::Inspect(BoxId::new("b").unwrap()),
            Stdio::null(),
            Box::new(move |_, got| tx.send(got).unwrap()),
            Box::new(|_| {}),
        )
        .unwrap();
    agent.join().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        if pump.ready() == Pumped::Ended {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the pump never saw its end"
        );
        let handle = pump.handle();
        let mut fds = [rustix::event::PollFd::new(
            &handle,
            rustix::event::PollFlags::IN,
        )];
        let wait = rustix::event::Timespec {
            tv_sec: 1,
            tv_nsec: 0,
        };
        let _ = rustix::event::poll(&mut fds, Some(&wait));
    }
    let got = rx.recv_timeout(Duration::from_secs(30)).unwrap();
    assert!(
        matches!(&got, Err(Refusal::Broken(why)) if why.contains("closed")),
        "{got:?}"
    );
    assert!(
        client
            .begin(
                Call::Inspect(BoxId::new("c").unwrap()),
                Stdio::null(),
                Box::new(|_, _| {}),
                Box::new(|_| {}),
            )
            .is_err()
    );
}

/// Native plan step 8, guest stdio (HEX-GUEST-41): the event form, a node's, copies no stream: an exec begun with
/// a pipe of the caller's own is refused as invalid stdio and nothing goes to the guest, so the guest's next
/// request is the inspect begun after it.
#[test]
fn the_event_form_takes_no_pipe_of_the_callers_own() {
    let (control, _stdio, link) = listeners();
    let agent = std::thread::spawn(move || {
        let (mut conn, _) = control.accept().unwrap();
        hello(&mut conn);
        let (_, request) = next_request(&mut conn);
        request
    });
    let (client, _pump) = Client::connect_evented(link, Arc::new(|| {})).unwrap();
    let (r, _w) = std::io::pipe().unwrap();
    let carried = client.begin(
        Call::Exec {
            id: BoxId::new("b").unwrap(),
            program: Program::of(&["cat"]),
            streams: wire::Streams::default(),
        },
        Stdio {
            stdin: Some(r),
            ..Stdio::null()
        },
        Box::new(|_, _| {}),
        Box::new(|_| {}),
    );
    assert!(
        matches!(&carried, Err(Refusal::Invalid { what: "stdio", .. })),
        "{carried:?}"
    );
    client
        .begin(
            Call::Inspect(BoxId::new("b").unwrap()),
            Stdio::null(),
            Box::new(|_, _| {}),
            Box::new(|_| {}),
        )
        .unwrap();
    assert_eq!(
        agent.join().unwrap(),
        Request::Call(Call::Inspect(BoxId::new("b").unwrap()))
    );
}

/// AC-T7.4.1: permanent selector mistakes and temporary storage exhaustion retain distinct
/// public categories; the adapter never turns either into an opaque operating-system retry.
#[test]
fn physical_gpu_refusals_preserve_retry_category() {
    for cause in [
        shards_api::PhysicalGpuError::Syntax,
        shards_api::PhysicalGpuError::Duplicate,
    ] {
        assert!(matches!(
            shards::refusal("booting", &shards_api::Refusal::Gpu(cause)),
            Refusal::Invalid {
                what: "physical GPU selection",
                ..
            }
        ));
    }
    assert!(
        matches!(shards::refusal("booting", &shards_api::Refusal::Gpu(shards_api::PhysicalGpuError::Allocation)),
        Refusal::Exhausted { what: "physical GPU selection storage", bound } if bound == shards_api::physical::MAX_PHYSICAL_GPUS)
    );
}
