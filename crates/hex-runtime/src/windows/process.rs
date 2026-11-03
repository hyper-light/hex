//! The `process` class on Windows (docs/design/runtime.md, decision "each OS runs its own boxes
//! natively"): a box is a job object. Its limits are the kernel's and hard — a commit cap, an
//! active-process cap, a CPU weight or hard cap, kill-on-close, and no breakaway — so the box holds its
//! bounds without a watcher enforcing them (`sys`, [`hex_core::job`]). A member is created suspended
//! inside the job (`PROC_THREAD_ATTRIBUTE_JOB_LIST`) and resumed, so nothing runs outside it.
//!
//! Reaching the memory bound is read from the one notification class Windows guarantees, queried from
//! the job (`JobObjectNotificationLimitInformation`). A fork refused at the active-process limit is a
//! hard, kernel-enforced refusal delivered synchronously to the forking process. Windows has no §3-clean
//! count of such refusals for the job's owner: its accounting doesn't move on a refusal, and the only
//! job-side signal is the completion-port active-process-limit message, which Microsoft documents as not
//! guaranteed to be delivered — a lossy message the step forbids as a source (CLAUDE.md §3,
//! `docs/research/runtime/windows-pids-refused.md`, which surveys how others handle this). So Hex counts
//! only the refusals it observes synchronously and reliably: the node's own `exec` into a full box,
//! whose `CreateProcess` returns `ERROR_NOT_ENOUGH_QUOTA` ([`JobBox::exec`]). A member's own internal
//! fork, refused by the kernel in that member, is enforced but not counted by the box (no §3-clean
//! source exists; the research note recommends HEX-RT-6 on Windows check the bound's deterministic
//! effect there — a plan decision for Ada and the lead, not taken in the shared conformance suite).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use hex_api::{Bounds, BoxId, Refusal, Signal, Usage};
use hex_core::job::{self, Sent};

use super::sys::{self, Job, Spawn, Started};

/// What the box counts beyond what the job's accounting gives: whether it reached its memory bound, how
/// many forks it saw refused at its process limit, and which signals Hex has ended the job for.
#[derive(Debug, Default)]
struct Counts {
    /// Times the box reached its memory bound: the guaranteed memory notification was violated. On
    /// Windows a commit past the bound is refused, not killed; this is what `Usage::oom_kills` reports
    /// (HEX-RT-5).
    oom_kills: u64,
    /// Forks the box saw refused at its active-process limit — the node's own `exec` into a full box,
    /// observed synchronously ([`JobBox::exec`]). A member's internal fork, refused in that member, has
    /// no §3-clean job-side signal and is not counted here (`Usage::pids_refused`).
    pids_refused: u64,
    /// Signals Hex ended the box's job for, so [`super::proc`] reads a terminated member's exit code as
    /// the signal.
    sent: Sent,
    /// Set once the box has been deleted, so a late call doesn't act on a terminated job.
    deleted: bool,
}

/// One box's shared state: its job and its counts. Held behind an `Arc` by the runtime and by each
/// member's [`super::proc::BoxProc`] (which reads `sent` to map an exit).
pub struct State {
    id: BoxId,
    bounds: Bounds,
    job: Job,
    /// Whether the box has a memory bound, so its notification violation is worth querying.
    bounded_memory: bool,
    counts: Mutex<Counts>,
    /// Whether the box is being deleted; a member started after this is refused (the job is ending).
    stopping: AtomicBool,
}

impl State {
    fn counts(&self) -> Result<std::sync::MutexGuard<'_, Counts>, Refusal> {
        self.counts
            .lock()
            .map_err(|_| Refusal::Broken("a box's counts lock is poisoned".to_owned()))
    }

    /// The signals Hex has ended the box's job for.
    pub fn sent(&self) -> Result<Sent, Refusal> {
        Ok(self.counts()?.sent)
    }
}

/// A Windows `process` box. Dropping it closes the job's last handle, which (kill-on-close) ends every
/// process still in it.
pub struct JobBox {
    state: std::sync::Arc<State>,
}

impl JobBox {
    /// A box with `bounds` on a machine of `processors` logical processors (the CPU hard cap's
    /// denominator). The job is made and limited while empty; nothing runs in it until
    /// [`JobBox::exec`].
    pub fn create(
        id: BoxId,
        bounds: &Bounds,
        processors: std::num::NonZeroU32,
    ) -> Result<Self, Refusal> {
        let limits = job::limits(bounds, processors)?;
        let job = Job::create().map_err(|e| Refusal::os("creating a box's job", e))?;
        job.configure(&sys_limits(&limits))
            .map_err(|e| Refusal::os("setting a box's job limits", e))?;
        Ok(Self {
            state: std::sync::Arc::new(State {
                id,
                bounds: bounds.clone(),
                job,
                bounded_memory: limits.memory_notify.is_some(),
                counts: Mutex::new(Counts::default()),
                stopping: AtomicBool::new(false),
            }),
        })
    }

    pub fn id(&self) -> &BoxId {
        &self.state.id
    }

    pub fn bounds(&self) -> Bounds {
        self.state.bounds.clone()
    }

