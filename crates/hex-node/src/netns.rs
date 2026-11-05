//! The node agent's network namespaces made ahead (decision 4 of docs/design/runtime-architecture.md,
//! goal 4 plan step 4): the shell around [`hex_core::netns::Pool`]. One thread of the agent makes each
//! namespace off the create path whenever the pool asks for more ([`net::make_ahead`]: unshared on that
//! thread, its loopback brought up inside it, the thread back home after). A create asks the pool once
//! the runtime has read its spec ([`Networks`]): a container that makes a network namespace of its own
//! in the agent's user namespace takes one, which its shim joins, and the pool counts every container's
//! namespace, taken or made on the path. A namespace taken is the container's alone, so it ends with the
//! container, and the kernel's deferred cleanup tears it down after delete returns.
//!
//! But for a box that may share its owner's (goal 5 extended, step 6; Ada's call of 2026-11-03): its
//! namespace, taken from the pool or made by the node on the path, is kept by the node while the box lives
//! ([`Netns::hold`]), and when the box and its record are gone it goes to the node's checker threads
//! ([`netcheck::check`]), one of which keeps it idle for the owner's next box ([`hex_core::netns_reuse::Idle`]) if
//! nothing of the box is left in it, and closes it, counted by why, if something is. Nothing runs in an idle
//! namespace, so what the check saw holds until it's handed on. That takes the making, the loopback's bringing
//! up and the teardown, each under the host's one RTNL and one `sysctl_lock`, off the owner's next box's
//! lifecycle (docs/research/runtime/goal5-netns-reuse.md).

use std::collections::BTreeMap;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use hex_api::{BoxId, BoxOwner, Refusal};
use hex_core::netns::{BOUND, Counts, IPC_UTS_BOUND, Pool, Take};
use hex_core::netns_reuse::{self, Idle, Refused as Why, Take as Kept, Whose};
use hex_runtime::linux::oci::OciError;
use hex_runtime::linux::oci::net;
use hex_runtime::linux::oci::netcheck::{self, Fresh, ProcNet, Scratch};
use hex_runtime::linux::oci::run::Networks;

/// The pool and the thread that fills it, and the namespaces kept for their owners' next boxes (goal 5
/// extended, step 6) with the thread that checks each as its box goes. Dropped, it shuts the pool down,
/// which closes what it held, closes every kept namespace, and waits for its threads.
pub struct Netns {
    shared: Arc<Shared>,
    refill: Option<JoinHandle<()>>,
    kept: Option<Keeping>,
}

/// How many threads check the namespaces given back. With one, [`netcheck::check`] took 127 to 131 us a
/// namespace at concurrency 32 on fedora, so it checked at most about 7,700 a second while that concurrency gave
/// back 8,364, and 12% of its boxes found none of their owner's idle and were given a fresh one
/// (bench/competitive/probes/reuse_pass.md, run 7). Four check about 30,000 a second, 3.6 times that, so a burst
/// of 32 given back at once clears in about a millisecond; a namespace given back with the line full is closed
/// unchecked, counted.
const CHECKERS: usize = 4;

/// The namespaces kept for owners: none on a node without a pool (a rootless node's containers make
/// theirs in user namespaces of their own), so its creates take as before.
struct Keeping {
    state: Arc<KeptState>,
    returns: Option<SyncSender<(String, KeptNet)>>,
    checkers: Vec<JoinHandle<()>>,
}

/// A namespace kept for an owner, the netlink socket the checks keep in it from its first check on, so they
/// never close one of their own there, and its `/proc/net` files they hold from its first clean check on
/// ([`netcheck::check`]), so they needn't enter it, with the place under the node's [`FilesBound`] they take:
/// closed together.
pub struct KeptNet {
    ns: OwnedFd,
    socket: Option<OwnedFd>,
    files: Option<netcheck::Files>,
    slot: Option<FilesSlot>,
}

