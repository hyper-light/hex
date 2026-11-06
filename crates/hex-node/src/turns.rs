//! One change of a box at a time. A change takes the box's turn and holds it until it's done: another
//! change of the same box waits for it, a change of another box never does (goal 5 plan step 2), a
//! wait ends at a stated patience, and the turns taken at once stop at a stated bound, each refused by
//! type past it (CLAUDE.md §2). The record store takes a turn for each change of a record; the node
//! takes one for each create and delete of a box, which has no record while it's being made (goal 5
//! extended, step 5).

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use hex_api::{BoxId, Refusal};

/// The boxes whose turns are taken now, and what a refusal of one names.
#[derive(Debug)]
pub struct Turns {
    boxes: Mutex<BTreeSet<BoxId>>,
    /// Told whenever a box is let go.
    free: Condvar,
    /// Turns that waited for one of the same box to end.
    waited: AtomicU64,
    /// What a turn is for, as a refusal past the patience names it.
    op: &'static str,
    /// The turns taken at once, as a refusal past the bound names them.
    what: &'static str,
    patience: Duration,
    bound: usize,
}

/// Box `id`'s turn, given up when dropped.
pub struct Turn<'a> {
    turns: &'a Turns,
    id: BoxId,
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        // The set stays whole whoever held its lock (nothing panics holding it: CLAUDE.md §1), so a
        // poisoned lock still lets the box go.
        let mut boxes = self
            .turns
            .boxes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        boxes.remove(&self.id);
        drop(boxes);
        self.turns.free.notify_all();
    }
}

impl Turns {
    /// Turns for `op`, a wait for one ending at `patience`, at most `bound` of them (`what`) taken at
    /// once.
    pub fn new(op: &'static str, what: &'static str, patience: Duration, bound: usize) -> Self {
        Self {
            boxes: Mutex::default(),
            free: Condvar::new(),
            waited: AtomicU64::new(0),
            op,
            what,
            patience,
            bound,
        }
    }

    /// Box `id`'s turn: waits for a turn of the same box under way, never for another box's, and
    /// refuses past the patience, or past the bound of turns taken at once.
    pub fn take(&self, id: &BoxId) -> Result<Turn<'_>, Refusal> {
        let deadline = Instant::now().checked_add(self.patience);
        let mut boxes = self.boxes.lock().map_err(|e| self.poisoned(e))?;
        if boxes.contains(id) {
            self.waited.fetch_add(1, Ordering::Relaxed);
        }
        while boxes.contains(id) {
            let left = deadline.map_or(Duration::ZERO, |d| {
                d.saturating_duration_since(Instant::now())
            });
            if left.is_zero() {
                return Err(Refusal::Os {
                    op: self.op,
                    why: format!(
                        "box {id}'s change before this one was still under way after {:?}",
                        self.patience
                    ),
                });
            }
            boxes = self
                .free
                .wait_timeout(boxes, left)
                .map_err(|e| self.poisoned(e))?
                .0;
        }
        if boxes.len() >= self.bound {
            return Err(Refusal::Exhausted {
                what: self.what,
                bound: self.bound,
            });
        }
        boxes.insert(id.clone());
        Ok(Turn {
            turns: self,
            id: id.clone(),
        })
    }

    /// How many turns have waited for one of the same box to end.
    pub fn waited(&self) -> u64 {
        self.waited.load(Ordering::Relaxed)
    }

    fn poisoned<T>(&self, _: PoisonError<T>) -> Refusal {
        Refusal::Broken(format!("the lock on {} was poisoned", self.what))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: &str) -> BoxId {
        BoxId::new(n).unwrap()
    }

    /// CLAUDE.md §2 (T-T7.1.2): the turns taken at once stop at their bound, refused by type past it. Do: turns
    /// bounded at two; take two boxes' turns, then a third's; let one go and take the third's again. Expect:
    /// `Exhausted` naming the bound, then the third's turn given.
    #[test]
    fn turns_past_the_bound_are_refused_by_type() {
        let turns = Turns::new("op", "turns under way", Duration::from_secs(60), 2);
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

    /// CLAUDE.md §2 (T-T7.1.2): a wait for the same box's turn ends at its patience, refused by type, never
    /// longer. Do: hold box a's turn, and take it again with a patience of 50 ms. Expect: refused as `Os` for the
    /// turns' op after at least the patience, the wait counted once.
    #[test]
    fn a_wait_for_the_same_box_ends_at_its_patience() {
        let turns = Turns::new("op", "turns under way", Duration::from_millis(50), 2);
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
    }
}
