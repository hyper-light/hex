//! A node VM the node boots for itself through the image class (joint plan step 39;
//! docs/design/node-under-shards-init.md §4 to §6): Hex's own init under shards-init, from Hex's node image, which its
//! node reaches through its run's agent-control endpoint alone (§5). It is an image VM, so its boot, its stop and its use of its host go through the
//! class's own runs (ROOT, 2026-11-13: one code path, not a third); and it is never a box: its record is a node VM's,
//! named by its run, which no box call reads.

use std::time::Duration;

use hex_api::{Allocation, Exit, HostUse, NodeRun, Refusal, Upstream};
use shards_api::image::{ControlAt, ImageShare, Ready};

use super::ImageClass;

/// A node VM as its node boots it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeVm {
    /// Its run, minted by the node's store as the VM is booked in its ledger.
    pub run: NodeRun,
    /// The node image, pinned by digest (`HEX_GUEST_IMAGE`): Hex's own init at `/init`, declaring `NESTING`.
    pub image: String,
    /// Its admitted allocation: its receipt in the node's ledger, and the size Shards boots.
    pub allocation: Allocation,
    /// The host directories it shares, each where Hex's init finds it (`/run/hex/shares/<i>`).
    pub shares: Vec<ImageShare>,
    /// The SHA-256 of its node's own key: the one key its run's endpoint takes (D252).
    pub controller: [u8; 32],
    /// The most lanes its run admits at once to the relay's sockets: what its node fitted (hex-node's `serve::fit`).
    pub lanes: u32,
}

/// A node VM whose init has said it is ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeBooted {
    pub run: NodeRun,
    /// Its run's agent-control endpoint, and its microVM's key, which its node pins (D252).
    pub control: ControlAt,
    /// How its init said it.
    pub ready: Ready,
    /// From the boot's ask to its READY.
    pub took: Duration,
}

impl ImageClass {
    /// Boots node VM `vm` and follows it to its init's READY. Kept here: the class boots no node VM.
    pub fn boot_node(&self, vm: &NodeVm) -> Result<NodeBooted, Refusal> {
        let _ = vm;
        Err(awaited())
    }

    /// Stops node VM `run`'s run: its end. Kept here: the class runs no node VM.
    pub fn stop_node(&self, run: NodeRun) -> Result<Exit, Refusal> {
        let _ = run;
        Err(awaited())
    }

    /// What node VM `run` costs its host. Kept here: the class runs no node VM.
    pub fn host_use_node(&self, run: NodeRun) -> Result<HostUse, Refusal> {
        let _ = run;
        Err(awaited())
    }
}

/// The node's shards-init form, refused by type until its node VM boots through the class (§5).
fn awaited() -> Refusal {
    Refusal::Awaits {
        needs: Upstream::NestedChannel,
    }
}
