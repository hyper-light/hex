//! The Link through a node VM's run's endpoint (joint plan step 39; Shards D251, D252), on a real agent-control
//! endpoint on loopback whose run host a test thread plays: each lane's ask connected to the relay's listener in a
//! test directory, as shards-init's relay connects one in `/run/hex/channel`; Hex's side the Link over it. On every
//! OS (the review's D-4): on Unix the listener is a real AF_UNIX one; on Windows the run host's end of each lane is
//! the plain pipe pair Shards' run host makes there, the relay holding the VM's end ([`lane_to`]).

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread::JoinHandle;

use hex_api::{ConnectFailure, ConnectPhase, IoKind, Refusal};
use hex_core::stream;
use shards_api::control::{Identity, SocketAdmission};
use shards_remote::control::unix::{Connect, Reply};
use shards_remote::control::{Endpoint, Revoke, Streams};
use shards_testdir::TempDir;

use super::*;

/// What the run admits: `lanes` at once, each with the guest wire's largest frame in flight each way.
fn admission(lanes: u32) -> SocketAdmission {
    SocketAdmission {
        lanes,
        window: u32::try_from(stream::MAX_FRAME).unwrap(),
    }
}

/// A node VM's run as these cases need it: its endpoint on loopback, which this node's key alone reaches, serving
/// lanes to the sockets in `dir` as `admitted`; its run host, a thread answering each ask; and the Link through it.
struct Run {
    link: Lanes,
    /// Each name its host was asked to connect, once the endpoint has ended.
    host: JoinHandle<Vec<String>>,
    revoke: Revoke,
    _dir: TempDir,
}

impl Run {
    fn new(dir: TempDir, admitted: SocketAdmission) -> Run {
        let (vm, node) = (Identity::generate().unwrap(), Identity::generate().unwrap());
        let (endpoint, served) = Endpoint::bind(
            &vm,
            node.fingerprint(),
            SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            Streams {
                outputs: &[],
                inputs: &[],
                execs: None,
                sockets: Some(admitted),
            },
        )
        .unwrap();
        let at = endpoint.address;
        std::thread::spawn(move || endpoint.run());
        let host = host(dir.path().to_owned(), served.sockets.unwrap());
        let link = Lanes::dial(&node, vm.fingerprint(), at, admitted).unwrap();
        Run {
            link,
            host,
            revoke: served.revoke,
            _dir: dir,
        }
    }

    /// The names its host was asked for, once the Link and the endpoint have ended.
    fn asked(self) -> Vec<String> {
        drop(self.link);
        drop(self.revoke);
        self.host.join().unwrap()
    }
}

/// The run host as shards-init's relay plays it: each ask's name connected in `dir` ([`lane_to`]), answered with its
/// end or its connect's errno as the guest's Linux numbers it ([`linux_errno`]), until the endpoint ends.
fn host(dir: PathBuf, asks: mpsc::Receiver<Connect>) -> JoinHandle<Vec<String>> {
    std::thread::spawn(move || {
        let mut asked = Vec::new();
        for ask in asks {
            asked.push(ask.name().to_owned());
            let reply = match lane_to(&dir.join(ask.name())) {
                Ok(socket) => Reply::Connected(socket),
                Err(e) => Reply::Unreached(linux_errno(&e)),
            };
            let _ = ask.reply(reply);
        }
        asked
    })
}

/// The errno shards-init's relay reports for a connect that failed so: Linux's, whatever this test's host, since the
/// run host is the guest's (`include/uapi/asm-generic/errno-base.h`, `errno.h`).
fn linux_errno(e: &std::io::Error) -> NonZeroU16 {
    let errno = match e.kind() {
        std::io::ErrorKind::NotFound => 2,
        std::io::ErrorKind::PermissionDenied => 13,
        std::io::ErrorKind::ConnectionRefused => 111,
        _ => panic!("a connect failed as no case here expects: {e}"),
    };
    NonZeroU16::new(errno).unwrap()
}

fn socket_dir() -> TempDir {
    TempDir::new("lanes").unwrap()
}

/// The relay's listener for `port`'s socket in `dir`, listening once this returns.
fn listen(dir: &Path, port: u32) -> Listener {
    Listener::bind(&dir.join(stream::socket_name(port).unwrap()))
}

