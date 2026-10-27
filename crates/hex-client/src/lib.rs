//! Hex's `Runtime` seam over the node agent's local socket (decision 3 of
//! docs/design/runtime-architecture.md): a [`Client`] is a `hex_api::Runtime` whose boxes are a node
//! agent's in another process, reached through its Unix `SOCK_SEQPACKET` socket in
//! [`hex_core::wire`]'s format, so a caller that doesn't link Hex holds the same contract as one that
//! does (the conformance suite, HEX-RT-1 on). An exec's stdio goes to the agent as descriptors, and
//! its `Proc` waits for the program's end, which the agent sends. Every thread sharing a client shares
//! its one connection, which closes once the client and the last `Proc` it returned are dropped; the
//! agent then ends the programs it still watches for it (the disconnect rule).
//!
//! Linux only: the socket passes descriptors over `SOCK_SEQPACKET` (unix(7)), which macOS's and
//! Windows' local sockets lack; elsewhere [`Client::connect`] refuses, naming the platform.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::disallowed_macros
    )
)]

use std::time::Duration;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Client;

#[cfg(not(target_os = "linux"))]
mod other;
#[cfg(not(target_os = "linux"))]
pub use other::Client;

/// Longest a call waits for the node agent's answer, and a send for room in its socket: five times the
/// agent's own longest wait (a delete's 60 s for its box to empty, `hex_node`'s `EMPTY_PATIENCE`), so
/// only a wedged agent misses it, and a call to one is refused rather than waited on forever.
pub const ANSWER_PATIENCE: Duration = Duration::from_secs(300);

/// Most boxes one list takes, in however many messages they come: a node holding more is past any
/// density Hex targets (10,560 idle containers per GiB held, docs/design/runtime-architecture.md),
/// so only a hostile agent sends them, and the list is refused.
pub const MAX_LIST: usize = 1 << 20;
