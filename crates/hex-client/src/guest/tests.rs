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
    fn dial(&self, port: u32) -> Result<DuplexOwner, Refusal> {
        let addr = match port {
            stream::CONTROL_PORT => self.control,
            stream::STDIO_PORT => self.stdio,
            other => panic!("dialed port {other:#x}"),
        };
        own_duplex(TcpStream::connect(addr).unwrap())
    }
}

fn listeners() -> (TcpListener, TcpListener, LinkOwner) {
    let control = TcpListener::bind("127.0.0.1:0").unwrap();
    let stdio = TcpListener::bind("127.0.0.1:0").unwrap();
    let link = own_link(Loopback {
        control: control.local_addr().unwrap(),
        stdio: stdio.local_addr().unwrap(),
    })
    .unwrap();
    (control, stdio, link)
}

/// AC-T7.4.1: owning a link makes no dial; clones retain the same link until the last drop.
#[test]
fn link_owner_retains_one_instance_without_dial_effects() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counted {
        dials: Arc<AtomicUsize>,
        drops: Arc<AtomicUsize>,
    }
    impl Link for Counted {
        fn dial(&self, _: u32) -> Result<DuplexOwner, Refusal> {
            self.dials.fetch_add(1, Ordering::SeqCst);
            Err(Refusal::Broken("owned test dial".into()))
        }
    }
    impl Drop for Counted {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }

    let dials = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let owner = own_link(Counted {
        dials: Arc::clone(&dials),
        drops: Arc::clone(&drops),
    })
    .unwrap();
    let retained = owner.clone();
    assert!(triomphe::Arc::ptr_eq(&owner, &retained));
    assert_eq!(dials.load(Ordering::SeqCst), 0);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    assert!(matches!(
        retained.dial(stream::CONTROL_PORT),
        Err(Refusal::Broken(_))
    ));
    assert_eq!(dials.load(Ordering::SeqCst), 1);
    drop(owner);
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    drop(retained);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
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
    let refused = Client::connect(link.clone()).err();
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
    fn try_clone(&self) -> std::io::Result<DuplexOwner> {
        own_duplex(Recorded {
            conn: self.conn.try_clone()?,
            port: self.port,
            bounds: Arc::clone(&self.bounds),
        })
        .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))
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
    fn dial(&self, port: u32) -> Result<DuplexOwner, Refusal> {
        let addr = if port == stream::CONTROL_PORT {
            self.control
        } else {
            self.stdio
        };
        own_duplex(Recorded {
            conn: TcpStream::connect(addr).unwrap(),
            port,
            bounds: Arc::clone(&self.bounds),
        })
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
    let link = own_link(Recording {
        control: control.local_addr().unwrap(),
        stdio: stdio.local_addr().unwrap(),
        bounds: Arc::clone(&bounds),
    })
    .unwrap();
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
    absolute: std::sync::Mutex<Vec<Instant>>,
}

impl shards_api::Guest for Patient {
    fn process_exited(&self) -> Result<bool, shards_api::Refusal> {
        Ok(false)
    }

    fn connect(
        &self,
        _port: u32,
        patience: Duration,
    ) -> Result<Box<dyn shards_api::Stream>, shards_api::Refusal> {
        self.given.lock().unwrap().push(patience);
        let conn = TcpStream::connect(self.listener.local_addr().unwrap()).unwrap();
        Ok(Box::new(Looped(conn)))
    }

    fn connect_before(
        &self,
        port: u32,
        at: std::time::Instant,
    ) -> Result<Box<dyn shards_api::Stream>, shards_api::Refusal> {
        self.absolute.lock().unwrap().push(at);
        let left = at
            .checked_duration_since(std::time::Instant::now())
            .filter(|left| !left.is_zero())
            .ok_or(shards_api::Refusal::Connect {
                port,
                failure: shards_api::HandshakeError::Expired {
                    phase: shards_api::HandshakePhase::Dial,
                },
            })?;
        let stream = self.connect(port, left)?;
        if std::time::Instant::now() >= at {
            return Err(shards_api::Refusal::Connect {
                port,
                failure: shards_api::HandshakeError::Expired {
                    phase: shards_api::HandshakePhase::ClearTimeouts,
                },
            });
        }
        Ok(stream)
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
        absolute: std::sync::Mutex::new(Vec::new()),
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
    let transport =
        Stream::dial(own_link(shards::ShardsLink::new(Arc::from(guest), Duration::MAX)).unwrap())
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

// Caller policies for these stand-ins, not production waits or calibration inputs.
const CONNECT_CAP: Duration = Duration::from_millis(25);
const CONNECT_OBSERVE: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Debug)]
enum ConnectForm {
    Ordinary,
    Evented,
}

