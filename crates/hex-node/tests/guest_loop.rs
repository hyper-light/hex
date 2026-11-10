//! Native plan step 8 (1), HEX-SOCK-5 through the real pieces: a node's socket over `Recorded<WithGuest<…>>`
//! with one worker, its guest a loopback stand-in for a Shards guest's relay whose answer to an inspect is
//! held until the test lets it go. Client A's inspect of a box in the guest waits on the guest; client B's
//! inspect of a host box is answered meanwhile, as no call the guest holds holds a worker.
//!
//! Native plan step 8, guest stdio (HEX-GUEST-41, HEX-RT-17 for a guest's box): through the same socket, a guest
//! program's streams are the guest's own stdio connections, whose ends the node gives the caller's client with its
//! answer. Asked for, they are the caller's; given as the caller's own pipes, the client copies between them, and the
//! node copies no byte and keeps none of the caller's pipes. The stand-in's programs echo their stdin to their stdout,
//! under a pid no host process has, so their ends come as the guest's messages.
//!
//! On Windows (native plan step 8, `watch_source`), HEX-SOCK-5's two tests hold through the same pieces: the
//! guest's control connection joins the node's completion port. Its guest programs' streams are Unix's here: a
//! guest stream's end as the caller's own handle is still to add on Windows, and refused by name there.
#![cfg(any(target_os = "linux", target_os = "macos", windows))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[cfg(unix)]
use hex_api::{Ask, Program, Stdio};
use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Exit, Refusal, Runtime};
use hex_core::record::ProcId;
use hex_core::{stream, wire};
use hex_node::recorded::{Recorded, Recover};
use hex_node::serve::Server;
use hex_runtime::guest::{GuestConfig, GuestRuntime, WithGuest};

/// The node's socket bounds as this host's platform gives them, as `hex node` takes them: Linux's from its sizes.
#[cfg(target_os = "linux")]
fn limits() -> wire::Limits {
    hex_node::serve::limits(&hex_node::sizes::Sizes::now().unwrap().platform())
}

/// The node's socket bounds as this Mac's kernel gives them, as `hex node` takes them.
#[cfg(target_os = "macos")]
fn limits() -> wire::Limits {
    hex_node::serve::limits(&hex_node::serve::macos::platform().unwrap())
}

/// The Windows node reads the same platform bounds used by `hex node`.
#[cfg(windows)]
fn limits() -> wire::Limits {
    hex_node::serve::limits(&hex_node::serve::windows::platform().unwrap())
}

/// Longest a test waits for a fact it needs: a bound, so a node that never makes it true fails the test
/// instead of hanging it.
const PATIENCE: Duration = Duration::from_secs(30);

/// The stand-in guest's programs' pid: past the highest any host gives, Linux's `PID_MAX_LIMIT` (4 << 20,
/// include/linux/threads.h) and macOS's `PID_MAX` (99999, bsd/sys/proc_internal.h), as a guest program's pid
/// names no process of its host.
const GUEST_PID: u32 = (4 << 20) + 1;

/// Whether the stand-in guest holds its answers to inspects, and how many it holds; and how many execs it
/// started.
#[derive(Default)]
struct Gate {
    state: Mutex<(bool, usize)>,
    changed: Condvar,
    execs: AtomicUsize,
}

impl Gate {
    fn hold(&self, held: bool) {
        self.state.lock().unwrap().0 = held;
        self.changed.notify_all();
    }

    /// Waits, while held, at most [`PATIENCE`]; counted while it waits.
    fn pass(&self) {
        let mut s = self.state.lock().unwrap();
        s.1 += 1;
        self.changed.notify_all();
        let deadline = std::time::Instant::now() + PATIENCE;
        while s.0 && std::time::Instant::now() < deadline {
            s = self.changed.wait_timeout(s, PATIENCE).unwrap().0;
        }
        s.1 -= 1;
        self.changed.notify_all();
    }

    fn wait_holding(&self, n: usize) {
        let deadline = std::time::Instant::now() + PATIENCE;
        let mut s = self.state.lock().unwrap();
        while s.1 != n {
            assert!(
                std::time::Instant::now() < deadline,
                "the guest never held {n}"
            );
            s = self.changed.wait_timeout(s, PATIENCE).unwrap().0;
        }
    }
}

