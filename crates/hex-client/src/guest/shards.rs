//! The guest's ports through Shards (docs/design/shards-guest.md §2, Appendix A "Hex's side"): a
//! [`Link`] whose first dial is `shards_api::Guest::connect_before` and every later one
//! `connect_once_before`, Firecracker's `CONNECT <port>` on the guest's vsock, and a [`Duplex`] over the
//! stream it returns. Hex never names Shards' stream type: on
//! Windows it is Shards' overlapped AF_UNIX socket, whose timeouts poll first, behind the same trait.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use hex_api::{
    ConnectFailure, ConnectPhase, ConnectReply, GuestEnd, GuestFailure, IoKind, Refusal,
};
use hex_core::patience::{Patience, Piece};

use super::{Duplex, DuplexOwner, Link, own_duplex};

/// The slowest recorded value of each wait that seeds a bound, by host class
/// (bench/native/guest/records.tsv). A class with no row for a measure seeds from the slowest row of any
/// class ([`seed_in`]); adding its own row, once its record is taken, makes that its seed, with no code
/// change.
pub const RECORDS: &str = include_str!("../../../../bench/native/guest/records.tsv");

/// This host's class, as the records name it: the hypervisor Shards boots its guests with here.
pub const CLASS: &str = if cfg!(target_os = "macos") {
    "macOS-HVF"
} else if cfg!(target_os = "windows") {
    "Windows-WHP"
} else {
    "Linux-KVM"
};

/// The slowest value `table` records for `measure` on host class `class`: each line `measure`, `class`,
/// microseconds and its record, tab-separated; `#` lines are comments. `None` where no row names both.
pub fn slowest_in(table: &str, measure: &str, class: &str) -> Option<Duration> {
    table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| line.split('\t'))
        .find_map(|mut row| {
            (row.next()? == measure && row.next()? == class)
                .then(|| row.next()?.parse().ok().map(Duration::from_micros))?
        })
}

/// The seed `table` gives `measure` on host class `class`: its own slowest row where it has one; else the slowest
/// row of any class, every class's records taken as one (decision 9: a bound is the slowest answer on record, with no
/// margin past it), a class with no record of its own starting from the slowest any host has been seen to take,
/// then rising with its node's own and backing off past each bound it runs out at (the learned bounds take the max
/// with what the node sees). `None` where no class records the measure.
pub fn seed_in(table: &str, measure: &str, class: &str) -> Option<Duration> {
    if let Some(own) = slowest_in(table, measure, class) {
        return Some(own);
    }
    table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            let mut row = line.split('\t');
            if row.next()? != measure {
                return None;
            }
            row.next()?;
            row.next()?.parse().ok().map(Duration::from_micros)
        })
        .max()
}

/// The seed for `measure` on this host's class ([`seed_in`] over records.tsv), or the refusal naming the
/// record to take where no class records it.
pub fn recorded(measure: &str) -> Result<Duration, Refusal> {
    seed_in(RECORDS, measure, CLASS).ok_or_else(|| Refusal::Platform {
        what: "the oci class's guest",
        platform: std::env::consts::OS,
        why: format!(
            "no {measure} record for any host class yet (bench/native/guest/records.tsv): record one first"
        ),
    })
}

/// The seed of the first dial's bound before this node has seen a boot: the slowest boot to ready on record
/// for this host's class, or with none, the slowest of any class (bench/native/guest/boot-ready/RECORD.md, its
/// rows in records.tsv).
pub fn recorded_boot() -> Result<Duration, Refusal> {
    recorded("boot_ready")
}

/// A connection to one of the guest's ports, as Shards gives it.
pub struct ShardsStream(pub(crate) Box<dyn shards_api::Stream>);

impl Read for ShardsStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for ShardsStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Duplex for ShardsStream {
    fn try_clone(&self) -> io::Result<DuplexOwner> {
        let clone = self.0.try_clone()?;
        // Winsock's duplicate is inheritable (hex_afunix::no_inherit).
        #[cfg(windows)]
        hex_afunix::no_inherit(clone.handle())?;
        own_duplex(Self(clone)).map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))
    }

    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.0.shutdown(how)
    }

    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_write_timeout(timeout)
    }

    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_read_timeout(timeout)
    }
    fn handle(&self) -> super::Handle<'_> {
        self.0.handle()
    }

    fn set_nonblocking(&self, on: bool) -> io::Result<()> {
        self.0.set_nonblocking(on)
    }
}

