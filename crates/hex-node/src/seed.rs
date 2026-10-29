//! The node agent's pool of exec seeds (decision 3 of docs/design/runtime-architecture.md, goal 3
//! plan step 2), for the execs its own threads can't start: a rootless node's, whose threads may not
//! choose a container's pid namespace (a node that runs as root starts them straight from the calling
//! thread, `hex_runtime::linux::oci::seed::exec_direct`). A seed is a prepared `hex __oci-seed` process
//! the agent keeps and reuses, so such an exec spawns no process of the agent's and re-execs no `hex`
//! binary per call: the agent hands the exec to an idle seed over a `SOCK_SEQPACKET` socket (the
//! request's bytes with the program's stdio and the init's pidfd as `SCM_RIGHTS` descriptors) and gets
//! back the program's pidfd, which the supervisor watches as it watches an init's (`seed.rs` in
//! `hex-runtime` is the seed's own side).
//!
//! Bounds (CLAUDE.md §2), each refused past by a typed [`Refusal`]:
//! - **seeds**, [`SeedPool::budget`]: the most seed processes one node keeps. Seeds start on demand up
//!   to it; past it an exec waits for a free one.
//! - **waiters**, [`WAITERS`]: the most execs waiting for a free seed at once. Past it an exec is
//!   refused, rather than let the wait grow without bound.
//!
//! A seed that dies, or a round trip that breaks, refuses its exec and is replaced; the agent reaps
//! every seed it started (by pidfd), so none is leaked.

use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

use hex_api::Refusal;
use hex_core::seed::{Answer, Failed, Request};
use hex_runtime::linux::oci::seed as rt_seed;
use rustix::process::{Pid, PidfdFlags, WaitId, WaitIdOptions};

/// The most execs that wait for a free seed at once; past it an exec is refused with
/// [`Refusal::Exhausted`]. Large, since a waiting exec holds only a little memory and the usual cause
/// of a full pool is a burst that drains in milliseconds, not a wedge.
pub const WAITERS: usize = 4_096;

/// How long the agent waits for a seed's answer before it takes the seed for wedged, kills it and
/// refuses the exec. A seed's work is a fork, a few `setns` and a clone — milliseconds; this is a
/// bound on a wait for a fact (CLAUDE.md §4), not a guess at how long the work takes.
const ANSWER_PATIENCE: Duration = Duration::from_secs(30);

/// A pool of reusable exec seeds, grown on demand to [`budget`](Self::budget).
pub struct SeedPool {
    /// The `hex` binary re-exec'd as `hex __oci-seed`.
    exe: PathBuf,
    budget: usize,
    inner: Mutex<Inner>,
    free: Condvar,
}

#[derive(Default)]
struct Inner {
    idle: Vec<Seed>,
    /// Seeds that exist (idle plus in use plus reserved while one is starting).
    total: usize,
    waiting: usize,
}

/// One seed process: the agent's end of its socket, and the pidfd by which the agent ends and reaps
/// it.
struct Seed {
    socket: OwnedFd,
    pidfd: OwnedFd,
}

/// The default seed budget: one per hardware thread, so a node that runs many execs at once rarely
/// waits, clamped to a range that keeps a laptop's memory small and a big node's seed set bounded. An
/// idle seed holds little (a `hex` process's pages, shared copy-on-write with the agent).
fn default_budget() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4)
        .clamp(4, 128)
}

impl SeedPool {
    /// A pool that re-execs `exe` as `hex __oci-seed`, up to [`default_budget`] seeds.
    pub fn new(exe: PathBuf) -> Self {
        Self::with_budget(exe, default_budget())
    }