fn connect_form(
    form: ConnectForm,
    link: LinkOwner,
    at: Instant,
) -> Result<(Client, Option<Pump>), Refusal> {
    match form {
        ConnectForm::Ordinary => Client::connect_before(link, at).map(|client| (client, None)),
        ConnectForm::Evented => Client::connect_evented_before(link, Arc::new(|| {}), at)
            .map(|(client, pump)| (client, Some(pump))),
    }
}

#[derive(Default)]
struct ConnectTrace {
    dials: Mutex<Vec<(u32, Option<Instant>)>>,
    read: std::sync::atomic::AtomicUsize,
    written: Mutex<Vec<u8>>,
    native_clones: std::sync::atomic::AtomicUsize,
    facade_drops: std::sync::atomic::AtomicUsize,
}

struct ConnectLink {
    socket: Mutex<Option<TcpStream>>,
    budget: Option<Duration>,
    trace: Arc<ConnectTrace>,
}

impl Link for ConnectLink {
    fn reply_budget(&self) -> Option<Duration> {
        self.budget
    }

    fn dial(&self, port: u32) -> Result<DuplexOwner, Refusal> {
        self.dial_before(port, None)
    }

    fn dial_before(&self, port: u32, at: Option<Instant>) -> Result<DuplexOwner, Refusal> {
        self.trace.dials.lock().unwrap().push((port, at));
        let socket = self
            .socket
            .lock()
            .unwrap()
            .take()
            .ok_or_else(|| Refusal::Broken("constructor probe has no connection".into()))?;
        own_duplex(ConnectSocket {
            socket,
            trace: Arc::clone(&self.trace),
        })
    }
}

struct ConnectSocket {
    socket: TcpStream,
    trace: Arc<ConnectTrace>,
}

