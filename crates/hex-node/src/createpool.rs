//! The node agent's pool of create seeds (decision 5 of docs/design/runtime-architecture.md, goal 5
//! plan step 4): the shell around [`hex_core::seeds::Seeds`], the pure policy step 3 built. A seed is a
//! prepared `hex __oci-seed` process the agent keeps and reuses; a create hands an idle one the
//! container's spec over its socket and the seed clones the container's init with `clone3` into its
//! cgroup and the fresh namespaces its spec asks for, as the agent's own child (`hex-runtime`'s
//! `seed::serve_create`), in place of the shim's re-exec of the hex binary. So a seam create spawns no
//! process of the agent's and re-execs no `hex` binary per call.
//!
//! The policy — how many seeds a node keeps ([`hex_core::seeds::Seeds::growing`], up to the node's bound,
//! [`crate::sizes::Sizes::seeds`]), which one each create is
//! handed, how long a create waits, when a seed is taken for dead or hung and restarted, and what it
//! counts — is the pure core's; this shell holds it behind one lock (no core call waits), and does each
//! create's socket round-trip with the lock let go, so creates to different seeds run at once. The core's
//! clock is the shell's monotonic time since the pool opened.
//!
//! **Seeds start ahead of the creates, never on one's path.** A thread of the pool's own starts each seed
//! the core asks for, with the lock let go: it keeps a spare up and idle beyond the creates waiting, as many
//! as the most creates it has seen waiting at once ([`Inner::spare`]), so a create finds one idle and no
//! create's path holds a seed's exec. The pool starts its
//! first spare as it opens. A create that finds none idle (a burst) waits in line for a seed to free or
//! to start, while the thread adds more, up to the bound.
//!
//! Bounds (CLAUDE.md §2), each a typed refusal past it: at most [`hex_core::seeds::Limits::seeds`] seeds
//! up or starting, [`Limits::waiting`] creates in line, a wait of [`Limits::wait`], and a seed killed
//! after [`Limits::hang`] over one create. A seed that dies, or a round trip that breaks, refuses its
//! create by type and leaves its slot, which the thread starts again, counted.

use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hex_api::Refusal;
use hex_core::demand::Learned;
use hex_core::seed::{Answer, Create, Failed, MountNamespace, Park};
use hex_core::seeds::{Admit, Claim, Counts, Limits, Refused, SeedId, Seeds, Stale};
use hex_runtime::linux::oci::run::{ClassicReason, CloneInit, CloneReq, Parked};
use hex_runtime::linux::oci::seed as rt_seed;
use hex_runtime::linux::oci::{OciError, mntns};
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::process::{Pid, PidfdFlags, Signal, WaitId, WaitIdOptions};

/// The node agent's create seeds. Dropped, it stops its starting thread and ends and reaps every seed it
/// started.
pub struct CreatePool {
    shared: Arc<Shared>,
    starter: Option<JoinHandle<()>>,
}

struct Shared {
    /// The `hex` binary re-exec'd as `hex __oci-seed`.
    exe: PathBuf,
    /// The mountpoint a create seed prunes its mount namespace at (goal 5 plan step 2), passed to each
    /// seed as [`rt_seed::ENV_PRUNE`]; `None` on a rootless node, whose seeds don't prune and whose
    /// creates take the classic root path in the init.
    prune: Option<PathBuf>,
    /// A debug-only crash point the node arms on each seed it spawns (its [`rt_seed::ENV_INJECT`]), for
    /// the crash-safety test of the pruned init's root build; `None` unless a test arms it, and absent
    /// from release builds (CLAUDE.md §6).
    #[cfg(debug_assertions)]
    inject: Option<String>,
    /// The monotonic point the core's durations are measured from.
    base: Instant,
    inner: Mutex<Inner>,
    /// A create in line waits on it; a seed handed to one, a refusal of one, and shutdown wake it.
    free: Condvar,
    /// The starting thread waits on it; a create that leaves it a seed to start, a seed's death or hang,
    /// and shutdown wake it.
    wake: Condvar,
    /// Creates on the classic root path, counted by reason ([`ClassicReason`], a fixed set, so no
    /// high-cardinality label): through the shim when a pruned seed couldn't serve them or they join a
    /// pid or user namespace by path, and by a seed with the classic root on a rootless node. Each stops
    /// at `u64::MAX`, as the core's counts do.
    classic: [AtomicU64; ClassicReason::COUNT],
    /// How this node makes a pruned create's mount namespace (decision 6's update), chosen as the pool
    /// opens: the copy on a rootless node, whose seeds don't prune.
    mount_namespace: MountNamespace,
    /// Creates a pruned seed made on the pruned root path (goal 5 plan step 2), with no `pivot_root`, by
    /// the way their mount namespace was made as the seed answered ([`MountNamespace::code`]'s index).
    /// Zero on a rootless node, whose seeds don't prune. Each stops at `u64::MAX`.
    made: [AtomicU64; 3],
    /// The wakes sent the starting thread and the line, for the tests.
    #[cfg(test)]
    woke: [AtomicU64; 2],
}

