//! The guest's ports through its run's agent-control endpoint (docs/design/node-under-shards-init.md §5): a node VM
//! under shards-init is reached by Shards' socket lanes (D251) to its relay's listeners in the run's socket directory,
//! [`DIR`], over the run's own endpoint (D252), whose controller is Shards' own (`shards_api::control`). [`Lanes`]
//! dials a port as a lane to the socket that port names ([`hex_core::stream::socket_name`]), bridged to a local
//! socket pair ([`super::bridge`]), so the client's event loop waits on an OS handle as it does on vsock's. The lane
//! carries a half-close each way, and its reset or its connection's end is told after its bytes.

use std::net::SocketAddr;
use std::time::Instant;

use hex_api::Refusal;
use shards_api::control::{Hangup, Identity, SocketAdmission, Sockets};

use super::{DuplexOwner, Link};

/// The run's socket directory: where the node's relay listens under shards-init (hex-cli's guest init), and the one
/// directory the run grants its controller lanes to.
pub const DIR: &str = "/run/hex/channel";

/// The controller's connection to a node VM's endpoint and the lanes it opens on it to the node's relay.
pub struct Lanes {
    sockets: Sockets,
    /// The connection: kept for the lanes' life, which end with it (dropped, it ends the connection and every lane).
    _hangup: Hangup,
    /// The most lanes open at once, as the run admits them: what a dial past it is refused naming.
    lanes: u32,
}

impl Lanes {
    /// Dials the endpoint at `at` of the node VM whose key's SHA-256 is `vm`, under this node's own key `own`, for
    /// socket lanes alone as `admission` admits them. Behavior kept here: the lanes aren't opened yet.
    pub fn dial(
        own: &Identity,
        vm: [u8; 32],
        at: SocketAddr,
        admission: SocketAdmission,
    ) -> Result<Lanes, Refusal> {
        let _ = (own, vm, at, admission);
        Err(Refusal::Awaits {
            needs: hex_api::Upstream::NestedChannel,
        })
    }
}

impl Link for Lanes {
    fn dial(&self, port: u32) -> Result<DuplexOwner, Refusal> {
        self.dial_before(port, None)
    }

    fn dial_before(&self, port: u32, deadline: Option<Instant>) -> Result<DuplexOwner, Refusal> {
        let _ = (&self.sockets, self.lanes, port, deadline);
        Err(Refusal::Awaits {
            needs: hex_api::Upstream::NestedChannel,
        })
    }

    fn carries_half_close(&self) -> bool {
        true
    }
}

/// Whether both ends of a node's control link took TLS13_AES_256_GCM_SHA384. Behavior kept here: none is judged.
fn tls(link: shards_api::control::Link) -> Result<(), Refusal> {
    let _ = link;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests;
