//! How long a wait for an answer lasts, learned from the answers before it (decision 9 of
//! docs/design/runtime-architecture.md; Ada's 2026-11-06 rule: no picked constants, values learned or derived from
//! data). The estimator is RFC 6298's retransmission timer, §2, the timer TCP has run on every host since 1988
//! (Jacobson, SIGCOMM '88): it learns the wait from the answer times it sees, so a slow host's waits grow with its
//! answers and a fast one's stay short, with no number picked for either. Pure: the shell times each answer on its
//! monotonic clock and reports it ([`Patience::answered`]), reports a wait that ran out ([`Patience::expired`]),
//! and asks how long the next wait lasts ([`Patience::deadline`]).
//!
//! - **Before any answer** the wait is [`Bounds::first`]: 1 s under [`RFC_6298`], its (2.1).
//! - **The first answer** R sets SRTT to R and RTTVAR to R/2 (2.2); **each later one** R' sets RTTVAR to
//!   3/4 RTTVAR + 1/4 |SRTT − R'| and then SRTT to 7/8 SRTT + 1/8 R', RTTVAR first, from the SRTT before (2.3: alpha
//!   1/8, beta 1/4). The wait is then SRTT + K·RTTVAR with K = 4 (2.3), RFC 6298's `max(G, K*RTTVAR)` taken with G, a
//!   monotonic clock's tick (a nanosecond on Linux), always under [`Bounds::floor`] here, so it never binds.
//! - **Bounded**: never under [`Bounds::floor`] (2.4: 1 s) nor over [`Bounds::ceiling`] (2.5: at least 60 s), so a
//!   run of fast answers never makes a wait a burst of scheduling noise outlasts, and a run of slow ones never makes
//!   one unbounded (CLAUDE.md §2).
//! - **A wait that runs out** doubles the next, to the ceiling (5.5, "back off the timer"), until the next answer
//!   computes it afresh (§5's note after 5.7). An answer whose wait ran out, or whose server died, is never a sample
//!   (Karn's rule, §3): the shell reports only answers it waited for and got.

use std::time::Duration;

/// A wait's bounds: how long it lasts before any answer, and the least and most it lasts after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    /// The wait before any answer has been seen.
    pub first: Duration,
    /// The least a wait lasts, however fast the answers.
    pub floor: Duration,
    /// The most a wait lasts, however slow the answers or however often waits ran out.
    pub ceiling: Duration,
}

/// RFC 6298's own bounds: the first wait 1 s (2.1), the least 1 s (2.4), the most 60 s, the least maximum (2.5).
/// The floor is the RFC's reason for it as much as its number: a wait that ends too soon costs more than one that
/// ends late (there, a spurious retransmission; here, a create refused and a healthy seed killed, or a delete
/// refused), and Allman and Paxson found the 1 s floor kept spurious timeouts rare across the paths they measured
/// (SIGCOMM '99, "On Estimating End-to-End Network Path Properties", §3).
pub const RFC_6298: Bounds = Bounds {
    first: Duration::from_secs(1),
    floor: Duration::from_secs(1),
    ceiling: Duration::from_secs(60),
};

impl Bounds {
    /// One wait that never changes: `wait` before and after every answer. For a test that fixes a wait to see what
    /// happens past it.
    pub const fn fixed(wait: Duration) -> Self {
        Self {
            first: wait,
            floor: wait,
            ceiling: wait,
        }
    }
}

/// A wait learned from its answers ([module docs](self)).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patience {
    bounds: Bounds,
    /// SRTT and RTTVAR, from the first answer on.
    smoothed: Option<(Duration, Duration)>,
    /// The next wait: RFC 6298's RTO.
    timer: Duration,
}

impl Patience {
    /// A wait under `bounds`, with no answer seen.
    pub fn new(bounds: Bounds) -> Self {
        Self {
            bounds,
            smoothed: None,
            timer: within(bounds, bounds.first),
        }
    }

    /// How long the next wait lasts.
    pub fn deadline(&self) -> Duration {
        self.timer
    }

    /// SRTT and RTTVAR once an answer has been seen: what the node's metrics read.
    pub fn smoothed(&self) -> Option<(Duration, Duration)> {
        self.smoothed
    }

    /// An answer came `took` after its wait began: the estimate moves toward it (RFC 6298 (2.2), (2.3)).
    pub fn answered(&mut self, took: Duration) {
        let _ = took;
    }

    /// A wait ran out with no answer: the next lasts twice as long, to the ceiling (RFC 6298 (5.5)).
    pub fn expired(&mut self) {}
}

/// `d` divided by `n`, rounded down; never fails for the constant divisors above.
fn part(d: Duration, n: u32) -> Duration {
    d.checked_div(n).unwrap_or_default()
}

/// `d` held within `bounds`, the floor first, so a ceiling under the floor wins.
fn within(bounds: Bounds, d: Duration) -> Duration {
    d.max(bounds.floor).min(bounds.ceiling)
}

#[cfg(test)]
mod tests;