/// The stand-in's stdio connections, by their exec (its control connection's token and its request id) and
/// stream, until that exec's program takes them.
#[derive(Default)]
struct Stdios {
    named: Mutex<BTreeMap<(u64, u64, stream::Stdio), TcpStream>>,
    came: Condvar,
}

impl Stdios {
    fn arrive(&self, named: stream::Named, conn: TcpStream) {
        self.named
            .lock()
            .unwrap()
            .insert((named.exec.conn, named.exec.id, named.stream), conn);
        self.came.notify_all();
    }

    /// Stream `s` of exec `id` on the control connection `token`, waiting at most [`PATIENCE`] for it.
    fn take(&self, token: u64, id: u64, s: stream::Stdio) -> Option<TcpStream> {
        let deadline = Instant::now() + PATIENCE;
        let mut named = self.named.lock().unwrap();
        loop {
            if let Some(conn) = named.remove(&(token, id, s)) {
                return Some(conn);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            named = self.came.wait_timeout(named, left).unwrap().0;
        }
    }
}

/// A guest's relay and node agent, on loopback: each control connection's own token first, a hello's answer,
/// every inspect answered `NoSuchBox` once the gate lets it pass, and every exec started as a program that
/// echoes its stdin to its stdout, its pid [`GUEST_PID`], its end sent once its stdin has ended. Its stdio
/// connections come on a port of their own, named by their first bytes. Each dial is served on a thread of its
/// own. The control port's address, then the stdio port's.
fn relay(gate: Arc<Gate>, conns: Arc<Mutex<Vec<TcpStream>>>) -> (SocketAddr, SocketAddr) {
    let control = TcpListener::bind("127.0.0.1:0").unwrap();
    let stdio = TcpListener::bind("127.0.0.1:0").unwrap();
    let addrs = (control.local_addr().unwrap(), stdio.local_addr().unwrap());
    let stdios = Arc::new(Stdios::default());
    {
        let (stdios, conns) = (Arc::clone(&stdios), Arc::clone(&conns));
        std::thread::spawn(move || {
            for conn in stdio.incoming() {
                let Ok(mut conn) = conn else { return };
                conns.lock().unwrap().push(conn.try_clone().unwrap());
                let mut name = [0u8; stream::NAME_BYTES];
                if conn.read_exact(&mut name).is_ok()
                    && let Ok(named) = stream::read_name(&name)
                {
                    stdios.arrive(named, conn);
                }
            }
        });
    }
    std::thread::spawn(move || {
        let tokens = AtomicU64::new(7);
        for conn in control.incoming() {
            let Ok(conn) = conn else { return };
            conns.lock().unwrap().push(conn.try_clone().unwrap());
            let (gate, stdios) = (Arc::clone(&gate), Arc::clone(&stdios));
            let token = tokens.fetch_add(1, Ordering::SeqCst);
            std::thread::spawn(move || serve(conn, &gate, &stdios, token));
        }
    });
    addrs
}

/// Writes `answer` to request `h` on the stand-in's control connection, framed.
fn answer(writer: &Mutex<TcpStream>, h: wire::Header, answer: &wire::Answer) {
    let bytes = wire::answer(h.id, answer, wire::MAX_MESSAGE).unwrap();
    let _ = writer
        .lock()
        .unwrap()
        .write_all(&stream::frame(&bytes, wire::MAX_MESSAGE).unwrap());
}

/// Exec `h`'s program on the control connection `token`: its stdin to its stdout, to its end, then its own
/// end, sent as the guest's message.
fn echo(stdios: &Stdios, writer: &Mutex<TcpStream>, token: u64, h: wire::Header) {
    let (Some(mut input), Some(mut output)) = (
        stdios.take(token, h.id, stream::Stdio::In),
        stdios.take(token, h.id, stream::Stdio::Out),
    ) else {
        return;
    };
    let _ = std::io::copy(&mut input, &mut output);
    // Its sending half ends: the relay's list of connections holds another handle on it.
    let _ = output.shutdown(Shutdown::Write);
    answer(writer, h, &wire::Answer::Exited(Exit::Code(0)));
}

fn serve(conn: TcpStream, gate: &Gate, stdios: &Arc<Stdios>, token: u64) {
    /// The connection's end on this side, once its client has ended it (or sent what isn't the control
    /// protocol), as a guest's relay ends it: the relay's list of connections holds another handle on it, and
    /// on Windows a client's own shutdown wakes no read of its that is blocked, so its reader goes only once
    /// this side has ended too.
    struct Ends(TcpStream);
    impl Drop for Ends {
        fn drop(&mut self) {
            let _ = self.0.shutdown(Shutdown::Both);
        }
    }
    let _ends = Ends(conn.try_clone().unwrap());
    let writer = Arc::new(Mutex::new(conn.try_clone().unwrap()));
    let mut conn = conn;
    writer
        .lock()
        .unwrap()
        .write_all(&token.to_le_bytes())
        .unwrap();
    let mut deframer = stream::Deframer::new(wire::MAX_MESSAGE);
    let mut buf = [0u8; 4096];
    loop {
        let n = match conn.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        let mut input = &buf[..n];
        while !input.is_empty() {
            // A connection that isn't the control protocol's (a stage's, say) is let go.
            let Ok((took, whole)) = deframer.take(input) else {
                return;
            };
            input = &input[took..];
            let Some(message) = whole else { continue };
            let Ok((h, request)) = wire::read_request_apart(&message, wire::MAX_MESSAGE) else {
                return;
            };
            let said = match request {
                wire::Request::Hello { .. } => wire::Answer::Hello(wire::Hello {
                    build: wire::build(),
                    classes: vec![hex_api::Class::Oci],
                    limits: wire::Limits {
                        connections: 4,
                        message_bytes: wire::MAX_MESSAGE,
                        in_flight: 4,
                        programs: 4,
                    },
                    budgets: wire::Budgets {
                        in_flight: 4,
                        programs: 4,
                    },
                }),
                wire::Request::Call(wire::Call::Inspect(id)) => {
                    gate.pass();
                    wire::Answer::Refused(Refusal::NoSuchBox(id))
                }
                wire::Request::Call(wire::Call::List) => wire::Answer::Boxes {
                    infos: Vec::new(),
                    more: false,
                },
                wire::Request::Call(wire::Call::Exec { .. }) => {
                    gate.execs.fetch_add(1, Ordering::SeqCst);
                    answer(&writer, h, &wire::Answer::Started { pid: GUEST_PID });
                    let (stdios, writer) = (Arc::clone(stdios), Arc::clone(&writer));
                    std::thread::spawn(move || echo(&stdios, &writer, token, h));
                    continue;
                }
                // Not answered, as a node answers no release.
                wire::Request::Release => continue,
                other => wire::Answer::Refused(Refusal::Broken(format!(
                    "the stand-in guest serves no {other:?}"
                ))),
            };
            answer(&writer, h, &said);
        }
    }
}

/// A loopback connection as a Shards stream.
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

/// A guest whose ports are the stand-in relay's: its stdio port, and its control port for every other; ended
/// once the test says so.
struct LoopGuest(SocketAddr, SocketAddr, Arc<std::sync::atomic::AtomicBool>);

impl shards_api::Guest for LoopGuest {
    fn connect(
        &self,
        port: u32,
        _patience: Duration,
    ) -> Result<Box<dyn shards_api::Stream>, shards_api::Refusal> {
        let to = if port == stream::STDIO_PORT {
            self.1
        } else {
            self.0
        };
        Ok(Box::new(Looped(TcpStream::connect(to).unwrap())))
    }

