//! The node agent's pool of create seeds (decision 5 of docs/design/runtime-architecture.md, goal 5
//! plan step 4): the shell around [`hex_core::seeds::Seeds`], the pure policy step 3 built. A seed is a
//! prepared `hex __oci-seed` process the agent keeps and reuses; a create hands an idle one the
//! container's spec over its socket and the seed clones the container's init with `clone3` into its
//! cgroup and the fresh namespaces its spec asks for, as the agent's own child (`hex-runtime`'s
//! `seed::serve_create`), in place of the shim's re-exec of the hex binary. So a seam create spawns no
//! process of the agent's and re-execs no `hex` binary per call.
//!
//! The policy — how many seeds a node keeps ([`hex_core::seeds::SEEDS`]), which one each create is
//! handed, how long a create waits, when a seed is taken for dead or hung and restarted, and what it
//! counts — is the pure core's; this shell holds it behind one lock (no core call waits), starts and
//! ends the seed processes it asks for, and does the socket round-trip for each create with the lock let
//! go, so creates to different seeds run at once. The core's clock is the shell's monotonic time since
//! the pool opened.
//!
//! Bounds (CLAUDE.md §2), each a typed refusal past it: at most [`hex_core::seeds::Limits::seeds`] seeds
//! up or starting, [`Limits::waiting`] creates in line, a wait of [`Limits::wait`], and a seed killed
//! after [`Limits::hang`] over one create. A seed that dies, or a round trip that breaks, refuses its
//! create by type and leaves its slot; the next create starts a replacement, counted.

use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use hex_api::Refusal;
use hex_core::seed::{Answer, Create, Failed};
use hex_core::seeds::{Admit, Claim, Counts, Limits, Refused, SeedId, Seeds, Stale};
use hex_runtime::linux::oci::OciError;
use hex_runtime::linux::oci::run::{CloneInit, CloneReq};
use hex_runtime::linux::oci::seed as rt_seed;
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::process::{Pid, PidfdFlags, Signal, WaitId, WaitIdOptions};

/// The node agent's create seeds. Dropped, it ends and reaps every seed it started.
pub struct CreatePool {
    /// The `hex` binary re-exec'd as `hex __oci-seed`.
    exe: PathBuf,
    /// The monotonic point the core's durations are measured from.
    base: Instant,
    inner: Mutex<Inner>,
    /// A create in line waits on it; a seed freed or handed, and a start, wake it.
    free: Condvar,
}

struct Inner {
    seeds: Seeds,
    /// Each slot's live seed process, `Some` while the seed is up and not checked out for a create.
    procs: Vec<Option<Proc>>,
}

/// One seed process: the agent's end of its socket and the pidfd by which the agent ends and reaps it.
struct Proc {
    socket: OwnedFd,
    pidfd: OwnedFd,
}

impl CreatePool {
    /// A pool that re-execs `exe` as `hex __oci-seed`, with the node's default bounds
    /// ([`hex_core::seeds::LIMITS`]).
    pub fn new(exe: PathBuf) -> Self {
        Self::with_limits(exe, hex_core::seeds::LIMITS)
    }

    /// A pool with `limits` (the model-based tests set their own).
    pub fn with_limits(exe: PathBuf, limits: Limits) -> Self {
        let slots = limits.seeds.get();
        Self {
            exe,
            base: Instant::now(),
            inner: Mutex::new(Inner {
                seeds: Seeds::new(limits),
                procs: (0..slots).map(|_| None).collect(),
            }),
            free: Condvar::new(),
        }
    }

    /// What the pool has done, as the bench and the node's metrics read it.
    pub fn counts(&self) -> Result<Counts, Refusal> {
        Ok(self.lock().map_err(Refusal::from)?.seeds.counts())
    }

    fn lock(&self) -> Result<MutexGuard<'_, Inner>, OciError> {
        self.inner
            .lock()
            .map_err(|_| OciError::Broken("the create seed pool's lock was poisoned".to_owned()))
    }

    fn now(&self) -> Duration {
        self.base.elapsed()
    }
}

