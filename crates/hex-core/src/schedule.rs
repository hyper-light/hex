//! Scheduler control carried by the one node wire (CP-T7.4, LB03).
//! Peer messages use `hex_sim::peer`'s bounded codec, including GPU model names rather than
//! process-local model IDs. Refusals use a complete `wire::Answer::Refused` message, retaining type.

use hex_api::{BoxId, BoxSpec};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Key {
    pub owner: u32,
    pub epoch: u64,
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub spec: BoxSpec,
    pub start: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    pub epoch: u64,
    pub seq: u64,
    pub message: String,
    pub payload: Option<Payload>,
    pub refusal: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Command {
    Submit { demand: String, payload: Payload },
    Hear { from: u32, packet: Packet },
    LinkUp { peer: u32 },
    Retry,
    Delete { key: Key },
    Status { key: Key },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub key: Option<Key>,
    pub at: Option<u32>,
    pub physical: Option<BoxId>,
    pub created: bool,
    pub ended: bool,
    /// Typed failures from actual runtime operations, encoded by the existing refusal codec.
    pub refusals: Vec<Vec<u8>>,
}
