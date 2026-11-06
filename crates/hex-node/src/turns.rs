//! One change of a box at a time. A change takes the box's turn and holds it until it's done: another
//! change of the same box waits for it, a change of another box never does (goal 5 plan step 2), a
//! wait ends at a learned patience, and the turns taken at once stop at a stated bound, each refused by
//! type past it (CLAUDE.md §2). The record store takes a turn for each change of a record; the node
//! takes one for each create and delete of a box, which has no record while it's being made (goal 5
//! extended, step 5).
//!
//! The patience is RFC 6298's timer over how long turns are held (decision 9; Ada's 2026-11-06 rule):
//! each turn let go is a sample of what a wait for one waits on, and a wait that runs out backs the timer
//! off ([`hex_core::patience`]).

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use hex_api::{BoxId, Refusal};
use hex_core::patience::{Bounds, Patience};

/// The boxes whose turns are taken now, and what a refusal of one names.
#[derive(Debug)]
pub struct Turns {
    held: Mutex<Held>,
    /// Told whenever a box is let go.
    free: Condvar,
    /// Turns that waited for one of the same box to end.
    waited: AtomicU64,
    /// What a turn is for, as a refusal past the patience names it.
    op: &'static str,
    /// The turns taken at once, as a refusal past the bound names them.
    what: &'static str,
    bound: usize,
}

/// The turns taken, and the wait for one learned from how long turns were held, under one lock.
#[derive(Debug)]
struct Held {
    boxes: BTreeSet<BoxId>,
    patience: Patience,
}

/// Box `id`'s turn, given up when dropped.
pub struct Turn<'a> {
    turns: &'a Turns,
    id: BoxId,
    since: Instant,
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        // The set stays whole whoever held its lock (nothing panics holding it: CLAUDE.md §1), so a
        // poisoned lock still lets the box go.
        let mut held = self
            .turns
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        held.boxes.remove(&self.id);
        drop(held);
        self.turns.free.notify_all();
    }
}

impl Turns {
    /// Turns for `op`, a wait for one learned under `patience`, at most `bound` of them (`what`) taken at
    /// once.
    pub fn new(op: &'static str, what: &'static str, patience: Bounds, bound: usize) -> Self {
        Self {
            held: Mutex::new(Held {
                boxes: BTreeSet::new(),
                patience: Patience::new(patience),
            }),
            free: Condvar::new(),
            waited: AtomicU64::new(0),
            op,
            what,
            bound,
        }
    }

    /// Box `id`'s turn: waits for a turn of the same box under way, never for another box's, and
    /// refuses past the patience learned now, or past the bound of turns taken at once.
    pub fn take(&self, id: &BoxId) -> Result<Turn<'_>, Refusal> {
        let asked = Instant::now();
        let mut held = self.held.lock().map_err(|e| self.poisoned(e))?;
        let patience = held.patience.deadline();
        let deadline = asked.checked_add(patience);
        if held.boxes.contains(id) {
            self.waited.fetch_add(1, Ordering::Relaxed);
        }
        while held.boxes.contains(id) {
            let left = deadline.map_or(Duration::ZERO, |d| {
                d.saturating_duration_since(Instant::now())
            });
            if left.is_zero() {
                return Err(Refusal::Os {
                    op: self.op,
                    why: format!(
                        "box {id}'s change before this one was still under way after {patience:?}"
                    ),
                });
            }
            held = self
                .free
                .wait_timeout(held, left)
                .map_err(|e| self.poisoned(e))?
                .0;
        }
        if held.boxes.len() >= self.bound {
            return Err(Refusal::Exhausted {
                what: self.what,
                bound: self.bound,
            });
        }
        held.boxes.insert(id.clone());
        Ok(Turn {
            turns: self,
            id: id.clone(),
            since: Instant::now(),
        })
    }

    /// How many turns have waited for one of the same box to end.
    pub fn waited(&self) -> u64 {
        self.waited.load(Ordering::Relaxed)
    }

    /// How long a wait for a turn lasts now, as learned.
    pub fn patience(&self) -> Duration {
        self.held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .patience
            .deadline()
    }

    fn poisoned<T>(&self, _: PoisonError<T>) -> Refusal {
        Refusal::Broken(format!("the lock on {} was poisoned", self.what))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_api::conformance::PATIENCE;
    use hex_core::patience::RFC_6298;

    fn id(n: &str) -> BoxId {
        BoxId::new(n).unwrap()
    }

    /// CLAUDE.md §2 (T-T7.1.2): the turns taken at once stop at their bound, refused by type past it. Do: turns
    /// bounded at two; take two boxes' turns, then a third's; let one go and take the third's again. Expect:
    /// `Exhausted` naming the bound, then the third's turn given.
    #[test]
    fn turns_past_the_bound_are_refused_by_type() {
        let turns = Turns::new("op", "turns under way", RFC_6298, 2);
        let a = turns.take(&id("a")).unwrap();
        let _b = turns.take(&id("b")).unwrap();
        assert_eq!(
            turns.take(&id("c")).err(),
            Some(Refusal::Exhausted {
                what: "turns under way",
                bound: 2
            })
        );
        drop(a);
        assert!(turns.take(&id("c")).is_ok());
    }

    /// CLAUDE.md §2 (T-T7.1.2), decision 9: a wait for the same box's turn ends at its patience, refused by type,
    /// never longer, and the next wait backs off (RFC 6298 (5.5)). Do: hold box a's turn under a patience of 50
    /// ms that may grow to 100 ms, and take it again. Expect: refused as `Os` for the turns' op after at least the
    /// patience, the wait counted once, and the next wait 100 ms.
    #[test]
    fn a_wait_for_the_same_box_ends_at_its_patience() {
        let bounds = Bounds {
            first: Duration::from_millis(50),
            floor: Duration::from_millis(50),
            ceiling: Duration::from_millis(100),
        };
        let turns = Turns::new("op", "turns under way", bounds, 2);
        let _held = turns.take(&id("a")).unwrap();
        let asked = Instant::now();
        let refused = turns.take(&id("a")).err();
        assert!(
            asked.elapsed() >= Duration::from_millis(50),
            "it waited out its patience"
        );
        assert!(
            matches!(refused, Some(Refusal::Os { op: "op", .. })),
            "{refused:?}"
        );
        assert_eq!(turns.waited(), 1);
        assert_eq!(turns.patience(), Duration::from_millis(100));
    }

    /// Decision 9 (Ada's 2026-11-06 rule; T-T7.1.2): the wait for a turn is learned from how long turns are
    /// held. Do: under bounds of a nanosecond to [`PATIENCE`], first 1 ms, hold a turn for at least 30 ms and let
    /// it go. Expect: the next wait at least three times the hold (SRTT the hold, RTTVAR half of it, RFC 6298
    /// (2.2)), so a change that waits for one held that long isn't refused at the first 1 ms.
    #[test]
    fn the_wait_for_a_turn_is_learned_from_the_turns_held() {
        let bounds = Bounds {
            first: Duration::from_millis(1),
            floor: Duration::from_nanos(1),
            ceiling: PATIENCE,
        };
        let turns = Turns::new("op", "turns under way", bounds, 2);
        assert_eq!(turns.patience(), Duration::from_millis(1));
        let held = turns.take(&id("a")).unwrap();
        let took = Instant::now();
        while took.elapsed() < Duration::from_millis(30) {
            std::thread::yield_now();
        }
        drop(held);
        let patience = turns.patience();
        assert!(
            patience >= Duration::from_millis(90),
            "learned {patience:?} from a hold of 30 ms or more"
        );
    }
}
