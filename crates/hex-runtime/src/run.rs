//! A run's box removed once its program ends (decision 7 of docs/design/runtime-architecture.md), on the
//! runtimes whose boxes nothing else watches: macOS's and Windows' `process` classes. (Linux's node agent's
//! supervisor removes a run's box there, watching the program's pidfd.) A thread of the run's own takes its
//! program, waits for its end, removes the box, and only then gives the caller the end, so a caller that saw
//! the end finds the box gone. It does so whether or not the caller still holds the run: a caller that lets
//! the run go leaves no box behind.

use std::sync::{Arc, PoisonError};
use std::time::{Duration, Instant};

use hex_api::{BoxId, Exit, Given, Proc, Refusal};
use hex_wait::{Condvar, Mutex};

/// What ends a run's program: its box's KILL, which a run's box, made for that program alone, holds.
pub(crate) type Kill = Box<dyn FnMut() -> Result<(), Refusal> + Send>;

/// What removes a run's box once its program has ended: the runtime's own removal, the one its delete takes.
pub(crate) type Remove = Box<dyn FnOnce() -> Result<(), Refusal> + Send>;

/// The run's end as its thread leaves it: the program's exit, or why the end or the removal failed.
struct Ended {
    end: Mutex<Option<Result<Exit, Refusal>>>,
    came: Condvar,
}

fn poisoned<T>(_: PoisonError<T>) -> Refusal {
    Refusal::Broken("a run's end lock was poisoned".to_owned())
}

/// A wait for a run's end that couldn't wait: its lock poisoned, or the OS refused it (macOS's kqueue).
fn wait_refused(why: hex_wait::Failed) -> Refusal {
    match why {
        hex_wait::Failed::Poisoned => Refusal::Broken("a run's end lock was poisoned".to_owned()),
        hex_wait::Failed::Os(why) => Refusal::Os {
            op: "waiting for a run's end",
            why: why.to_string(),
        },
    }
}

/// A run's program as its caller holds it ([`hex_api::Runtime::run`]): its end comes from the run's thread.
pub(crate) struct Run {
    pid: u32,
    ended: Arc<Ended>,
    given: Option<Given>,
    kill: Kill,
}

impl Run {
    /// Hands `program`, started in run box `id`, to a thread of its own, which waits for its end, calls
    /// `remove`, then gives the end to the caller. The caller's ends of the streams the run asked for are
    /// taken here, before the program goes to the thread.
    pub(crate) fn start(
        mut program: Box<dyn Proc>,
        id: &BoxId,
        kill: Kill,
        remove: Remove,
    ) -> Result<Self, Refusal> {
        #[cfg(test)]
        hold::stop_here(id, hold::At::Making);
        let given = program.streams()?;
        let pid = program.pid();
        let ended = Arc::new(Ended {
            end: Mutex::new(None),
            came: Condvar::new(),
        });
        let theirs = Arc::clone(&ended);
        let named = id.clone();
        #[cfg(test)]
        let token = hold::token(id);
        std::thread::Builder::new()
            .name(format!("hex-run-{id}"))
            .spawn(move || {
                #[cfg(test)]
                let _token = token;
                // Waits as long as the program runs: each wait returns at its end, or with none yet.
                let exit = loop {
                    match program.wait_for(Duration::MAX) {
                        Ok(Some(exit)) => break Ok(exit),
                        Ok(None) => {}
                        Err(why) => break Err(why),
                    }
                };
                drop(program);
                #[cfg(test)]
                {
                    hold::stop_here(&named, hold::At::End);
                    hold::stop_here(&named, hold::At::Removal);
                }
                // The box goes before the end is given, so a caller that saw the end finds it gone; a
                // removal that fails is the run's to report, never dropped (CLAUDE.md §3).
                let end = match (exit, remove()) {
                    (Ok(exit), Ok(())) => Ok(exit),
                    (Err(why), _) => Err(why),
                    (Ok(exit), Err(left)) => Err(Refusal::Broken(format!(
                        "run box {named}'s program ended ({exit:?}) but its box wasn't removed: {left}"
                    ))),
                };
                if let Ok(mut slot) = theirs.end.lock() {
                    *slot = Some(end);
                }
                theirs.came.notify_all();
            })
            .map_err(|e| Refusal::os("starting a run's watch", e))?;
        Ok(Self {
            pid,
            ended,
            given: Some(given),
            kill,
        })
    }
}

/// The refusal a run that couldn't start gives: `why`, and what removing the box it made then left
/// (`cleanup`), so neither is dropped.
pub(crate) fn undone(why: Refusal, cleanup: Result<(), Refusal>, id: &BoxId) -> Refusal {
    match cleanup {
        Ok(()) => why,
        Err(left) => Refusal::Broken(format!(
            "{why}; and run box {id} it made couldn't be removed: {left}"
        )),
    }
}

