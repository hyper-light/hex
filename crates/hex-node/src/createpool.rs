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
//! counts — is the pure core's; this shell holds it behind one lock (no core call waits), and does each
//! create's socket round-trip with the lock let go, so creates to different seeds run at once. The core's
//! clock is the shell's monotonic time since the pool opened.
//!
//! **Seeds start ahead of the creates, never on one's path.** A thread of the pool's own starts each seed
//! the core asks for, with the lock let go: it keeps [`SPARE`] seed up and idle beyond the creates
//! waiting, so a create finds one idle and no create waits on a seed's exec. The pool starts its first
//! spare as it opens. A create that finds none idle (a burst) waits in line for the seeds busy cloning to
//! free, while the thread adds more, up to the bound.
//!
//! Bounds (CLAUDE.md §2), each a typed refusal past it: at most [`hex_core::seeds::Limits::seeds`] seeds
//! up or starting, [`Limits::waiting`] creates in line, a wait of [`Limits::wait`], and a seed killed
//! after [`Limits::hang`] over one create. A seed that dies, or a round trip that breaks, refuses its
//! create by type and leaves its slot, which the thread starts again, counted.

use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hex_api::Refusal;
use hex_core::seed::{Answer, Create, Failed};
use hex_core::seeds::{Admit, Claim, Counts, Limits, Refused, SeedId, Seeds, Stale};
use hex_runtime::linux::oci::OciError;
use hex_runtime::linux::oci::run::{CloneInit, CloneReq};
use hex_runtime::linux::oci::seed as rt_seed;
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::process::{Pid, PidfdFlags, Signal, WaitId, WaitIdOptions};

/// The seeds the pool keeps up and idle beyond the creates waiting: one, so a create finds a seed idle
/// rather than wait on a seed's exec. A seed is busy for its clone (0.209 ms p50 on fedora at
/// concurrency 1, `bench/competitive/probes/seed_path.md`), so under a burst the line drains through the
/// seeds freeing faster than an exec would start one, and the pool grows to the burst's need meanwhile:
/// on a loaded fedora at concurrency 32 it grew to all 32, and 45 of 10,000 creates waited while it did.
/// A stated bound, not a tuning: each spare holds a seed's memory (1.5 to 2.4 MiB resident, measured on
/// fedora), charged to the node agent's cgroup.
pub const SPARE: usize = 1;

/// The node agent's create seeds. Dropped, it stops its starting thread and ends and reaps every seed it
/// started.
pub struct CreatePool {
    shared: Arc<Shared>,
    starter: Option<JoinHandle<()>>,
}

struct Shared {
    /// The `hex` binary re-exec'd as `hex __oci-seed`.
    exe: PathBuf,
    /// The monotonic point the core's durations are measured from.
    base: Instant,
    inner: Mutex<Inner>,
    /// A create in line waits on it; a seed freed, handed or started wakes it.
    free: Condvar,
    /// The starting thread waits on it; a create taking or waiting for a seed, a seed's death or hang,
    /// and shutdown wake it.
    wake: Condvar,
    /// Creates made through the shim because no seed's one clone can make them
    /// ([`CloneInit::through_shim`]); stops at `u64::MAX`, as the core's counts do.
    through_shim: AtomicU64,
}

struct Inner {
    seeds: Seeds,
    /// Each slot's live seed process, `Some` while the seed is up and not checked out for a create.
    procs: Vec<Option<Proc>>,
    /// Set as the pool is dropped: the starting thread ends.
    shut: bool,
}

/// One seed process: the agent's end of its socket and the pidfd by which the agent ends and reaps it.
struct Proc {
    socket: OwnedFd,
    pidfd: OwnedFd,
}

impl CreatePool {
    /// A pool that re-execs `exe` as `hex __oci-seed`, with the node's default bounds
    /// ([`hex_core::seeds::LIMITS`]), its starting thread up and starting the first spare.
    pub fn new(exe: PathBuf) -> Result<Self, Refusal> {
        Self::with_limits(exe, hex_core::seeds::LIMITS)
    }

    /// A pool with `limits`.
    pub fn with_limits(exe: PathBuf, limits: Limits) -> Result<Self, Refusal> {
        let slots = limits.seeds.get();
        let shared = Arc::new(Shared {
            exe,
            base: Instant::now(),
            inner: Mutex::new(Inner {
                seeds: Seeds::new(limits),
                procs: (0..slots).map(|_| None).collect(),
                shut: false,
            }),
            free: Condvar::new(),
            wake: Condvar::new(),
            through_shim: AtomicU64::new(0),
        });
        let starting = Arc::clone(&shared);
        let starter = std::thread::Builder::new()
            .name("hex-seeds".to_owned())
            .spawn(move || start_seeds(&starting))
            .map_err(|e| Refusal::os("starting the create seeds' thread", e))?;
        Ok(Self {
            shared,
            starter: Some(starter),
        })
    }

    /// How many creates were made through the shim since the pool opened: those whose container joins a
    /// pid or user namespace by path, which no seed's one clone can make.
    pub fn creates_through_shim(&self) -> u64 {
        self.shared.through_shim.load(Ordering::Relaxed)
    }

    /// What the pool has done, as the bench and the node's metrics read it.
    pub fn counts(&self) -> Result<Counts, Refusal> {
        Ok(self.shared.lock().map_err(Refusal::from)?.seeds.counts())
    }
}