    fn stop(&self) -> Result<(), shards_api::Refusal> {
        Ok(())
    }

    fn wait_for(&self, _: Duration) -> Result<Option<shards_api::End>, shards_api::Refusal> {
        Ok(if self.2.load(std::sync::atomic::Ordering::SeqCst) {
            Some(shards_api::End::Stopped)
        } else {
            None
        })
    }

    fn identity(&self) -> Result<shards_api::Identity, shards_api::Refusal> {
        Err(shards_api::Refusal::Unsupported {
            what: "identity",
            why: "a loopback stand-in names no VM".into(),
        })
    }
}

/// One booted stand-in guest: whether it has ended, and its relay's connections, which end with it.
type Booted = (
    Arc<std::sync::atomic::AtomicBool>,
    Arc<Mutex<Vec<TcpStream>>>,
);

/// Guests each on a stand-in relay of its own, behind one gate; each one's end the test can bring about,
/// as a guest that dies: its connections end with it.
struct LoopGuests(Arc<Gate>, Mutex<Vec<Booted>>);

impl LoopGuests {
    fn new(gate: Arc<Gate>) -> Self {
        Self(gate, Mutex::new(Vec::new()))
    }

    /// Ends every guest booted so far, and its connections.
    fn end_all(&self) {
        for (ended, conns) in self.1.lock().unwrap().iter() {
            ended.store(true, std::sync::atomic::Ordering::SeqCst);
            for conn in conns.lock().unwrap().drain(..) {
                let _ = conn.shutdown(Shutdown::Both);
            }
        }
    }

