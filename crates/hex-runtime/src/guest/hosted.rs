//! The node's guest under shards-init (docs/design/node-under-shards-init.md §4, §6): its node VM booted through
//! Shards' image boot (D231) from Hex's node image (`scripts/guest/node.Agentfile`). Hex's own init is the run's own
//! command under the nesting grant (D234), PID 1 of the workload's own PID namespace, and the boot is followed to the
//! READY that init says once its node and relay are ready.
//!
//! Its host reaches that node only through a lane of Shards' agent-control endpoint to the relay's sockets (§5), which
//! Shards doesn't give yet. Until it does, a node configured for this form is refused `Awaits(NestedChannel)`
//! ([`super::GuestConfig::from_vars`]), and `node_hosted_real` boots it to READY for ROOT's measurement.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hex_api::Refusal;
use hex_core::patience::Patience;
use shards_api::image::{
    ImageGuest, ImageRefusal, ImageShare, ImageSpec, Images, Keep, Narrowing, Ready,
};

use super::shares::Share;

/// The command shards-init runs: Hex's own init, `argv[0]` `/init` as the kernel names an initramfs's first process,
/// so that `hex` runs as init under either host (hex-cli's `runs_as_init`).
pub const INIT: &str = "/init";

/// The node VM under shards-init, as its node boots it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedConfig {
    /// The `shards` binary that hosts its run (`SHARDS_BIN`).
    pub binary: PathBuf,
    /// The node image, pinned by digest (`HEX_GUEST_IMAGE`): Hex's own init at `/init`, declaring `NESTING`.
    pub image: String,
    /// The VM's size, the node's to give from its budget (shards-guest.md §1), as the direct boot's.
    pub vcpus: u32,
    pub memory_mib: u64,
    /// The host directories the node shares, each where Hex's init finds it (`/run/hex/shares/<i>`).
    pub shares: Vec<Share>,
    /// The run host's own environment: `SHARDS_HOME`, whose store holds the image, and the rest of the node's Shards
    /// paths.
    pub env: Vec<(OsString, OsString)>,
}

impl HostedConfig {
    /// The image boot that runs Hex's own init under the nesting grant. Its parts:
    /// - **command**: [`INIT`] alone;
    /// - **ready_by**: the deadline given, none while the wait follows its run;
    /// - **shares**: at the paths the direct boot's init mounts them at, so the host's rewrite is one;
    /// - **no member**: none of the image's own (`NESTING` declares none);
    /// - **no published port and no grant past the VM** (default deny): the host reaches the node through its run's
    ///   agent-control endpoint alone (§5);
    /// - **no credential**.
    pub fn spec(&self, ready_by: Option<Duration>) -> ImageSpec {
        ImageSpec {
            image: self.image.clone(),
            vcpus: self.vcpus,
            memory_mib: self.memory_mib,
            command: Some(vec![INIT.to_owned()]),
            members: Vec::new(),
            publish: Vec::new(),
            egress: Narrowing {
                egress: Keep::Only(Vec::new()),
                mcp: Keep::Only(Vec::new()),
                destinations: Keep::Only(Vec::new()),
                dns: false,
                members: None,
            },
            credentials: Vec::new(),
            shares: self
                .shares
                .iter()
                .map(|share| ImageShare {
                    host: share.dir.clone(),
                    at: share.at.clone(),
                    read_only: share.read_only,
                })
                .collect(),
            nesting: true,
            ready_by,
            binary: Some(self.binary.clone()),
            env: self.env.clone(),
        }
    }
}

/// A node VM under shards-init whose init has said it is ready.
pub struct HostedGuest {
    pub guest: Box<dyn ImageGuest>,
    /// How its init said it: on time, or late past the deadline its run was given.
    pub ready: Ready,
    /// From the boot's ask to its READY: the node VM's start, its run host's, its VM's and its init's.
    pub took: Duration,
}

/// The node's guest under shards-init: what boots it, and how long its READY takes, learned from its own boots.
pub struct Hosted {
    images: Arc<dyn Images>,
    config: HostedConfig,
    /// `guest.hosted.ready`, the person's rule of 2026-11-13: none before the first READY, when the wait follows its
    /// run, then the slowest READY this node has seen; a run-out is no sample, and the next wait follows again.
    ready: Patience,
}

impl Hosted {
    pub fn new(images: Arc<dyn Images>, config: HostedConfig) -> Self {
        Self {
            images,
            config,
            ready: Patience::measuring("guest.hosted.ready"),
        }
    }

    /// Boots the node VM and follows it to READY.
    /// - **The boot** waits on its run host: `Duration::MAX`, until the run starts or its host ends.
    /// - **READY** is waited for to the learned deadline, and the run is given that deadline too, so Hex's init holds
    ///   its node and relay to it inside the VM. At the cold start, or after a run-out, there is no deadline: the
    ///   wait follows the run, and the run's end refuses it.
    /// - **Past the deadline** with no READY is a run-out, no sample (Karn's point): the run is stopped, the boot
    ///   refused, typed, and the next boot follows again.
    /// - **A refusal** of Shards' is typed by what it refused.
    pub fn boot(&self) -> Result<HostedGuest, Refusal> {
        let bound = self.ready.deadline();
        let began = Instant::now();
        let guest = self
            .images
            .boot(&self.config.spec(bound), Duration::MAX)
            .map_err(|why| refused("booting the node VM under shards-init", why))?;
        let said = guest.wait_ready(None, bound.unwrap_or(Duration::MAX));
        let took = began.elapsed();
        match said {
            Ok(Some(ready @ (Ready::OnTime | Ready::Late(_)))) => {
                self.ready.answered(took);
                Ok(HostedGuest { guest, ready, took })
            }
            Ok(Some(Ready::Passed(_)) | None) => {
                if let Some(after) = bound {
                    self.ready.ran_out(after);
                }
                stop(guest.as_ref());
                Err(Refusal::Os {
                    op: "waiting for the node VM under shards-init to say it is ready",
                    why: format!("no READY within {bound:?}, the slowest this node has seen"),
                })
            }
            Err(why) => {
                stop(guest.as_ref());
                Err(refused(
                    "waiting for the node VM under shards-init to say it is ready",
                    why,
                ))
            }
        }
    }
}

/// Ends a node VM that won't serve: a stop is never refused (D231), and its run's end is the stop's own return.
fn stop(guest: &dyn ImageGuest) {
    let _ = guest.stop();
}

/// Shards' refusal, typed by what it refused.
fn refused(op: &'static str, why: ImageRefusal) -> Refusal {
    const WHAT: &str = "the node VM under shards-init";
    match why {
        ImageRefusal::NoBackend { os, arch } => Refusal::Platform {
            what: WHAT,
            platform: os,
            why: format!("shards boots no guest on {os} {arch}"),
        },
        ImageRefusal::Spec { what, why } => Refusal::Invalid {
            what: WHAT,
            why: format!("{what}: {why}"),
        },
        ImageRefusal::Image(why) | ImageRefusal::Widening(why) => Refusal::Invalid {
            what: "the node image (HEX_GUEST_IMAGE)",
            why,
        },
        ImageRefusal::Unsupported { what, why } => Refusal::Platform {
            what,
            platform: std::env::consts::OS,
            why,
        },
        ImageRefusal::Ended(end) => Refusal::Os {
            op,
            why: format!("its run ended before its init said it was ready: {end:?}"),
        },
        other => Refusal::Os {
            op,
            why: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests;
