//! A box's network grants as Shards' image boot (D231) takes them (joint plan step 15; docs/design/network-grants.md
//! N4): exactly what Shards `482c5d78` can hold, or a typed refusal naming the capability Hex awaits. Pure: the Shards
//! image class hands [`ImageNetwork`] to `shards_api::image::ImageSpec` as it boots, and records what the boot bound.

use hex_api::{Destination, Grant, Proto, Refusal, Upstream, check_grants};
use shards_api::image::{Keep, Narrowing, Publish};

/// What an image boot is told of a box's grants: the ports it publishes, and how the image's own grants past the
/// microVM are narrowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageNetwork {
    pub publish: Vec<Publish>,
    pub egress: Narrowing,
}

/// `grants` as an image boot takes them, or the refusal of the first one 482c5d78 can't hold exactly:
/// - a name, for any member: a harness's egress is ports to every address and its DNS any name (Shards'
///   harness-bringup-v1, H2), and an agent's remote MCP grant is kept image-wide, its member's scope out of sight;
/// - an address, and the VM's own host: no grant of either form exists.
///
/// The image's own grants are never kept, where `Narrowing::default()` would keep them whole. So with no grant the
/// run has no network at all (`shards api-run`'s `network none`), and every published port is a grant's, at the host
/// address it names.
pub fn program(grants: &[Grant]) -> Result<ImageNetwork, Refusal> {
    check_grants(grants)?;
    let mut publish = Vec::new();
    publish
        .try_reserve_exact(grants.len())
        .map_err(|_| Refusal::Exhausted {
            what: "published ports",
            bound: grants.len(),
        })?;
    for grant in grants {
        let needs = match grant {
            Grant::Egress {
                to: Destination::Name(_),
                ..
            } => Upstream::MemberGrantsByName,
            Grant::Egress {
                to: Destination::Address(_),
                ..
            } => Upstream::MemberGrantsByAddress,
            Grant::Host { .. } => Upstream::MemberHostService,
            Grant::Publish {
                proto,
                port,
                host,
                host_port,
                ..
            } => {
                publish.push(Publish {
                    guest_port: *port,
                    proto: match proto {
                        Proto::Tcp => shards_api::image::Proto::Tcp,
                        Proto::Udp => shards_api::image::Proto::Udp,
                    },
                    host: Some(*host),
                    // shards-api's word for a port the host's OS picks.
                    host_port: host_port.unwrap_or(0),
                });
                continue;
            }
        };
        return Err(Refusal::Awaits { needs });
    }
    Ok(ImageNetwork {
        publish,
        egress: Narrowing {
            egress: Keep::Only(Vec::new()),
            mcp: Keep::Only(Vec::new()),
            dns: false,
        },
    })
}

#[cfg(test)]
mod tests;
