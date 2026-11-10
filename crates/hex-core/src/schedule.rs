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

/// Sender-owned lifetime aggregates; decimal text preserves the full nanosecond sum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchObservation {
    pub class: String,
    pub sum_ns: String,
    pub count: u64,
}

/// A node's facts in the model's serde schema, with its own model-name table for remapping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub spec: String,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Packet {
    pub epoch: u64,
    pub seq: u64,
    pub observations: Vec<LaunchObservation>,
    pub message: String,
    pub payload: Option<Payload>,
    pub refusal: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Command {
    Describe {
        from: u32,
    },
    Advertise {
        from: u32,
        offer: Offer,
        observations: Vec<LaunchObservation>,
    },
    Submit {
        demand: String,
        payload: Payload,
    },
    Hear {
        from: u32,
        packet: Packet,
    },
    LinkUp {
        peer: u32,
    },
    Retry,
    Delete {
        key: Key,
    },
    Status {
        key: Key,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub key: Option<Key>,
    pub at: Option<u32>,
    pub tracked: bool,
    pub physical: Option<BoxId>,
    pub created: bool,
    pub ended: bool,
    pub offer: Option<Offer>,
    pub observations: Vec<LaunchObservation>,
    /// Typed failures from actual runtime operations, encoded by the existing refusal codec.
    pub refusals: Vec<Vec<u8>>,
}