    fn boots(&self) -> usize {
        self.1.lock().unwrap().len()
    }
}

/// The guests as the runtime takes them, shared with the test that ends them.
struct Shared(Arc<LoopGuests>);

impl shards_api::Guests for Shared {
    fn backend(&self) -> Result<shards_api::Backend, shards_api::Refusal> {
        shards_api::backend()
    }

    fn boot(
        &self,
        _spec: &shards_api::GuestSpec,
    ) -> Result<Box<dyn shards_api::Guest>, shards_api::Refusal> {
        let ended = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let conns = Arc::new(Mutex::new(Vec::new()));
        let (control, stdio) = relay(Arc::clone(&self.0.0), Arc::clone(&conns));
        self.0.1.lock().unwrap().push((Arc::clone(&ended), conns));
        Ok(Box::new(LoopGuest(control, stdio, ended)))
    }

    fn attach(
        &self,
        identity: &shards_api::Identity,
        _spec: &shards_api::GuestSpec,
    ) -> Result<Box<dyn shards_api::Guest>, shards_api::Refusal> {
        Err(shards_api::Refusal::Gone {
            pid: identity.pid,
            why: "a loopback stand-in keeps no guest".into(),
        })
    }
}

/// The host's own boxes: the simulator's, with no processes to take back.
struct Host(hex_api::sim::SimRuntime);

impl Runtime for Host {
    fn classes(&self) -> Vec<hex_api::Class> {
        vec![hex_api::Class::Process]
    }
    fn create(&self, spec: BoxSpec) -> Result<hex_api::BoxInfo, Refusal> {
        self.0.create(spec)
    }
    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.0.start(id)
    }
    fn exec(
        &self,
        id: &BoxId,
        program: hex_api::Program,
        stdio: hex_api::Stdio,
    ) -> Result<Box<dyn hex_api::Proc>, Refusal> {
        self.0.exec(id, program, stdio)
    }
    fn run(
        &self,
        spec: BoxSpec,
        program: hex_api::Program,
        stdio: hex_api::Stdio,
    ) -> Result<Box<dyn hex_api::Proc>, Refusal> {
        self.0.run(spec, program, stdio)
    }
    fn signal(&self, id: &BoxId, signal: hex_api::Signal) -> Result<(), Refusal> {
        self.0.signal(id, signal)
    }
    fn inspect(&self, id: &BoxId) -> Result<hex_api::BoxInfo, Refusal> {
        self.0.inspect(id)
    }
    fn usage(&self, id: &BoxId) -> Result<hex_api::Usage, Refusal> {
        self.0.usage(id)
    }
    fn list(&self) -> Result<Vec<hex_api::BoxInfo>, Refusal> {
        self.0.list()
    }
    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        self.0.delete(id)
    }
}

impl Recover for Host {
    fn trace(&self, _id: &BoxId) -> Result<Option<ProcId>, Refusal> {
        Ok(None)
    }
    fn adopt(&self, _spec: &BoxSpec, _trace: Option<ProcId>) -> Result<(), Refusal> {
        Ok(())
    }
    fn end(&self, _trace: Option<ProcId>) -> Result<(), Refusal> {
        Ok(())
    }
}