/// A guest's ports. A dial given its caller's deadline waits within it; one given none follows the guest
/// past `bound` ([`ShardsLink::dial_following`], decision 9), each of its attempts within `bound` from
/// its start. Until a dial has reached the guest's relay an attempt is one `connect_before`, within
/// which Shards' client asks again while the guest doesn't listen there yet. The relay binds every port
/// it serves before it serves any (`hex guest`), so after that a refused port has no relay behind it:
/// each later attempt is one `connect_once_before`, refused at once if nothing listens. `bound` is the
/// caller's learned bound on a boot to ready (`hex_runtime::guest::GuestRuntime::first_dial_bound`),
/// whose ready includes a whole CONNECT/OK exchange. Never a relative patience: Shards' client awaits a
/// zero or `Duration::MAX` patience's answer without a bound (HEX-GUEST-33).
pub struct ShardsLink {
    guest: ShardsGuest,
    bound: Duration,
    listened: AtomicBool,
}

enum ShardsGuest {
    Owned(Arc<dyn shards_api::Guest>),
    Managed(shards_api::ManagedGuest),
}

impl ShardsGuest {
    fn guest(&self) -> &dyn shards_api::Guest {
        match self {
            Self::Owned(guest) => guest.as_ref(),
            Self::Managed(guest) => guest,
        }
    }
}

/// A guest link's dials with no caller deadline, as the waits census keeps them (decision 9): each stream, timed from
/// its dial's start, and each dial's run-out past its link's bound ([`ShardsLink::dial_following`]).
static FOLLOWED: Patience = Patience::measuring("client.guest.link.dial");

impl ShardsLink {
    pub fn new(guest: Arc<dyn shards_api::Guest>, bound: Duration) -> Self {
        Self {
            guest: ShardsGuest::Owned(guest),
            bound,
            listened: AtomicBool::new(false),
        }
    }

    /// AC-T7.4.1: retain the API's existing native/protocol receipt directly. The
    /// caller uses ManagedGuest::try_clone when it also retains a boot handle;
    /// adapting that handle needs no second Arc allocation or lifecycle owner.
    pub fn managed(guest: shards_api::ManagedGuest, bound: Duration) -> Self {
        Self {
            guest: ShardsGuest::Managed(guest),
            bound,
            listened: AtomicBool::new(false),
        }
    }

    /// A dial to `port` with no caller deadline, as decision 9 waits (the person's ruling of 2026-11-13). Its W* is the
    /// link's bound: the slowest boot to ready its node has seen, whose ready includes a whole CONNECT/OK exchange, and
    /// the one finite deadline Shards' connector waits in here.
    /// - **Within it** the stream is the answer.
    /// - **Past it** the run-out is told, once ([`FOLLOWED`]), and the dial follows the guest's VM by its life: made
    ///   again, waiting the bound again, as each attempt's passes, refused at the guest's end (Shards' connector looks
    ///   at its VM before each CONNECT and after) or by any refusal that came before its attempt's bound passed, never
    ///   at a bound.
    ///
    /// No attempt is given up sooner: one given up is closed unanswered, a connection the guest takes for nothing.
    fn dial_following(&self, port: u32) -> Result<DuplexOwner, Refusal> {
        let began = Instant::now();
        let mut told = false;
        loop {
            let at = self.bound_from_now()?;
            let guest = self.guest.guest();
            let dialed = if self.listened.load(Ordering::Acquire) {
                guest.connect_once_before(port, at)
            } else {
                guest.connect_before(port, at)
            };
            match dialed {
                Ok(stream) => {
                    self.listened.store(true, Ordering::Release);
                    FOLLOWED.answered(began.elapsed());
                    return own_duplex(ShardsStream(stream));
                }
                Err(why) if Instant::now() < at || gone(&why) => {
                    return Err(refusal("connecting to the guest", &why));
                }
                Err(_) => {
                    if !told {
                        // No sample (Karn's point): the run-out is told, and the dial goes on.
                        FOLLOWED.ran_out(
                            self.bound,
                            began.elapsed(),
                            &[
                                Piece::Kind("the guest's VM, its port "),
                                Piece::Number(u64::from(port)),
                                Piece::Kind("'s answer to a dial"),
                            ],
                        );
                        told = true;
                    }
                }
            }
        }
    }

