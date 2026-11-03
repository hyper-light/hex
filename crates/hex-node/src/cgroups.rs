//! The `oci` boxes' cgroups that may be handed from box to box of one owner (goal 5 extended, step 6;
//! decision 5, "The cgroup lock"): the shell over [`hex_core::reuse`]'s pool. A create of an `oci` box
//! whose spec names an owner asks [`Choice`] for its cgroup once the runtime has read its spec: the
//! owner's idle cgroup under the box's device policy whose files read clean, with every bound written
//! again, else a fresh slot, else a cgroup of the box's own named by its id. A delete gives the box's
//! slot back once its record is gone, and what the pool's bounds or a failed check put out is removed by
//! a thread of its own, off create's path. How each box was ended is counted here too: through its
//! init's pidfd, that falling back to `cgroup.kill`, or `cgroup.kill` itself.

use std::collections::BTreeMap;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use hex_api::{Bounds, BoxId, BoxOwner, Refusal, Usage};
use hex_core::cgroup::Baseline;
use hex_core::reuse::{self, PER_NODE, PER_OWNER, Pool, Refused, Take, Whose};
use hex_runtime::linux::cgroup::{BoxCgroup, BoxRoot};
use hex_runtime::linux::oci::OciError;
use hex_runtime::linux::oci::resources::Insn;
use hex_runtime::linux::oci::run::{Choose, Chosen, Wants};

/// Most memory an idle cgroup may still have charged to be handed on (research f27-f30). A box whose
/// processes have all ended leaves the charge the kernel caches per CPU and some kernel memory: 3.5 to
/// 7.1 MiB on adacachy, none of it the box's own (anon, shmem and dirty file pages 0). A box that left
/// files in memory (its tmpfs, `/dev/shm`, page cache of what it wrote) leaves tens of megabytes (64 MiB
/// in oci_reuse's test 3, 85 MB in f27), which the next box would be charged for, so its cgroup goes.
/// Measured: bench/competitive/probes/reuse_residual.md.
pub const RESIDUAL: u64 = 16 << 20;

/// How many cgroups may wait for the remover at once: every idle cgroup the pool can put out at one time
/// ([`PER_NODE`]). Past it the caller removes the cgroup itself, as every delete did before.
const REMOVALS: usize = PER_NODE;

/// How long the remover waits for a cgroup it ended to empty, as [`crate::node::EMPTY_PATIENCE`].
const REMOVE_PATIENCE: Duration = Duration::from_secs(60);

/// The device policy a cgroup's own program is under: `None` when it carries none.
type Policy = Option<Vec<Insn>>;

/// A live box's slot: what its usage is reported from and what its delete gives back.
#[derive(Debug)]
pub struct Held {
    pub slot: u64,
    pub baseline: Baseline,
    /// Its `memory.peak`, written when the box was given the slot ([`BoxCgroup::peak_from_now`]).
    peak: Option<OwnedFd>,
    owner: String,
    /// The device policy whose program its cgroup carries; `None` when it isn't known (a box taken back
    /// after a restart), so its cgroup is removed with it, never handed on.
    policy: Option<Policy>,
    /// Whether `cgroup.kill` was written to its cgroup in its box's life. Linux 6.19 compares a cgroup's
    /// count of kills with the one the forking task's own cgroup had when a task is cloned into it
    /// (`CLONE_INTO_CGROUP`) and kills the child if they differ (kernel/cgroup/cgroup.c:6702-6705 sample the
    /// forking task's own cgroup, `kargs->cgrp` set only at :6768; :6902 and :6941 compare it with the
    /// target's), so every init later cloned into such a cgroup would die: it is removed with its box.
    killed: bool,
}

impl Held {
    /// `cgroup.kill` was written to its cgroup.
    pub fn mark_killed(&mut self) {
        self.killed = true;
    }
}

/// How the node's deletes ended their boxes, counted: what tests and the bench read (research f45-f50).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Kills {
    /// `oci` boxes ended through their init's pidfd, with no `cgroup.kill`.
    pub through_init: u64,
    /// `oci` boxes the pidfd didn't empty, something outside the init's pid namespace still in the
    /// cgroup, ended by `cgroup.kill` after it.
    pub fell_back: u64,
    /// Boxes ended by `cgroup.kill` alone: `process` boxes, and `oci` boxes whose init has no pid
    /// namespace of its own or isn't watched.
    pub cgroup_kill: u64,
}

