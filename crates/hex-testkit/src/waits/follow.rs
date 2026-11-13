//! The tests' waits as decision 9 waits them (the person's ruling of 2026-11-13; `docs/research/runtime/learned-waits.md`
//! "Owed", 3): each follows its fact's party, what makes the fact true as the test can look at it. W* is a bound only
//! while answers come within it:
//! - **Within W***, the slowest answer the wait has seen, the fact is the answer.
//! - **Past W*** the run-out is told to the census, once and with no sample, and never fails the test: the wait goes on
//!   while its party lives and advances, and fails ([`Unmet`]) only at its party's end or a whole window of W* with no
//!   advance ([`Follow`]).
//! - **With no W*** (before the fact's first answer, and from a run-out to the next) it follows its party from its
//!   start ([`Patience::follow`]).
//! - **Its answer**, however long it took, is the sample W* rises to.
//!
//! A party ([`Party`]) is [`Party::Ended`] once nothing can make the fact true (the thread that would has finished,
//! the child is reaped, the connection closed), [`Party::Alive`] with a count that rises while it works, or
//! [`Party::Live`] where its work shows no count the test can read, followed by its life alone. [`Thread`] gives a
//! thread's. The waits of [`super`] that take no party go as their tests move here.

use std::fmt;
use std::time::{Duration, Instant};

use hex_core::patience::{Follow, Patience};
pub use hex_core::patience::{Party, Stopped};

/// A wait's fact that didn't come: its party ended, or stood still over a whole window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmet {
    /// The fact waited for.
    pub what: String,
    /// Why the wait ended without it.
    pub stopped: Stopped,
    /// How long it was waited for.
    pub after: Duration,
}

impl fmt::Display for Unmet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.stopped {
            Stopped::Ended => write!(
                f,
                "no {} after {:?}: its party ended without it",
                self.what, self.after
            ),
            Stopped::Stalled { since, window } => write!(
                f,
                "no {} after {:?}: its party stood still from {since:?} on, past a whole window of {window:?}",
                self.what, self.after
            ),
        }
    }
}

/// Waits for `fact`, giving the CPU up between looks, as the [module](self) says: `party` is looked at beside it, and
/// the looks it took are returned. A party that ends is taken for ended only once a last look at the fact, as it ends,
/// hasn't found it.
pub fn until(
    wait: &Patience,
    what: &str,
    mut party: impl FnMut() -> Party,
    mut fact: impl FnMut() -> bool,
) -> Result<u64, Unmet> {
    super::census();
    let mut waiting = Waiting::new(wait);
    let mut looks = 0u64;
    loop {
        looks = looks.saturating_add(1);
        if fact() {
            wait.answered(waiting.began.elapsed());
            return Ok(looks);
        }
        if let Err(stopped) = waiting.look(wait, party()) {
            if fact() {
                wait.answered(waiting.began.elapsed());
                return Ok(looks.saturating_add(1));
            }
            return Err(waiting.unmet(what, stopped));
        }
        std::thread::yield_now();
    }
}

/// Waits for an answer through `call`, a wait that takes its own deadline (a read's, a channel's), as the
/// [module](self) says: `call` is given the end of each slice (W* from the start; past it, where its party would be
/// taken for stalled; `None` with no window, when its own answer or its party's end, as `call` sees it, ends it) and
/// gives its answer, or `None` where the slice ended first; `party` is looked at between slices. A party that ends
/// is taken for ended only once a last slice, with no time left, hasn't given the answer.
pub fn within<T>(
    wait: &Patience,
    what: &str,
    mut party: impl FnMut() -> Party,
    mut call: impl FnMut(Option<Instant>) -> Option<T>,
) -> Result<T, Unmet> {
    super::census();
    let mut waiting = Waiting::new(wait);
    let mut by = waiting.first();
    loop {
        if let Some(answer) = call(by) {
            wait.answered(waiting.began.elapsed());
            return Ok(answer);
        }
        match waiting.look(wait, party()) {
            Ok(next) => by = next,
            Err(stopped) => {
                // The answer may have come as its party ended: one more look, with no time left.
                if let Some(answer) = call(Some(Instant::now())) {
                    wait.answered(waiting.began.elapsed());
                    return Ok(answer);
                }
                return Err(waiting.unmet(what, stopped));
            }
        }
    }
}

/// A wait under way: within its W* until that passes, then following its party.
struct Waiting {
    began: Instant,
    /// W*, where the wait has one.
    bound: Option<Duration>,
    /// How it follows its party: from its start with no W*, from W*'s end with one.
    follow: Option<Follow>,
}

impl Waiting {
    fn new(wait: &Patience) -> Self {
        let bound = wait.deadline();
        Self {
            began: Instant::now(),
            bound,
            follow: bound.is_none().then(|| wait.follow()),
        }
    }

    /// When the first look comes at the latest: W*'s end, or none with no W*.
    fn first(&self) -> Option<Instant> {
        self.bound.and_then(|bound| self.began.checked_add(bound))
    }

    /// The party as looked at now: when to look next at the latest (`None`: no time of its own, only the next look),
    /// or why the wait is over. Past W* its run-out is told, once.
    fn look(&mut self, wait: &Patience, party: Party) -> Result<Option<Instant>, Stopped> {
        let now = self.began.elapsed();
        if self.follow.is_none()
            && let Some(bound) = self.bound
            && now >= bound
        {
            // No sample (Karn's point), but the census is told, and the wait goes on.
            wait.ran_out(bound);
            self.follow = Some(Follow::past(bound));
        }
        let Some(follow) = self.follow.as_mut() else {
            // Within W*: only the party's end ends the wait.
            return match party {
                Party::Ended => Err(Stopped::Ended),
                Party::Alive { .. } | Party::Live => Ok(self.first()),
            };
        };
        follow.look(party, now)?;
        Ok(follow
            .stalls_after()
            .and_then(|at| self.began.checked_add(at)))
    }

    fn unmet(&self, what: &str, stopped: Stopped) -> Unmet {
        Unmet {
            what: what.to_owned(),
            stopped,
            after: self.began.elapsed(),
        }
    }
}

/// A thread a test follows: captured on the thread itself ([`Thread::current`]) and looked at from any other
/// ([`Thread::party`]).
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thread(sys::Id);

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Thread {
    /// The calling thread.
    pub fn current() -> std::io::Result<Self> {
        sys::current().map(Self)
    }

    /// The thread as a party: ended once it has gone, which comes a moment after its join, as its task exits;
    /// [`Party::Live`] while it runs or waits for a core to run on, so a loaded host's queue is never taken for a stall;
    /// else alive, its count its CPU time, which stands while it sleeps. Linux reads `/proc/self/task/<tid>/stat` and `schedstat`; macOS `proc_pidinfo`'s
    /// `PROC_PIDTHREADID64INFO`.
    pub fn party(&self) -> Party {
        sys::party(self.0)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod sys;

#[cfg(test)]
mod tests;
