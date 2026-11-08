//! Native plan step 8 (1), HEX-SOCK-5 through the real pieces: a node's socket over `Recorded<WithGuest<…>>`
//! with one worker, its guest a loopback stand-in for a Shards guest's relay whose answer to an inspect is
//! held until the test lets it go. Client A's inspect of a box in the guest waits on the guest; client B's
//! inspect of a host box is answered meanwhile, as no call the guest holds holds a worker.
#![cfg(any(target_os = "linux", target_os = "macos"))]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Refusal, Runtime};
use hex_core::record::ProcId;
use hex_core::{stream, wire};
use hex_node::recorded::{Recorded, Recover};
use hex_node::serve::{LIMITS, Server};
use hex_runtime::guest::{GuestConfig, GuestRuntime, WithGuest};

/// Longest a test waits for a fact it needs: a bound, so a node that never makes it true fails the test
/// instead of hanging it.
const PATIENCE: Duration = Duration::from_secs(30);

/// Whether the stand-in guest holds its answers to inspects, and how many it holds.
#[derive(Default)]
struct Gate {
    state: Mutex<(bool, usize)>,
    changed: Condvar,
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

/// A guest's relay and node agent, on loopback: the connection's token, a hello's answer, and every
/// inspect answered `NoSuchBox` once the gate lets it pass. Each dial is served on a thread of its own.
fn relay(gate: Arc<Gate>) -> std::net::SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(conn) = conn else { return };
            let gate = Arc::clone(&gate);
            std::thread::spawn(move || serve(conn, &gate));
        }
    });
    addr
}

fn serve(mut conn: TcpStream, gate: &Gate) {
    conn.write_all(&7u64.to_le_bytes()).unwrap();
    let mut deframer = stream::Deframer::new(wire::MAX_MESSAGE);
    let mut buf = [0u8; 4096];
    loop {
        let n = match conn.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        let mut input = &buf[..n];
        while !input.is_empty() {
            let (took, whole) = deframer.take(input).unwrap();
            input = &input[took..];
            let Some(message) = whole else { continue };
            let (h, request) = wire::read_request_apart(&message, wire::MAX_MESSAGE).unwrap();
            let answer = match request {
                wire::Request::Hello => wire::Answer::Hello(wire::Hello {
                    version: wire::VERSION,
                    classes: vec![hex_api::Class::Oci],
                    limits: wire::Limits {
                        connections: 4,
                        message_bytes: wire::MAX_MESSAGE,
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
                other => wire::Answer::Refused(Refusal::Broken(format!(
                    "the stand-in guest serves no {other:?}"
                ))),
            };
            let version = if h.kind == 0x0001 {
                wire::FIRST
            } else {
                h.version
            };
            let bytes = wire::answer(version, h.id, &answer, wire::MAX_MESSAGE).unwrap();
            conn.write_all(&stream::frame(&bytes, wire::MAX_MESSAGE).unwrap())
                .unwrap();
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

    fn handle(&self) -> shards_api::Handle {
        std::os::fd::AsRawFd::as_raw_fd(&self.0)
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
}

/// A guest whose every port is the stand-in relay.
struct LoopGuest(std::net::SocketAddr);

impl shards_api::Guest for LoopGuest {
    fn connect(
        &self,
        _port: u32,
        _patience: Duration,
    ) -> Result<Box<dyn shards_api::Stream>, shards_api::Refusal> {
        Ok(Box::new(Looped(TcpStream::connect(self.0).unwrap())))
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

struct LoopGuests(std::net::SocketAddr);

impl shards_api::Guests for LoopGuests {
    fn backend(&self) -> Result<shards_api::Backend, shards_api::Refusal> {
        shards_api::backend()
    }

    fn boot(
        &self,
        _spec: &shards_api::GuestSpec,
    ) -> Result<Box<dyn shards_api::Guest>, shards_api::Refusal> {
        Ok(Box::new(LoopGuest(self.0)))
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
    if shards_api::backend().is_err() {
        eprintln!("SKIP: Shards has no backend on this host, so no guest boots");
        return;
    }
    let dir = tempfile::Builder::new().prefix("gl-").tempdir().unwrap();
    let gate = Arc::new(Gate::default());
    let addr = relay(Arc::clone(&gate));
    let host = Host(hex_api::sim::SimRuntime::new());
    let here = BoxId::new("here").unwrap();
    host.create(BoxSpec {
        id: here.clone(),
        bounds: Bounds::default(),
        class: ClassSpec::Process,
    })
    .unwrap();
    let node = WithGuest::new(
        host,
        GuestRuntime::new(Arc::new(LoopGuests(addr)), config()),
    )
    .unwrap();
    let node = Recorded::open(node, &dir.path().join("state")).unwrap();
    let socket = dir.path().join("s");
    let _server = Server::bind(&socket, Arc::new(node), LIMITS, 1).unwrap();
    gate.hold(true);
    let a = hex_client::Client::connect(&socket).unwrap();
    let held = std::thread::spawn(move || a.inspect(&BoxId::new("there").unwrap()));
    gate.wait_holding(1);
    let b = hex_client::Client::connect(&socket).unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let other = std::thread::spawn(move || tx.send(b.inspect(&here)));
    let answered = rx.recv_timeout(PATIENCE);
    gate.hold(false);
    let _ = other.join();
    let held = held.join().unwrap();
    assert!(
        matches!(answered, Ok(Ok(_))),
        "B's inspect of a host box waited on the guest's answer to A's: {answered:?}"
    );
    assert_eq!(held, Err(Refusal::NoSuchBox(BoxId::new("there").unwrap())));
}