/// What the pool and its remover have done since the node opened: [`reuse::Counts`] and the cgroups
/// removed off create's path, with the last removal that failed, which recovery finishes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts {
    pub pool: reuse::Counts,
    /// Slots removed rather than kept idle because `cgroup.kill` was written to them in their box's life.
    pub discarded_killed: u64,
    pub removed: u64,
    pub removed_inline: u64,
    pub removals_failed: u64,
    pub last_failure: Option<String>,
}

#[derive(Debug, Default)]
struct Removal {
    removed: AtomicU64,
    inline: AtomicU64,
    failed: AtomicU64,
    last: Mutex<Option<String>>,
}

impl Removal {
    /// Ends what runs in `cg` and removes it, counting how it went; a removal that failed is kept as the
    /// last failure ([`Counts::last_failure`]) and returned.
    fn remove(&self, cg: &BoxCgroup) -> Result<(), Refusal> {
        match end_and_remove(cg) {
            Ok(()) => {
                self.removed.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
            Err(e) => {
                self.failed.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut last) = self.last.lock() {
                    *last = Some(format!("{}: {e}", cg.dir().display()));
                }
                Err(e)
            }
        }
    }
}

/// The node's slots, idle and held, and how its boxes were ended.
pub struct Cgroups {
    pool: Mutex<Pool<Policy>>,
    held: Mutex<BTreeMap<BoxId, Held>>,
    remover: SyncSender<BoxCgroup>,
    removal: Arc<Removal>,
    through_init: AtomicU64,
    fell_back: AtomicU64,
    cgroup_kill: AtomicU64,
    discarded_killed: AtomicU64,
    /// `oci` boxes made with no owner, each given a cgroup of its own without asking the pool, so the
    /// path every box took before takes no lock of the pool's: counted here, read as the pool's
    /// [`Refused::NoOwner`].
    no_owner: AtomicU64,
}

impl Cgroups {
    /// The node's cgroups, fresh slots numbered from `first` ([`Pool::new`]), with the remover's thread
    /// started; it ends when the node does.
    pub fn start(first: u64) -> Result<Self, Refusal> {
        let (remover, queue) = mpsc::sync_channel(REMOVALS);
        let removal = Arc::new(Removal::default());
        let theirs = Arc::clone(&removal);
        std::thread::Builder::new()
            .name("hex-cgroup-remover".to_owned())
            .spawn(move || remove_each(&queue, &theirs))
            .map_err(|e| Refusal::os("starting the cgroup remover", e))?;
        Ok(Self {
            pool: Mutex::new(Pool::new(PER_OWNER, PER_NODE, first)),
            held: Mutex::new(BTreeMap::new()),
            remover,
            removal,
            through_init: AtomicU64::new(0),
            fell_back: AtomicU64::new(0),
            cgroup_kill: AtomicU64::new(0),
            discarded_killed: AtomicU64::new(0),
            no_owner: AtomicU64::new(0),
        })
    }