impl Proc for Run {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        let end = self.ended.end.lock().map_err(poisoned)?;
        (*end).clone().transpose()
    }

    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal> {
        let deadline = Instant::now().checked_add(timeout);
        let mut end = self.ended.end.lock().map_err(poisoned)?;
        loop {
            if let Some(got) = end.as_ref() {
                return got.clone().map(Some);
            }
            let left = match deadline {
                Some(d) => d.saturating_duration_since(Instant::now()),
                None => timeout,
            };
            if left.is_zero() {
                return Ok(None);
            }
            end = self
                .ended
                .came
                .wait_timeout(end, left)
                .map_err(wait_refused)?
                .0;
        }
    }

    /// Ends the run's program through its box's KILL: the box holds that program alone. A program that ended
    /// already is `Ok`, its box perhaps gone.
    fn kill(&mut self) -> Result<(), Refusal> {
        if self.try_wait()?.is_some() {
            return Ok(());
        }
        match (self.kill)() {
            Ok(()) | Err(Refusal::NoSuchBox(_)) => Ok(()),
            Err(why) => Err(why),
        }
    }

    fn streams(&mut self) -> Result<Given, Refusal> {
        Ok(self.given.take().unwrap_or_default())
    }
}

/// The review's E-11 test control (runtime goal 7): a hold on one run box's setup or thread. While it stands for box
/// `id`, the run stops where the hold says, as a thread descheduled there would: its setup once the box is made and its
/// program started, before the watch starts ([`hold::At::Making`]); its thread once its program has ended, before it
/// claims the box's removal ([`hold::At::End`]), or once it has claimed it, before it removes the box
/// ([`hold::At::Removal`]). It counts the deletes that waited on the run's setup or a claimer's word, and gives the
/// run's thread as a party to follow. Keyed by box id, so tests side by side hold their own boxes alone.
#[cfg(test)]
pub(crate) mod hold {
    use std::collections::BTreeMap;
    use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};

    use hex_api::BoxId;
    use hex_core::patience::Party;

    /// Where a held run's setup or thread stops.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum At {
        /// The run's setup, once its box is made and its program started, before its watch starts.
        Making,
        /// Once its program has ended, before it claims the box's removal.
        End,
        /// Once it has claimed the box's removal, before it removes the box.
        Removal,
    }

    struct Held {
        at: At,
        /// Whether the run's setup or thread is stopped at the hold now.
        stopped: bool,
        /// The deletes that waited on the run's setup or the word of the box's claimer.
        waiting: u64,
        /// The run's thread's token, which the thread holds for its life.
        thread: Weak<()>,
    }

    static HOLDS: Mutex<BTreeMap<BoxId, Held>> = Mutex::new(BTreeMap::new());
    static GONE: Condvar = Condvar::new();

    fn holds() -> MutexGuard<'static, BTreeMap<BoxId, Held>> {
        HOLDS.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The hold on box `id`'s run at `at`, standing until it goes. Taken before the run starts.
    pub(crate) fn hold(id: &BoxId, at: At) -> Hold {
        holds().insert(
            id.clone(),
            Held {
                at,
                stopped: false,
                waiting: 0,
                thread: Weak::new(),
            },
        );
        Hold(id.clone())
    }

    /// A hold, standing until this goes.
    pub(crate) struct Hold(BoxId);

    impl Hold {
        /// Whether the run's setup or thread is stopped at the hold now.
        pub(crate) fn stopped(&self) -> bool {
            holds().get(&self.0).is_some_and(|held| held.stopped)
        }

        /// The deletes that waited on the run's setup or the word of the box's claimer.
        pub(crate) fn waiting(&self) -> u64 {
            holds().get(&self.0).map_or(0, |held| held.waiting)
        }

        /// The run's thread as a party: live while it runs, ended once it has gone.
        pub(crate) fn thread(&self) -> Party {
            match holds().get(&self.0) {
                Some(held) if held.thread.strong_count() > 0 => Party::Live,
                _ => Party::Ended,
            }
        }
    }

    impl Drop for Hold {
        fn drop(&mut self) {
            holds().remove(&self.0);
            GONE.notify_all();
        }
    }

    /// The token box `id`'s run's thread holds for its life, where a hold stands for the box.
    pub(super) fn token(id: &BoxId) -> Option<Arc<()>> {
        let token = Arc::new(());
        holds().get_mut(id)?.thread = Arc::downgrade(&token);
        Some(token)
    }

    /// Box `id`'s run's setup or thread at `at`: stopped there while a hold at `at` stands for the box.
    pub(super) fn stop_here(id: &BoxId, at: At) {
        let mut holds = holds();
        match holds.get_mut(id) {
            Some(held) if held.at == at => held.stopped = true,
            _ => return,
        }
        let _gone = GONE
            .wait_while(holds, |holds| holds.contains_key(id))
            .unwrap_or_else(PoisonError::into_inner);
    }
}
