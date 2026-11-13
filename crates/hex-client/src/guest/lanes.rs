//! The guest's ports through its run's agent-control endpoint (docs/design/node-under-shards-init.md §5): a node VM
//! under shards-init is reached by Shards' socket lanes (D251) to its relay's listeners in the run's socket directory,
//! [`DIR`], over the run's own endpoint (D252), whose controller is Shards' own (`shards_api::control`). [`Lanes`]
//! dials a port as a lane to the socket that port names ([`hex_core::stream::socket_name`]), bridged to a local
//! socket pair ([`super::bridge`]), so the client's event loop waits on an OS handle as it does on vsock's. The lane
//! carries a half-close each way, and its reset or its connection's end is told after its bytes.

use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr};
use std::sync::{Arc, mpsc};
use std::time::Instant;

use hex_api::{ConnectFailure, ConnectPhase, IoKind, Refusal};
use hex_core::lanes::DATA_BOUND;
use hex_core::stream;
use shards_api::control::{
    Controller, Dialed, Fault, Hangup, Identity, Reason, Socket, SocketAdmission, SocketReader,
    SocketWriter, Sockets, Subscriptions,
};

use super::bridge::{Bridged, Local, end, pair};
use super::{DuplexOwner, Link, own_duplex};
use crate::Cut;

/// The run's socket directory: where the node's relay listens under shards-init (hex-cli's guest init), and the one
/// directory the run grants its controller lanes to.
pub const DIR: &str = "/run/hex/channel";

/// The controller's connection to a node VM's endpoint and the lanes it opens on it to the node's relay.
pub struct Lanes {
    sockets: Arc<Sockets>,
    /// The connection: kept for the lanes' life, which end with it (dropped, it ends the connection and every lane).
    _hangup: Hangup,
    /// The most lanes open at once, as the run admits them: what a dial past it is refused naming.
    lanes: u32,
}

impl Lanes {
    /// Dials the endpoint at `at` of the node VM whose key's SHA-256 is `vm`, under this node's own key `own`, for
    /// socket lanes alone as `admission` admits them (its run's grant, D252), its controller's driver on a thread of
    /// its own; then waits for both ends' word on the connection's TLS ([`tls`]). That wait follows the connection:
    /// its handshake's end, the endpoint's refusal or the connection's end answers it (decision 9).
    pub fn dial(
        own: &Identity,
        vm: [u8; 32],
        at: SocketAddr,
        admission: SocketAdmission,
    ) -> Result<Lanes, Refusal> {
        let (controller, dialed) = Controller::dial(
            own,
            vm,
            at,
            Subscriptions {
                outputs: &[],
                inputs: &[],
                execs: None,
                sockets: Some(admission),
            },
        )
        .map_err(|e| Refusal::Os {
            op: DIALING_VM,
            why: e.to_string(),
        })?;
        let Dialed {
            sockets,
            mut link,
            hangup,
            ..
        } = dialed;
        std::thread::Builder::new()
            .name("hex-lanes-driver".to_owned())
            .spawn(move || {
                // Its end is the connection's, which every lane on it then reads as its fault.
                let _ = controller.run();
            })
            .map_err(|e| Refusal::os("starting the node VM's controller", e))?;
        let negotiated = link.wait().map_err(|fault| Refusal::Os {
            op: DIALING_VM,
            why: format!("its endpoint's connection: {fault:?}"),
        })?;
        tls(negotiated)?;
        let sockets = sockets.ok_or_else(|| {
            Refusal::Broken("a controller dialed for socket lanes was given none".to_owned())
        })?;
        Ok(Lanes {
            sockets: Arc::new(sockets),
            _hangup: hangup,
            lanes: admission.lanes,
        })
    }
}

impl Link for Lanes {
    fn dial(&self, port: u32) -> Result<DuplexOwner, Refusal> {
        self.dial_before(port, None)
    }

    /// A lane to the socket `port` names, bridged to a local socket. Its answer (connected, the endpoint's refusal, its
    /// run host's errno) is waited for by the connection's life, and within `deadline` where the caller gives one: past
    /// it the dial is refused, expired, and a lane that connects after it is let go, aborted.
    fn dial_before(&self, port: u32, deadline: Option<Instant>) -> Result<DuplexOwner, Refusal> {
        let name = named(port)?;
        let answer = match deadline {
            None => self.sockets.connect(name),
            Some(at) => {
                let left = at
                    .checked_duration_since(Instant::now())
                    .filter(|left| !left.is_zero())
                    .ok_or_else(|| expired(port))?;
                let (told, heard) = mpsc::channel();
                let sockets = Arc::clone(&self.sockets);
                std::thread::Builder::new()
                    .name("hex-lanes-dial".to_owned())
                    .spawn(move || {
                        // Past the deadline no one hears it, and the lane is let go with it.
                        let _ = told.send(sockets.connect(name));
                    })
                    .map_err(|e| Refusal::os("starting a lane's dial", e))?;
                match heard.recv_timeout(left) {
                    Ok(answer) => answer,
                    Err(mpsc::RecvTimeoutError::Timeout) => return Err(expired(port)),
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(Refusal::Broken("a lane's dial ended unanswered".to_owned()));
                    }
                }
            }
        };
        let socket = answer.map_err(|fault| refused(port, fault, self.lanes))?;
        own_duplex(bridge(socket).map_err(|e| Refusal::os("bridging a lane to a local socket", e))?)
    }

    fn carries_half_close(&self) -> bool {
        true
    }
}

const DIALING_VM: &str = "dialing the node VM's run's endpoint";