    /// The link's bound from now, refused by type where it names no finite deadline: zero (a boot that spent its
    /// bound), or past a monotonic one (`Duration::MAX`).
    fn bound_from_now(&self) -> Result<Instant, Refusal> {
        (!self.bound.is_zero())
            .then(|| Instant::now().checked_add(self.bound))
            .flatten()
            .ok_or_else(|| Refusal::Invalid {
                what: "guest dial bound",
                why: "it must be positive and fit a monotonic deadline".to_owned(),
            })
    }
}

/// Whether Shards' refusal says its guest is gone: ended, no longer at its pid, its process unobservable or another,
/// or its VM stalled.
fn gone(why: &shards_api::Refusal) -> bool {
    matches!(
        why,
        shards_api::Refusal::Ended(_)
            | shards_api::Refusal::Gone { .. }
            | shards_api::Refusal::Process { .. }
            | shards_api::Refusal::Scope(_)
            | shards_api::Refusal::Stalled(_)
    )
}

impl Link for ShardsLink {
    fn identity(&self) -> Result<shards_api::Identity, Refusal> {
        self.guest
            .guest()
            .identity()
            .map_err(|why| refusal("naming the guest's stream endpoint", &why))
    }

    /// The learned bound on a boot to ready, whose ready includes the CONNECT/OK exchange: what each
    /// attempt of a dial with no caller deadline waits ([`ShardsLink::dial_following`]), and what a
    /// stream offer carries for a caller's borrowed CONNECTs (HEX-RT-17).
    fn setup_bound(&self) -> Option<Duration> {
        Some(self.bound)
    }

    /// Shards carries a half-close (its D12, held by its conformance case).
    fn carries_half_close(&self) -> bool {
        true
    }

    fn dial(&self, port: u32) -> Result<DuplexOwner, Refusal> {
        self.dial_before(port, None)
    }

    fn dial_before(&self, port: u32, deadline: Option<Instant>) -> Result<DuplexOwner, Refusal> {
        let Some(at) = deadline else {
            return self.dial_following(port);
        };
        let cap = crate::calls::ReplyDeadline::caller_before(at)?;
        cap.remaining("connecting to the guest within the original operation deadline")?;
        let guest = self.guest.guest();
        let stream = if self.listened.load(Ordering::Acquire) {
            guest.connect_once_before(port, at)
        } else {
            guest.connect_before(port, at)
        }
        .map_err(|why| refusal("connecting to the guest", &why))?;
        self.listened.store(true, Ordering::Release);
        let stream = own_duplex(ShardsStream(stream))?;
        cap.remaining("retaining the guest connection within the original operation deadline")?;
        Ok(stream)
    }
}

