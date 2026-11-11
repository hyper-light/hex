//! The guest's ports through Shards (docs/design/shards-guest.md §2, Appendix A "Hex's side"): a
//! [`Link`] whose every dial is `shards_api::Guest::connect`, Firecracker's `CONNECT <port>` on the
//! guest's vsock, and a [`Duplex`] over the stream it returns. Hex never names Shards' stream type: on
//! Windows it is Shards' overlapped AF_UNIX socket, whose timeouts poll first, behind the same trait.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use hex_api::Refusal;

use super::{Duplex, DuplexOwner, Link, own_duplex};

/// A connection to one of the guest's ports, as Shards gives it.
pub struct ShardsStream(Box<dyn shards_api::Stream>);

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
        own_duplex(Self(self.0.try_clone()?))
            .map_err(|_| io::Error::from(io::ErrorKind::OutOfMemory))
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

/// A guest's ports. Until a dial has connected, a dial waits for the guest to listen, retried by
/// Shards' client until it listens, ends, or `first` passes, the caller's bound on a boot (learned,
/// `hex_runtime::guest::GuestRuntime::first_dial_bound`). Once one has, the guest's relay listens, so
/// each later dial is one try (`Duration::ZERO`): a refusal then is the relay's end.
pub struct ShardsLink {
    guest: ShardsGuest,
    first: Duration,
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

impl ShardsLink {
    pub fn new(guest: Arc<dyn shards_api::Guest>, first: Duration) -> Self {
        Self {
            guest: ShardsGuest::Owned(guest),
            first,
            listened: AtomicBool::new(false),
        }
    }

    /// AC-T7.4.1: retain the API's existing native/protocol receipt directly. The
    /// caller uses ManagedGuest::try_clone when it also retains a boot handle;
    /// adapting that handle needs no second Arc allocation or lifecycle owner.
    pub fn managed(guest: shards_api::ManagedGuest, first: Duration) -> Self {
        Self {
            guest: ShardsGuest::Managed(guest),
            first,
            listened: AtomicBool::new(false),
        }
    }
}

impl Link for ShardsLink {
    fn identity(&self) -> Result<shards_api::Identity, Refusal> {
        self.guest
            .guest()
            .identity()
            .map_err(|why| refusal("naming the guest's stream endpoint", &why))
    }

    fn dial(&self, port: u32) -> Result<DuplexOwner, Refusal> {
        let patience = if self.listened.load(Ordering::Acquire) {
            Duration::ZERO
        } else {
            self.first
        };
        let stream = self
            .guest
            .guest()
            .connect(port, patience)
            .map_err(|why| refusal("connecting to the guest", &why))?;
        self.listened.store(true, Ordering::Release);
        own_duplex(ShardsStream(stream))
    }

    fn dial_before(&self, port: u32, deadline: Option<Instant>) -> Result<DuplexOwner, Refusal> {
        let Some(at) = deadline else {
            return self.dial(port);
        };
        let cap = crate::calls::ReplyDeadline::caller_before(at)?;
        cap.remaining("connecting to the guest within the original operation deadline")?;
        let stream = self
            .guest
            .guest()
            .connect_before(port, at)
            .map_err(|why| refusal("connecting to the guest", &why))?;
        let stream = own_duplex(ShardsStream(stream))?;
        cap.remaining("retaining the guest connection within the original operation deadline")?;
        self.listened.store(true, Ordering::Release);
        Ok(stream)
    }
}

/// Shards' refusal as Hex's: a missing backend is the platform's, a bound is a spec's, the rest is the
/// guest's channel or its end, which Hex reads as an operating-system failure naming what it did.
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
        other => Refusal::os(op, other),
    }
}

#[cfg(test)]
mod connect_original_tests;