/// A relay's listener in a run's socket directory: on Unix an AF_UNIX listener, std's, as shards-init's relay listens.
#[cfg(unix)]
struct Listener(std::os::unix::net::UnixListener);

/// A relay's connection: on Unix the socket itself.
#[cfg(unix)]
type Relayed = std::os::unix::net::UnixStream;

#[cfg(unix)]
impl Listener {
    fn bind(path: &Path) -> Listener {
        Listener(std::os::unix::net::UnixListener::bind(path).unwrap())
    }

    /// Its next connection, waited for.
    fn accept(&self) -> Relayed {
        self.0.accept().unwrap().0
    }
}

/// The run host's end of a lane to the relay's socket at `path` (D251): on Unix the socket itself, as shards-init's
/// relay host connects it.
#[cfg(unix)]
fn lane_to(path: &Path) -> std::io::Result<Relayed> {
    std::os::unix::net::UnixStream::connect(path)
}

/// A relay's listener on Windows: the lanes the run host hands it ([`lane_to`]), each the VM's end of the plain pipe
/// pair Shards' run host makes there (`crates/shards/src/api_run.rs`'s `open_lane` and `handed_pair`, at 097ec0047),
/// whose ends each say the end of their writes as a socket's half-close does. The guest's AF_UNIX socket is a Linux
/// one, which no Windows host carries, and afunix.sys, which would stand in for it, now and then misses waking a read
/// with its peer's end (HEX-SOCK-3, beaf22ee). Dropped, a lane to its path is refused, as a connect to a socket whose
/// listener closed is.
#[cfg(windows)]
struct Listener(mpsc::Receiver<Relayed>);

/// A relay's connection: on Windows the VM's end of a lane's plain pipe pair.
#[cfg(windows)]
type Relayed = shards_ipc::Stream;

/// The relays listening in the runs' socket directories, by their sockets' paths: the guests' directories, as one map
/// of this process's on Windows.
#[cfg(windows)]
static RELAYS: std::sync::Mutex<std::collections::BTreeMap<PathBuf, mpsc::Sender<Relayed>>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

#[cfg(windows)]
impl Listener {
    fn bind(path: &Path) -> Listener {
        let (listening, lanes) = mpsc::channel();
        RELAYS.lock().unwrap().insert(path.to_owned(), listening);
        Listener(lanes)
    }

    /// Its next connection, waited for.
    fn accept(&self) -> Relayed {
        self.0.recv().unwrap()
    }
}

/// The run host's end of a lane to the relay's socket at `path` (D251), the VM's end handed to that relay: a path no
/// relay listens at is not found, and one whose listener has gone refused, as a connect to either would be.
#[cfg(windows)]
fn lane_to(path: &Path) -> std::io::Result<Relayed> {
    let relays = RELAYS.lock().unwrap();
    let relay = relays.get(path).ok_or(std::io::ErrorKind::NotFound)?;
    let (ours, theirs) = shards_ipc::Stream::plain_pair()?;
    relay
        .send(theirs)
        .map_err(|_| std::io::ErrorKind::ConnectionRefused)?;
    Ok(ours)
}

/// The relay as far as these cases need it: reads its connection to its end, writes back what it read reversed,
/// and ends its own half.
fn echo_reversed(listener: Listener) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut conn = listener.accept();
        let mut got = Vec::new();
        conn.read_to_end(&mut got).unwrap();
        let back: Vec<u8> = got.iter().rev().copied().collect();
        conn.write_all(&back).unwrap();
        conn.shutdown(Shutdown::Write).unwrap();
        got
    })
}

/// Every byte value, past a lane's window three times so its credit turns over, ending short of a whole frame.
fn pattern() -> Vec<u8> {
    (0..=255u8)
        .cycle()
        .take(3 * stream::MAX_FRAME + 1)
        .collect()
}