    fn pool(&self) -> Result<MutexGuard<'_, Pool<Policy>>, Refusal> {
        self.pool
            .lock()
            .map_err(|_| Refusal::Broken("the node's cgroup pool's lock was poisoned".to_owned()))
    }

    fn held(&self) -> Result<MutexGuard<'_, BTreeMap<BoxId, Held>>, Refusal> {
        self.held
            .lock()
            .map_err(|_| Refusal::Broken("the node's held slots' lock was poisoned".to_owned()))
    }

    /// Box `id`'s slot, while it lives.
    pub fn slot_of(&self, id: &BoxId) -> Result<Option<u64>, Refusal> {
        Ok(self.held()?.get(id).map(|h| h.slot))
    }

    /// Box `id`'s usage from its slot's cgroup `cg`, reported since it was given the slot; `None` for a
    /// box holding no slot.
    pub fn usage(&self, id: &BoxId, cg: &BoxCgroup) -> Result<Option<Usage>, Refusal> {
        let held = self.held()?;
        Ok(held
            .get(id)
            .map(|h| cg.usage_since(&h.baseline, h.peak.as_ref())))
    }

    /// Box `id`, made, holds the slot its create chose.
    pub fn hold(&self, id: &BoxId, held: Held) -> Result<(), Refusal> {
        self.held()?.insert(id.clone(), held);
        Ok(())
    }

    /// `cgroup.kill` is about to be written to box `id`'s cgroup (a signal KILL to it): the slot it holds,
    /// if any, is removed with the box rather than kept ([`Held::killed`]).
    pub fn killed_in(&self, id: &BoxId) -> Result<(), Refusal> {
        if let Some(held) = self.held()?.get_mut(id) {
            held.killed = true;
        }
        Ok(())
    }

    /// Box `id`, taken back after a restart, holds slot `slot` its record names, at `baseline`: its peak
    /// is read from its adoption on, and its cgroup is removed with it, since the device policy its
    /// cgroup carries isn't known.
    pub fn adopt(
        &self,
        id: &BoxId,
        slot: u64,
        baseline: Baseline,
        cg: &BoxCgroup,
    ) -> Result<(), Refusal> {
        let peak = cg.peak_from_now()?;
        self.hold(
            id,
            Held {
                slot,
                baseline,
                peak,
                owner: String::new(),
                policy: None,
                killed: false,
            },
        )
    }

    /// Box `id`'s slot, let go by its delete: what it holds, if anything.
    pub fn release(&self, id: &BoxId) -> Result<Option<Held>, Refusal> {
        Ok(self.held()?.remove(id))
    }

    /// Slot `cg`, held as `held`, whose box has ended and whose record is gone: idle for its owner when
    /// its device policy is known, else removed. What the pool's bounds put out is removed off create's
    /// path.
    pub fn give_back(&self, cg: BoxCgroup, held: Held, root: &BoxRoot) -> Result<(), Refusal> {
        if held.killed {
            self.discarded_killed.fetch_add(1, Ordering::Relaxed);
            return cg.remove();
        }
        let Some(policy) = held.policy else {
            return cg.remove();
        };
        let out = self.pool()?.give(&held.owner, held.slot, policy);
        for n in out {
            if let Some(old) = root.slot(n) {
                self.remove_later(old);
            }
        }
        Ok(())
    }

    /// Hands `cg` to the remover; with its queue full, removes it here.
    pub fn remove_later(&self, cg: BoxCgroup) {
        match self.remover.try_send(cg) {
            Ok(()) => {}
            Err(TrySendError::Full(cg) | TrySendError::Disconnected(cg)) => {
                self.removal.inline.fetch_add(1, Ordering::Relaxed);
                // Counted and kept as the last failure when it fails; recovery removes what's left.
                let _ = self.removal.remove(&cg);
            }
        }
    }

    /// Every idle cgroup, let go and removed before this returns: the node's caller's to call when it
    /// wants none left waiting (a test's end, a shutdown). Each is tried; the first that couldn't be
    /// removed is the refusal. Recovery removes any a crash leaves.
    pub fn drain(&self, root: &BoxRoot) -> Result<(), Refusal> {
        let out = self.pool()?.drain();
        let mut first = None;
        for n in out {
            if let Some(cg) = root.slot(n)
                && let Err(e) = self.removal.remove(&cg)
            {
                first.get_or_insert(e);
            }
        }
        first.map_or(Ok(()), Err)
    }

    /// An `oci` box with no owner was given a cgroup of its own.
    pub fn no_owner(&self) {
        self.no_owner.fetch_add(1, Ordering::Relaxed);
    }

    pub fn ended_through_init(&self) {
        self.through_init.fetch_add(1, Ordering::Relaxed);
    }

    pub fn fell_back(&self) {
        self.fell_back.fetch_add(1, Ordering::Relaxed);
    }

    pub fn killed(&self) {
        self.cgroup_kill.fetch_add(1, Ordering::Relaxed);
    }

    pub fn kills(&self) -> Kills {
        Kills {
            through_init: self.through_init.load(Ordering::Relaxed),
            fell_back: self.fell_back.load(Ordering::Relaxed),
            cgroup_kill: self.cgroup_kill.load(Ordering::Relaxed),
        }
    }

    pub fn counts(&self) -> Result<Counts, Refusal> {
        let mut pool = self.pool()?.counts();
        let no_owner = pool.refused.entry(Refused::NoOwner).or_default();
        *no_owner = no_owner.saturating_add(self.no_owner.load(Ordering::Relaxed));
        Ok(Counts {
            pool,
            discarded_killed: self.discarded_killed.load(Ordering::Relaxed),
            removed: self.removal.removed.load(Ordering::Relaxed),
            removed_inline: self.removal.inline.load(Ordering::Relaxed),
            removals_failed: self.removal.failed.load(Ordering::Relaxed),
            last_failure: self
                .removal
                .last
                .lock()
                .map_err(|_| Refusal::Broken("the cgroup remover's lock was poisoned".to_owned()))?
                .clone(),
        })
    }
}

