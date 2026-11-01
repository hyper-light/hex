//! Hex's sans-IO cores (CLAUDE.md §9): what to write to a box's cgroup for its bounds, what its
//! cgroup files say it used, the runtime-spec bundle an `oci` box's structured spec renders to, the
//! record the node agent keeps of each box, the plan that takes every box back or removes it after
//! the node agent crashed (T-T7.1.2), the pool of network namespaces it makes ahead for its
//! containers ([`netns`]), the seeds it keeps to clone them ([`seeds`]), the node agent's socket
//! as bytes ([`wire`]), the rules a macOS box is bounded by — rootless by polling ([`watch`]) and
//! under a privileged helper by the kernel ([`boxuser`]) — macOS's signal numbers ([`darwin`]), and
//! the macOS box helper's protocol ([`helper`]). Shells in `hex-runtime`, `hex-node` and `hex-client`
//! do the I/O these decide.

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

pub mod boxuser;
pub mod bundle;
pub mod cgroup;
pub mod darwin;
pub mod helper;
pub mod netns;
pub mod record;
pub mod recovery;
pub mod seed;
pub mod seeds;
pub mod watch;
pub mod wire;