/// A kept namespace's own descriptors: the namespace and the check's socket.
const FDS_A_KEPT: u64 = 2;
/// The `/proc/net` files a kept namespace may hold besides ([`netcheck::Files`]): `sockstat`, `dev` and the two
/// iptables files.
const FILES_A_KEPT: u64 = 4;
/// A live box's besides its kept namespace's: the supervisor's pidfd of its init and its cgroup's directory.
const FDS_A_BOX: u64 = 2;
/// A seed's: its socket and its pidfd.
const FDS_A_SEED: u64 = 2;
/// The most exec seeds a node keeps (`seed::default_budget`'s ceiling).
const EXEC_SEEDS: u64 = 128;
/// What the agent holds open past those bounds, its listener, its polls and what its threads open for a moment:
/// a margin, stated rather than measured, well past the few dozen it is.
const FDS_BESIDE: u64 = 1_024;
/// A pool's descriptors past what it holds: its refill thread's own namespace, which it goes back to, and the one
/// it is making.
const FDS_A_REFILL: u64 = 2;

/// The bounds of the node's pools made ahead: decision 4's network namespaces, one descriptor each, and step 4's
/// ipc and uts pairs, two each; and whether it keeps step 3's prepared `/dev` mounts ([`crate::devfs::Devfs`]), a
/// root node's alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pools {
    /// The most network namespaces the pool holds.
    pub netns: usize,
    /// The most ipc and uts pairs the pool holds.
    pub ipc_uts: usize,
    /// Whether the node keeps step 3's prepared `/dev` pool.
    pub devfs: bool,
}

impl Pools {
    /// A root node's, each pool at its bound unless set.
    pub const ROOT: Self = Self {
        netns: BOUND,
        ipc_uts: IPC_UTS_BOUND,
        devfs: true,
    };

    /// The descriptors the pools hold at their bounds, each pool's refill thread's beside, and the takes of every
    /// call [`crate::serve::WORKERS`] runs at once, one of each pool's until its box's init joins them. A pool of
    /// none has no thread.
    fn descriptors(self) -> u64 {
        let pool = |bound: usize, each: u64| {
            if bound == 0 {
                0
            } else {
                of(bound, each).saturating_add(FDS_A_REFILL.saturating_mul(each))
            }
        };
        let takes =
            u64::from(self.netns > 0).saturating_add(u64::from(self.ipc_uts > 0).saturating_mul(2));
        pool(self.netns, 1)
            .saturating_add(pool(self.ipc_uts, 2))
            .saturating_add(of(crate::serve::WORKERS, takes))
    }
}

fn of(count: usize, each: u64) -> u64 {
    u64::try_from(count)
        .unwrap_or(u64::MAX)
        .saturating_mul(each)
}

/// The kept namespaces there may be at once: idle, on the checkers' line, and one for each live box.
fn kept_at_most() -> u64 {
    of(netns_reuse::PER_NODE, 2).saturating_add(of(hex_runtime::linux::cgroup::MAX_BOXES, 1))
}

/// The descriptors this node may hold at once without the namespaces' `/proc/net` files, each by its bound: the
/// kept namespaces; each live box's own; a connection and the programs it watches; the seeds of both pools; the
/// namespaces made ahead under `pools`.
fn descriptors_of_the_node(pools: Pools) -> u64 {
    let connections = crate::serve::LIMITS.connections;
    [
        pools.descriptors(),
        kept_at_most().saturating_mul(FDS_A_KEPT),
        of(hex_runtime::linux::cgroup::MAX_BOXES, FDS_A_BOX),
        of(connections, 1),
        of(connections.saturating_mul(crate::serve::LIMITS.programs), 1),
        of(hex_core::seeds::SEEDS.get(), FDS_A_SEED),
        EXEC_SEEDS.saturating_mul(FDS_A_SEED),
        FDS_BESIDE,
    ]
    .into_iter()
    .fold(0, u64::saturating_add)
}

/// How many kept namespaces may hold their `/proc/net` files at once: what the soft descriptor limit the node was
/// given leaves past its own descriptors at its bounds, four a namespace, none where it leaves none. The limit
/// stays as given: a box without an rlimit of its own sees the limit it would without the files (the lead,
/// 2026-11-03T23:07Z). A namespace past the bound is checked by entering it, counted (`files_bounded`), as one
/// whose files couldn't be read is, and as a rootless node's are; a limit too small for the node's own work
/// shows where it runs out, as each call's own typed refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilesBound {
    /// The soft limit, `None` where it is unlimited.
    pub soft: Option<u64>,
    /// The node's own descriptors at its bounds, without the files ([`descriptors_of_the_node`]).
    pub own: u64,
    /// The namespaces that may hold their files at once.
    pub namespaces: u64,
}

