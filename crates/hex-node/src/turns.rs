//! One change of a box at a time. A change takes the box's turn and holds it until it's done: another
//! change of the same box waits for it, a change of another box never does (goal 5 plan step 2), a
//! wait ends at a learned patience, and the turns taken at once stop at a stated bound, each refused by
//! type past it (CLAUDE.md §2). The record store takes a turn for each change of a record; the node
//! takes one for each create and delete of a box, which has no record while it's being made (goal 5
//! extended, step 5).
//!
//! The patience is twice the longest a turn has been held (decision 9; Ada's 2026-11-06 rule, the lead's
//! failure-bound rule; [`hex_core::patience`]): each turn let go is a sample of what a wait for one waits on.
//! Each owner seeds its turns from the record of its holds, or, with none yet, measures ([`Patience::measuring`]):
//! until the first sample a measuring wait is unarmed and lasts until the turn is let go.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use hex_api::{BoxId, Refusal};
use hex_core::patience::Patience;

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
    /// Each box whose turn is taken, with when its holder took it.
    boxes: BTreeMap<BoxId, Instant>,
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
        held.patience.answered(self.since.elapsed());
        drop(held);
        self.turns.free.notify_all();
    }
}

impl Turns {
    /// Turns for `op`, a wait for one seeded as `patience` and learned from the turns held, at most `bound` of
    /// them (`what`) taken at once.
    pub fn new(op: &'static str, what: &'static str, bound: usize, patience: Patience) -> Self {
        Self {
            held: Mutex::new(Held {
                boxes: BTreeMap::new(),
                patience,
            }),
            free: Condvar::new(),
            waited: AtomicU64::new(0),
            op,
            what,
            bound,
        }
    }

    /// Box `id`'s turn: waits for a turn of the same box under way, never for another box's, and
    /// refuses past the patience learned now, or past the bound of turns taken at once. The wait is for
    /// the turn under way: it ends at the patience past the time its holder took it, and starts again for
    /// each holder after, since each turn let go is the answer it waited on (several changes of one box
    /// at once each wait for the one before theirs).
    pub fn take(&self, id: &BoxId) -> Result<Turn<'_>, Refusal> {
        let mut held = self.held.lock().map_err(|e| self.poisoned(e))?;
        if held.boxes.contains_key(id) {
            self.waited.fetch_add(1, Ordering::Relaxed);
        }
        while let Some(&since) = held.boxes.get(id) {
            let patience = held.patience.deadline();
            held = match patience.and_then(|p| since.checked_add(p)) {
                // Unarmed: measuring, and no turn has been let go; this wait is the first sample's.
                None => self.free.wait(held).map_err(|e| self.poisoned(e))?,
                Some(deadline) => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(Refusal::Os {
                            op: self.op,
                            why: format!(
                                "box {id}'s change before this one was still under way after {:?}",
                                since.elapsed()
                            ),
                        });
                    }
                    self.free
                        .wait_timeout(held, left)
                        .map_err(|e| self.poisoned(e))?
                        .0
                }
            };
        }
        if held.boxes.len() >= self.bound {
            return Err(Refusal::Exhausted {
                what: self.what,
                bound: self.bound,
            });
        }
        let since = Instant::now();
        held.boxes.insert(id.clone(), since);
        Ok(Turn {
            turns: self,
            id: id.clone(),
            since,
        })
    }

    /// How many turns have waited for one of the same box to end.
    pub fn waited(&self) -> u64 {
        self.waited.load(Ordering::Relaxed)
    }

    /// How long a wait for a turn lasts now, as learned (`None` before any sample): what the tests read.
    #[cfg(test)]
    fn patience(&self) -> Option<Duration> {
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
    use hex_testkit::waits::until;

    fn id(n: &str) -> BoxId {
        BoxId::new(n).unwrap()
    }

    /// CLAUDE.md §2 (T-T7.1.2): the turns taken at once stop at their bound, refused by type past it. Do: turns
    /// bounded at two; take two boxes' turns, then a third's; let one go and take the third's again. Expect:
    /// `Exhausted` naming the bound, then the third's turn given.
    #[test]
    fn turns_past_the_bound_are_refused_by_type() {
        let turns = Turns::new(
            "op",
            "turns under way",
            2,
            Patience::measuring("test.turns"),
        );
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

    /// CLAUDE.md §2 (T-T7.1.2), decision 9: a wait for the same box's turn ends at twice the slowest hold recorded,
    /// refused by type, never longer, and the refusal is no sample. Do: seed the turns from a hold of 25 ms,
    /// hold box a's turn and take it again. Expect: refused as `Os` for the turns' op after at least 50 ms, the wait
    /// counted once, and the wait still 50 ms.
    #[test]
    fn a_wait_for_the_same_box_ends_at_twice_the_slowest_hold() {
        let turns = Turns::new(
            "op",
            "turns under way",
            2,
            Patience::seeded("test.turns", Duration::from_millis(25)),
        );
        let asked = Instant::now();
        let _held = turns.take(&id("a")).unwrap();
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
        assert_eq!(turns.patience(), Some(Duration::from_millis(50)));
    }

    /// Decision 9 (Ada's 2026-11-06 rule; T-T7.1.2): measuring, a wait for a turn lasts until it is let go, and the
    /// hold is recorded, never arming it. Do: on new turns, hold box a's turn while another thread takes it, until that
    /// take waits; hold on for at least 30 ms and let go. Expect: the waiting take given the turn; the wait still
    /// unarmed.
    #[test]
    fn a_measuring_wait_lasts_until_the_turn_is_let_go_and_stays_unarmed() {
        let turns = Turns::new(
            "op",
            "turns under way",
            2,
            Patience::measuring("test.turns"),
        );
        assert_eq!(turns.patience(), None);
        let held = turns.take(&id("a")).unwrap();
        let took = Instant::now();
        std::thread::scope(|s| {
            let waiter = s.spawn(|| turns.take(&id("a")).map(drop));
            // No record times a take's wait starting on turns of their own (the census's row is to come, from the
            // suites run under it): measuring, for the fact alone.
            let waits = Patience::measuring("test.turns.second_take_waits");
            until(&waits, "the second take waiting", || turns.waited() > 0).unwrap();
            while took.elapsed() < Duration::from_millis(30) {
                std::thread::yield_now();
            }
            drop(held);
            waiter.join().unwrap().unwrap();
        });
        assert_eq!(turns.patience(), None, "a measuring wait is never armed");
    }
}
