//! Hex's runtime classes (docs/design/runtime.md): the `process` class, a resource box holding host
//! processes (cgroup v2 on Linux), and the `oci` class, an OCI runtime-spec container run by Hex's
//! own runtime (Linux). `hex-node` holds the records and drives these; they keep no state of their
//! own beyond what the kernel holds.

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

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(not(target_os = "linux"))]
pub mod elsewhere;
