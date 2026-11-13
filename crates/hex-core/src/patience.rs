//! How long a wait for an answer lasts (decision 9 of docs/design/runtime-architecture.md; Ada's rules of 2026-11-06
//! and 2026-11-13: no picked constant, and no value measured on our machines and baked in, per profile, OS or build:
//! "citing a number doesn't count as derived; an estimator of what's seen does"). Pure: the shell times each answer on
//! its monotonic clock and reports it ([`Patience::answered`]), and asks how long the next wait lasts
//! ([`Patience::deadline`]). One type for every wait the node times; shared by threads as it is (`&self`, atomic
//! words), so a static may hold it.
//!
//! - **Learned**: once this process has an answer, a wait's bound is the slowest answer it has seen, a running maximum
//!   that only rises: the least bound of least expected cost per call where no answer fails to come, as the waits
//!   census measures them (decision 9). No factor, no margin: past the slowest of `n` answers no bound holds whatever
//!   their distribution (Wilks, 1941), and the next answer outruns them with chance at most `1 / (n + 1)`.
//! - **Following** (the cold start): before its first answer a wait has no bound ([`Patience::deadline`] is `None`). It
//!   follows its party: it lasts while the party is alive and advancing, and is refused, typed, at the party's death, a
//!   reset or a stall of its own measured progress ([`Follow`]). No record is read: a record is another machine's
//!   answers, which the waits census keeps as evidence of the method, never as a seed.
//! - **Past W*** (the person's ruling of 2026-11-13): W*, the slowest answer seen, is a bound only while answers come
//!   within it. A wait past it is a run-out, no sample (Karn's point), told to the census ([`Patience::ran_out`]), and
//!   it refuses nothing by itself: the wait goes on following its party ([`Patience::follow`]), refused only where no
//!   progress shows over a whole W* window, or at the party's end or a reset. Its answer comes in uncensored, and W*
//!   rises to it. No factor: the following wait measures how long to wait.
//! - **After a run-out**: the tail it ran into is unknown, the cold start's case, so the next wait follows its party
//!   from its start ([`Patience::ran_out`], [`Patience::expired`]) until an answer comes.
//! - **The census**: each wait is named, and every answer it records goes to the census sink as well, where a shell
//!   installs one ([`census`]): the waits census probe's record of each wait's distribution
//!   (`bench/competitive/probes/waits_census.md`), from the code's own timing points, the evidence decision 9's rule is
//!   held to. A wait that ran out for a slow answer tells the census too ([`Patience::ran_out`]); a party taken for
//!   dead or hung tells it nothing ([`Patience::expired`]): it says nothing of how long an answer takes. Where the
//!   census runs, every wait follows its party, unarmed ([`measure_all`]), so the distribution keeps its tail.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

/// What a wait tells the census: an answer it got, the time after its wait began, or that it ran out, the bound that
/// passed with no answer, the least its slowest is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sample {
    Answered(Duration),
    RanOut(Duration),
    /// A completed typed refusal: learned exactly as an answer, separately marked for census analysis.
    Refused(Duration),
}

/// Where every named wait's answers and run-outs go besides its own maximum, once a shell installs it: none unless one
/// does.
static CENSUS: OnceLock<fn(&'static str, Sample)> = OnceLock::new();

/// Whether every wait in this process measures, unarmed: the waits census's, so it records the answers past a bound,
/// which a wait that runs out never gives (Karn's point).
static MEASURE_ALL: AtomicBool = AtomicBool::new(false);

/// Every wait in this process measures from now on, never armed, its answers only recorded: the waits census's
/// (`bench/competitive/probes/waits_census.md`), whose shell calls it as it installs its sink, so the distribution it
/// records has its tail.
pub fn measure_all() {
    MEASURE_ALL.store(true, Ordering::Relaxed);
}

/// Whether every wait in this process measures ([`measure_all`]): the waits census runs, so a wait whose measure has
/// no record yet may measure too, its first record taken there (the Shards image class's waits).
pub fn measures_all() -> bool {
    MEASURE_ALL.load(Ordering::Relaxed)
}

/// Installs `sink` to be told every answer every wait records, and every run-out, by the wait's name: the waits census
/// probe's. Only the first install takes; says whether this one did.
pub fn census(sink: fn(&'static str, Sample)) -> bool {
    CENSUS.set(sink).is_ok()
}

/// One sample as the census's file keeps it, an answer `name nanos`, refusal `name !nanos`, or run-out `name >nanos`, and a newline: the
/// line every census sink writes (the node's, `hex_node::census`, and the tests', `hex_testkit::waits`), so
/// `waits_census.py` reads one form.
pub fn census_line(name: &str, sample: Sample) -> String {
    match sample {
        Sample::Answered(took) => format!("{name} {}\n", took.as_nanos()),
        Sample::RanOut(after) => format!("{name} >{}\n", after.as_nanos()),
        Sample::Refused(took) => format!("{name} !{}\n", took.as_nanos()),
    }
}

/// The line a census sink writes once it has written its `max` lines, then nothing more: the record says it was
/// cut (CLAUDE.md §3), and `waits_census.py` says so for the arm.
pub fn census_full_line(max: u64) -> String {
    format!("census full at {max} lines\n")
}

/// What a following wait's party is doing as its shell looks at it: alive, with how far it has got (a count that only
/// rises while it works: its CPU time, the bytes it has sent, the entries listed); alive with no such count its shell
/// can read; or ended (it died, or its connection was reset or closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Party {
    Alive {
        progress: u64,
    },
    /// Alive, its progress beyond its shell's count: followed by its life alone, never taken for stalled.
    Live,
    Ended,
}

/// Why a following wait ended with no answer: the refusal's cause, typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stopped {
    /// The party died, or its connection was reset or closed.
    Ended,
    /// The party is alive but its progress has stood still since `since`, longer than a whole `window`: W*, the slowest
    /// answer its wait has seen, past it; at the cold start, the longest it went between advances in this wait.
    Stalled { since: Duration, window: Duration },
}