impl FilesBound {
    /// The bound under `limit` for a node whose pools are `pools`, an unlimited one holding them for every
    /// namespace there may be.
    pub fn of(limit: &rustix::process::Rlimit, pools: Pools) -> Self {
        let own = descriptors_of_the_node(pools);
        let namespaces = match limit.current {
            None => kept_at_most(),
            Some(soft) => (soft.saturating_sub(own) / FILES_A_KEPT).min(kept_at_most()),
        };
        Self {
            soft: limit.current,
            own,
            namespaces,
        }
    }

    /// The bound under this process's descriptor limit now, for a node whose pools are `pools`.
    pub fn now(pools: Pools) -> Self {
        Self::of(
            &rustix::process::getrlimit(rustix::process::Resource::Nofile),
            pools,
        )
    }
}

/// A place under the [`FilesBound`], taken before a check opens a namespace's files and given back as it drops
/// with them, so at most the bound's namespaces hold files at once.
struct FilesSlot(Arc<AtomicU64>);

impl FilesSlot {
    /// One of `held`'s places, `None` with all `bound` of them taken.
    fn take(held: &Arc<AtomicU64>, bound: u64) -> Option<Self> {
        held.try_update(Ordering::AcqRel, Ordering::Acquire, |n| {
            (n < bound).then(|| n.checked_add(1)).flatten()
        })
        .ok()
        .map(|_| Self(Arc::clone(held)))
    }
}

impl Drop for FilesSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

struct KeptState {
    idle: Mutex<Idle<KeptNet>>,
    /// Each live box's namespace, kept from its create to its delete, with its owner: at most one a box.
    held: Mutex<BTreeMap<BoxId, (String, KeptNet)>>,
    /// Namespaces given back as their boxes went, each then checked or closed unchecked.
    given_back: AtomicU64,
    /// Namespaces the checker ruled on, kept idle or closed.
    checked: AtomicU64,
    /// Namespaces given back with the checkers' line full ([`netns_reuse::PER_NODE`]), or after every checker
    /// ended,
    /// closed unchecked.
    unchecked: AtomicU64,
    /// Checks after which the checker couldn't go back to its own namespace.
    lost_home: AtomicU64,
    /// Namespaces the node made on a create's path for a box it keeps one for, the pool dry, which the box's
    /// init joins as it joins one taken from the pool. The pool counts each among its takes made on the path
    /// too, with the takes whose container makes its own, so this tells the two apart.
    made_on_path: AtomicU64,
    /// Checks whose sockets' count wasn't the check's own socket alone, which read the protocols.
    protocols: AtomicU64,
    /// Checks that came to their verdict through the namespace's held files, with no entry.
    through_files: AtomicU64,
    /// Checks in which a read or an open of those files failed, which looked by entering instead.
    files_failed: AtomicU64,
    /// Checks that entered because the namespace held no files with the [`FilesBound`] all taken.
    files_bounded: AtomicU64,
    /// The namespaces that may hold their files at once.
    files_bound: FilesBound,
    /// The namespaces holding them now ([`FilesSlot`]).
    files_held: Arc<AtomicU64>,
    /// The time the checks took, all together, in nanoseconds: each check's wall time on the checker.
    check_ns: AtomicU64,
}

/// What the kept namespaces have done, as the bench and the tests read it. Every namespace given back is
/// checked or closed unchecked, so `checked + unchecked` reaches `given_back` once the checker has ruled on
/// each.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReuseCounts {
    pub pool: netns_reuse::Counts,
    pub given_back: u64,
    pub checked: u64,
    pub unchecked: u64,
    pub lost_home: u64,
    pub made_on_path: u64,
    /// Checks that read the protocols, `sockstat`'s count past the check's own socket.
    pub protocols: u64,
    /// Checks that came to their verdict through the namespace's held files, with no entry.
    pub through_files: u64,
    /// Checks in which a read or an open of those files failed, which looked by entering instead.
    pub files_failed: u64,
    /// Checks that entered because the namespace held no files with the [`FilesBound`] all taken.
    pub files_bounded: u64,
    /// The bound the node runs with.
    pub files_bound: Option<FilesBound>,
    /// The namespaces holding their files now.
    pub files_held: u64,
    /// The checks' time, all together, in nanoseconds.
    pub check_ns: u64,
}