/// With one worker: client A's inspect of a box in the guest is held by the guest while client B's inspect
/// of host box `here` is answered; then A's is answered once the guest lets it go.
fn held_holds_no_other(socket: &std::path::Path, gate: &Gate, here: &BoxId) {
    gate.hold(true);
    let a = hex_client::Client::connect(socket).unwrap();
    let held = std::thread::spawn(move || a.inspect(&BoxId::new("there").unwrap()).err());
    gate.wait_holding(1);
    let b = hex_client::Client::connect(socket).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let here = here.clone();
    let other = std::thread::spawn(move || tx.send(b.inspect(&here).is_ok()));
    let answered = rx.recv_timeout(PATIENCE);
    gate.hold(false);
    let _ = other.join();
    let held = held.join().unwrap();
    assert!(
        answered == Ok(true),
        "B's inspect of a host box waited on the guest's answer to A's: {answered:?}"
    );
    assert_eq!(held, Some(Refusal::NoSuchBox(BoxId::new("there").unwrap())));
}

fn config() -> GuestConfig {
    GuestConfig {
        binary: PathBuf::from("/shards"),
        kernel: PathBuf::from("/k"),
        initrd: PathBuf::from("/i"),
        vcpus: 1,
        memory_mib: shards_api::min_memory_mib(std::env::consts::ARCH),
        shares: Vec::new(),
        staged: "/staged".to_owned(),
    }
}

/// Native plan step 8 (1), HEX-SOCK-5: with one worker, client A's inspect of a box its guest is slow to
/// answer holds no worker, so client B's inspect of a host box is answered while the guest holds A's; A's is
/// answered (NoSuchBox, the stand-in's) once the guest lets it go.
#[test]
fn a_call_its_guest_holds_holds_no_other_clients_call_through_the_guest_client() {
    let _ = held_holds_no_other;
    if shards_api::backend().is_err() {
        eprintln!("SKIP: Shards has no backend on this host, so no guest boots");
        return;
    }
    let dir = tempfile::Builder::new().prefix("gl-").tempdir().unwrap();
    let gate = Arc::new(Gate::default());
    let host = Host(hex_api::sim::SimRuntime::new());
    let here = BoxId::new("here").unwrap();
    host.create(BoxSpec {
        id: here.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Process,
        owner: None,
    })
    .unwrap();
    let node = WithGuest::new(
        host,
        GuestRuntime::new(
            Arc::new(Shared(Arc::new(LoopGuests::new(Arc::clone(&gate))))),
            config(),
        ),
    )
    .unwrap();
    let node = Recorded::open(node, &dir.path().join("state")).unwrap();
    let socket = dir.path().join("s");
    let _server = Server::bind(&socket, Arc::new(node), limits(), 1).unwrap();
    gate.hold(true);
    let a = hex_client::Client::connect(&socket).unwrap();
    let held = std::thread::spawn(move || a.inspect(&BoxId::new("there").unwrap()).err());
    gate.wait_holding(1);
    let b = hex_client::Client::connect(&socket).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let other = std::thread::spawn(move || tx.send(b.inspect(&here).is_ok()));
    let answered = rx.recv_timeout(PATIENCE);
    gate.hold(false);
    let _ = other.join();
    let held = held.join().unwrap();
    assert!(
        answered == Ok(true),
        "B's inspect of a host box waited on the guest's answer to A's: {answered:?}"
    );
    assert_eq!(held, Some(Refusal::NoSuchBox(BoxId::new("there").unwrap())));
}