/// The socket `port` names in the run's socket directory: none is the caller's error, before any lane.
fn named(port: u32) -> Result<&'static str, Refusal> {
    stream::socket_name(port).ok_or_else(|| Refusal::Invalid {
        what: "a port of the node VM's relay",
        why: format!("{port:#x} names no socket the relay listens on"),
    })
}

fn expired(port: u32) -> Refusal {
    Refusal::Connect {
        port,
        failure: ConnectFailure::Expired {
            phase: ConnectPhase::Dial,
        },
    }
}

/// Linux's errno values the run host's connect reports for a lane (D251): the guest is Linux whatever the host, so its
/// numbers are Linux's (`include/uapi/asm-generic/errno-base.h`, `errno.h`), not this host's.
const ENOENT: u16 = 2;
const EACCES: u16 = 13;
const ECONNREFUSED: u16 = 111;

/// A lane's fault as the dial's refusal to `port`, typed: its run host's connect's errno as a connect failure at the
/// dial; the run's admission passed as Hex's own exhaustion, naming it; the connection's end, or the lane's reset
/// before it connected, as the port closed unanswered.
fn refused(port: u32, fault: Fault, lanes: u32) -> Refusal {
    match fault {
        Fault::Unreached(errno) => Refusal::Connect {
            port,
            failure: ConnectFailure::Io {
                phase: ConnectPhase::Dial,
                kind: IoKind::new(match errno.get() {
                    ENOENT => std::io::ErrorKind::NotFound,
                    EACCES => std::io::ErrorKind::PermissionDenied,
                    ECONNREFUSED => std::io::ErrorKind::ConnectionRefused,
                    _ => std::io::ErrorKind::Other,
                }),
                code: Some(i32::from(errno.get())),
            },
        },
        Fault::Refused(Reason::Capacity) => Refusal::Exhausted {
            what: "socket lanes to the node VM's relay",
            bound: usize::try_from(lanes).unwrap_or(usize::MAX),
        },
        Fault::Lost(_) | Fault::Revoked => Refusal::Connect {
            port,
            failure: ConnectFailure::Closed,
        },
        other => Refusal::Os {
            op: "dialing the node VM's relay through its run's endpoint",
            why: format!("a lane to port {port:#x}: {other:?}"),
        },
    }
}

/// Both ends of a node's control link took TLS13_AES_256_GCM_SHA384, the person's directive for every link of Hex's
/// and Shards' D239; a link either end of which took another suite is refused, naming it.
fn tls(link: shards_api::control::Link) -> Result<(), Refusal> {
    const SUITE: &str = "TLS13_AES_256_GCM_SHA384";
    for (end, negotiated) in [
        ("this node's end", link.controller),
        ("the node VM's endpoint", link.endpoint),
    ] {
        let suite = negotiated.suite_name();
        if suite != Some(SUITE) {
            return Err(Refusal::Invalid {
                what: "the node VM's control link",
                why: format!(
                    "{end} took {}, not {SUITE}",
                    suite.unwrap_or("a suite rustls doesn't name")
                ),
            });
        }
    }
    Ok(())
}

/// `socket` as the owner's end of a local socket pair, its bytes the lane's: two threads copy between the pair's other
/// end and the lane, one a way, each on one of the lane's halves ([`Socket::split`]). Each ends on a fact: the owner's
/// end of writing or its close, the lane's end in order, or its reset or its connection's end, which the owner reads
/// as the cut after the bytes before it.
fn bridge(socket: Socket) -> std::io::Result<Bridged> {
    let (owner, ours) = pair()?;
    let (reader, writer) = socket.split();
    let cut = Cut::default();
    let up = ours.try_clone()?;
    std::thread::Builder::new()
        .name("hex-lane-up".to_owned())
        .spawn(move || upward(writer, up))?;
    let recorded = cut.clone();
    std::thread::Builder::new()
        .name("hex-lane-down".to_owned())
        .spawn(move || downward(reader, ours, &recorded))?;
    Ok(Bridged::new(owner, cut))
}

/// What the owner writes, to the lane; the owner's end of writing, or its close, is the lane's FIN: the relay's socket
/// shut for writing, its reads going on. A write the lane refuses (its reset, its connection's end) ends this way, and
/// the owner's writes then fail as the pair's other end stops reading; an error reading the pair aborts the lane.
fn upward(mut writer: SocketWriter, mut from: Local) {
    let mut buf = vec![0u8; DATA_BOUND];
    loop {
        #[cfg(target_os = "macos")]
        if crate::wait_readable(std::os::fd::AsFd::as_fd(&from)).is_err() {
            let _ = writer.abort();
            return;
        }
        match from.read(&mut buf) {
            Ok(0) => {
                let _ = writer.shutdown_write();
                return;
            }
            Ok(n) => {
                if writer.write_all(buf.get(..n).unwrap_or_default()).is_err() {
                    let _ = from.shutdown(Shutdown::Read);
                    return;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => {
                let _ = writer.abort();
                return;
            }
        }
    }
}

/// What the relay sends, to the owner; the lane's end in order is the owner's end of input, and its reset or its
/// connection's end the cut, recorded before the owner's socket ends. An owner gone lets the lane's reading go, which
/// stops the relay's sending (STOP_SENDING).
fn downward(mut reader: SocketReader, mut to: Local, cut: &Cut) {
    let mut buf = vec![0u8; DATA_BOUND];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => {
                let _ = to.shutdown(Shutdown::Write);
                return;
            }
            Ok(n) => {
                if to.write_all(buf.get(..n).unwrap_or_default()).is_err() {
                    end(&to);
                    return;
                }
            }
            Err(e) => {
                cut.record(&e);
                end(&to);
                return;
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests;