impl CloneInit for CreatePool {
    fn clone_init(&self, req: CloneReq<'_>) -> Result<(i32, OwnedFd), OciError> {
        let (id, proc) = self.acquire()?;
        let create = Create {
            bundle: req.bundle.to_string_lossy().into_owned(),
            dir: req.dir.to_string_lossy().into_owned(),
            cgroup: Some(req.cgroup.dir().to_string_lossy().into_owned()),
            network: req.network.is_some(),
        };
        let mut fds: Vec<BorrowedFd<'_>> = vec![req.sync_w.as_fd(), req.map_r.as_fd()];
        if let Some(network) = &req.network {
            fds.push(network.as_fd());
        }
        match round_trip(proc.socket.as_fd(), &create, &fds) {
            Ok(started) => {
                self.done(id, proc);
                Ok(started)
            }
            // The seed refused by type (a bad request, or a container it can't make) and is whole: back
            // to the pool, the refusal to the caller.
            Err(Trip::Refused(e)) => {
                self.done(id, proc);
                Err(e)
            }
            // The seed is unusable (its socket broke, or it says its own machinery failed, or it didn't
            // answer within the hang bound): end it, free its slot for a restart, refuse this create.
            Err(Trip::Broken(e)) => {
                self.died(id, proc);
                Err(e)
            }
        }
    }
}

impl CreatePool {
    /// An idle seed for a create: handed one at once, or, with none idle, a replacement started and the
    /// create made to wait for it, within the core's bounds. Returns the seed and its checked-out
    /// process (its slot's `procs` entry taken until [`done`](Self::done) or [`died`](Self::died)).
    fn acquire(&self) -> Result<(SeedId, Proc), OciError> {
        let mut inner = self.lock()?;
        // A seed that has served a create for the hang bound is taken for hung here, killed and its slot
        // freed for a restart, so a wedged seed never holds a slot for ever.
        self.reap_hung(&mut inner);
        let now = self.now();
        let ticket = match inner.seeds.create(now).map_err(refused)? {
            Admit::Seed(id) => return Ok((id, take(&mut inner, id)?)),
            Admit::Wait { ticket, .. } => ticket,
        };
        // None idle: start the replacements the core asks for, then wait in line for one.
        self.grow(&mut inner);
        loop {
            let now = self.now();
            let until = match inner.seeds.claim(ticket, now).map_err(stale)? {
                Claim::Seed(id) => return Ok((id, take(&mut inner, id)?)),
                Claim::Refused(r) => return Err(refused(r)),
                Claim::Wait { until } => until,
            };
            let left = until.saturating_sub(self.now());
            let (guard, _timed_out) = self.free.wait_timeout(inner, left).map_err(|_| {
                OciError::Broken("the create seed pool's lock was poisoned".to_owned())
            })?;
            inner = guard;
            self.grow(&mut inner);
        }
    }

    /// Starts the seeds the core asks for, one for each create waiting past those a start is already
    /// coming for, up to the bound. Each is spawned with the lock held — rare, only while the pool warms
    /// or replaces a seed, never on the steady path where every create takes an idle seed — and reported
    /// up or failed; a started seed goes to the create that has waited longest, which this then wakes. A
    /// failed start stops the core's asking until the next create, so a node whose seeds can't start
    /// tries each slot once a create, not in a loop.
    fn grow(&self, inner: &mut Inner) {
        let mut woke = false;
        while inner.seeds.idle() == 0 && inner.seeds.waiting() > inner.seeds.starting() {
            let Some(id) = inner.seeds.start() else {
                break;
            };
            match self.spawn() {
                Ok(proc) => {
                    let now = self.now();
                    match inner.seeds.started(id, now) {
                        Ok(_handed) => {
                            if let Some(slot) = inner.procs.get_mut(id.slot()) {
                                *slot = Some(proc);
                            }
                            woke = true;
                        }
                        // The core isn't starting it (it was reported dead meanwhile): end it.
                        Err(_stale) => end(&proc),
                    }
                }
                Err(_e) => {
                    let _ = inner.seeds.start_failed(id);
                    break;
                }
            }
        }
        if woke {
            self.free.notify_all();
        }
    }