/// Native plan step 8 (1): a guest that ends is booted again on the node's next create, and the new guest's
/// control connection joins the core's loop as the first's did: with one worker, a call the new guest holds
/// holds no other client's call either.
#[test]
fn a_guest_booted_again_answers_on_the_loop_too() {
    if shards_api::backend().is_err() {
        eprintln!("SKIP: Shards has no backend on this host, so no guest boots");
        return;
    }
    let dir = tempfile::Builder::new().prefix("gl-").tempdir().unwrap();
    let gate = Arc::new(Gate::default());
    let guests = Arc::new(LoopGuests::new(Arc::clone(&gate)));
    let host = Host(hex_api::sim::SimRuntime::new());
    let here = BoxId::new("here").unwrap();
    host.create(BoxSpec {
        id: here.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Process,
        owner: None,
    })
    .unwrap();
    let node = WithGuest::new(
        host,
        GuestRuntime::new(Arc::new(Shared(Arc::clone(&guests))), config()),
    )
    .unwrap();
    let node = Recorded::open(node, &dir.path().join("state")).unwrap();
    let socket = dir.path().join("s");
    let _server = Server::bind(&socket, Arc::new(node), limits(), 1).unwrap();
    held_holds_no_other(&socket, &gate, &here);
    // The guest ends; the next create boots another (and is then refused by the stand-in, which stages
    // nothing: the boot is what the test needs). Its root is a tarball, which every OS's node takes as a Linux
    // root (Windows refuses a directory as one, by name, before anything boots).
    guests.end_all();
    let client = hex_client::Client::connect(&socket).unwrap();
    let rootfs = dir.path().join("root.tar");
    std::fs::File::create(&rootfs).unwrap();
    let _ = client.create(BoxSpec {
        id: BoxId::new("anew").unwrap(),
        bounds: Bounds::default(),
        class: ClassSpec::Oci(Box::new(hex_api::OciSpec {
            rootfs: hex_api::Rootfs::Dir(rootfs),
            init: hex_api::Program::of(&["/bin/true"]),
            mounts: Vec::new(),
            hostname: None,
            network: hex_api::Network::Private,
            hosts: Vec::new(),
        })),
        owner: None,
    });
    drop(client);
    assert_eq!(guests.boots(), 2, "the ended guest wasn't booted again");
    held_holds_no_other(&socket, &gate, &here);
}

/// A node's socket over `Recorded<WithGuest<…>>`, its guest the stand-in behind `gate`: the test's folder, the
/// socket's path and its server.
#[cfg(unix)]
fn node(gate: &Arc<Gate>) -> (tempfile::TempDir, PathBuf, Server) {
    let dir = tempfile::Builder::new().prefix("gl-").tempdir().unwrap();
    let node = WithGuest::new(
        Host(hex_api::sim::SimRuntime::new()),
        GuestRuntime::new(
            Arc::new(Shared(Arc::new(LoopGuests::new(Arc::clone(gate))))),
            config(),
        ),
    )
    .unwrap();
    let node = Recorded::open(node, &dir.path().join("state")).unwrap();
    let socket = dir.path().join("s");
    let server = Server::bind(&socket, Arc::new(node), limits(), 4).unwrap();
    (dir, socket, server)
}

/// A box the stand-in guest holds: none of the host's.
#[cfg(unix)]
fn there() -> BoxId {
    BoxId::new("there").unwrap()
}

/// HEX-GUEST-41, HEX-RT-17 for a box in the node's guest: through the node's socket, exec a guest program that
/// echoes, asking for its stdin and stdout: expect their ends to be the guest's own stdio connections, what is
/// written to the stdin end read back from the stdout end to its end, and the program's end as the guest sent it,
/// its pid naming no host process.
#[cfg(unix)]
#[test]
fn a_guest_programs_asked_streams_are_the_guests_own_connections_through_the_socket() {
    if shards_api::backend().is_err() {
        eprintln!("SKIP: Shards has no backend on this host, so no guest boots");
        return;
    }
    let gate = Arc::new(Gate::default());
    let (_dir, socket, _server) = node(&gate);
    let client = hex_client::Client::connect(&socket).unwrap();
    let mut p = client
        .exec(
            &there(),
            Program::of(&["cat"]),
            Stdio {
                ask: Ask {
                    stdin: true,
                    stdout: true,
                    stderr: false,
                },
                ..Stdio::null()
            },
        )
        .unwrap();
    assert_eq!(p.pid(), GUEST_PID);
    let given = p.streams().unwrap();
    let (Some(stdin), Some(stdout)) = (given.stdin, given.stdout) else {
        panic!("the streams asked for weren't given");
    };
    let mut stdin = std::fs::File::from(stdin);
    stdin.write_all(b"round trip\n").unwrap();
    drop(stdin);
    let mut out = String::new();
    std::fs::File::from(stdout)
        .read_to_string(&mut out)
        .unwrap();
    assert_eq!(out, "round trip\n");
    assert_eq!(p.wait_for(PATIENCE).unwrap(), Some(Exit::Code(0)));
    assert_eq!(gate.execs.load(Ordering::SeqCst), 1);
}

