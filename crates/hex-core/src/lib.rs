//! Hex's sans-IO cores (CLAUDE.md §9): what to write to a box's cgroup for its bounds, what its
//! cgroup files say it used, the runtime-spec bundle an `oci` box's structured spec renders to, the
//! record the node agent keeps of each box, the plan that takes every box back or removes it after
//! the node agent crashed (T-T7.1.2), the pool of network namespaces it makes ahead for its
//! containers ([`netns`]), the seeds it keeps to clone them ([`seeds`]), the idle cgroups and network
//! namespaces it hands from box to box of one owner ([`reuse`], [`netns_reuse`]), and the node agent's socket as bytes ([`wire`]). Shells in `hex-runtime`, `hex-node` and `hex-client` do the I/O these decide.

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

pub mod bundle;
pub mod cgroup;
pub mod devfs;
pub mod netns;
pub mod netns_reuse;
pub mod record;
pub mod recovery;
pub mod reuse;
pub mod seed;
pub mod seeds;
pub mod wire;
