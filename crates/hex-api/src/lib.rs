//! Hex's seam (CLAUDE.md §9): the [`Runtime`] trait every runtime class implements, the types it
//! speaks, a simulator implementation ([`sim::SimRuntime`]) and the conformance suite
//! ([`conformance`]) that both it and every real runtime pass, and the [`differential`] suite that
//! holds each real class to the simulator's trace. Other products reach Hex only through
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
pub mod differential;
mod id;
mod runtime;
pub mod sim;
mod spec;

pub use bounds::{Bounds, IoRates, MIN_CPU_MILLI, MIN_MEMORY};
pub use id::BoxId;
pub use runtime::{
    Asked, BoxInfo, BoxState, Exit, Membership, Proc, Refusal, Runtime, Signal, Usage,
};
pub use spec::{
    Ask, BlockCount, BoxOwner, BoxSpec, Class, ClassSpec, End, ExecLimit, Given, Mount, Network,
    OciSpec, Program, Rootfs, Stdio, StringUnit, User, drive_absolute,
};