struct Inner {
    seeds: Seeds,
    /// Each slot's live seed process, `Some` while the seed is up and not checked out for a create.
    procs: Vec<Option<Proc>>,
    /// Why the last seed start failed, while none has started since: a create the core refuses names it,
    /// so a node whose seeds can't start says why (CLAUDE.md §3), not only that none came free.
    start_failure: Option<String>,
    /// Set as the pool is dropped: the starting thread ends.
    shut: bool,
    /// The seeds kept up and idle beyond the creates waiting (Ada's rule, 2026-11-06): one at first, the fewest
    /// that lets a create find a seed idle rather than wait on a seed's exec, raised to the most creates seen
    /// waiting at once, so a burst that waited once finds as many idle the next time; never lowered, at most the
    /// seeds' bound. Each spare holds a seed's memory, 2,488 KiB at most (`seed_path.md`), within the node's
    /// share for seeds ([`crate::sizes`]).
    spare: Learned,
}

/// One seed process: the agent's end of its socket and the pidfd by which the agent ends and reaps it.
struct Proc {
    socket: OwnedFd,
    pidfd: OwnedFd,
}

impl CreatePool {
    /// A pool that re-execs `exe` as `hex __oci-seed`, with the node's bounds on its host (`sizes`: at most its
    /// `seeds`, slots growing from its `start`, [`Seeds::growing`]) and [`hex_core::seeds::LIMITS`]' others, its
    /// starting thread up and starting the first spare. `prune`, when
    /// given, is the mountpoint each seed prunes its mount namespace at (a root node, goal 5 plan
    /// step 2); `None` on a rootless node, whose seeds keep the agent's mount namespace. `asked`, when
    /// given, is the way a pruned create's mount namespace is made, in [`mntns::ENV_WAY`]'s words
    /// (decision 6's update); a way this node can't take is refused by type.
    pub fn new(
        exe: PathBuf,
        sizes: crate::sizes::Sizes,
        prune: Option<PathBuf>,
        asked: Option<&str>,
        #[cfg(debug_assertions)] inject: Option<String>,
    ) -> Result<Self, Refusal> {
        let limits = Limits {
            seeds: sizes.seeds,
            ..hex_core::seeds::LIMITS
        };
        Self::open(
            exe,
            Seeds::growing(limits, sizes.start),
            limits,
            prune,
            asked,
            #[cfg(debug_assertions)]
            inject,
        )
    }

    /// A pool with `limits`, every slot from the start: the tests', which set each bound.
    #[cfg(test)]
    pub fn with_limits(
        exe: PathBuf,
        limits: Limits,
        prune: Option<PathBuf>,
        asked: Option<&str>,
        #[cfg(debug_assertions)] inject: Option<String>,
    ) -> Result<Self, Refusal> {
        Self::open(
            exe,
            Seeds::new(limits),
            limits,
            prune,
            asked,
            #[cfg(debug_assertions)]
            inject,
        )
    }

