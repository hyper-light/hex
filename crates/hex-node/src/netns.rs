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
use hex_core::netns::{Counts, Pool, Take};
use hex_core::netns_reuse::{self, Idle, Refused as Why, Take as Kept, Whose};
use hex_runtime::linux::oci::OciError;
use hex_runtime::linux::oci::net;
use hex_runtime::linux::oci::netcheck::{self, Fresh, ProcNet};
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
    returns: Option<SyncSender<(String, OwnedFd)>>,
    checkers: Vec<JoinHandle<()>>,
}

struct KeptState {
    idle: Mutex<Idle<OwnedFd>>,
    /// Each live box's namespace, kept from its create to its delete, with its owner: at most one a box.
    held: Mutex<BTreeMap<BoxId, (String, OwnedFd)>>,
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
    /// A pool that holds at most `bound` namespaces, and the thread that starts filling it at once.
    /// With a bound of 0 there is no thread, and every container that makes a network namespace makes
    /// it on its create's path, counted, as a rootless node's containers do (decision 4).
    pub fn start(bound: usize) -> Result<Self, Refusal> {
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
    pub fn hold(&self, id: &BoxId, owner: &str, ns: OwnedFd) -> Result<(), Refusal> {
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
    /// as the pool makes them ([`net::make_ahead`]), so the node holds it.
    fn make_on_path(&self) -> Result<OwnedFd, OciError> {
        let home = net::thread_network()?;
        net::make_ahead(home.as_fd())
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
    kept: Mutex<Option<OwnedFd>>,
}

impl NetChoice<'_> {
    /// The namespace this create was given and the node keeps for its owner, if it was.
    pub fn kept(self) -> Option<OwnedFd> {
        self.kept.into_inner().ok().flatten()
    }

    fn whose(&self, eligible: Result<(), Why>) -> Whose<'_> {
        Whose {
            owner: self.owner.map(|o| o.name.as_str()),
            reuse: self.owner.is_some_and(|o| o.reuse),
            eligible,
        }
    }

    fn idle(&self) -> Result<Option<MutexGuard<'_, Idle<OwnedFd>>>, OciError> {
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
        let ns = match taken {
            Kept::Own(_) => return self.netns.take(),
            Kept::Idle(ns) => ns,
            Kept::Fresh => match self.netns.take()? {
                Some(ns) => ns,
                None => self.netns.make_on_path()?,
            },
        };
        let keep = ns
            .try_clone()
            .map_err(|e| OciError::os("keeping a box's network namespace", e))?;
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
fn check_returns(state: &KeptState, line: &Mutex<Receiver<(String, OwnedFd)>>) {
    let home = net::thread_network().ok();
    let proc_net = ProcNet::of_this_thread().ok();
    let mut fresh = None;
    loop {
        // One checker waits on the line at a time, the others on its lock; each checks what it took apart.
        let next = match line.lock() {
            Ok(line) => line.recv(),
            Err(_) => return,
        };
        let Ok((owner, ns)) = next else {
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
                let checked = netcheck::check(ns.as_fd(), home.as_fd(), proc_net, fresh);
                let took = u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX);
                state.check_ns.fetch_add(took, Ordering::AcqRel);
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
            Ok(()) => idle.give(&owner, ns),
            Err(why) => {
                idle.refuse(why);
                vec![ns]
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
