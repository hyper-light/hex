//! Hex's sans-IO cores (CLAUDE.md §9): what to write to a box's cgroup for its bounds, what its
//! cgroup files say it used, the record the node agent keeps of each box, and the plan that takes
//! every box back or removes it after the node agent crashed (T-T7.1.2). Shells in `hex-runtime`
//! and `hex-node` do the I/O these decide.

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

pub mod cgroup;
pub mod record;
pub mod recovery;