/// Shards' refusal as Hex's: a missing backend is the platform's, a bound is a spec's, a CONNECT's
/// failure keeps its port, phase and cause (Shards R04), an owned guest's or a borrowed one's; the
/// rest is the guest's channel or its end, which Hex reads as an operating-system failure naming
/// what it did.
pub fn refusal(op: &'static str, why: &shards_api::Refusal) -> Refusal {
    match why {
        shards_api::Refusal::NoBackend { os, arch } => Refusal::Platform {
            what: "the oci class",
            platform: os,
            why: format!(
                "Shards has no backend on {os} {arch} to boot the Linux guest the class runs in"
            ),
        },
        shards_api::Refusal::Bounds { what, why } => Refusal::Invalid {
            what: "the node's guest",
            why: format!("its {what}: {why}"),
        },
        shards_api::Refusal::Gpu(why) => {
            use shards_api::PhysicalGpuError;
            let message = match why {
                PhysicalGpuError::Syntax => "malformed native GPU selector",
                PhysicalGpuError::PciAddress => "GPU PCI address fields are out of range",
                PhysicalGpuError::MetalRegistry => "Metal registry identity is unavailable",
                PhysicalGpuError::Count => "GPU count exceeds the native device address space",
                PhysicalGpuError::Duplicate => "GPU selection repeats a native identity",
                PhysicalGpuError::Allocation => {
                    return Refusal::Exhausted {
                        what: "physical GPU selection storage",
                        bound: shards_api::physical::MAX_PHYSICAL_GPUS,
                    };
                }
            };
            let mut reason = String::new();
            if reason.try_reserve_exact(message.len()).is_err() {
                return Refusal::Exhausted {
                    what: "physical GPU refusal text",
                    bound: message.len(),
                };
            }
            reason.push_str(message);
            Refusal::Invalid {
                what: "physical GPU selection",
                why: reason,
            }
        }
        // The VM process refused its selection, as it said in place of a receipt (Shards 18f59d497): certain, the
        // selection's own refusal, with its reason, the join's or the renderer's.
        shards_api::Refusal::GpuProbe(shards_api::GpuProbeError::Refused(refused)) => {
            Refusal::Invalid {
                what: "physical GPU selection",
                why: format!("its VM process refused it: {refused}"),
            }
        }
        // Its report writer closed with nothing written: the VM process ended before its renderer said anything of its
        // selection, which is left unknown; the process's own end says why.
        shards_api::Refusal::GpuProbe(shards_api::GpuProbeError::Unreported) => Refusal::Os {
            op,
            why: "its VM process ended before its renderer said anything of its GPU selection: no report \
                  was written"
                .to_owned(),
        },
        shards_api::Refusal::Connect { port, failure }
        | shards_api::Refusal::Borrowed {
            port,
            failure: shards_api::BorrowFailure::Channel(failure),
        } => Refusal::Connect {
            port: *port,
            failure: connect_failure(failure),
        },
        // The guest's own failures, each in its kind (the review's B-15).
        shards_api::Refusal::Stalled(stall) => Refusal::Guest {
            op,
            failure: GuestFailure::Stalled {
                waited: stall.waited(),
                over: stall.over(),
                slowest: stall.slowest(),
            },
        },
        shards_api::Refusal::Ended(end) => Refusal::Guest {
            op,
            failure: GuestFailure::Ended(match end {
                shards_api::End::PowerOff => GuestEnd::PowerOff,
                shards_api::End::Reset => GuestEnd::Reset,
                shards_api::End::Stopped => GuestEnd::Stopped,
                shards_api::End::Failed(why) => GuestEnd::Failed(why.clone()),
            }),
        },
        shards_api::Refusal::Gone { pid, why } => Refusal::Guest {
            op,
            failure: GuestFailure::Gone {
                pid: *pid,
                why: why.clone(),
            },
        },
        shards_api::Refusal::Rate(why) => Refusal::Guest {
            op,
            failure: GuestFailure::Rate(why.clone()),
        },
        shards_api::Refusal::HostUse(why) => Refusal::Guest {
            op,
            failure: GuestFailure::HostUse(why.to_string()),
        },
        other => Refusal::os(op, other),
    }
}

fn connect_failure(failure: &shards_api::HandshakeError) -> ConnectFailure {
    use shards_api::{HandshakeError, ReplyError};
    match failure {
        HandshakeError::Expired { phase } => ConnectFailure::Expired {
            phase: connect_phase(*phase),
        },
        HandshakeError::Io { phase, kind, code } => ConnectFailure::Io {
            phase: connect_phase(*phase),
            kind: IoKind::new(*kind),
            code: *code,
        },
        HandshakeError::Closed => ConnectFailure::Closed,
        HandshakeError::Reply(ReplyError::TooLong) => ConnectFailure::Reply(ConnectReply::TooLong),
        HandshakeError::Reply(ReplyError::NotOk) => ConnectFailure::Reply(ConnectReply::NotOk),
    }
}

fn connect_phase(phase: shards_api::HandshakePhase) -> ConnectPhase {
    match phase {
        shards_api::HandshakePhase::Dial => ConnectPhase::Dial,
        shards_api::HandshakePhase::Write => ConnectPhase::Write,
        shards_api::HandshakePhase::Read => ConnectPhase::Read,
        shards_api::HandshakePhase::ClearTimeouts => ConnectPhase::ClearTimeouts,
    }
}

#[cfg(test)]
mod connect_original_tests;