    /// The shared state, for a member's waiter to read which signals the box was ended for.
    pub fn shared(&self) -> std::sync::Arc<State> {
        std::sync::Arc::clone(&self.state)
    }

    /// Starts `spawn` suspended inside the box's job and resumes it, returning its process handle and
    /// pid. Nothing runs outside the job: the member is a job member from its creation.
    pub fn exec(&self, mut spawn: Spawn) -> Result<Started, Refusal> {
        if self.state.stopping.load(Ordering::SeqCst) {
            return Err(Refusal::NoSuchBox(self.state.id.clone()));
        }
        let started = match sys::create(&mut spawn, &[self.state.job.raw()]) {
            Ok(started) => started,
            // The box is at its active-process limit: the node's own start is refused synchronously
            // (`ERROR_NOT_ENOUGH_QUOTA`). This refusal Hex sees directly, so it is counted exactly
            // (unlike a member's own fork, whose refusal only reaches the box as an async notification);
            // refused by name (docs/research/runtime/windows-pids-refused.md).
            Err(e) if sys::is_process_limit(&e) => {
                let mut c = self.state.counts()?;
                c.pids_refused = c.pids_refused.saturating_add(1);
                let bound = self
                    .state
                    .bounds
                    .pids_max
                    .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX));
                return Err(Refusal::Exhausted {
                    what: "processes",
                    bound,
                });
            }
            Err(e) => return Err(Refusal::os("starting a program in a box", e)),
        };
        sys::resume(&started.thread).map_err(|e| Refusal::os("resuming a program in a box", e))?;
        Ok(started)
    }

    /// Whether a member runs in the box now, from the job's live process-id list (not its accounting,
    /// which lingers while a terminated member's handle is held).
    pub fn running(&self) -> Result<bool, Refusal> {
        Ok(self.live_processes()? > 0)
    }

    fn accounting(&self) -> Result<sys::Accounting, Refusal> {
        self.state
            .job
            .accounting()
            .map_err(|e| Refusal::os("reading a box's job accounting", e))
    }

    fn live_processes(&self) -> Result<u64, Refusal> {
        self.state
            .job
            .live_processes()
            .map_err(|e| Refusal::os("reading a box's live processes", e))
    }

    /// What the box has used and holds now: the job's accounting and peak, the memory-bound count from
    /// the guaranteed notification violation, and the refused-fork count tallied from the job's port.
    pub fn usage(&self) -> Result<Usage, Refusal> {
        // Reaching the memory bound: the guaranteed notification violation, from the job's accounting.
        // Reading it re-arms the notification, so each crossing is counted once.
        if self.state.bounded_memory
            && self
                .state
                .job
                .memory_violated()
                .map_err(|e| Refusal::os("reading a box's limit violation", e))?
        {
            let mut c = self.state.counts()?;
            c.oom_kills = c.oom_kills.saturating_add(1);
        }
        let a = self.accounting()?;
        let live = self.live_processes()?;
        // pids_refused is the count the box saw synchronously (node-mediated exec refusals, [`exec`]).
        let (oom_kills, pids_refused) = {
            let c = self.state.counts()?;
            (c.oom_kills, c.pids_refused)
        };
        Ok(Usage {
            memory_bytes: a.peak_memory,
            memory_peak_bytes: a.peak_memory,
            oom_kills,
            pids: live,
            pids_refused,
            // The job keeps CPU in 100 ns ticks; report microseconds.
            cpu_usec: a.cpu_100ns.saturating_div(10),
            io_read_bytes: a.read_bytes,
            io_write_bytes: a.write_bytes,
        })
    }

    /// Ends every process in the box with `signal` (KILL or TERM, which both end the job). A member's
    /// exit code then reads as `signal` ([`super::proc`]).
    pub fn signal(&self, signal: Signal) -> Result<(), Refusal> {
        job::ends_job(signal)?;
        {
            let mut c = self.state.counts()?;
            c.sent.add(signal);
        }
        self.state
            .job
            .terminate(job::terminated_code(signal))
            .map_err(|e| Refusal::os("ending a box's job", e))
    }

    /// Ends the box: marks it stopping, records that it ended the job for KILL (so a member's exit reads
    /// as Signal(9)), and terminates the job. Closing the last handle when the box is dropped is the
    /// backstop (kill-on-close). Idempotent — a second delete is a no-op.
    pub fn delete(&self) -> Result<(), Refusal> {
        self.state.stopping.store(true, Ordering::SeqCst);
        {
            let mut c = self.state.counts()?;
            if c.deleted {
                return Ok(());
            }
            c.deleted = true;
            c.sent.add(Signal::KILL);
        }
        self.state
            .job
            .terminate(job::terminated_code(Signal::KILL))
            .map_err(|e| Refusal::os("ending a box's job", e))
    }
}

impl Drop for JobBox {
    fn drop(&mut self) {
        let _ = self.delete();
    }
}

/// The job limits for `limits` ([`hex_core::job`] already sets the commit limit a margin under the
/// bound and the notification under that).
fn sys_limits(limits: &job::Limits) -> sys::Limits {
    sys::Limits {
        memory: limits.memory,
        processes: limits.processes,
        cpu: limits.cpu,
        memory_notify: limits.memory_notify,
    }
}