impl CloneInit for CreatePool {
    fn through_shim(&self) {
        let _ = self
            .shared
            .through_shim
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_add(1))
            });
    }

    fn clone_init(&self, req: CloneReq<'_>) -> Result<(i32, OwnedFd), OciError> {
        let shared = &self.shared;
        let (id, proc) = shared.acquire()?;
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
                shared.done(id, proc);
                Ok(started)
            }
            // The seed refused by type (a bad request, or a container it can't make) and is whole: back
            // to the pool, the refusal to the caller.
            Err(Trip::Refused(e)) => {
                shared.done(id, proc);
                Err(e)
            }
            // The seed is unusable (its socket broke, or it says its own machinery failed, or it didn't
            // answer within the hang bound): end it, free its slot for a restart, refuse this create.
            Err(Trip::Broken(e)) => {
                shared.died(id, proc);
                Err(e)
            }
        }
    }
}

impl Drop for CreatePool {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.shared.inner.lock() {
            inner.shut = true;
            for proc in inner.procs.iter_mut().filter_map(Option::take) {
                end(&proc);
            }
        }
        self.shared.wake.notify_all();
        self.shared.free.notify_all();
        if let Some(starter) = self.starter.take() {
            let _ = starter.join();
        }
    }
}

impl Shared {
    fn lock(&self) -> Result<MutexGuard<'_, Inner>, OciError> {
        self.inner
            .lock()
            .map_err(|_| OciError::Broken("the create seed pool's lock was poisoned".to_owned()))
    }

    fn now(&self) -> Duration {
        self.base.elapsed()
    }

    /// An idle seed for a create: handed one at once, or, with none idle, a wait in line for one, within
    /// the core's bounds. Either way the starting thread is woken, to keep its spare. Returns the seed and
    /// its checked-out process (its slot's `procs` entry taken until [`done`](Self::done) or
    /// [`died`](Self::died)).
    fn acquire(&self) -> Result<(SeedId, Proc), OciError> {
        let mut inner = self.lock()?;
        // A seed that has served a create for the hang bound is taken for hung here, killed and its slot
        // freed for a restart, so a wedged seed never holds a slot for ever.
        self.reap_hung(&mut inner);
        let now = self.now();
        let admit = inner.seeds.create(now).map_err(refused)?;
        self.wake.notify_one();
        let ticket = match admit {
            Admit::Seed(id) => return Ok((id, take(&mut inner, id)?)),
            Admit::Wait { ticket, .. } => ticket,
        };
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
        }
    }

    /// Takes each seed past the hang bound over its create, killing and reaping it and freeing its slot;
    /// the core refuses that create and asks for a restart. A waiting create handed the seed meets the
    /// refusal at its claim, so the line and the starting thread are woken.
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
            self.wake.notify_one();
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
    /// it and asks for a restart), the waiters woken, and the starting thread woken to replace it.
    fn died(&self, id: SeedId, proc: Proc) {
        end(&proc);
        if let Ok(mut inner) = self.lock() {
            let _ = inner.seeds.died(id);
            self.free.notify_all();
            self.wake.notify_one();
        }
    }
}

/// Whether the starting thread should start a seed: the core asks for one (a vacant slot, its asking not
/// stopped by a failed start), and fewer than [`SPARE`] beyond the creates waiting are up idle or starting.
fn wants_start(inner: &Inner) -> bool {
    inner.seeds.wanted() > 0
        && inner.seeds.idle().saturating_add(inner.seeds.starting())
            < SPARE.saturating_add(inner.seeds.waiting())
}

/// The starting thread: starts each seed [`wants_start`] asks for, one at a time and with the lock let go,
/// so no create's path holds a seed's exec and no create waits on the lock meanwhile. A started seed goes
/// to the create that has waited longest, which is woken. A failed start stops the core's asking until the
/// next create, so a node whose seeds can't start tries each slot once a create, not in a loop. It ends at
/// shutdown, ending a seed it started meanwhile, or on a poisoned lock, whereupon every create refuses on
/// the same lock.
fn start_seeds(shared: &Shared) {
    loop {
        let Ok(mut inner) = shared.inner.lock() else {
            return;
        };
        while !inner.shut && !wants_start(&inner) {
            inner = match shared.wake.wait(inner) {
                Ok(inner) => inner,
                Err(_) => return,
            };
        }
        if inner.shut {
            return;
        }
        let Some(id) = inner.seeds.start() else {
            // `wants_start` saw room; were the core to give none, wait for the next change rather than
            // ask again at once.
            drop(match shared.wake.wait(inner) {
                Ok(inner) => inner,
                Err(_) => return,
            });
            continue;
        };
        drop(inner);
        let spawned = spawn(&shared.exe);
        let Ok(mut inner) = shared.inner.lock() else {
            if let Ok(proc) = &spawned {
                end(proc);
            }
            return;
        };
        match spawned {
            Ok(proc) if inner.shut => {
                end(&proc);
                return;
            }
            Ok(proc) => match inner.seeds.started(id, shared.now()) {
                Ok(_handed) => {
                    if let Some(slot) = inner.procs.get_mut(id.slot()) {
                        *slot = Some(proc);
                    }
                }
                // The core isn't starting it (it was reported dead meanwhile): end it.
                Err(_stale) => end(&proc),
            },
            Err(_e) => {
                let _ = inner.seeds.start_failed(id);
            }
        }
        shared.free.notify_all();
    }
}

/// Spawns a seed: a `hex __oci-seed` child of the agent, holding one end of a fresh socket; the agent
/// keeps the other and the child's pidfd, by which it ends and reaps it.
fn spawn(exe: &std::path::Path) -> Result<Proc, OciError> {
    let (ours, theirs) = rt_seed::socketpair()?;
    rt_seed::clear_cloexec(&theirs)?;
    let mut cmd = Command::new(exe);
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