struct Shared {
    pool: Mutex<Pool<OwnedFd>>,
    /// The refill thread waits on it while the pool wants nothing. A take, which may leave the pool
    /// wanting one, and shutdown wake it; nothing else changes what it waits for.
    wake: Condvar,
}

impl Shared {
    fn pool(&self) -> Result<MutexGuard<'_, Pool<OwnedFd>>, OciError> {
        self.pool.lock().map_err(|_| {
            OciError::Broken("the network-namespace pool's lock was poisoned".to_owned())
        })
    }
}

impl Netns {
    /// A pool that holds at most `bound` namespaces, and the thread that starts filling it at once, its kept
    /// namespaces holding their `/proc/net` files under `files`. With a bound of 0 there is no thread, and every
    /// container that makes a network namespace makes it on its create's path, counted, as a rootless node's
    /// containers do (decision 4).
    pub fn start(bound: usize, files: FilesBound) -> Result<Self, Refusal> {
        let shared = Arc::new(Shared {
            pool: Mutex::new(Pool::new(bound)),
            wake: Condvar::new(),
        });
        if bound == 0 {
            return Ok(Self {
                shared,
                refill: None,
                kept: None,
            });
        }
        let filling = Arc::clone(&shared);
        let refill = std::thread::Builder::new()
            .name("hex-netns".to_owned())
            .spawn(move || refill(&filling))
            .map_err(|e| Refusal::os("starting the network-namespace thread", e))?;
        let state = Arc::new(KeptState {
            idle: Mutex::new(Idle::new(netns_reuse::PER_OWNER, netns_reuse::PER_NODE)),
            held: Mutex::new(BTreeMap::new()),
            given_back: AtomicU64::new(0),
            checked: AtomicU64::new(0),
            unchecked: AtomicU64::new(0),
            lost_home: AtomicU64::new(0),
            made_on_path: AtomicU64::new(0),
            protocols: AtomicU64::new(0),
            through_files: AtomicU64::new(0),
            files_failed: AtomicU64::new(0),
            files_bounded: AtomicU64::new(0),
            files_bound: files,
            files_held: Arc::new(AtomicU64::new(0)),
            check_ns: AtomicU64::new(0),
        });
        let (returns, line) = std::sync::mpsc::sync_channel(netns_reuse::PER_NODE);
        let line = Arc::new(Mutex::new(line));
        let mut checkers = Vec::with_capacity(CHECKERS);
        for n in 0..CHECKERS {
            let (checking, taking) = (Arc::clone(&state), Arc::clone(&line));
            let checker = std::thread::Builder::new()
                .name(format!("hex-netns-check-{n}"))
                .spawn(move || check_returns(&checking, &taking))
                .map_err(|e| Refusal::os("starting a network-namespace checker", e))?;
            checkers.push(checker);
        }
        Ok(Self {
            shared,
            refill: Some(refill),
            kept: Some(Keeping {
                state,
                returns: Some(returns),
                checkers,
            }),
        })
    }

    /// The chooser for one create of a box of `owner` (none when `None`): it answers the runtime's take
    /// from the owner's kept namespaces, or keeps the one it gives, and says after which it kept.
    pub fn choice<'a>(&'a self, owner: Option<&'a BoxOwner>) -> NetChoice<'a> {
        NetChoice {
            netns: self,
            owner,
            kept: Mutex::new(None),
        }
    }

    /// Keeps box `id`'s namespace, `ns`, for its owner, from its create on.
    pub fn hold(&self, id: &BoxId, owner: &str, ns: KeptNet) -> Result<(), Refusal> {
        let Some(kept) = &self.kept else {
            return Ok(());
        };
        kept.state
            .held
            .lock()
            .map_err(|_| Refusal::Broken("the kept namespaces' lock was poisoned".to_owned()))?
            .insert(id.clone(), (owner.to_owned(), ns));
        Ok(())
    }