/// A following wait: it lasts while its party is alive and advancing, refused only at the party's end, a reset, or a
/// stall, its progress standing longer than a whole window (the person's ruling of 2026-11-13, as decision 9 builds it):
/// - **Past W*** ([`Follow::past`], [`Patience::follow`]): W* is a bound only while answers come within it, and past it
///   the window is W* itself.
/// - **At the cold start** ([`Follow::new`]), where no answer has been seen, the window is what the wait measures as it
///   runs, the longest the party has gone between advances. Until it has measured such a gap it judges no stall, only
///   the party's end.
/// - A party with no progress count ([`Party::Live`]) is followed by its life alone.
///
/// Pure: the shell looks at the party (its pidfd, process handle or connection state, and its progress count) on its
/// monotonic clock, at the party's own events and at [`Follow::stalls_after`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Follow {
    /// W*, the window a stall is judged over past it; none at the cold start.
    window: Option<Duration>,
    /// The progress at the last advance, and when it was seen.
    last: Option<(u64, Duration)>,
    /// The longest gap between advances measured in this wait.
    longest: Duration,
}

impl Follow {
    /// A following wait with nothing seen of its party yet, and no answer seen of its wait: the cold start's.
    pub const fn new() -> Self {
        Self {
            window: None,
            last: None,
            longest: Duration::ZERO,
        }
    }

    /// A following wait past `window`, W*, the slowest answer its wait has seen: refused, stalled, only once its
    /// party's progress stands longer than a whole window.
    pub const fn past(window: Duration) -> Self {
        Self {
            window: Some(window),
            last: None,
            longest: Duration::ZERO,
        }
    }

    /// The window a stall is judged over now: W* past it, else the longest gap measured, none before one is.
    fn window(&self) -> Option<Duration> {
        if self.window.is_some() || self.longest.is_zero() {
            return self.window;
        }
        Some(self.longest)
    }

    /// The party as looked at `now`: `Ok` to wait on, or why the wait is refused. An advance (more progress than at
    /// the last) measures the gap since the last advance; a look with none judges a stall only past a whole window.
    pub fn look(&mut self, party: Party, now: Duration) -> Result<(), Stopped> {
        let progress = match party {
            Party::Ended => return Err(Stopped::Ended),
            Party::Live => return Ok(()),
            Party::Alive { progress } => progress,
        };
        match self.last {
            Some((was, at)) if progress > was => {
                self.longest = self.longest.max(now.saturating_sub(at));
                self.last = Some((progress, now));
                Ok(())
            }
            Some((_, at)) => match self.window() {
                Some(window) if now.saturating_sub(at) > window => {
                    Err(Stopped::Stalled { since: at, window })
                }
                _ => Ok(()),
            },
            None => {
                self.last = Some((progress, now));
                Ok(())
            }
        }
    }

    /// When the party, advancing no further, is taken for stalled: its last advance plus the window. `None` before a
    /// progress count has been seen, or at the cold start before a gap is measured, when only the party's end or its
    /// answer ends the wait.
    pub fn stalls_after(&self) -> Option<Duration> {
        let (_, at) = self.last?;
        self.window().map(|window| at.saturating_add(window))
    }
}

