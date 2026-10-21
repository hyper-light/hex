//! The Linux runtime: cgroup v2 boxes (`cgroup`), processes watched through pidfds (`proc`), the
//! disks under a path (`disks`), and the few system calls that need `unsafe` (`sys`, named in
//! CLAUDE.md §12).

pub mod cgroup;
pub mod disks;
pub mod oci;
pub mod proc;
pub mod process;
pub mod sys;

use hex_api::Refusal;

/// An I/O error from `op` on `path`, as a typed refusal.
pub(crate) fn os_err(
    op: &'static str,
    path: &std::path::Path,
    e: impl std::fmt::Display,
) -> Refusal {
    Refusal::Os {
        op,
        why: format!("{}: {e}", path.display()),
    }
}