    fn open(
        exe: PathBuf,
        seeds: Seeds,
        limits: Limits,
        prune: Option<PathBuf>,
        asked: Option<&str>,
        #[cfg(debug_assertions)] inject: Option<String>,
    ) -> Result<Self, Refusal> {
        // Only a pruning node's creates have a mount namespace to make another way; the probe makes
        // nothing and takes no lock (`goal6-mntns.md` §6).
        let mount_namespace = match prune {
            Some(_) => mntns::choose(mntns::probe()?, asked)?,
            None => MountNamespace::Copy,
        };
        let slots = limits.seeds.get();
        let shared = Arc::new(Shared {
            exe,
            prune,
            #[cfg(debug_assertions)]
            inject,
            base: Instant::now(),
            inner: Mutex::new(Inner {
                seeds,
                procs: (0..slots).map(|_| None).collect(),
                start_failure: None,
                shut: false,
                spare: Learned::new(1, slots),
            }),
            free: Condvar::new(),
            wake: Condvar::new(),
            classic: std::array::from_fn(|_| AtomicU64::new(0)),
            mount_namespace,
            made: std::array::from_fn(|_| AtomicU64::new(0)),
            #[cfg(test)]
            woke: [AtomicU64::new(0), AtomicU64::new(0)],
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

    /// How many creates were made through the shim since the pool opened: those a pruned seed couldn't
    /// serve, and those that join a pid or user namespace by path. Every reason but
    /// [`ClassicReason::Rootless`], whose creates a seed makes with the classic root.
    pub fn creates_through_shim(&self) -> u64 {
        self.shared
            .classic
            .iter()
            .enumerate()
            .filter(|&(i, _)| i != ClassicReason::Rootless.index())
            .map(|(_, c)| c.load(Ordering::Relaxed))
            .fold(0u64, u64::saturating_add)
    }

    /// How many creates took the classic root path for `reason` since the pool opened.
    pub fn classic_creates_for(&self, reason: ClassicReason) -> u64 {
        self.shared
            .classic
            .get(reason.index())
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// How many creates a pruned seed made on the pruned root path since the pool opened (goal 5 plan
    /// step 2): zero on a rootless node, positive on a root node once it has made a pruned-eligible box.
    pub fn pruned_creates(&self) -> u64 {
        self.shared
            .made
            .iter()
            .map(|c| c.load(Ordering::Relaxed))
            .fold(0u64, u64::saturating_add)
    }

    /// How many pruned creates had their mount namespace made `way` since the pool opened, as their
    /// seeds answered (decision 6's update).
    pub fn mount_namespaces_made(&self, way: MountNamespace) -> u64 {
        self.shared
            .made
            .get(usize::from(way.code()))
            .map_or(0, |c| c.load(Ordering::Relaxed))
    }

    /// What the pool has done, as the bench and the node's metrics read it.
    pub fn counts(&self) -> Result<Counts, Refusal> {
        Ok(self.shared.lock().map_err(Refusal::from)?.seeds.counts())
    }
}

impl CloneInit for CreatePool {
    fn prunes_root(&self) -> bool {
        self.shared.prune.is_some()
    }

    fn mount_namespace(&self) -> MountNamespace {
        self.shared.mount_namespace
    }

    fn classic(&self, reason: ClassicReason) {
        if let Some(count) = self.shared.classic.get(reason.index()) {
            let _ = count.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                Some(n.saturating_add(1))
            });
        }
    }

    fn clone_init(&self, req: CloneReq<'_>) -> Result<(i32, OwnedFd), OciError> {
        let shared = &self.shared;
        let (id, proc) = shared.acquire()?;
        let pruned = req.pruned.is_some();
        let (create, fds) = message(&req);
        match round_trip(proc.socket.as_fd(), &create, &fds) {
            Ok((pid, pidfd, made)) => {
                if pruned && let Some(count) = shared.made.get(usize::from(made.code())) {
                    let _ = count.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                        Some(n.saturating_add(1))
                    });
                }
                let started = (pid, pidfd);
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

    fn claim(&self, parked: Parked, req: &CloneReq<'_>) -> Result<(i32, OwnedFd), OciError> {
        let pruned = req.pruned.is_some();
        let Some((pid, pidfd, socket)) = parked.take() else {
            return Err(OciError::Broken(
                "a parked init was claimed twice".to_owned(),
            ));
        };
        let (create, fds) = message(req);
        // The parked init answers as a seed does, with a pidfd of its own, which the agent's stands for.
        match round_trip(socket.as_fd(), &create, &fds) {
            Ok((_, _, made)) => {
                // Counted by how its mount namespace was made, as a seed's clone is (clone_init).
                if pruned && let Some(count) = self.shared.made.get(usize::from(made.code())) {
                    let _ = count.try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                        Some(n.saturating_add(1))
                    });
                }
                Ok((pid, pidfd))
            }
            Err(Trip::Refused(e) | Trip::Broken(e)) => {
                // Refused, the init ends itself; ended either way, and reaped, so none is left.
                drop(socket);
                let _ = rustix::process::pidfd_send_signal(&pidfd, Signal::KILL);
                let _ =
                    rustix::process::waitid(WaitId::PidFd(pidfd.as_fd()), WaitIdOptions::EXITED);
                Err(e)
            }
        }
    }
}

impl CreatePool {
    /// A handle on the pool's seeds that parks inits ahead from another thread (decision 8, "The p99.9 that
    /// stayed").
    pub fn parker(&self) -> Parker {
        Parker(Arc::clone(&self.shared))
    }
}

/// A handle on a create pool's seeds that makes inits ahead ([`Parker::park`]).
#[derive(Clone)]
pub struct Parker(Arc<Shared>);

impl Parker {
    /// Has a seed clone an init into `cgroup` with the namespaces `flags`, where it waits to be claimed
    /// (decision 8, "The p99.9 that stayed"). The clone, and its wait for the host-wide `cgroup_mutex`, are
    /// here, off every create's path.
    pub fn park(&self, cgroup: BorrowedFd<'_>, flags: u64) -> Result<Parked, OciError> {
        let shared = &self.0;
        let (id, proc) = shared.acquire()?;
        let (ours, theirs) = rt_seed::socketpair()?;
        let sent = rt_seed::send_park(
            proc.socket.as_fd(),
            &Park { flags },
            [cgroup, theirs.as_fd()],
        );
        drop(theirs);
        let answered = sent.and_then(|()| {
            if readable(proc.socket.as_fd(), ANSWER_PATIENCE)? {
                rt_seed::recv_answer(proc.socket.as_fd())
            } else {
                Err(OciError::Broken(format!(
                    "a create seed did not answer a park within {ANSWER_PATIENCE:?}"
                )))
            }
        });
        match answered {
            Ok((Answer::Started { pid }, Some(pidfd))) => {
                shared.done(id, proc);
                let pid = i32::try_from(pid)
                    .map_err(|_| OciError::Broken("a parked init's pid out of range".to_owned()))?;
                // Held from here, so ended and reaped if it never says it is parked.
                let parked = Parked::new(pid, pidfd, ours, flags);
                parked.wait_parked(ANSWER_PATIENCE)?;
                Ok(parked)
            }
            Ok((Answer::Refused { failed, why }, _)) => {
                if failed == Failed::Seed {
                    shared.died(id, proc);
                } else {
                    shared.done(id, proc);
                }
                Err(OciError::Os {
                    op: "parking an init",
                    why,
                })
            }
            Ok((Answer::Started { .. }, None)) => {
                shared.died(id, proc);
                Err(OciError::Broken(
                    "a create seed said an init was parked but sent no pidfd".to_owned(),
                ))
            }
            // A create's answer (decision 6's update), which no park is given.
            Ok((Answer::StartedIn { .. }, _)) => {
                shared.died(id, proc);
                Err(OciError::Broken(
                    "a create seed answered a park as a create".to_owned(),
                ))
            }
            Err(e) => {
                shared.died(id, proc);
                Err(e)
            }
        }
    }
}

/// A create as the seed's message and its descriptors, in the order the seed reads them (`serve_create`):
/// the two pipes, a network namespace, then on the pruned path the four fixed descriptors and the bind trees,
/// in `mount::plan`'s order (the order the init attaches them), and last the spec the runtime checked.
fn message<'r>(req: &'r CloneReq<'_>) -> (Create, Vec<BorrowedFd<'r>>) {
    let pruned = req.pruned.is_some();
    let create = Create {
        bundle: req.bundle.to_string_lossy().into_owned(),
        dir: req.dir.to_string_lossy().into_owned(),
        cgroup: Some(req.cgroup.dir().to_string_lossy().into_owned()),
        network: req.network.is_some(),
        pruned,
        binds: req
            .pruned
            .as_ref()
            .map_or(0, |p| u32::try_from(p.binds.len()).unwrap_or(u32::MAX)),
        dev_mount: req
            .pruned
            .as_ref()
            .and_then(|p| p.dev_mount.as_ref().map(|d| d.mount)),
        path_joins: req
            .pruned
            .as_ref()
            .map_or(0, |p| u32::try_from(p.path_joins.len()).unwrap_or(u32::MAX)),
        ipc_uts: req.pruned.as_ref().is_some_and(|p| p.ipc_uts.is_some()),
        config: true,
        mount_namespace: req
            .pruned
            .as_ref()
            .map_or(MountNamespace::Copy, |p| p.mount_namespace),
        proc_mount: req
            .pruned
            .as_ref()
            .and_then(|p| p.proc_mount.as_ref().map(|(mount, _)| *mount)),
        masked_directory: req
            .pruned
            .as_ref()
            .is_some_and(|p| p.masked_directory.is_some()),
    };
    let mut fds: Vec<BorrowedFd<'_>> = vec![req.sync_w.as_fd(), req.map_r.as_fd()];
    if let Some(network) = &req.network {
        fds.push(network.as_fd());
    }
    if let Some(pruned) = &req.pruned {
        fds.push(pruned.bundle_dir.as_fd());
        fds.push(pruned.cgroup_dir.as_fd());
        fds.push(pruned.container_dir.as_fd());
        fds.push(pruned.rootfs.as_fd());
        fds.extend(pruned.binds.iter().map(AsFd::as_fd));
        fds.extend(pruned.path_joins.iter().map(AsFd::as_fd));
        if let Some(dev) = &pruned.dev_mount {
            fds.push(dev.tree.as_fd());
        }
        if let Some(pair) = &pruned.ipc_uts {
            fds.push(pair.ipc.as_fd());
            fds.push(pair.uts.as_fd());
        }
        if let Some((_, proc)) = &pruned.proc_mount {
            fds.push(proc.pid_namespace.as_fd());
            fds.push(proc.tree.as_fd());
        }
        if let Some(empty) = &pruned.masked_directory {
            fds.push(empty.as_fd());
        }
    }
    // Last, the spec the runtime checked, which the seed makes the container from (AC-T7.1.1).
    fds.push(req.config);
    (create, fds)
}