    /// Takes each seed past the hang bound over its create, killing and reaping it and freeing its slot;
    /// the core refuses that create and asks for a restart. A waiting create handed the seed meets the
    /// refusal at its claim, so it is woken.
    fn reap_hung(&self, inner: &mut Inner) {
        let now = self.now();
        let mut woke = false;
        while let Some((id, _refused)) = inner.seeds.hung(now) {
            if let Some(proc) = inner.procs.get_mut(id.slot()).and_then(Option::take) {
                end(&proc);
            }
            woke = true;
        }
        if woke {
            self.free.notify_all();
        }
    }

    /// A healthy seed back to the pool after its create: its process returned to its slot, the core told
    /// it is done, and the create that has waited longest, if any, woken.
    fn done(&self, id: SeedId, proc: Proc) {
        if let Ok(mut inner) = self.lock() {
            if let Some(slot) = inner.procs.get_mut(id.slot()) {
                *slot = Some(proc);
            }
            let now = self.now();
            // Stale only if the seed left meanwhile (it can't: this create held it); either way the
            // waiters are woken.
            let _ = inner.seeds.done(id, now);
            self.free.notify_all();
        }
    }

    /// An unusable seed ended and its slot freed: killed and reaped, the core told it died (which counts
    /// it and asks for a restart), and the waiters woken.
    fn died(&self, id: SeedId, proc: Proc) {
        end(&proc);
        if let Ok(mut inner) = self.lock() {
            let _ = inner.seeds.died(id);
            self.free.notify_all();
        }
    }

    /// Spawns a seed: a `hex __oci-seed` child of the agent, holding one end of a fresh socket; the agent
    /// keeps the other and the child's pidfd, by which it ends and reaps it.
    fn spawn(&self) -> Result<Proc, OciError> {
        let (ours, theirs) = rt_seed::socketpair()?;
        rt_seed::clear_cloexec(&theirs)?;
        let mut cmd = Command::new(&self.exe);
        cmd.arg("__oci-seed")
            .env(rt_seed::ENV_SOCKET, theirs.as_fd().as_raw_fd().to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        let child = cmd
            .spawn()
            .map_err(|e| OciError::os("starting a create seed", e))?;
        // The seed keeps only its socket; every other inherited descriptor closes on its re-exec.
        drop(theirs);
        let pid = i32::try_from(child.id())
            .ok()
            .and_then(Pid::from_raw)
            .ok_or_else(|| OciError::os("starting a create seed", "the seed's pid isn't a pid"))?;
        let pidfd = rustix::process::pidfd_open(pid, PidfdFlags::empty())
            .map_err(|e| OciError::os("opening a create seed's pidfd", e))?;
        Ok(Proc {
            socket: ours,
            pidfd,
        })
    }
}

impl Drop for CreatePool {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.lock() {
            for proc in inner.procs.iter_mut().filter_map(Option::take) {
                end(&proc);
            }
        }
    }
}

/// The seed's process checked out of its slot for a create (the core has marked it busy, so no other
/// create takes it).
fn take(inner: &mut Inner, id: SeedId) -> Result<Proc, OciError> {
    inner
        .procs
        .get_mut(id.slot())
        .and_then(Option::take)
        .ok_or_else(|| OciError::Broken(format!("create seed {id} has no process")))
}

/// Ends a seed (KILL by its pidfd) and reaps it, so none the agent started is left running or a zombie.
/// A seed that already exited takes a harmless failed signal.
fn end(proc: &Proc) {
    let _ = rustix::process::pidfd_send_signal(proc.pidfd.as_fd(), Signal::KILL);
    let _ = rustix::process::waitid(WaitId::PidFd(proc.pidfd.as_fd()), WaitIdOptions::EXITED);
}