/// The remover's thread: ends and removes each cgroup it's handed, until the node's end closes its
/// queue.
fn remove_each(queue: &Receiver<BoxCgroup>, removal: &Removal) {
    while let Ok(cg) = queue.recv() {
        // Counted and kept as the last failure when it fails; recovery removes what's left.
        let _ = removal.remove(&cg);
    }
}

/// Most sub-cgroups [`end_and_remove`] takes apart under one cgroup: a box that may change its cgroup
/// never holds a slot, so a slot's are what something outside the node made in it while it was idle;
/// past this many the removal is refused, by type, and recovery finishes it.
const REMOVE_DIRS: usize = 256;

/// Ends whatever runs in `cg` (a cgroup no box holds now: idle, refused, or a failed create's) and
/// removes it with any sub-cgroups it holds, deepest first: a flat walk, bounded (CLAUDE.md §2, §11).
fn end_and_remove(cg: &BoxCgroup) -> Result<(), Refusal> {
    if !cg.dir().exists() {
        return Ok(());
    }
    if cg.populated()? {
        cg.kill()?;
        if !cg.wait_empty(REMOVE_PATIENCE)? {
            return Err(Refusal::Os {
                op: "removing an idle cgroup",
                why: format!(
                    "{} still holds processes {REMOVE_PATIENCE:?} after cgroup.kill",
                    cg.dir().display()
                ),
            });
        }
    }
    let mut found = Vec::new();
    let mut queue = vec![cg.dir().to_path_buf()];
    while let Some(dir) = queue.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(Refusal::os("listing an idle cgroup's sub-cgroups", e)),
        };
        for entry in entries {
            let entry =
                entry.map_err(|e| Refusal::os("listing an idle cgroup's sub-cgroups", e))?;
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                if found.len() >= REMOVE_DIRS {
                    return Err(Refusal::Full(REMOVE_DIRS));
                }
                found.push(entry.path());
                queue.push(entry.path());
            }
        }
    }
    // Found parents first, so taken apart in reverse every child goes before its parent.
    for dir in found.iter().rev() {
        BoxCgroup::at(dir.clone()).remove()?;
    }
    cg.remove()
}

/// What a create's [`Choice`] gave its box.
#[derive(Debug)]
pub enum Made {
    /// A slot: its cgroup may be handed on after the box.
    Slot(Held),
    /// A cgroup of the box's own, named by its id, for this reason.
    Own(Refused),
}

/// One create's choice of cgroup, made when the runtime has read the box's spec ([`Choose`]).
pub struct Choice<'a> {
    pub cgroups: &'a Cgroups,
    /// The oci boxes' root, which slots and the boxes' own cgroups are made under.
    pub root: &'a BoxRoot,
    pub id: &'a BoxId,
    pub owner: &'a BoxOwner,
    pub bounds: &'a Bounds,
    made: Mutex<Option<Made>>,
    /// A refusal its choosing met (the id taken since, the node full, a bound's controller missing),
    /// kept as it was so the create is refused by its type, not as the runtime's error that carried it.
    refused: Mutex<Option<Refusal>>,
}

impl<'a> Choice<'a> {
    pub fn new(
        cgroups: &'a Cgroups,
        root: &'a BoxRoot,
        id: &'a BoxId,
        owner: &'a BoxOwner,
        bounds: &'a Bounds,
    ) -> Self {
        Self {
            cgroups,
            root,
            id,
            owner,
            bounds,
            made: Mutex::new(None),
            refused: Mutex::new(None),
        }
    }