impl Drop for CreatePool {
    fn drop(&mut self) {
        // A poisoned lock still holds the seeds' processes: they are ended either way, none left running
        // or unreaped.
        let mut inner = self
            .shared
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.shut = true;
        for proc in inner.procs.iter_mut().filter_map(Option::take) {
            end(&proc);
        }
        drop(inner);
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

    /// Wakes the starting thread, for a seed to start.
    fn wake_starter(&self) {
        #[cfg(test)]
        self.woke[0].fetch_add(1, Ordering::Relaxed);
        self.wake.notify_one();
    }

    /// Wakes the creates in line, each to claim by its ticket: a seed was handed to one or one refused.
    fn wake_line(&self) {
        #[cfg(test)]
        self.woke[1].fetch_add(1, Ordering::Relaxed);
        self.free.notify_all();
    }

    /// An idle seed for a create: handed one at once, or, with none idle, a wait in line for one, within
    /// the core's bounds. The starting thread is woken when the create leaves it a seed to start, to keep
    /// its spare, and not otherwise: a wake is a system call, and the woken thread takes the lock this
    /// create holds. Returns the seed and its checked-out process (its slot's `procs` entry taken until
    /// [`done`](Self::done) or [`died`](Self::died)).
    fn acquire(&self) -> Result<(SeedId, Proc), OciError> {
        let mut inner = self.lock()?;
        // A seed that has served a create for the hang bound is taken for hung here, killed and its slot
        // freed for a restart, so a wedged seed never holds a slot for ever.
        self.reap_hung(&mut inner);
        let now = self.now();
        let admit = inner
            .seeds
            .create(now)
            .map_err(|r| refused(r, inner.start_failure.as_deref()))?;
        if matches!(admit, Admit::Wait { .. }) {
            let waiting = inner.seeds.waiting();
            inner.spare.observe(waiting);
        }
        if wants_start(&inner) {
            self.wake_starter();
        }
        let ticket = match admit {
            Admit::Seed(id) => return Ok((id, take(&mut inner, id)?)),
            Admit::Wait { ticket, .. } => ticket,
        };
        loop {
            let now = self.now();
            let until = match inner.seeds.claim(ticket, now).map_err(stale)? {
                Claim::Seed(id) => return Ok((id, take(&mut inner, id)?)),
                Claim::Refused(r) => return Err(refused(r, inner.start_failure.as_deref())),
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
            self.wake_line();
            self.wake_starter();
        }
    }

    /// A healthy seed back to the pool after its create: its process returned to its slot, the core told
    /// it is done, and the create that has waited longest, if any, woken.
    fn done(&self, id: SeedId, proc: Proc) {
        // A pool whose lock is poisoned takes nothing back: the seed is ended and reaped, not dropped.
        let Ok(mut inner) = self.lock() else {
            end(&proc);
            return;
        };
        match inner.procs.get_mut(id.slot()) {
            Some(slot) => *slot = Some(proc),
            None => end(&proc),
        }
        let now = self.now();
        // The line is woken for the create the seed went to, and not when none waits. Stale only if the
        // seed left meanwhile (it can't: this create held it); the line is woken then too.
        if !matches!(inner.seeds.done(id, now), Ok(None)) {
            self.wake_line();
        }
    }

    /// An unusable seed ended and its slot freed: killed and reaped, the core told it died (which counts
    /// it and asks for a restart), the waiters woken, and the starting thread woken to replace it.
    fn died(&self, id: SeedId, proc: Proc) {
        end(&proc);
        if let Ok(mut inner) = self.lock() {
            let _ = inner.seeds.died(id);
            self.wake_line();
            self.wake_starter();
        }
    }
}

/// Whether the starting thread should start a seed: the core asks for one (a vacant slot, its asking not
/// stopped by a failed start), and fewer than the spare ([`Inner::spare`]) beyond the creates waiting are up idle or
/// starting.
fn wants_start(inner: &Inner) -> bool {
    inner.seeds.wanted() > 0
        && inner.seeds.idle().saturating_add(inner.seeds.starting())
            < inner.spare.size().saturating_add(inner.seeds.waiting())
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
        let spawned = spawn(
            &shared.exe,
            shared.prune.as_deref(),
            #[cfg(debug_assertions)]
            shared.inject.as_deref(),
        );
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
                Ok(handed) => {
                    inner.start_failure = None;
                    match inner.procs.get_mut(id.slot()) {
                        Some(slot) => *slot = Some(proc),
                        None => end(&proc),
                    }
                    // The create it went to, if any, is woken; one it didn't go to waits on.
                    if handed.is_some() {
                        shared.wake_line();
                    }
                }
                // The core isn't starting it (it was reported dead meanwhile): end it.
                Err(_stale) => end(&proc),
            },
            // Counted by the core, which stops asking until the next create; the cause is kept for the
            // refusals of the creates that find no seed meanwhile, each at its wait's end.
            Err(e) => {
                inner.start_failure = Some(e.to_string());
                let _ = inner.seeds.start_failed(id);
            }
        }
    }
}

