//! Hex's seam (CLAUDE.md §9): the [`Runtime`] trait every runtime class implements, the types it
//! speaks, a simulator implementation ([`sim::SimRuntime`]) and the conformance suite
//! ([`conformance`]) that both it and every real runtime pass. Other products reach Hex only through
//! this crate.
//!
//! A box is the unit Hex runs: a resource bound ([`Bounds`]) holding processes. The `process` class
//! holds host processes in it; the `oci` class holds an OCI container (CP-T7.1, docs/design/).

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

mod bounds;
pub mod conformance;
mod id;
mod runtime;
pub mod sim;
mod spec;

pub use bounds::{Bounds, IoRates};
pub use id::BoxId;
pub use runtime::{BoxInfo, BoxState, Exit, Proc, Refusal, Runtime, Signal, Usage};
pub use spec::{BoxSpec, Class, ClassSpec, Mount, Network, OciSpec, Program, Rootfs, Stdio, User};
