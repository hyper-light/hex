//! The `oci` class (CP-T7.1): an OCI Runtime Specification container, run by Hex's own runtime (the
//! decision, with its evidence, is docs/design/runtime.md). The lifecycle is the runtime-spec's —
//! `create`, `start`, `state`, `kill`, `delete` — so the opencontainers/runtime-tools validation
//! suite drives it unchanged (AC-T7.1.4).
//!
//! Layering, sans-IO core first (CLAUDE.md §9): [`spec`] parses `config.json`, [`state`] is the
//! on-disk container state, [`mount`] turns a spec's mounts and paths into an ordered plan of
//! operations, [`resources`] turns `linux.resources` into cgroup v2 writes and a device program. The
//! imperative shell that makes the cgroup and namespaces, pivots the root and runs the init is in
//! [`run`] and [`init`]; the system calls that need `unsafe` are in [`sys`].

pub mod caps;
pub mod init;
pub mod mount;
pub mod resources;
pub mod run;
pub mod spec;
pub mod state;
pub mod sys;

use hex_api::Refusal;

/// Why an OCI runtime operation was refused. OCI runtimes report failures on stderr with a non-zero
/// exit; this is Hex's typed form, mapped to that at the CLI edge.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OciError {
    #[error("config: {0}")]
    Config(String),
    #[error("no container {0}")]
    NoSuchContainer(String),
    #[error("container {0} exists already")]
    Exists(String),
    #[error("container {0} is {1}, not {2}")]
    WrongState(String, &'static str, &'static str),
    #[error("{op} failed: {why}")]
    Os { op: &'static str, why: String },
    #[error("the runtime's state is unusable: {0}")]
    Broken(String),
}

impl OciError {
    pub fn os(op: &'static str, err: impl std::fmt::Display) -> Self {
        Self::Os {
            op,
            why: err.to_string(),
        }
    }
}

impl From<OciError> for Refusal {
    fn from(e: OciError) -> Refusal {
        match e {
            OciError::Config(why) => Refusal::Invalid {
                what: "oci config",
                why,
            },
            OciError::NoSuchContainer(id) => Refusal::Os {
                op: "oci",
                why: format!("no container {id}"),
            },
            OciError::Exists(id) => Refusal::Os {
                op: "oci",
                why: format!("container {id} exists"),
            },
            OciError::WrongState(id, is, want) => Refusal::Os {
                op: "oci",
                why: format!("container {id} is {is}, not {want}"),
            },
            OciError::Os { op, why } => Refusal::Os { op, why },
            OciError::Broken(why) => Refusal::Broken(why),
        }
    }
}