/// HEX-GUEST-41, HEX-RT-9 for a box in the node's guest: through the node's socket, exec a guest program that
/// echoes, its stdin and stdout the caller's own pipes: expect what the caller writes to its stdin pipe read back
/// from its stdout pipe to its end (its client copies between them and the guest's connections), and the
/// program's end as the guest sent it.
#[cfg(unix)]
#[test]
fn a_guest_programs_streams_the_callers_own_pipes_carry_through_the_socket() {
    if shards_api::backend().is_err() {
        eprintln!("SKIP: Shards has no backend on this host, so no guest boots");
        return;
    }
    let gate = Arc::new(Gate::default());
    let (_dir, socket, _server) = node(&gate);
    let client = hex_client::Client::connect(&socket).unwrap();
    let (in_r, mut in_w) = std::io::pipe().unwrap();
    let (mut out_r, out_w) = std::io::pipe().unwrap();
    let mut p = client
        .exec(
            &there(),
            Program::of(&["cat"]),
            Stdio {
                stdin: Some(in_r),
                stdout: Some(out_w),
                ..Stdio::null()
            },
        )
        .unwrap();
    in_w.write_all(b"round trip\n").unwrap();
    drop(in_w);
    let mut out = String::new();
    out_r.read_to_string(&mut out).unwrap();
    assert_eq!(out, "round trip\n");
    assert_eq!(p.wait_for(PATIENCE).unwrap(), Some(Exit::Code(0)));
}

/// What the node's socket answers, as bytes on the wire: a client that writes and reads its messages itself.
#[cfg(target_os = "linux")]
mod raw {
    use super::*;
    use std::io::{IoSlice, IoSliceMut};
    use std::mem::MaybeUninit;
    use std::os::fd::{BorrowedFd, OwnedFd};

    use rustix::event::{PollFd, PollFlags, Timespec};
    use rustix::net::{
        AddressFamily, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
        SendAncillaryMessage, SendFlags, SocketAddrUnix, SocketFlags, SocketType,
    };

    pub struct Raw(OwnedFd);

    impl Raw {
        pub fn connect(path: &std::path::Path) -> Self {
            let fd = rustix::net::socket_with(
                AddressFamily::UNIX,
                SocketType::SEQPACKET,
                SocketFlags::CLOEXEC,
                None,
            )
            .unwrap();
            rustix::net::connect(&fd, &SocketAddrUnix::new(path).unwrap()).unwrap();
            Self(fd)
        }

        /// Sends `request` as request `id`, `fds` riding with it.
        pub fn send(&self, id: u64, request: &wire::Request, fds: &[BorrowedFd<'_>]) {
            let bytes = wire::request(id, request, wire::MAX_MESSAGE).unwrap();
            let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
            let mut control = SendAncillaryBuffer::new(&mut space);
            if !fds.is_empty() {
                assert!(control.push(SendAncillaryMessage::ScmRights(fds)));
            }
            rustix::net::sendmsg(
                &self.0,
                &[IoSlice::new(&bytes)],
                &mut control,
                SendFlags::NOSIGNAL,
            )
            .unwrap();
        }

        /// The next answer and the descriptors that came with it, waiting at most [`PATIENCE`] for it.
        pub fn answer(&self) -> (wire::Header, wire::Answer, Vec<OwnedFd>) {
            let mut ready = [PollFd::new(&self.0, PollFlags::IN)];
            let ts = Timespec {
                tv_sec: i64::try_from(PATIENCE.as_secs()).unwrap(),
                tv_nsec: 0,
            };
            assert_eq!(
                rustix::event::poll(&mut ready, Some(&ts)).unwrap(),
                1,
                "no answer came"
            );
            let mut buf = vec![0u8; wire::MAX_MESSAGE];
            let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(wire::MAX_FDS))];
            let mut control = RecvAncillaryBuffer::new(&mut space);
            let msg = rustix::net::recvmsg(
                &self.0,
                &mut [IoSliceMut::new(&mut buf)],
                &mut control,
                RecvFlags::CMSG_CLOEXEC,
            )
            .unwrap();
            let mut fds = Vec::new();
            for m in control.drain() {
                if let RecvAncillaryMessage::ScmRights(came) = m {
                    fds.extend(came);
                }
            }
            let received = wire::Received {
                bytes: &buf[..msg.bytes],
                size: msg.bytes,
                descriptors: fds.len(),
                descriptors_cut: false,
            };
            let (h, answer) = wire::read_answer(&received, wire::MAX_MESSAGE).unwrap();
            (h, answer, fds)
        }