/// Spawns a seed: a `hex __oci-seed` child of the agent, holding one end of a fresh socket; the agent
/// keeps the other and the child's pidfd, by which it ends and reaps it. On a root node `prune` names
/// the mountpoint the seed prunes its mount namespace at (goal 5 plan step 2).
fn spawn(
    exe: &std::path::Path,
    prune: Option<&Path>,
    #[cfg(debug_assertions)] inject: Option<&str>,
) -> Result<Proc, OciError> {
    let (ours, theirs) = rt_seed::socketpair()?;
    // The seed's end rides in as its stdin, which the spawn duplicates into the child alone: it stays
    // close-on-exec here, so no process another thread of the agent starts meanwhile inherits it, as one
    // would in the window a cleared flag leaves (CVE-2024-21626's class). The seed keeps only its socket;
    // every other inherited descriptor closes on its re-exec.
    let mut cmd = Command::new(exe);
    cmd.arg("__oci-seed")
        .stdin(std::process::Stdio::from(theirs))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    if let Some(mountpoint) = prune {
        cmd.env(rt_seed::ENV_PRUNE, mountpoint);
    }
    // The seed's init looks for a crash point only when a test arms one on the seed; otherwise it is
    // cleared from the inherited environment, so nothing but this node arms it (CLAUDE.md §6). Debug only.
    #[cfg(debug_assertions)]
    match inject {
        Some(point) => {
            cmd.env(rt_seed::ENV_INJECT, point);
        }
        None => {
            cmd.env_remove(rt_seed::ENV_INJECT);
        }
    }
    let child = cmd
        .spawn()
        .map_err(|e| OciError::os("starting a create seed", e))?;
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
) -> Result<(i32, OwnedFd, MountNamespace), Trip> {
    rt_seed::send_create(socket, create, fds).map_err(Trip::Broken)?;
    if !readable(socket, ANSWER_PATIENCE).map_err(Trip::Broken)? {
        return Err(Trip::Broken(OciError::Broken(format!(
            "a create seed did not answer within {ANSWER_PATIENCE:?}"
        ))));
    }
    let (answer, pidfd) = rt_seed::recv_answer(socket).map_err(Trip::Broken)?;
    let (pid, made) = match answer {
        // A classic create's answer names no way: its namespace, if any, is the seed's copied.
        Answer::Started { pid } => (pid, MountNamespace::Copy),
        Answer::StartedIn {
            pid,
            mount_namespace,
        } => (pid, mount_namespace),
        Answer::Refused { failed, why } => {
            let e = OciError::Os {
                op: "create in a container",
                why,
            };
            return match failed {
                // The seed's own machinery broke: replace it.
                Failed::Seed => Err(Trip::Broken(e)),
                // A bad request, a container the seed couldn't make, or one no clone3-into-cgroup kernel
                // can make here: the seed is whole, the create refused by type.
                Failed::Request | Failed::Start | Failed::Unable => Err(Trip::Refused(e)),
            };
        }
    };
    let pid = i32::try_from(pid).map_err(|_| {
        Trip::Broken(OciError::Broken(
            "a create seed sent a pid out of range".to_owned(),
        ))
    })?;
    match pidfd {
        Some(pidfd) => Ok((pid, pidfd, made)),
        None => Err(Trip::Broken(OciError::Broken(
            "a create seed said an init started but sent no pidfd".to_owned(),
        ))),
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
/// taken for hung, or its wait ran out), with why the last seed start failed when none has started since;
/// it reaches the caller as a [`Refusal::Os`].
fn refused(r: Refused, start_failure: Option<&str>) -> OciError {
    let why = match start_failure {
        Some(cause) => format!("{r}; the last create seed start failed: {cause}"),
        None => r.to_string(),
    };
    OciError::Os {
        op: "create via a seed",
        why,
    }
}

/// A core stale report as a typed node error (the shell reports only what it was handed, so this is a
/// bug if it happens).
fn stale(s: Stale) -> OciError {
    OciError::Broken(format!("the create seed pool: {s}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Goal 5 plan step 2 (AC-T7.1.4; CLAUDE.md §10): classic-path creates are counted by a fixed set
    /// of reasons. Do: tell a pool a create went through the shim for each of two reasons, one twice,
    /// and that a rootless node's seed made one with the classic root. Expect: each reason's count is its
    /// own, the shim's total is the two shim reasons' sum without the rootless create, and a reason never
    /// told stays 0 (non-vacuity, so a miscounted reason shows).
    #[test]
    fn classic_creates_are_counted_by_reason() {
        let pool = CreatePool::new(
            PathBuf::from("/nonexistent/hex-for-the-test"),
            crate::sizes::Sizes::now().unwrap(),
            None,
            None,
            #[cfg(debug_assertions)]
            None,
        )
        .unwrap();
        pool.classic(ClassicReason::UserNamespace);
        pool.classic(ClassicReason::UserNamespace);
        pool.classic(ClassicReason::Hooks);
        pool.classic(ClassicReason::Rootless);
        assert_eq!(pool.classic_creates_for(ClassicReason::UserNamespace), 2);
        assert_eq!(pool.classic_creates_for(ClassicReason::Hooks), 1);
        assert_eq!(pool.classic_creates_for(ClassicReason::Rootless), 1);
        assert_eq!(pool.classic_creates_for(ClassicReason::Source), 0);
        assert_eq!(pool.creates_through_shim(), 3);
    }

    /// Goal 5 plan step 4 (T-T7.1.4; CLAUDE.md §3): a node whose seeds can't start says why. Do: open a
    /// pool on a `hex` binary that isn't there, wait until it has counted a failed start, then ask for a
    /// seed with a wait bound of 50 ms. Expect: the create is refused by type after the wait, naming the
    /// wait that ran out and the failed start's cause, not only that no seed came free.
    #[test]
    fn a_pool_whose_seeds_cannot_start_names_why_it_refuses() {
        let limits = Limits {
            wait: Duration::from_millis(50),
            ..hex_core::seeds::LIMITS
        };
        let pool = CreatePool::with_limits(
            PathBuf::from("/nonexistent/hex-for-the-test"),
            limits,
            None,
            None,
            #[cfg(debug_assertions)]
            None,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while pool.counts().unwrap().start_failed == 0 {
            assert!(Instant::now() < deadline, "the pool never tried a start");
            std::thread::park_timeout(Duration::from_millis(5));
        }
        let Err(OciError::Os { op, why }) = pool.shared.acquire() else {
            panic!("a pool with no seed refuses its create by type");
        };
        assert_eq!(op, "create via a seed");
        assert!(
            why.contains("the last create seed start failed: starting a create seed"),
            "the refusal names the failed start's cause: {why}"
        );
        let counts = pool.counts().unwrap();
        assert_eq!(
            (counts.refused_waited, counts.started),
            (1, 0),
            "{counts:?}"
        );
    }

    /// Goal 5 extended step 6 (criterion 2, the rate at concurrency 32): a create wakes the starting thread
    /// only when it leaves it a seed to start, and a seed given back wakes the line only when a create waits
    /// in it. At concurrency 32 the creates waited 0.190 ms a lifecycle on the pool's lock (create_serial.md
    /// run 21): each woke the thread, which took the lock the create held to find nothing to start, and
    /// each seed given back made a system call to wake a line no create was in. Do: open a pool of two slots
    /// on a stand-in seed program; take a seed and give it back until both slots' seeds are up and idle;
    /// then take and give back a seed 1,000 times. Expect: the takes before woke the thread, which started
    /// the second seed (non-vacuity); the 1,000 woke neither the thread nor the line.
    #[test]
    fn a_create_wakes_the_starting_thread_only_for_a_seed_to_start() {
        let limits = Limits {
            seeds: std::num::NonZeroUsize::new(2).unwrap(),
            wait: Duration::from_secs(10),
            ..hex_core::seeds::LIMITS
        };
        let pool = CreatePool::with_limits(
            PathBuf::from("/bin/sh"),
            limits,
            None,
            None,
            #[cfg(debug_assertions)]
            None,
        )
        .unwrap();
        let shared = &pool.shared;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let (id, proc) = shared.acquire().unwrap();
            shared.done(id, proc);
            if shared.lock().unwrap().seeds.idle() == 2 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "the pool never had both seeds up"
            );
            std::thread::park_timeout(Duration::from_millis(5));
        }
        let (starter, line) = (
            shared.woke[0].load(Ordering::Relaxed),
            shared.woke[1].load(Ordering::Relaxed),
        );
        assert!(
            starter >= 1,
            "the take of the only spare woke the starting thread"
        );
        assert_eq!(pool.counts().unwrap().started, 2);
        for _ in 0..1_000 {
            let (id, proc) = shared.acquire().unwrap();
            shared.done(id, proc);
        }
        assert_eq!(
            (
                shared.woke[0].load(Ordering::Relaxed) - starter,
                shared.woke[1].load(Ordering::Relaxed) - line
            ),
            (0, 0),
            "the wakes sent the starting thread and the line over 1,000 creates that left nothing to start"
        );
    }

    /// Goal 5 extended step 6: a seed given back goes at once to the create waiting in line, woken by the
    /// hand-off and not by its wait's end. Do: open a pool of one slot whose creates wait up to an hour;
    /// take its seed; ask for another from a thread of the test's, and once that create waits in line,
    /// give the first back. Expect: the waiting create is handed the seed given back (the test waits a
    /// minute for it, the create an hour, so a hand-off that woke no one fails).
    #[test]
    fn a_seed_given_back_goes_at_once_to_the_create_in_line() {
        let limits = Limits {
            seeds: std::num::NonZeroUsize::new(1).unwrap(),
            wait: Duration::from_secs(3600),
            ..hex_core::seeds::LIMITS
        };
        let pool = CreatePool::with_limits(
            PathBuf::from("/bin/sh"),
            limits,
            None,
            None,
            #[cfg(debug_assertions)]
            None,
        )
        .unwrap();
        let (id, proc) = pool.shared.acquire().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let shared = Arc::clone(&pool.shared);
        std::thread::spawn(move || {
            let handed = shared.acquire().map(|(id, proc)| {
                shared.done(id, proc);
                id
            });
            tx.send(handed).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(60);
        while pool.shared.lock().unwrap().seeds.waiting() == 0 {
            assert!(
                Instant::now() < deadline,
                "the second create never waited in line"
            );
            std::thread::park_timeout(Duration::from_millis(1));
        }
        pool.shared.done(id, proc);
        let handed = rx
            .recv_timeout(Duration::from_secs(60))
            .expect("the create in line was handed the seed given back");
        assert_eq!(handed.unwrap(), id);
    }

    /// Ada's rule, 2026-11-06 (AC-T7.1.3): `SPARE`'s one seed beyond the creates waiting was picked; the spare is now
    /// the most creates seen waiting at once. Do: a pool's state with four slots and none up, its spare at one, then
    /// told three creates waited at once. Expect: one seed is wanted at first, none once one is starting, and with
    /// three seen waiting, starts are wanted until three are up idle or starting; the spare never passes the slots.
    #[test]
    fn the_spare_is_the_most_creates_seen_waiting_at_once() {
        let limits = Limits {
            seeds: std::num::NonZeroUsize::new(4).unwrap(),
            ..hex_core::seeds::LIMITS
        };
        let mut inner = Inner {
            seeds: Seeds::growing(limits, 4),
            procs: (0..4).map(|_| None).collect(),
            start_failure: None,
            shut: false,
            spare: Learned::new(1, 4),
        };
        assert!(wants_start(&inner));
        let _one = inner.seeds.start().unwrap();
        assert!(!wants_start(&inner), "one spare starting");
        inner.spare.observe(3);
        let mut started = 1;
        while wants_start(&inner) {
            let _ = inner.seeds.start().unwrap();
            started += 1;
        }
        assert_eq!(started, 3);
        inner.spare.observe(9);
        assert_eq!(inner.spare.size(), 4, "never past the slots");
    }
}