/// A wait's bound, learned from its answers, following its party where it has none ([module docs](self)).
#[derive(Debug)]
pub struct Patience {
    /// The wait's name, as the census records it: dotted, as the node's event targets are.
    name: &'static str,
    /// The slowest answer this process has seen, in nanoseconds and one; 0 before any.
    seen: AtomicU64,
    /// Whether the next wait follows its party though an answer was seen: from a run-out until the next answer.
    following: AtomicBool,
}

impl Patience {
    /// The wait `name`, with no answer yet: it follows its party until its first answer ([`Follow`]).
    pub const fn measuring(name: &'static str) -> Self {
        Self {
            name,
            seen: AtomicU64::new(0),
            following: AtomicBool::new(false),
        }
    }

    /// For tests alone (cfg(test), or the `test-seeds` feature, which only hex-testkit's dependency turns on and which
    /// is a dev-dependency everywhere): the wait `name` as it would be once it had seen an answer of `slowest`. No
    /// production wait is seeded (decision 9).
    #[cfg(any(test, feature = "test-seeds"))]
    pub const fn seeded(name: &'static str, slowest: Duration) -> Self {
        Self {
            name,
            seen: AtomicU64::new(stored(slowest)),
            following: AtomicBool::new(false),
        }
    }

    /// An answer came `took` after its wait began: this process's slowest if it is slower than any before, the wait
    /// bounded again by the slowest, and told to the census where one is installed.
    pub fn answered(&self, took: Duration) {
        self.learn(took);
        if let Some(sink) = CENSUS.get() {
            sink(self.name, Sample::Answered(took));
        }
    }

    /// AC-T7.4.1: a completed typed refusal keeps exactly the same online maximum/deadline update
    /// as an answer. Only the optional census distinguishes it from successful completions.
    pub fn refused(&self, took: Duration) {
        self.learn(took);
        if let Some(sink) = CENSUS.get() {
            sink(self.name, Sample::Refused(took));
        }
    }

    fn learn(&self, took: Duration) {
        self.seen.fetch_max(stored(took), Ordering::Relaxed);
        self.following.store(false, Ordering::Relaxed);
    }

    /// The wait ran out, `after` passing with no answer: no sample (Karn's point), so the slowest seen stays, but the
    /// tail is unknown, so the next wait follows its party until an answer comes; told to the census where one is
    /// installed. A run-out refuses nothing by itself: the wait that ran out goes on, following its party past W*
    /// ([`Patience::follow`]), its answer the uncensored sample W* rises to. Said where the answer is slow, never where
    /// its party is taken for dead or hung ([`Patience::expired`]).
    pub fn ran_out(&self, after: Duration) {
        self.following.store(true, Ordering::Relaxed);
        if let Some(sink) = CENSUS.get() {
            sink(self.name, Sample::RanOut(after));
        }
    }

    /// The wait's bound passed and its party was taken for dead or hung (a seed's hang bound, a parked init's silence):
    /// no sample and nothing for the census, a hung party saying nothing of how long an answer takes, but the next
    /// wait follows its party, so a slow party past the slowest seen is waited for while it lives and advances.
    pub fn expired(&self) {
        self.following.store(true, Ordering::Relaxed);
    }

    /// How a wait follows its party once it has no bound left: past W*, the slowest answer seen, over a W* window
    /// ([`Follow::past`]); with no answer seen, by the gaps it measures ([`Follow::new`]).
    pub fn follow(&self) -> Follow {
        self.slowest().map_or(Follow::new(), Follow::past)
    }

    /// The wait's name.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The slowest answer this process has seen: `None` before any.
    pub fn slowest(&self) -> Option<Duration> {
        self.seen
            .load(Ordering::Relaxed)
            .checked_sub(1)
            .map(Duration::from_nanos)
    }

    /// How long the next wait lasts: the slowest answer seen; `None` where it follows its party ([`Follow`]): before
    /// any answer, from a run-out until the next answer, and where the census measures every wait ([`measure_all`]).
    pub fn deadline(&self) -> Option<Duration> {
        if self.following.load(Ordering::Relaxed) || MEASURE_ALL.load(Ordering::Relaxed) {
            return None;
        }
        self.slowest()
    }
}

/// `d` in nanoseconds and one, saturating: the form the word keeps, so 0 is none. Const for the constructors.
const fn stored(d: Duration) -> u64 {
    d.as_secs()
        .saturating_mul(1_000_000_000)
        .saturating_add(d.subsec_nanos() as u64)
        .saturating_add(1)
}

#[cfg(test)]
mod tests;
