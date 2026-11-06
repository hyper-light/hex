//! How long a wait for an answer lasts, learned from the answers before it (decision 9 of
//! docs/design/runtime-architecture.md; Ada's 2026-11-06 rule, no picked constants, and the lead's failure-bound
//! rule of the same day). Pure: the shell times each answer on its monotonic clock and reports it
//! ([`Patience::answered`]), and asks how long the next wait lasts ([`Patience::deadline`]).
//!
//! - **The rule**: a wait lasts [`TIMES_SLOWEST`] times the slowest answer recorded, a running maximum. A wait runs
//!   out only for an answer more than twice slower than any this node has seen, which is a fault to report, not a
//!   limit to raise (CLAUDE.md §4). The bound only grows: a slow answer that came is a fact about this host, and a
//!   later run of fast ones doesn't make it less possible.
//! - **Calibration**: with no answer recorded there is no bound to arm ([`Patience::deadline`] is `None`). The shell
//!   makes one calibration read first, the same operation the wait bounds, timed with no bound, and records it, as
//!   goal 5's disk probe measures the disk before trusting it; each shell says where its read runs.
//! - **Karn's point**: an answer that never came is no sample; nor is one from a server taken for dead or hung, whose
//!   time measures nothing the next wait waits on. The shell reports only answers it waited for and got.

use std::time::Duration;

/// How many times the slowest answer recorded a wait lasts: twice, so a wait runs out only for an answer more than
/// twice slower than any seen. The lead's failure-bound rule (2026-11-06), stated here.
pub const TIMES_SLOWEST: u32 = 2;

/// A wait learned from its answers ([module docs](self)).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Patience {
    /// The slowest answer recorded, once one has been.
    slowest: Option<Duration>,
}

impl Patience {
    /// A wait with no answer recorded: uncalibrated. Const, so a shell may keep one in a static.
    pub const fn new() -> Self {
        Self { slowest: None }
    }

    /// An answer came `took` after its wait began: the slowest recorded if it is slower than any before.
    pub fn answered(&mut self, took: Duration) {
        self.slowest = Some(self.slowest.map_or(took, |slowest| slowest.max(took)));
    }

    /// The slowest answer recorded: what the node's metrics read.
    pub fn slowest(&self) -> Option<Duration> {
        self.slowest
    }

    /// How long the next wait lasts: [`TIMES_SLOWEST`] times the slowest answer recorded, or `None` before any,
    /// when the shell's wait is its calibration read.
    pub fn deadline(&self) -> Option<Duration> {
        self.slowest
            .map(|slowest| slowest.saturating_mul(TIMES_SLOWEST))
    }
}

#[cfg(test)]
mod tests;
