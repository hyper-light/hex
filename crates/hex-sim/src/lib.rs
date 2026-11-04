//! hex-sim: the simulated cluster the scheduler's bars are measured in (`docs/design/scheduler-architecture.md`,
//! "hex-sim: the measurement vehicle"; `docs/design/hex-sim.md`). A pure core (CLAUDE.md §9): one
//! seed, virtual time and a single-threaded event loop decide everything, so a run is reproducible
//! and a regression is a changed number, not noise.
//!
//! - [`model`]: nodes, workloads and their resources.
//! - [`index`]: the best-fit index both the scheduler and its serial specification place with.
//! - [`store`]: the control plane's durable records, committed by compare-and-set (S6).
//! - [`sim`]: the cluster: clients, a control plane of parallel deciders and an in-order validating
//!   committer, fake node agents, and the nemesis that crashes the control plane at a chosen write.
//! - [`spec`]: the serial specification (one scheduler, one cycle) as the oracle a run's log is
//!   checked against (`T-T7.4.1`), and the end-state check that nothing was lost or started twice
//!   (`AC-T7.3.3`).
//! - [`generate`]: generated clusters and workloads from a seed.
//! - [`replay`]: a recorded workload read from text, for replaying a real day.
//! - [`packing`]: stranded capacity at a load, the measure of `AC-T7.4.3`.

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

pub mod error;
pub mod generate;
pub mod index;
pub mod model;
pub mod packing;
pub mod replay;
pub mod rng;
pub mod sim;
pub mod spec;
pub mod store;
pub mod trace;

pub use error::SimError;
