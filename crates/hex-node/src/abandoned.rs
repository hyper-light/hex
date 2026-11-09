//! Each box's abandoned programs (decision 3's disconnect rule: exec'd programs the node ended because their
//! caller's connection closed), counted into the box's record, which inspect reports. One copy for every OS's
//! node (native plan step 8). A count a record write can't take now is kept, reported all the same, and
//! written with the next, never dropped (CLAUDE.md §3). Each box's count has a lock of its own, held across the
//! write that takes it, so one box's durable write holds up no other box's inspect or create (goal 5 plan
//! step 2). An entry goes with its box's delete.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

use hex_api::{BoxId, Refusal};

use crate::store::Store;

/// Each box's count not yet in its record, each behind a lock of its own.
type Pending = BTreeMap<BoxId, Arc<Mutex<u64>>>;

/// The counts not yet in their boxes' records, by box.
#[derive(Default)]
pub struct Abandoned {
    pending: Mutex<Pending>,
}

fn poisoned() -> Refusal {
    Refusal::Broken("the node's abandoned-program counts' lock was poisoned".to_owned())
}

impl Abandoned {
    fn pending(&self) -> Result<MutexGuard<'_, Pending>, Refusal> {
        self.pending.lock().map_err(|_| poisoned())
    }

    /// Counts one more in box `id`, and writes every count it holds for it to the box's record in `store`.
    /// A box with no readable record (deleted since) has none to take it.
    pub fn count(&self, store: &Store, id: &BoxId) -> Result<(), Refusal> {
        let count = Arc::clone(self.pending()?.entry(id.clone()).or_default());
        let mut count = count.lock().map_err(|_| poisoned())?;
        // Saturating: 2^64 programs is past any box's life, so the cap is never met.
        *count = count.saturating_add(1);
        let add = *count;
        store.update(id, |r| {
            r.abandoned = r.abandoned.saturating_add(add);
            true
        })?;
        *count = 0;
        Ok(())
    }

    /// Box `id`'s count not yet in its record; waits only for a write of its own count.
    pub fn unrecorded(&self, id: &BoxId) -> Result<u64, Refusal> {
        let Some(count) = self.pending()?.get(id).map(Arc::clone) else {
            return Ok(0);
        };
        let count = count.lock().map_err(|_| poisoned())?;
        Ok(*count)
    }

    /// Box `id` was deleted: nothing more to count for it.
    pub fn forget(&self, id: &BoxId) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(id);
        }
    }
}
