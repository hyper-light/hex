//! How long a wait for an answer lasts (decision 9 of docs/design/runtime-architecture.md; Ada's 2026-11-06 rule, no
//! picked constants, and the lead's failure-bound rule of the same day). Pure: the shell times each answer on its
//! monotonic clock and reports it ([`Patience::answered`]), and asks how long the next wait lasts
//! ([`Patience::deadline`]). One type for every wait the node times, the seeds', the turns', an emptying's, a
//! listing's and the network check's; shared by threads as it is (`&self`, one atomic word), so a static may hold it.
//!
//! - **Seeded from a record**: before this node has an answer of its own, a wait's bound is [`TIMES_SLOWEST`] times
//!   the slowest answer in that wait's recorded distribution for the host class, a named record cited where the
//!   wait is made ([`Patience::seeded`]). No calibration read, no unbounded first wait.
//! - **Learned**: the bound is [`TIMES_SLOWEST`] times the larger of the record's slowest and this node's slowest
//!   answer, a running maximum, so it only rises. A wait runs out only for an answer more than twice slower than any
//!   recorded or seen.
//! - **Measuring**: a wait whose distribution has no record yet says so ([`Patience::measuring`], with the record to
//!   come named at the wait). Its bound is unarmed until this node's first answer: no value is picked for it.
//! - **Karn's point**: a wait that ran out is no sample, nor is an answer from a server taken for dead or hung; the
//!   shell reports only answers it waited for and got.
//! - **The census**: each wait is named, and every answer it records goes to the census sink as well, where a shell
//!   installs one ([`census`]): the waits census probe's record of each wait's distribution
//!   (`bench/competitive/probes/waits_census.md`), from the code's own timing points.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// How many times the slowest answer a wait lasts: twice, so a wait runs out only for an answer more than twice
/// slower than any recorded or seen. The lead's failure-bound rule (2026-11-06), stated here.
pub const TIMES_SLOWEST: u32 = 2;

/// Where every named wait's answers go besides its own maximum, once a shell installs it: none unless one does.
static CENSUS: OnceLock<fn(&'static str, Duration)> = OnceLock::new();

/// Installs `sink` to be told every answer every wait records, by the wait's name: the waits census probe's. Only
/// the first install takes; says whether this one did.
pub fn census(sink: fn(&'static str, Duration)) -> bool {
    CENSUS.set(sink).is_ok()
}

/// A wait's bound, seeded from a record and learned from its answers ([module docs](self)).
#[derive(Debug)]
pub struct Patience {
    /// The wait's name, as the census records it: dotted, as the node's event targets are.
    name: &'static str,
    /// The record's slowest answer, in nanoseconds and one; 0 while measuring.
    recorded: u64,
    /// This node's slowest answer, in nanoseconds and one; 0 before any.
    seen: AtomicU64,
}

impl Patience {
    /// The wait `name`, whose recorded distribution's slowest answer for this host class is `slowest`, cited at the
    /// caller.
    pub const fn seeded(name: &'static str, slowest: Duration) -> Self {
        Self {
            name,
            recorded: stored(slowest),
            seen: AtomicU64::new(0),
        }
    }

    /// The wait `name`, with no record yet (MEASURING, the record to come named at the caller): unarmed until its
    /// first answer.
    pub const fn measuring(name: &'static str) -> Self {
        Self {
            name,
            recorded: 0,
            seen: AtomicU64::new(0),
        }
    }

    /// An answer came `took` after its wait began: this node's slowest if it is slower than any before, and told
    /// to the census where one is installed.
    pub fn answered(&self, took: Duration) {
        self.seen.fetch_max(stored(took), Ordering::Relaxed);
        let _ = CENSUS.get();
    }

    /// The wait's name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The slowest answer, the record's or this node's, whichever is slower: `None` while measuring with none seen.
    pub fn slowest(&self) -> Option<Duration> {
        let slowest = self.recorded.max(self.seen.load(Ordering::Relaxed));
        slowest.checked_sub(1).map(Duration::from_nanos)
    }

    /// How long the next wait lasts: [`TIMES_SLOWEST`] times [`Patience::slowest`], `None` while unarmed.
    pub fn deadline(&self) -> Option<Duration> {
        self.slowest()
            .map(|slowest| slowest.saturating_mul(TIMES_SLOWEST))
    }
}

/// `d` in nanoseconds and one, saturating: the form both words keep, so 0 is none. Const for the constructors.
const fn stored(d: Duration) -> u64 {
    d.as_secs()
        .saturating_mul(1_000_000_000)
        .saturating_add(d.subsec_nanos() as u64)
        .saturating_add(1)
}

#[cfg(test)]
mod tests;