/// D251 through Hex's Link: a port dialed through its node VM's endpoint is the relay's socket that port names, byte
/// for byte both ways, each way's end its half-close, the other way still open. Do: dial the control port, write a
/// pattern past the lane's window, half-close, and read the relay's answer to its end. Expect: the relay read the
/// pattern exactly, and Hex read it back reversed exactly; the run host was asked for `control.sock` alone.
#[test]
fn a_port_dialed_through_its_runs_endpoint_is_its_socket_exactly_both_ways_each_half_closed() {
    let dir = socket_dir();
    let relay = echo_reversed(listen(dir.path(), stream::CONTROL_PORT));
    let run = Run::new(dir, admission(4));
    assert!(run.link.carries_half_close());
    let mut conn = run.link.dial(stream::CONTROL_PORT).unwrap();
    let sent = pattern();
    conn.write_all(&sent).unwrap();
    conn.shutdown(Shutdown::Write).unwrap();
    let mut back = Vec::new();
    conn.read_to_end(&mut back).unwrap();
    assert_eq!(
        relay.join().unwrap(),
        sent,
        "the relay read Hex's bytes exactly"
    );
    let expected: Vec<u8> = sent.iter().rev().copied().collect();
    assert_eq!(back, expected, "Hex read the relay's bytes exactly");
    drop(conn);
    assert_eq!(run.asked(), ["control.sock"]);
}

/// D251: a lane to a socket its run host can't connect is the guest's errno, typed, Linux's whatever the host. Do:
/// dial the stdio port, whose socket isn't there, and the staging port, whose socket's listener is gone. Expect: each
/// refused as a connect failure at its dial, `ENOENT` (2) as not found and `ECONNREFUSED` (111) as refused.
#[test]
fn a_port_whose_socket_its_run_host_cant_connect_is_refused_by_the_guests_errno() {
    let dir = socket_dir();
    drop(listen(dir.path(), stream::files::FILES_PORT));
    let run = Run::new(dir, admission(4));
    let failed = |port, kind, code| {
        Err(Refusal::Connect {
            port,
            failure: ConnectFailure::Io {
                phase: ConnectPhase::Dial,
                kind: IoKind::new(kind),
                code: Some(code),
            },
        })
    };
    assert_eq!(
        run.link.dial(stream::STDIO_PORT).map(drop),
        failed(stream::STDIO_PORT, std::io::ErrorKind::NotFound, 2)
    );
    assert_eq!(
        run.link.dial(stream::files::FILES_PORT).map(drop),
        failed(
            stream::files::FILES_PORT,
            std::io::ErrorKind::ConnectionRefused,
            111
        )
    );
}

/// D251's admission: a lane past the run's most open at once is refused `Capacity`, never queued, which Hex reads as
/// its own exhaustion, naming the bound. Do: with one lane admitted, hold one open, then dial again. Expect: the second
/// refused `Exhausted` with a bound of 1.
#[test]
fn a_dial_past_the_lanes_its_run_admits_is_exhausted_naming_them() {
    let dir = socket_dir();
    let listener = listen(dir.path(), stream::CONTROL_PORT);
    let run = Run::new(dir, admission(1));
    let held = run.link.dial(stream::CONTROL_PORT).unwrap();
    let _accepted = listener.accept();
    assert_eq!(
        run.link.dial(stream::CONTROL_PORT).map(drop),
        Err(Refusal::Exhausted {
            what: "socket lanes to the node VM's relay",
            bound: 1,
        })
    );
    drop(held);
}

/// A port the relay listens on no socket for is the caller's error, refused before any lane is asked for. Do: dial
/// the readiness port, which only a Firecracker guest's init speaks. Expect: refused invalid; the run host asked for
/// nothing.
#[test]
fn a_port_the_relay_has_no_socket_for_is_refused_before_a_lane() {
    let run = Run::new(socket_dir(), admission(4));
    assert!(matches!(
        run.link.dial(stream::GUEST_READY_PORT).map(drop),
        Err(Refusal::Invalid { .. })
    ));
    assert!(
        run.asked().is_empty(),
        "its run host was asked for a socket"
    );
}