    /// A pool bounded to `budget` seeds.
    pub fn with_budget(exe: PathBuf, budget: usize) -> Self {
        Self {
            exe,
            budget: budget.max(1),
            inner: Mutex::new(Inner::default()),
            free: Condvar::new(),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner>, Refusal> {
        self.inner
            .lock()
            .map_err(|_| Refusal::Broken("the seed pool's lock was poisoned".to_owned()))
    }

    /// Runs `req` with `stdio` in an idle seed (starting one, or waiting for one, within the bounds),
    /// the program joining the namespaces of the container init `init` (its pidfd), and returns the
    /// started program's pid and pidfd for the supervisor to own. The caller holds the supervisor's
    /// spawning lock across this and the own, so the program — the agent's child — isn't reaped by a
    /// sweep before it is owned, and closes its own `stdio` after, so the program's streams end when it
    /// does.
    pub fn exec(
        &self,
        req: &Request,
        stdio: [BorrowedFd<'_>; 3],
        init: OwnedFd,
    ) -> Result<(u32, OwnedFd), Refusal> {
        let mut seed = self.acquire()?;
        match round_trip(&mut seed, req, stdio, init) {
            Ok(started) => {
                self.release(seed);
                Ok(started)
            }
            // The seed is still good (it refused a bad request or a start it couldn't do): back to the
            // pool, the refusal to the caller.
            Err(Trip::Refused(r)) => {
                self.release(seed);
                Err(r)
            }
            // The seed is unusable: end it, make room for a replacement, refuse this exec.
            Err(Trip::Broken(r)) => {
                self.discard(seed);
                Err(r)
            }
        }
    }

    /// An idle seed, a freshly started one if the pool may grow, or a wait for one freed; refused if
    /// too many already wait.
    fn acquire(&self) -> Result<Seed, Refusal> {
        let mut inner = self.lock()?;
        loop {
            if let Some(seed) = inner.idle.pop() {
                return Ok(seed);
            }
            if inner.total < self.budget {
                // Reserve the slot, start the seed without the lock (a fork and exec), then take the
                // slot back on failure so the count stays true.
                inner.total = inner.total.saturating_add(1);
                drop(inner);
                let started = self.start();
                if started.is_err() {
                    let mut inner = self.lock()?;
                    inner.total = inner.total.saturating_sub(1);
                    self.free.notify_one();
                }
                return started;
            }
            if inner.waiting >= WAITERS {
                return Err(Refusal::Exhausted {
                    what: "execs waiting for a seed",
                    bound: WAITERS,
                });
            }
            inner.waiting = inner.waiting.saturating_add(1);
            let (guard, timed_out) = self
                .free
                .wait_timeout(inner, ANSWER_PATIENCE)
                .map_err(|_| Refusal::Broken("the seed pool's lock was poisoned".to_owned()))?;
            inner = guard;
            inner.waiting = inner.waiting.saturating_sub(1);
            if timed_out.timed_out() && inner.idle.is_empty() && inner.total >= self.budget {
                return Err(Refusal::Exhausted {
                    what: "exec seeds",
                    bound: self.budget,
                });
            }
        }
    }

    /// Returns a healthy seed to the pool and wakes one waiter.
    fn release(&self, seed: Seed) {
        if let Ok(mut inner) = self.lock() {
            inner.idle.push(seed);
            self.free.notify_one();
        }
    }

    /// Ends and reaps an unusable seed, frees its slot and wakes a waiter (which may start a fresh
    /// one).
    fn discard(&self, seed: Seed) {
        end_seed(&seed);
        if let Ok(mut inner) = self.lock() {
            inner.total = inner.total.saturating_sub(1);
            self.free.notify_one();
        }
    }

    /// Starts a seed: a `hex __oci-seed` child of the agent, holding one end of a fresh socket; the
    /// agent keeps the other and the child's pidfd.
    fn start(&self) -> Result<Seed, Refusal> {
        let (ours, theirs) = rt_seed::socketpair()?;
        rt_seed::clear_cloexec(&theirs)?;
        let mut cmd = Command::new(&self.exe);
        cmd.arg("__oci-seed")
            .env(rt_seed::ENV_SOCKET, theirs.as_fd().as_raw_fd().to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // The seed keeps only its socket; every other inherited descriptor closes on its re-exec, as
        // the init's do. `theirs` is passed by number with close-on-exec cleared, so keep it open past
        // the spawn until the child has it.
        let child = cmd
            .spawn()
            .map_err(|e| Refusal::os("starting an exec seed", e))?;
        drop(theirs);
        let pid = i32::try_from(child.id())
            .map_err(|_| Refusal::os("starting an exec seed", "a pid out of range"))?;
        let raw = Pid::from_raw(pid)
            .ok_or_else(|| Refusal::os("starting an exec seed", "the seed's pid isn't a pid"))?;
        let pidfd = rustix::process::pidfd_open(raw, PidfdFlags::empty())
            .map_err(|e| Refusal::os("opening a seed's pidfd", e))?;
        Ok(Seed {
            socket: ours,
            pidfd,
        })
    }
}

impl Drop for SeedPool {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            for seed in inner.idle.drain(..) {
                end_seed(&seed);
            }
        }
    }
}

/// Ends a seed (KILL by its pidfd) and reaps it, so no seed the agent started is left running or a
/// zombie. A seed that already exited (its socket closed) takes a harmless failed signal.
fn end_seed(seed: &Seed) {
    let _ = rustix::process::pidfd_send_signal(seed.pidfd.as_fd(), rustix::process::Signal::KILL);
    let _ = rustix::process::waitid(WaitId::PidFd(seed.pidfd.as_fd()), WaitIdOptions::EXITED);
}

/// Why a round trip with a seed didn't start the program.
enum Trip {
    /// The seed refused by type and is still usable.
    Refused(Refusal),
    /// The seed is unusable (its socket broke, or it says its own machinery failed): replace it.
    Broken(Refusal),
}

/// Sends the request, its stdio and the init's pidfd to the seed and reads the answer: the program's pid
/// and pidfd, or a refusal.
fn round_trip(
    seed: &mut Seed,
    req: &Request,
    stdio: [BorrowedFd<'_>; 3],
    init: OwnedFd,
) -> Result<(u32, OwnedFd), Trip> {
    let [stdin, stdout, stderr] = stdio;
    let fds = [stdin, stdout, stderr, init.as_fd()];
    // The agent↔seed wire lives once in `hex-runtime`'s seed module, used by the seed, its
    // intermediate and here (CLAUDE.md §13). A broken socket is the seed's death: take it for broken.
    rt_seed::send_request(seed.socket.as_fd(), req, fds)
        .map_err(|e| Trip::Broken(Refusal::from(e)))?;
    // The seed holds its own copy of the init's pidfd now.
    drop(init);
    let (answer, pidfd) =
        rt_seed::recv_answer(seed.socket.as_fd()).map_err(|e| Trip::Broken(Refusal::from(e)))?;
    match answer {
        Answer::Started { pid } => match pidfd {
            Some(pidfd) => Ok((pid, pidfd)),
            None => Err(Trip::Broken(Refusal::Broken(
                "a seed said a program started but sent no pidfd".to_owned(),
            ))),
        },
        Answer::Refused { failed, why } => {
            let refusal = Refusal::Os {
                op: "exec in a container",
                why,
            };
            match failed {
                Failed::Seed => Err(Trip::Broken(refusal)),
                Failed::Request | Failed::Start => Err(Trip::Refused(refusal)),
            }
        }
    }
}
