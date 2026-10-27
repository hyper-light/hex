//! Hex's node agent ("hexlet", CP-T7.1): it drives a runtime class on one node, keeps a record of
//! every box that its own crash leaves whole ([`store`]), and after a restart takes each box back or
//! cleans it up so none is leaked ([`recover`], T-T7.1.2). The scheduling and control plane that drive
//! many nodes come in CP-T7.3; this is the node by itself.

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

pub mod store;

#[cfg(target_os = "linux")]
mod node;
#[cfg(target_os = "linux")]
pub mod supervise;

#[cfg(target_os = "linux")]
pub use node::{Node, RecoverySummary};