/// Why a round trip with a seed didn't make the container.
enum Trip {
    /// The seed refused by type and is still usable.
    Refused(OciError),
    /// The seed is unusable (its socket broke, it says its own machinery failed, or it didn't answer):
    /// replace it.
    Broken(OciError),
}

/// Sends the create and its descriptors to the seed and reads the answer: the init's pid and pidfd, or a
/// refusal. The wait for the answer is bounded by the hang bound, so a wedged seed is taken for broken
/// rather than waited on for ever (CLAUDE.md §2, §4).
fn round_trip(
    socket: BorrowedFd<'_>,
    create: &Create,
    fds: &[BorrowedFd<'_>],
) -> Result<(i32, OwnedFd), Trip> {
    rt_seed::send_create(socket, create, fds).map_err(Trip::Broken)?;
    if !readable(socket, ANSWER_PATIENCE).map_err(Trip::Broken)? {
        return Err(Trip::Broken(OciError::Broken(format!(
            "a create seed did not answer within {ANSWER_PATIENCE:?}"
        ))));
    }
    let (answer, pidfd) = rt_seed::recv_answer(socket).map_err(Trip::Broken)?;
    match answer {
        Answer::Started { pid } => {
            let pid = i32::try_from(pid).map_err(|_| {
                Trip::Broken(OciError::Broken(
                    "a create seed sent a pid out of range".to_owned(),
                ))
            })?;
            match pidfd {
                Some(pidfd) => Ok((pid, pidfd)),
                None => Err(Trip::Broken(OciError::Broken(
                    "a create seed said an init started but sent no pidfd".to_owned(),
                ))),
            }
        }
        Answer::Refused { failed, why } => {
            let e = OciError::Os {
                op: "create in a container",
                why,
            };
            match failed {
                // The seed's own machinery broke: replace it.
                Failed::Seed => Err(Trip::Broken(e)),
                // A bad request, a container the seed couldn't make, or one no clone3-into-cgroup kernel
                // can make here: the seed is whole, the create refused by type.
                Failed::Request | Failed::Start | Failed::Unable => Err(Trip::Refused(e)),
            }
        }
    }
}

/// How long the agent waits for a seed's answer before it takes the seed for wedged: the core's hang
/// bound, far past a clone (milliseconds), a bound on a wait for a fact (CLAUDE.md §4), not a guess at
/// the work's length.
const ANSWER_PATIENCE: Duration = hex_core::seeds::LIMITS.hang;

/// Whether `fd` turns readable within `patience`.
fn readable(fd: BorrowedFd<'_>, patience: Duration) -> Result<bool, OciError> {
    let deadline = Instant::now().checked_add(patience);
    loop {
        let left = deadline.map_or(patience, |d| d.saturating_duration_since(Instant::now()));
        let ts = Timespec {
            tv_sec: i64::try_from(left.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(left.subsec_nanos()),
        };
        let mut fds = [PollFd::new(&fd, PollFlags::IN)];
        match rustix::event::poll(&mut fds, Some(&ts)) {
            Ok(n) if n > 0 => return Ok(true),
            Ok(_) if left.is_zero() => return Ok(false),
            Ok(_) | Err(rustix::io::Errno::INTR) => {}
            Err(e) => return Err(OciError::os("poll on a create seed's socket", e)),
        }
    }
}

/// A core refusal as a typed node error naming why (the line was full, the create's seed died or was
/// taken for hung, or its wait ran out); it reaches the caller as a [`Refusal::Os`].
fn refused(r: Refused) -> OciError {
    OciError::Os {
        op: "create via a seed",
        why: r.to_string(),
    }
}

/// A core stale report as a typed node error (the shell reports only what it was handed, so this is a
/// bug if it happens).
fn stale(s: Stale) -> OciError {
    OciError::Broken(format!("the create seed pool: {s}"))
}