        /// Says hello, as a client's first message.
        pub fn hello(&self) {
            let hello = wire::Request::Hello {
                build: wire::build(),
            };
            self.send(1, &hello, &[]);
            let (_, hello, _) = self.answer();
            assert!(matches!(hello, wire::Answer::Hello(_)), "{hello:?}");
        }
    }
}

/// An exec of the stand-in's echo in box [`there`], its stdin and stdout carried as the caller's own pipes.
#[cfg(target_os = "linux")]
fn carried_exec() -> wire::Request {
    wire::Request::Call(wire::Call::Exec {
        id: there(),
        program: Program::of(&["cat"]),
        streams: wire::Streams {
            stdin: true,
            stdout: true,
            stderr: false,
        },
    })
}

/// HEX-GUEST-41 on the wire: exec a guest program, its stdin and stdout the caller's own pipes carried as
/// descriptors: expect the node to answer with the guest's own stdio connections' ends (`Given`), for the caller's
/// client to copy between, and to keep none of the caller's pipes (with this test's own ends of them gone, its stdin
/// pipe has no reader and its stdout pipe no writer); once the caller says it took the ends, what goes in the stdin
/// end comes back on the stdout end, to its end, and the program's end follows.
#[cfg(target_os = "linux")]
#[test]
fn the_node_gives_a_guest_programs_streams_to_the_callers_client_and_keeps_none_of_its_pipes() {
    use std::os::fd::AsFd;
    if shards_api::backend().is_err() {
        eprintln!("SKIP: Shards has no backend on this host, so no guest boots");
        return;
    }
    let gate = Arc::new(Gate::default());
    let (_dir, socket, _server) = node(&gate);
    let raw = raw::Raw::connect(&socket);
    raw.hello();
    let (in_r, mut in_w) = std::io::pipe().unwrap();
    let (mut out_r, out_w) = std::io::pipe().unwrap();
    raw.send(2, &carried_exec(), &[in_r.as_fd(), out_w.as_fd()]);
    drop((in_r, out_w));
    let (h, answered, ends) = raw.answer();
    assert_eq!(h.id, 2);
    let wire::Answer::Given { pid, given } = answered else {
        panic!("the node answered {answered:?}, not with the guest's streams' ends");
    };
    assert_eq!((pid, given), (GUEST_PID, carried_streams()));
    assert_eq!(ends.len(), 2);
    // The node keeps its copies of the ends until it hears they came, as a client says first.
    raw.send(2, &wire::Request::Taken, &[]);
    assert_eq!(
        in_w.write(b"x").map_err(|e| e.kind()),
        Err(std::io::ErrorKind::BrokenPipe),
        "the node kept the caller's stdin pipe"
    );
    assert_eq!(
        out_r.read(&mut [0u8; 1]).unwrap(),
        0,
        "the node kept the caller's stdout pipe"
    );
    let mut ends = ends.into_iter();
    let (stdin, stdout) = (ends.next().unwrap(), ends.next().unwrap());
    let mut stdin = std::fs::File::from(stdin);
    stdin.write_all(b"round trip\n").unwrap();
    drop(stdin);
    let mut out = String::new();
    std::fs::File::from(stdout)
        .read_to_string(&mut out)
        .unwrap();
    assert_eq!(out, "round trip\n");
    let (h, end, _) = raw.answer();
    assert_eq!((h.id, end), (2, wire::Answer::Exited(Exit::Code(0))));
}

#[cfg(target_os = "linux")]
fn carried_streams() -> wire::Streams {
    wire::Streams {
        stdin: true,
        stdout: true,
        stderr: false,
    }
}
