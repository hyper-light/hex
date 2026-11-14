//! A killed `process` box's emptying on macOS and Windows (the review's E-12): the test control that holds one box's
//! emptying, so a test meets a delete, or a recovery's end, whose killed box empties late.

/// The review's E-12 test control: a hold on one killed box's emptying. While it stands for a box, the box reads as not
/// yet empty, as a box whose last member's teardown runs long reads, so a test meets a wait on it past any bound; it
/// counts the waits stopped at it. Keyed by what names the box to its OS's wait (a macOS box's process group, by its
/// id), so tests side by side hold their own boxes alone.
#[cfg(test)]
pub(crate) mod hold {
    use std::collections::BTreeMap;
    use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};

    /// The holds standing, each with how many waits are stopped at it.
    static HOLDS: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
    static GONE: Condvar = Condvar::new();

    fn holds() -> MutexGuard<'static, BTreeMap<String, u64>> {
        HOLDS.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The hold on box `key`'s emptying, standing until it goes.
    pub(crate) fn hold(key: &str) -> Hold {
        holds().insert(key.to_owned(), 0);
        Hold(key.to_owned())
    }

    /// A hold, standing until this goes.
    pub(crate) struct Hold(String);

    impl Hold {
        /// Whether a wait is stopped at the hold now, following the box past any bound.
        pub(crate) fn following(&self) -> bool {
            holds().get(&self.0).is_some_and(|stopped| *stopped > 0)
        }
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            holds().remove(&self.0);
            GONE.notify_all();
        }
    }

    /// Whether box `key`'s emptying is held now: while it is, the box reads as not yet empty.
    pub(crate) fn held(key: &str) -> bool {
        holds().contains_key(key)
    }
}