    /// Box `id` is gone, its record too: its namespace, if kept, goes to the checker, which keeps it idle
    /// for the owner's next box if it passes ([`netcheck::check`]) and closes it if not. With the checker's
    /// line full it's closed unchecked, counted.
    pub fn give_back(&self, id: &BoxId) -> Result<(), Refusal> {
        let Some(kept) = &self.kept else {
            return Ok(());
        };
        let held = kept
            .state
            .held
            .lock()
            .map_err(|_| Refusal::Broken("the kept namespaces' lock was poisoned".to_owned()))?
            .remove(id);
        let (Some((owner, ns)), Some(returns)) = (held, &kept.returns) else {
            return Ok(());
        };
        // Counted before it's sent, so a reader never sees it ruled on and not given back.
        kept.state.given_back.fetch_add(1, Ordering::AcqRel);
        match returns.try_send((owner, ns)) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(gone) | TrySendError::Disconnected(gone)) => {
                kept.state.unchecked.fetch_add(1, Ordering::AcqRel);
                drop(gone);
                Ok(())
            }
        }
    }

    /// What the kept namespaces have done; `None` on a node that keeps none.
    pub fn reuse_counts(&self) -> Result<Option<ReuseCounts>, Refusal> {
        let Some(kept) = &self.kept else {
            return Ok(None);
        };
        // The rulings first: each is counted after its verdict is in the pool, so the pool read after shows
        // every verdict counted here.
        let checked = kept.state.checked.load(Ordering::Acquire);
        let unchecked = kept.state.unchecked.load(Ordering::Acquire);
        let pool = kept
            .state
            .idle
            .lock()
            .map_err(|_| Refusal::Broken("the kept namespaces' lock was poisoned".to_owned()))?
            .counts();
        Ok(Some(ReuseCounts {
            pool,
            given_back: kept.state.given_back.load(Ordering::Acquire),
            checked,
            unchecked,
            lost_home: kept.state.lost_home.load(Ordering::Acquire),
            made_on_path: kept.state.made_on_path.load(Ordering::Acquire),
            protocols: kept.state.protocols.load(Ordering::Acquire),
            through_files: kept.state.through_files.load(Ordering::Acquire),
            files_failed: kept.state.files_failed.load(Ordering::Acquire),
            files_bounded: kept.state.files_bounded.load(Ordering::Acquire),
            files_bound: Some(kept.state.files_bound),
            files_held: kept.state.files_held.load(Ordering::Acquire),
            check_ns: kept.state.check_ns.load(Ordering::Acquire),
        }))
    }

    /// Closes every idle kept namespace, counted as drained: the node lets them go.
    pub fn release_idle(&self) -> Result<(), Refusal> {
        let Some(kept) = &self.kept else {
            return Ok(());
        };
        let gone = kept
            .state
            .idle
            .lock()
            .map_err(|_| Refusal::Broken("the kept namespaces' lock was poisoned".to_owned()))?
            .drain();
        drop(gone);
        Ok(())
    }

    /// A namespace made on the calling thread, when the pool is dry and the node keeps the box's: made
    /// as the pool makes them ([`net::make_ahead`]), so the node holds it, and counted
    /// ([`ReuseCounts::made_on_path`]).
    fn make_on_path(&self) -> Result<OwnedFd, OciError> {
        let home = net::thread_network()?;
        let ns = net::make_ahead(home.as_fd())?;
        if let Some(kept) = &self.kept {
            kept.state.made_on_path.fetch_add(1, Ordering::AcqRel);
        }
        Ok(ns)
    }

    /// What the pool has done. Counted at the take, so a create that fails after it took a namespace,
    /// or found the pool dry, is counted all the same.
    pub fn counts(&self) -> Result<Counts, Refusal> {
        Ok(self.shared.pool().map_err(Refusal::from)?.counts())
    }
}

impl Networks for Netns {
    fn take(&self) -> Result<Option<OwnedFd>, OciError> {
        let taken = self
            .shared
            .pool()?
            .take()
            .map_err(|e| OciError::Broken(e.to_string()))?;
        self.shared.wake.notify_one();
        Ok(match taken {
            Take::Pooled(ns) => Some(ns),
            Take::MakeOnPath => None,
        })
    }

