//! AC-T7.4.1: Firecracker's Linux KVM backend, with exact preboot device attachment.
//! The existing native record service owns every claim; this backend owns only its VM effects.

mod config;
pub use config::{Config, PREBOOT_REVISION};

#[cfg(target_os = "linux")]
mod native;
#[cfg(target_os = "linux")]
mod transport;
#[cfg(target_os = "linux")]
mod vfio;
#[cfg(target_os = "linux")]
pub use native::FirecrackerRuntime;

pub use crate::devices::{Books, Transaction};
use hex_api::Refusal;

fn invalid(why: impl Into<String>) -> Refusal {
    Refusal::Invalid {
        what: "Firecracker backend",
        why: why.into(),
    }
}

#[cfg(target_os = "linux")]
fn exhausted(what: &'static str, bound: usize) -> Refusal {
    Refusal::Exhausted { what, bound }
}
