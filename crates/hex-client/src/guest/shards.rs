//! The guest's ports through Shards (docs/design/shards-guest.md §2, Appendix A "Hex's side"): a
//! [`Link`] whose every dial is `shards_api::Guest::connect`, Firecracker's `CONNECT <port>` on the
//! guest's vsock, and a [`Duplex`] over the stream it returns. Hex never names Shards' stream type: on
//! Windows it is Shards' overlapped AF_UNIX socket, whose timeouts poll first, behind the same trait.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use hex_api::Refusal;

use super::{Duplex, Link};

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
    fn try_clone(&self) -> io::Result<Box<dyn Duplex>> {
        Ok(Box::new(Self(self.0.try_clone()?)))
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
    guest: Arc<dyn shards_api::Guest>,
    first: Duration,
    listened: AtomicBool,
}

impl ShardsLink {
    pub fn new(guest: Arc<dyn shards_api::Guest>, first: Duration) -> Self {
        Self {
            guest,
            first,
            listened: AtomicBool::new(false),
        }
    }
}

impl Link for ShardsLink {
    fn identity(&self) -> Result<shards_api::Identity, Refusal> {
        self.guest
            .identity()
            .map_err(|why| refusal("naming the guest's stream endpoint", &why))
    }

    fn dial(&self, port: u32) -> Result<Box<dyn Duplex>, Refusal> {
        let patience = if self.listened.load(Ordering::Acquire) {
            Duration::ZERO
        } else {
            self.first
        };
        let stream = self
            .guest
            .connect(port, patience)
            .map_err(|why| refusal("connecting to the guest", &why))?;
        self.listened.store(true, Ordering::Release);
        Ok(Box::new(ShardsStream(stream)))
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
        other => Refusal::os(op, other),
    }
}