    fn on_path(&self) -> Result<(), OciError> {
        self.shared
            .pool()?
            .on_path()
            .map_err(|e| OciError::Broken(e.to_string()))
    }
}

impl Drop for Netns {
    fn drop(&mut self) {
        if let Ok(mut pool) = self.shared.pool.lock() {
            // Closed as they go: namespaces never used.
            drop(pool.shut_down());
        }
        self.shared.wake.notify_all();
        if let Some(refill) = self.refill.take() {
            let _ = refill.join();
        }
        if let Some(mut kept) = self.kept.take() {
            // The checker ends once its line is closed and drained.
            drop(kept.returns.take());
            for checker in kept.checkers.drain(..) {
                let _ = checker.join();
            }
            if let Ok(mut idle) = kept.state.idle.lock() {
                drop(idle.drain());
            }
        }
    }
}

/// One create's chooser ([`Netns::choice`]): the runtime's take answered from the box's owner's kept
/// namespaces when its spec lets it share, and the namespace it was given kept for the owner.
pub struct NetChoice<'a> {
    netns: &'a Netns,
    owner: Option<&'a BoxOwner>,
    kept: Mutex<Option<KeptNet>>,
}

impl NetChoice<'_> {
    /// The namespace this create was given and the node keeps for its owner, if it was.
    pub fn kept(self) -> Option<KeptNet> {
        self.kept.into_inner().ok().flatten()
    }

    fn whose(&self, eligible: Result<(), Why>) -> Whose<'_> {
        Whose {
            owner: self.owner.map(|o| o.name.as_str()),
            reuse: self.owner.is_some_and(|o| o.reuse),
            eligible,
        }
    }

    fn idle(&self) -> Result<Option<MutexGuard<'_, Idle<KeptNet>>>, OciError> {
        match &self.netns.kept {
            None => Ok(None),
            Some(kept) => {
                kept.state.idle.lock().map(Some).map_err(|_| {
                    OciError::Broken("the kept namespaces' lock was poisoned".to_owned())
                })
            }
        }
    }
}

impl Networks for NetChoice<'_> {
    fn take(&self) -> Result<Option<OwnedFd>, OciError> {
        self.netns.take()
    }

    fn on_path(&self) -> Result<(), OciError> {
        if let Some(mut idle) = self.idle()? {
            // Counted, never kept: a namespace in a user namespace of the box's own.
            let _ = idle.take(self.whose(Err(Why::OwnUserNamespace)));
        }
        self.netns.on_path()
    }

    fn take_for(&self, may_share: Result<(), Why>) -> Result<Option<OwnedFd>, OciError> {
        let taken = match self.idle()? {
            None => return self.netns.take(),
            Some(mut idle) => {
                let taken = idle.take(self.whose(may_share));
                if matches!(taken, Kept::Idle(_)) {
                    // Checked as its box went (`check_returns`), and nothing ran in it since.
                    idle.hand_on();
                }
                taken
            }
        };
        let (ns, socket, files, slot) = match taken {
            Kept::Own(_) => return self.netns.take(),
            Kept::Idle(kept) => (kept.ns, kept.socket, kept.files, kept.slot),
            Kept::Fresh => match self.netns.take()? {
                Some(ns) => (ns, None, None, None),
                None => (self.netns.make_on_path()?, None, None, None),
            },
        };
        let keep = KeptNet {
            ns: ns
                .try_clone()
                .map_err(|e| OciError::os("keeping a box's network namespace", e))?,
            socket,
            files,
            slot,
        };
        *self.kept.lock().map_err(|_| {
            OciError::Broken("a create's kept namespace's lock was poisoned".to_owned())
        })? = Some(keep);
        Ok(Some(ns))
    }
}