/// D251: the relay's socket closed while Hex still sends is the lane's reset, never its end in order, and Hex reads
/// the bytes that came before it, then the cut. Do: the relay writes a line, reads a byte of what Hex sends, and closes;
/// Hex sends past the lane's window and reads. Expect: the line, then an error that is a reset, not `Ok(0)`. On Linux
/// and macOS alike: this case's stand-in run host runs Shards' unix-lane pump on the test's own host, which told the
/// socket's end as the lane's end in order before its writer failed (4 of 4 on the Mac Studio, once under the whole
/// client suite on Linux) until Shards 8754b451f, vendored with Shards 3114e082f.
#[test]
fn a_lane_its_socket_cuts_short_reads_its_bytes_then_the_cut() {
    let dir = socket_dir();
    let listener = listen(dir.path(), stream::CONTROL_PORT);
    let relay = std::thread::spawn(move || {
        let mut conn = listener.accept();
        conn.write_all(b"before the cut\n").unwrap();
        let mut one = [0u8];
        conn.read_exact(&mut one).unwrap();
    });
    let run = Run::new(dir, admission(4));
    let mut conn = run.link.dial(stream::CONTROL_PORT).unwrap();
    let mut writer = conn.try_clone().unwrap();
    let sending = std::thread::spawn(move || {
        let sent = pattern();
        // Past the relay's close the lane is reset, and so this write fails: the cut is the read's to tell.
        let _ = writer.write_all(&sent);
    });
    let mut got = Vec::new();
    let end = conn.read_to_end(&mut got);
    relay.join().unwrap();
    sending.join().unwrap();
    assert_eq!(got, b"before the cut\n", "the bytes before the cut");
    let cut = end.expect_err("a cut read as the lane's end in order");
    assert_eq!(cut.kind(), std::io::ErrorKind::ConnectionReset, "{cut}");
}

/// The review's D-4 (D251; AC-T7.4.1), on every OS: a lane its socket cuts short while Hex still sends, Hex reading
/// only once a write of its has failed. Do: the relay writes a line, reads a byte of what Hex sends, and closes; Hex
/// writes a lane's window at a time until a write fails, then reads. Expect the line, then the cut, a reset: no byte
/// that came before the cut lost to how the bridge ends Hex's socket. (On Windows that socket is one end of a loopback
/// TCP pair, where a shutdown of receiving resets the connection if bytes wait or come, Winsock's documented
/// contract for `shutdown(SD_RECEIVE)`.)
#[test]
fn a_lane_cut_while_its_owner_sends_keeps_the_bytes_before_the_cut_for_a_later_read() {
    let dir = socket_dir();
    let listener = listen(dir.path(), stream::CONTROL_PORT);
    let relay = std::thread::spawn(move || {
        let mut conn = listener.accept();
        conn.write_all(b"before the cut\n").unwrap();
        let mut one = [0u8];
        conn.read_exact(&mut one).unwrap();
    });
    let run = Run::new(dir, admission(4));
    let mut conn = run.link.dial(stream::CONTROL_PORT).unwrap();
    let window = vec![0x5a; stream::MAX_FRAME];
    let refused = loop {
        if let Err(e) = conn.write_all(&window) {
            break e;
        }
    };
    relay.join().unwrap();
    let mut got = Vec::new();
    let end = conn.read_to_end(&mut got);
    eprintln!("d_4_cut_while_sending refused={refused:?} got={got:?} end={end:?}");
    assert_eq!(got, b"before the cut\n", "the bytes before the cut");
    let cut = end.expect_err("a cut read as the lane's end in order");
    assert_eq!(cut.kind(), std::io::ErrorKind::ConnectionReset, "{cut}");
}

/// The person's directive, and Shards' D239: a node's control link holds TLS13_AES_256_GCM_SHA384 at both ends. Do:
/// judge a link each of whose ends negotiated another suite in turn. Expect: each refused, naming the suite.
#[test]
fn a_link_either_end_of_which_took_another_suite_is_refused() {
    use shards_remote::tls::Negotiated;
    let held = Negotiated {
        suite: 0x1302,
        group: 0x11ec,
    };
    let other = Negotiated {
        suite: 0x1301,
        group: 0x11ec,
    };
    assert_eq!(
        tls(shards_api::control::Link {
            controller: held,
            endpoint: held,
        }),
        Ok(())
    );
    for link in [
        shards_api::control::Link {
            controller: other,
            endpoint: held,
        },
        shards_api::control::Link {
            controller: held,
            endpoint: other,
        },
    ] {
        match tls(link) {
            Err(Refusal::Invalid { why, .. }) => {
                assert!(why.contains("TLS13_AES_128_GCM_SHA256"), "{why}")
            }
            got => panic!("{link:?}: {got:?}"),
        }
    }
}
