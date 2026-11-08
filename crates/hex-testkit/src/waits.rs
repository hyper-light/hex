//! What the tests' waits take (decision 9 of docs/design/runtime-architecture.md; the lead's rules for test patiences
//! of 2026-11-06 and 2026-11-08: every wait in the tests uses its own record's patience). A test waits for a fact for
//! at most twice the slowest answer recorded for that fact, so a slow but healthy fact passes and only a stuck one
//! fails:
//! - a fact a census row records takes that row's record, here or `hex_api::conformance::PROGRAM_END` (a program's
//!   end);
//! - a fact one node operation makes within itself takes that operation's record, the relation stated where it is
//!   waited for and tested there (a hook holding a create is bounded by the create's answer);
//! - a fact no recorded operation makes is waited for measuring, for the fact alone, its answers recorded for the
//!   census that will seed it ([`hex_core::patience::Patience::measuring`], named `test.<crate>.<fact>`).
//!
//! The tests run debug builds, so each record here is the waits census's debug arm's
//! (`bench/competitive/probes/waits_census.md`, run 3, waits-census-20261108T025856Z on fedora, at 946e06f), named by
//! its row, in nanoseconds as the census keeps them. [`until`] waits for a fact on a [`Patience`] seeded from one of
//! them or measuring; where the census probe runs the suites (`HEX_WAITS_CENSUS`), every answer goes to its file, so
//! the tests' own waits have rows of their own.

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use hex_core::patience::Patience;

/// `lifecycle.exec`: a program's run to its end (39.131 ms of 10,000), the record of `hex_api::conformance::PROGRAM_END`
/// too, here for a program a test runs as a child of its own (`std::process::Child`) rather than through a `Proc`:
/// its end, and what it does on its way there.
pub const PROGRAM: Duration = Duration::from_nanos(39_130_836);
/// `node.emptying`: a box's kill to its cgroup's `populated 0` (127.753 ms of 22,092), what a test waits on for a
/// killed box's processes to end, its emptying or the end of a process killed with it.
pub const EMPTYING: Duration = Duration::from_nanos(127_752_710);
/// `netns.make`: the pool's refill thread's make of a network namespace (120.814 ms of 47,210).
pub const NETNS_MAKE: Duration = Duration::from_nanos(120_814_130);
/// `store.turns`: a record change's wait for the same box's, a held turn's write and its fsyncs (54.401 ms of
/// 139,118).
pub const STORE_TURN: Duration = Duration::from_nanos(54_401_152);
/// `node.turns`: a create's or delete's wait for the same box's, the one under way's hold of its turn (305.809 ms of
/// 92,042).
pub const NODE_TURN: Duration = Duration::from_nanos(305_809_337);
/// `client.answer.create`: a create's answer over the node's socket (220.155 ms of 46,021), which bounds an
/// in-process create's, and every fact a create makes within itself.
pub const CREATE: Duration = Duration::from_nanos(220_155_458);
/// `client.answer.start`: a start's answer (7.610 ms of 20,021), and every fact a start makes within itself.
pub const START: Duration = Duration::from_nanos(7_610_223);
/// `client.answer.delete`: a delete's answer (306.052 ms of 46,021), and every fact a delete makes within itself.
pub const DELETE: Duration = Duration::from_nanos(306_051_519);
/// `client.answer.exec`: an exec's answer (38.754 ms of 10,020), once its program is started.
pub const EXEC: Duration = Duration::from_nanos(38_753_534);
/// `node.listing`: a recovery's listing, an entry (68.562 us of 1,105), what a restart's recovery takes for each
/// entry of the directories it lists.
pub const LISTING_ENTRY: Duration = Duration::from_nanos(68_562);

/// A wait's fact didn't come within its patience.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waited {
    /// The fact waited for.
    pub what: String,
    /// How long it was waited for: twice the slowest answer recorded or seen.
    pub patience: Duration,
}

impl std::fmt::Display for Waited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no {} within {:?}, twice the slowest recorded",
            self.what, self.patience
        )
    }
}

/// Waits for `fact`, giving the CPU up between looks rather than sleeping a picked period, for at most `wait`'s
/// deadline (none while it measures: the fact alone); says how many looks it took, and tells `wait`, and the census
/// where the probe runs the suites, how long. Refused, naming `what` and the patience, once the deadline has passed
/// without the fact; a wait that ran out is no sample (Karn's point).
pub fn until(wait: &Patience, what: &str, mut fact: impl FnMut() -> bool) -> Result<u64, Waited> {
    census();
    let began = Instant::now();
    let patience = wait.deadline();
    let deadline = patience.and_then(|p| began.checked_add(p));
    let mut looks = 0u64;
    loop {
        looks = looks.saturating_add(1);
        if fact() {
            wait.answered(began.elapsed());
            return Ok(looks);
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(Waited {
                what: what.to_owned(),
                patience: patience.unwrap_or(Duration::MAX),
            });
        }
        std::thread::yield_now();
    }
}

/// An answer a test timed itself, a fact it waited for its own way, told to `wait` and to the census where the probe
/// runs the suites, as [`until`] tells it.
pub fn answered(wait: &Patience, took: Duration) {
    census();
    wait.answered(took);
}

/// The variable naming the census's file, and the one bounding its lines: the node's (`hex_node::census`).
pub const CENSUS_FILE: &str = "HEX_WAITS_CENSUS";
/// The most lines this process writes to the census.
pub const CENSUS_MAX: &str = "HEX_WAITS_CENSUS_MAX";

static OUT: OnceLock<Option<Mutex<File>>> = OnceLock::new();
static MAX: AtomicU64 = AtomicU64::new(0);
static LINES: AtomicU64 = AtomicU64::new(0);

/// The census of the tests' own waits, installed at the first wait where the probe runs the suites: [`CENSUS_FILE`]
/// names its file, [`CENSUS_MAX`] its most lines, as for the node. Unlike the node's, it disarms no wait: a test's
/// patiences keep their bounds, and its answers are only recorded. Where either is unset, or the file won't open, it
/// installs nothing and the waits run as they do without it.
fn census() {
    let out = OUT.get_or_init(|| {
        let file = std::env::var_os(CENSUS_FILE)?;
        let max: u64 = std::env::var(CENSUS_MAX).ok()?.parse().ok()?;
        let out = File::options().create(true).append(true).open(file).ok()?;
        MAX.store(max, Ordering::Relaxed);
        Some(Mutex::new(out))
    });
    if out.is_some() {
        hex_core::patience::census(record);
    }
}

/// One answer, appended as the census keeps it; past the bound, one line saying the census is full, then nothing.
fn record(name: &'static str, took: Duration) {
    let Some(Some(out)) = OUT.get() else {
        return;
    };
    let n = LINES.fetch_add(1, Ordering::Relaxed);
    let max = MAX.load(Ordering::Relaxed);
    let line = if n < max {
        hex_core::patience::census_line(name, took)
    } else if n == max {
        hex_core::patience::census_full_line(max)
    } else {
        return;
    };
    if let Ok(mut out) = out.lock() {
        let _ = out.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests;