    /// What it gave the box, once the runtime asked, and the refusal it met if one ended the create.
    pub fn made(self) -> (Option<Made>, Option<Refusal>) {
        (
            self.made.into_inner().ok().flatten(),
            self.refused.into_inner().ok().flatten(),
        )
    }

    /// `r`, kept as it is, carried through the runtime as its error.
    fn refuse_create(&self, r: Refusal) -> OciError {
        let carried = OciError::Broken(r.to_string());
        if let Ok(mut kept) = self.refused.lock() {
            *kept = Some(r);
        }
        carried
    }

    fn record(&self, made: Made) -> Result<(), OciError> {
        *self.made.lock().map_err(|_| {
            OciError::Broken("a create's cgroup choice's lock was poisoned".to_owned())
        })? = Some(made);
        Ok(())
    }

    /// Idle slot `n`'s cgroup, checked and given the box's bounds again, or why it can't be handed on.
    fn hand_on(&self, n: u64) -> Result<(BoxCgroup, Held), (Option<BoxCgroup>, Refused)> {
        let Some(cg) = self.root.slot(n) else {
            return Err((None, Refused::Unreadable));
        };
        let files = match cg.idle_files() {
            Ok(files) => files,
            Err(_) => return Err((Some(cg), Refused::Unreadable)),
        };
        if let Err(reason) = reuse::check(&files, self.bounds.memory_max, RESIDUAL) {
            return Err((Some(cg), reason));
        }
        // Every bound again, then the counters this box's usage is reported from, then its peak's
        // descriptor: a cgroup none of these can be done to goes, and the box gets a fresh one.
        if self.root.rebound(&cg, self.bounds).is_err() {
            return Err((Some(cg), Refused::Unreadable));
        }
        let baseline = cg.baseline();
        let peak = match cg.peak_from_now() {
            Ok(peak) => peak,
            Err(_) => return Err((Some(cg), Refused::Unreadable)),
        };
        let held = Held {
            slot: n,
            baseline,
            peak,
            owner: self.owner.name.clone(),
            policy: None,
            killed: false,
        };
        Ok((cg, held))
    }
}

impl Choose for Choice<'_> {
    fn choose(&self, wants: &Wants<'_>) -> Result<Chosen, OciError> {
        let policy: Policy = wants.program.map(<[Insn]>::to_vec);
        let whose = Whose {
            owner: Some(&self.owner.name),
            reuse: self.owner.reuse,
            privileged: wants.privileged,
        };
        // Each refused idle cgroup leaves the pool, so the takes end within the owner's bound.
        for _ in 0..=PER_OWNER {
            let take = self
                .cgroups
                .pool()
                .map_err(|r| self.refuse_create(r))?
                .take(whose, &policy);
            match take {
                Take::Own(reason) => {
                    let cgroup = self
                        .root
                        .create(self.id, self.bounds)
                        .map_err(|r| self.refuse_create(r))?;
                    self.record(Made::Own(reason))?;
                    return Ok(Chosen {
                        cgroup,
                        attached: false,
                    });
                }
                Take::Fresh(n) => {
                    let cgroup = self
                        .root
                        .create_slot(n, self.bounds)
                        .map_err(|r| self.refuse_create(r))?;
                    self.record(Made::Slot(Held {
                        slot: n,
                        baseline: Baseline::default(),
                        peak: None,
                        owner: self.owner.name.clone(),
                        policy: Some(policy),
                        killed: false,
                    }))?;
                    return Ok(Chosen {
                        cgroup,
                        attached: false,
                    });
                }
                Take::Idle(n) => match self.hand_on(n) {
                    Ok((cgroup, mut held)) => {
                        self.cgroups
                            .pool()
                            .map_err(|r| self.refuse_create(r))?
                            .hand_on();
                        held.policy = Some(policy);
                        self.record(Made::Slot(held))?;
                        return Ok(Chosen {
                            cgroup,
                            attached: true,
                        });
                    }
                    Err((cg, reason)) => {
                        self.cgroups
                            .pool()
                            .map_err(|r| self.refuse_create(r))?
                            .refuse(reason);
                        if let Some(cg) = cg {
                            self.cgroups.remove_later(cg);
                        }
                    }
                },
            }
        }
        Err(OciError::Broken(format!(
            "box {} found more than {PER_OWNER} of its owner's idle cgroups refused in one create",
            self.id
        )))
    }
}