/// A checker, one of [`CHECKERS`]: checks each namespace it takes off the line against a fresh namespace's
/// dumps ([`Fresh::read`]), keeping it idle for its owner if it passes and closing it, counted by why, if not.
/// It ends once the line is closed. Until it has read a fresh namespace's dumps, which it tries again at each
/// namespace it takes, it refuses each as unreadable, so a node whose kernel it can't read keeps none and says
/// so in its counts. A check after which it couldn't go back to its own namespace refuses the namespace and
/// ends that checker, counted, so no namespace is checked from another's; once every checker has ended, those
/// given back are closed unchecked, counted ([`Netns::give_back`]).
fn check_returns(state: &KeptState, line: &Mutex<Receiver<(String, KeptNet)>>) {
    let home = net::thread_network().ok();
    let proc_net = ProcNet::of_this_thread().ok();
    let mut fresh = None;
    let mut scratch = Scratch::default();
    loop {
        // One checker waits on the line at a time, the others on its lock; each checks what it took apart.
        let next = match line.lock() {
            Ok(line) => line.recv(),
            Err(_) => return,
        };
        let Ok((owner, mut kept)) = next else {
            return;
        };
        if fresh.is_none()
            && let (Some(home), Some(proc_net)) = (&home, &proc_net)
        {
            fresh = Fresh::read(home.as_fd(), proc_net).ok();
        }
        let (verdict, lost) = match (&home, &proc_net, &fresh) {
            (Some(home), Some(proc_net), Some(fresh)) => {
                let started = std::time::Instant::now();
                // A namespace holding no files takes a place under the bound before its check may open them,
                // and gives it back below unless it holds them after.
                if kept.files.is_none() && kept.slot.is_none() {
                    kept.slot = FilesSlot::take(&state.files_held, state.files_bound.namespaces);
                    if kept.slot.is_none() {
                        state.files_bounded.fetch_add(1, Ordering::AcqRel);
                    }
                }
                let checked = netcheck::check(
                    kept.ns.as_fd(),
                    home.as_fd(),
                    proc_net,
                    fresh,
                    &mut scratch,
                    &mut kept.socket,
                    if kept.slot.is_some() {
                        Some(&mut kept.files)
                    } else {
                        None
                    },
                );
                if kept.files.is_none() {
                    kept.slot = None;
                }
                let took = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                state.check_ns.fetch_add(took, Ordering::AcqRel);
                if checked.protocols {
                    state.protocols.fetch_add(1, Ordering::AcqRel);
                }
                if checked.through_files {
                    state.through_files.fetch_add(1, Ordering::AcqRel);
                }
                if checked.files_failed {
                    state.files_failed.fetch_add(1, Ordering::AcqRel);
                }
                (checked.verdict, !checked.home)
            }
            _ => (Err(Why::Unreadable), false),
        };
        let verdict = if lost {
            state.lost_home.fetch_add(1, Ordering::Relaxed);
            Err(Why::Unreadable)
        } else {
            verdict
        };
        let Ok(mut idle) = state.idle.lock() else {
            state.unchecked.fetch_add(1, Ordering::AcqRel);
            return;
        };
        let closed = match verdict {
            Ok(()) => idle.give(&owner, kept),
            Err(why) => {
                idle.refuse(why);
                vec![kept]
            }
        };
        // Closed with the lock let go.
        drop(idle);
        drop(closed);
        state.checked.fetch_add(1, Ordering::AcqRel);
        if lost {
            return;
        }
    }
}

/// The refill thread: makes a namespace whenever the pool asks for one and hands it in, one at a time,
/// as the pool's count of what it asks for needs, and ends once the pool is shut down. It opens its
/// home namespace first; a thread that can't makes none, each make it is asked for failing, counted.
/// A namespace the pool refuses (past its bound, which one maker never reaches, or after shutdown) is
/// closed here. A poisoned lock ends it, and every create then refuses on the same lock.
fn refill(shared: &Shared) {
    let home = net::thread_network();
    loop {
        let Ok(mut pool) = shared.pool.lock() else {
            return;
        };
        while !pool.is_shut_down() && pool.wanted() == 0 {
            pool = match shared.wake.wait(pool) {
                Ok(pool) => pool,
                Err(_) => return,
            };
        }
        if pool.is_shut_down() {
            return;
        }
        // Made with the lock let go, so a take never waits on a make.
        drop(pool);
        let made = match &home {
            Ok(home) => net::make_ahead(home.as_fd()),
            Err(e) => Err(e.clone()),
        };
        let Ok(mut pool) = shared.pool.lock() else {
            return;
        };
        match made {
            Ok(ns) => {
                let _ = pool.made(ns);
            }
            Err(_) => pool.failed(),
        }
    }
}

#[cfg(test)]
mod tests;