impl Drop for ConnectSocket {
    fn drop(&mut self) {
        self.trace
            .facade_drops
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Read for ConnectSocket {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let n = self.socket.read(bytes)?;
        self.trace
            .read
            .fetch_add(n, std::sync::atomic::Ordering::SeqCst);
        Ok(n)
    }
}

impl Write for ConnectSocket {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let n = self.socket.write(bytes)?;
        self.trace
            .written
            .lock()
            .unwrap()
            .extend_from_slice(&bytes[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}

impl Duplex for ConnectSocket {
    fn try_clone(&self) -> io::Result<DuplexOwner> {
        self.trace
            .native_clones
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        own_duplex(Self {
            socket: self.socket.try_clone()?,
            trace: Arc::clone(&self.trace),
        })
        .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.socket.shutdown(how)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.socket.set_write_timeout(timeout)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.socket.set_read_timeout(timeout)
    }

    fn handle(&self) -> Handle<'_> {
        Duplex::handle(&self.socket)
    }

    fn set_nonblocking(&self, on: bool) -> io::Result<()> {
        self.socket.set_nonblocking(on)
    }
}

fn connect_link(
    socket: Option<TcpStream>,
    budget: Option<Duration>,
) -> (LinkOwner, Arc<ConnectTrace>) {
    let trace = Arc::new(ConnectTrace::default());
    let link = own_link(ConnectLink {
        socket: Mutex::new(socket),
        budget,
        trace: Arc::clone(&trace),
    })
    .unwrap();
    (link, trace)
}

fn connect_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client =
        TcpStream::connect_timeout(&listener.local_addr().unwrap(), CONNECT_OBSERVE).unwrap();
    listener.set_nonblocking(true).unwrap();
    let (peer, _) = listener.accept().unwrap();
    (client, peer)
}

#[derive(Debug)]
struct ConstructorHello {
    header: Option<Header>,
    written: Vec<u8>,
    pending: bool,
    buffered: usize,
}

fn constructor_hello(trace: &ConnectTrace) -> ConstructorHello {
    let written = trace.written.lock().unwrap();
    assert!(written.len() <= stream::MAX_FRAME);
    let mut decoder = Deframer::new(wire::MAX_MESSAGE);
    let mut consumed = 0;
    let mut header = None;
    // take() first consumes the length prefix, then the body. Every nonempty
    // step must make progress, so captured bytes also bound the step count.
    for _ in 0..written.len() {
        if consumed == written.len() {
            break;
        }
        let (took, message) = decoder.take(&written[consumed..]).unwrap();
        assert!(took > 0 && took <= written.len() - consumed);
        consumed += took;
        if let Some(message) = message {
            assert!(
                header.is_none(),
                "constructor emitted more than one message"
            );
            assert_eq!(consumed, written.len(), "bytes after the complete Hello");
            let (got, request) = wire::read_request_apart(&message, wire::MAX_MESSAGE).unwrap();
            assert!(matches!(request, Request::Hello { build } if build == wire::build()));
            header = Some(got);
        }
    }
    assert_eq!(consumed, written.len());
    ConstructorHello {
        header,
        written: written.clone(),
        pending: decoder.in_frame(),
        buffered: decoder.buffered(),
    }
}

/// AC-T7.4.1: both public constructors reject expired/invalid authority before dial,
/// preserve an exact absolute instant with no Link budget, and honor a smaller configured cap.
#[test]
fn absolute_connect_rejects_before_dial_and_preserves_smaller_caps() {
    for form in [ConnectForm::Ordinary, ConnectForm::Evented] {
        let (link, trace) = connect_link(None, None);
        let expired = connect_form(form, link, Instant::now()).map(drop);
        assert!(matches!(expired, Err(Refusal::Os { .. })), "{expired:?}");
        assert!(trace.dials.lock().unwrap().is_empty());

        for invalid in [Duration::ZERO, Duration::MAX] {
            let (link, trace) = connect_link(None, Some(invalid));
            let at = Instant::now().checked_add(CONNECT_OBSERVE).unwrap();
            let refused = connect_form(form, link, at).map(drop);
            assert!(
                matches!(
                    refused,
                    Err(Refusal::Invalid {
                        what: "client reply budget",
                        ..
                    })
                ),
                "{refused:?}"
            );
            assert!(trace.dials.lock().unwrap().is_empty());
        }

        let (link, trace) = connect_link(None, None);
        let at = Instant::now().checked_add(CONNECT_OBSERVE).unwrap();
        let refused = connect_form(form, link, at).map(drop);
        assert!(matches!(refused, Err(Refusal::Broken(_))));
        assert_eq!(
            *trace.dials.lock().unwrap(),
            [(stream::CONTROL_PORT, Some(at))]
        );

        let (link, trace) = connect_link(None, Some(CONNECT_CAP));
        let at = Instant::now().checked_add(CONNECT_OBSERVE).unwrap();
        let before = Instant::now();
        let refused = connect_form(form, link, at).map(drop);
        let after = Instant::now();
        assert!(matches!(refused, Err(Refusal::Broken(_))));
        let dials = trace.dials.lock().unwrap();
        assert_eq!(dials.len(), 1);
        let (port, selected) = dials[0];
        let selected = selected.unwrap();
        assert_eq!(port, stream::CONTROL_PORT);
        assert!(before.checked_add(CONNECT_CAP).unwrap() <= selected);
        assert!(selected <= after.checked_add(CONNECT_CAP).unwrap());
        assert!(selected < at);
    }
}

/// AC-T7.4.1: actual partial token reads and the hello send/wait consume the same
/// original cap even when Link has no default budget. Both constructor forms are covered.
#[test]
fn absolute_connect_caps_partial_token_and_unanswered_hello() {
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;

    let mut observations = Vec::with_capacity(4);
    for form in [ConnectForm::Ordinary, ConnectForm::Evented] {
        for whole_token in [false, true] {
            let (socket, mut peer) = connect_pair();
            let token = TOKEN.to_le_bytes();
            let prefix = if whole_token { &token[..] } else { &token[..1] };
            peer.write_all(prefix).unwrap();
            let (link, trace) = connect_link(Some(socket), None);
            let at = Instant::now().checked_add(CONNECT_CAP).unwrap();
            let (finished, answer) = mpsc::sync_channel(1);
            let worker = std::thread::spawn(move || {
                let result = connect_form(form, link, at).map(drop);
                let _ = finished.send((Instant::now(), result));
            });
            // No assertion may bypass this owned socket close and actor join.
            let observed = answer.recv_timeout(CONNECT_OBSERVE);
            let stopped = peer.shutdown(Shutdown::Both);
            drop(peer);
            let joined = worker.join();
            let read = trace.read.load(Ordering::SeqCst);
            observations.push((
                form,
                whole_token,
                at,
                observed,
                stopped,
                joined.is_ok(),
                read,
                trace,
            ));
        }
    }
    let mut uncommitted = Vec::with_capacity(2);
    // All four owned peers are closed and actors joined before any assertion.
    for (form, whole_token, at, observed, stopped, joined, read, trace) in observations {
        let hello = constructor_hello(&trace);
        eprintln!(
            "client_connect_absolute form={form:?} whole_token={whole_token} at={at:?} observed={observed:?} read={read} stopped={stopped:?} joined={joined} hello={hello:?}"
        );
        assert!(joined);
        let (returned, result) = observed.expect("constructor did not return before owned cleanup");
        assert_eq!(
            read,
            if whole_token { stream::TOKEN_BYTES } else { 1 },
            "missing actual read progress is a harness miss"
        );
        assert!(matches!(result, Err(Refusal::Os { .. })), "{result:?}");
        assert_eq!(
            *trace.dials.lock().unwrap(),
            [(stream::CONTROL_PORT, Some(at))]
        );
        if whole_token {
            if let Some(header) = hello.header {
                assert_eq!(header.id, 1);
                assert!(!hello.pending && hello.buffered == 0);
                eprintln!(
                    "client_connect_hello form={form:?} phase=committed_hello withheld_answer_proof=true"
                );
            } else {
                let phase = if hello.written.is_empty() {
                    "learned_before_send"
                } else {
                    "learned_during_incomplete_send"
                };
                eprintln!(
                    "client_connect_hello form={form:?} phase={phase} withheld_answer_proof=false status=UNVERIFIED written={:02x?}",
                    hello.written
                );
                assert!(
                    matches!(&result, Err(Refusal::Os { op: "sending to the node agent", why }) if why.contains("twice")),
                    "uncommitted Hello must be explained by its smaller learned send bound: {result:?}"
                );
                uncommitted.push((form, phase));
            }
        } else {
            assert!(hello.written.is_empty());
            assert!(
                matches!(&result, Err(Refusal::Os { why, .. }) if why.contains("absolute"))
                    && returned >= at
            );
        }
    }
    assert!(
        uncommitted.is_empty(),
        "UNVERIFIED withheld-answer phase: no complete actual Hello; harness witness miss, not a production timeout failure: {uncommitted:?}"
    );
}

/// AC-T7.4.1: a complete real hello reaches both constructors; the event form keeps
/// the same Link owner in its existing Pump and stores no caller deadline as a lifetime cap.
#[test]
fn absolute_connect_completes_and_evented_retains_the_same_link() {
    use std::sync::mpsc;

    for form in [ConnectForm::Ordinary, ConnectForm::Evented] {
        let (socket, mut peer) = connect_pair();
        peer.write_all(&TOKEN.to_le_bytes()).unwrap();
        // The fresh Shared's first request is 1; independently decode its actual Hello below.
        send_answer(&mut peer, 1, &Answer::Hello(agents_hello()));
        let (link, trace) = connect_link(Some(socket), None);
        let same_link = triomphe::Arc::clone(&link);
        let at = Instant::now().checked_add(CONNECT_OBSERVE).unwrap();
        let (finished, answer) = mpsc::sync_channel(1);
        let worker = std::thread::spawn(move || {
            let result = connect_form(form, link, at).map(|(client, pump)| {
                let evented = pump.as_ref().map(|pump| {
                    (
                        triomphe::Arc::ptr_eq(&pump.shared.transport.link, &same_link),
                        pump.shared.transport.reply_budget,
                        pump.shared.transport.token,
                    )
                });
                (client.classes(), evented)
            });
            let _ = finished.send(result);
        });
        let observed = answer.recv_timeout(CONNECT_OBSERVE);
        let stopped = peer.shutdown(Shutdown::Both);
        drop(peer);
        let joined = worker.join();
        eprintln!(
            "client_connect_positive form={form:?} at={at:?} observed={observed:?} stopped={stopped:?} joined={}",
            joined.is_ok()
        );
        joined.unwrap();
        let (classes, evented) = observed.unwrap().unwrap();
        assert_eq!(classes, vec![Class::Oci]);
        match form {
            ConnectForm::Ordinary => assert_eq!(evented, None),
            ConnectForm::Evented => assert_eq!(evented, Some((true, None, TOKEN))),
        }
        let hello = constructor_hello(&trace);
        eprintln!("client_connect_positive_hello form={form:?} hello={hello:?}");
        assert!(!hello.pending && hello.buffered == 0);
        assert_eq!(
            hello
                .header
                .expect("positive requires a complete actual Hello")
                .id,
            1
        );
        assert_eq!(
            *trace.dials.lock().unwrap(),
            [(stream::CONTROL_PORT, Some(at))]
        );
    }
}

/// AC-T7.4.1: the actual Shards adapter rejects before effects, then carries the
/// identical absolute authority and bytes through the existing unique facade.
#[test]
fn shards_link_carries_the_original_absolute_deadline_and_bytes() {
    let guest = Arc::new(Patient {
        listener: TcpListener::bind("127.0.0.1:0").unwrap(),
        given: std::sync::Mutex::new(Vec::new()),
        absolute: std::sync::Mutex::new(Vec::new()),
    });
    let link = shards::ShardsLink::new(guest.clone(), Duration::from_secs(1));
    assert!(
        link.dial_before(stream::CONTROL_PORT, Some(Instant::now()))
            .is_err()
    );
    assert!(guest.absolute.lock().unwrap().is_empty());
    assert!(guest.given.lock().unwrap().is_empty());
    let at = Instant::now().checked_add(Duration::from_secs(1)).unwrap();
    let mut stream = link.dial_before(stream::CONTROL_PORT, Some(at)).unwrap();
    let (mut peer, _) = guest.listener.accept().unwrap();
    peer.set_read_timeout(Some(Duration::from_secs(1))).unwrap();
    stream.write_all(b"exact operation authority").unwrap();
    let mut bytes = [0u8; 25];
    peer.read_exact(&mut bytes).unwrap();
    assert_eq!(&bytes, b"exact operation authority");
    assert_eq!(*guest.absolute.lock().unwrap(), [at]);
}

/// AC-T7.4.1: moving the unique facade through the existing copier adapter clones
/// no native stream; an explicit native clone retains I/O until its own final drop.
#[test]
fn duplex_owner_preserves_io_and_explicit_native_clone_lifetime() {
    use std::sync::atomic::Ordering;

    let (socket, mut peer) = connect_pair();
    socket.set_read_timeout(Some(CONNECT_OBSERVE)).unwrap();
    socket.set_write_timeout(Some(CONNECT_OBSERVE)).unwrap();
    peer.set_read_timeout(Some(CONNECT_OBSERVE)).unwrap();
    peer.set_write_timeout(Some(CONNECT_OBSERVE)).unwrap();
    let trace = Arc::new(ConnectTrace::default());
    let owner = own_duplex(ConnectSocket {
        socket,
        trace: Arc::clone(&trace),
    })
    .unwrap();
    let before_move = std::ptr::from_ref(&*owner);
    let mut moved = DuplexIo(owner);
    let same_facade = std::ptr::addr_eq(before_move, std::ptr::from_ref(&*moved.0));
    let clones_before = trace.native_clones.load(Ordering::SeqCst);
    let input_sent = peer.write_all(b"in");
    let mut input = [0; 2];
    let input_read = moved.read_exact(&mut input);
    let output_sent = moved.write_all(b"out");
    let mut output = [0; 3];
    let output_read = peer.read_exact(&mut output);
    let clone = moved.0.try_clone();
    drop(moved);
    let first_drop = trace.facade_drops.load(Ordering::SeqCst);
    let final_write = match clone {
        Ok(mut clone) => {
            let sent = clone.write_all(b"last");
            drop(clone);
            sent
        }
        Err(why) => Err(why),
    };
    let mut last = [0; 4];
    let last_read = peer.read_exact(&mut last);
    let eof = peer.read(&mut [0]);
    drop(peer);
    let clones = trace.native_clones.load(Ordering::SeqCst);
    let drops = trace.facade_drops.load(Ordering::SeqCst);
    eprintln!(
        "client_duplex_owner same={same_facade} clones_before={clones_before} clones={clones} first_drop={first_drop} drops={drops} eof={eof:?}"
    );
    // All owned facade/peer handles are gone before any result assertion.
    input_sent.unwrap();
    input_read.unwrap();
    output_sent.unwrap();
    output_read.unwrap();
    final_write.unwrap();
    last_read.unwrap();
    assert!(same_facade);
    assert_eq!(clones_before, 0);
    assert_eq!(clones, 1);
    assert_eq!(first_drop, 1);
    assert_eq!(drops, 2);
    assert_eq!(input, *b"in");
    assert_eq!(output, *b"out");
    assert_eq!(last, *b"last");
    assert_eq!(eof.unwrap(), 0);
}
