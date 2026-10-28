//! The node agent as its containers' parent (decision 2 of docs/design/runtime-architecture.md):
//! the agent is a child subreaper, so an `oci` box's init, orphaned when the shim exits after
//! forking it, becomes the agent's child (PR_SET_CHILD_SUBREAPER(2const)). The agent holds each
//! init's pidfd, which turns readable when the init ends (pidfd_open(2)); a thread waits on them all
//! through one epoll set, reaps each ended init with `waitid(P_PIDFD)` and records how it ended in
//! the box's record. A box's other processes that are orphaned (a daemon a `process` box's program
//! forks, a container's process without a pid namespace) become the agent's children too; a sweep
//! each [`SWEEP_PERIOD`] reaps those that have ended, and only those: a zombie in one of the node's
//! box cgroups that no [`Proc`] handle and no watched init owns.

use std::collections::{BTreeMap, BTreeSet};
use std::os::fd::{AsFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, Weak};
use std::time::{Duration, Instant};

use hex_api::{BoxId, Exit, Proc, Refusal};
use rustix::buffer::spare_capacity;
use rustix::event::epoll::{self, CreateFlags, EventData, EventFlags};
use rustix::event::{PollFd, PollFlags, Timespec};
use rustix::io::Errno;
use rustix::process::{Pid, PidfdFlags, WaitId, WaitIdOptions, WaitIdStatus};

use crate::store::Store;

/// Most inits whose pidfds one node agent holds: one fd per live `oci` box. Past it `create` refuses
/// with [`Refusal::Exhausted`] before making anything. It sits under the fd limit systemd gives a
/// service (`DefaultLimitNOFILE=1024:524288`, systemd-system.conf(5)) once the agent raises its soft
/// limit to the hard one, with room left for the agent's own files; and above the density the
/// runtime design targets on one node (10,560 idle containers per GiB held). An operator sets
/// another with [`crate::Node::with_init_budget`].
pub const INIT_BUDGET: usize = 65_536;

/// Most exec'd programs the agent watches for one box at once (to reap on its delete), past which exec
/// is refused ([`Refusal::Exhausted`]), so the per-box tracking is bounded (CLAUDE.md §2). A program
/// the caller reaps is forgotten at once, so this bounds concurrent unreaped execs, not a box's total;
/// it sits well above a box's steps, which run one at a time.
pub const PROGRAM_BUDGET: usize = 1_024;

/// How often the supervisor sweeps for orphaned zombies, and so the longest one waits to be reaped.
/// A stated bound on a zombie's life, not a tuning: a box would have to orphan and end
/// `pid_max / SWEEP_PERIOD` processes a second to fill the pid space with them (pid_max is 4,194,304
/// on a 64-bit kernel, proc_sys_kernel(5)).
pub const SWEEP_PERIOD: Duration = Duration::from_secs(1);

/// Most children one sweep looks at; more wait for the next sweep. Bounds a sweep's time (§2).
const SWEEP_CHILDREN: usize = 4_096;

/// Most threads of the agent whose children a sweep reads (`/proc/self/task/*/children`).
const SWEEP_TASKS: usize = 4_096;

/// Most epoll events one wake takes; more come on the next wait.
const EVENTS: usize = 256;

/// Held to read while a process is spawned (or a shim reaped) until its pid is in [`OWNED`]; held to
/// write by a sweep, which so never takes a child before it is owned. Process-wide, as the subreaper
/// attribute is: every node in the process spawns under it and every node's sweep honours it.
static SPAWNING: RwLock<()> = RwLock::new(());

/// The pids some node in this process still reaps itself: programs whose [`Proc`] may wait on them,
/// and watched inits. A sweep leaves them alone. Bounded by the live programs and inits, each held
/// to its own bound ([`INIT_BUDGET`], a box's process bound).
static OWNED: Mutex<BTreeSet<u32>> = Mutex::new(BTreeSet::new());

fn owned() -> Result<MutexGuard<'static, BTreeSet<u32>>, Refusal> {
    OWNED.lock().map_err(poisoned)
}

/// One `oci` box's init, watched through its pidfd.
struct Init {
    pid: i32,
    /// `None` once reaped.
    pidfd: Option<OwnedFd>,
    /// How it ended, kept until the box's record holds it.
    exit: Option<Exit>,
}

#[derive(Default)]
struct Inits {
    by_box: BTreeMap<BoxId, Init>,
    /// The epoll token each watched init's pidfd carries, back to its box.
    by_token: BTreeMap<u64, BoxId>,
    next_token: u64,
    /// Creates under way that hold a place in the budget.
    reserved: usize,
    budget: usize,
}

/// The node's supervision of its boxes' processes.
pub struct Supervisor {
    epoll: Arc<OwnedFd>,
    inits: Mutex<Inits>,
    /// The node's box root as `/proc/<pid>/cgroup` names it: a zombie under it is the node's.
    box_root: String,
    store: Store,
    /// Why the last pass of the supervising thread failed, if it did; cleared by a clean pass.
    fault: Mutex<Option<String>>,
    /// Exec'd programs by box, each with a pidfd the agent reaps on the box's delete. An `oci` box's
    /// init blocks in the kernel's pid-namespace teardown until the programs in its pid namespace are
    /// reaped (decision 3); the caller's [`Proc`] usually reaps its program, but a delete beats it, so
    /// the agent reaps what is left and records each exit for the caller to read.
    programs: Mutex<BTreeMap<BoxId, Vec<Program>>>,
}

/// An exec'd program the agent may reap on its box's delete: the agent's own duplicate of its pidfd,
/// and the slot its exit is recorded in for the caller's [`Proc`] once the agent has reaped it.
struct Program {
    pid: u32,
    pidfd: OwnedFd,
    exit: Arc<Mutex<Option<Exit>>>,
}

fn poisoned<T>(_: std::sync::PoisonError<T>) -> Refusal {
    Refusal::Broken("a supervisor lock was poisoned".to_owned())
}

impl Supervisor {
    /// Makes this process a child subreaper and starts the supervising thread. `box_root` is the
    /// node's box root under the cgroup root; records of reaped inits go to `store`.
    pub fn start(box_root: &Path, store: Store) -> Result<Arc<Self>, Refusal> {
        rustix::process::set_child_subreaper(Some(rustix::process::getpid()))
            .map_err(|e| Refusal::os("making the node agent a child subreaper", e))?;
        let rel = box_root
            .strip_prefix(hex_runtime::linux::cgroup::CGROUP_ROOT)
            .map_err(|_| Refusal::Invalid {
                what: "box root",
                why: format!("{} isn't under the cgroup root", box_root.display()),
            })?;
        let epoll = Arc::new(
            epoll::create(CreateFlags::CLOEXEC)
                .map_err(|e| Refusal::os("making the supervisor's epoll set", e))?,
        );
        let supervisor = Arc::new(Self {
            epoll: Arc::clone(&epoll),
            inits: Mutex::new(Inits {
                budget: INIT_BUDGET,
                ..Inits::default()
            }),
            box_root: Path::new("/").join(rel).to_string_lossy().into_owned(),
            store,
            fault: Mutex::new(None),
            programs: Mutex::new(BTreeMap::new()),
        });
        let weak = Arc::downgrade(&supervisor);
        std::thread::Builder::new()
            .name("hex-supervisor".to_owned())
            .spawn(move || supervise(&epoll, &weak))
            .map_err(|e| Refusal::os("starting the supervisor thread", e))?;
        Ok(supervisor)
    }

    fn inits(&self) -> Result<MutexGuard<'_, Inits>, Refusal> {
        self.inits.lock().map_err(poisoned)
    }

    /// Sets the init budget.
    pub fn set_budget(&self, budget: usize) -> Result<(), Refusal> {
        self.inits()?.budget = budget;
        Ok(())
    }

    /// Takes a place in the init budget for a create, or refuses with [`Refusal::Exhausted`] naming
    /// the budget when none is left. The place is given back when the [`Slot`] is dropped, or turned
    /// into the init's by [`Supervisor::watch`].
    pub fn reserve(self: &Arc<Self>) -> Result<Slot, Refusal> {
        let mut inits = self.inits()?;
        if let Some(fault) = self.fault.lock().map_err(poisoned)?.as_ref() {
            return Err(Refusal::Broken(format!("the node's supervisor: {fault}")));
        }
        let held = inits.by_box.len().checked_add(inits.reserved);
        if held.is_none_or(|h| h >= inits.budget) {
            return Err(Refusal::Exhausted {
                what: "container init pidfds",
                bound: inits.budget,
            });
        }
        inits.reserved = inits.reserved.saturating_add(1);
        Ok(Slot {
            supervisor: Arc::clone(self),
            held: true,
        })
    }

    /// Held while a process is spawned and until it is owned, so a sweep never reaps it first.
    pub fn spawning(&self) -> Result<RwLockReadGuard<'static, ()>, Refusal> {
        SPAWNING.read().map_err(poisoned)
    }

    /// Watches box `id`'s init, `pid`, which create recorded with `start_ticks`: opens its pidfd,
    /// then checks it is the agent's child with that start time, so a later process given its pid
    /// is never taken for it (proc_pid_stat(5) fields 4 and 22). Uses `slot`'s place in the budget.
    pub fn watch(
        &self,
        mut slot: Slot,
        id: &BoxId,
        pid: i32,
        start_ticks: u64,
    ) -> Result<(), Refusal> {
        let raw = Pid::from_raw(pid).ok_or_else(|| Refusal::Invalid {
            what: "pid",
            why: format!("{pid} isn't a process id"),
        })?;
        let pidfd = rustix::process::pidfd_open(raw, PidfdFlags::empty())
            .map_err(|e| Refusal::os("pidfd_open of a container's init", e))?;
        let stat = Stat::of(pid)
            .ok_or_else(|| Refusal::Broken(format!("box {id}'s init {pid} has no /proc stat")))?;
        let me = rustix::process::getpid().as_raw_nonzero().get();
        if stat.ppid != me || stat.start_ticks != start_ticks {
            return Err(Refusal::Broken(format!(
                "box {id}'s init {pid} isn't the node agent's child started at {start_ticks} \
                 (parent {}, started at {})",
                stat.ppid, stat.start_ticks
            )));
        }
        let mut inits = self.inits()?;
        let token = inits.next_token;
        inits.next_token = token
            .checked_add(1)
            .ok_or_else(|| Refusal::Broken("the supervisor's tokens ran out".to_owned()))?;
        epoll::add(
            self.epoll.as_fd(),
            pidfd.as_fd(),
            EventData::new_u64(token),
            EventFlags::IN,
        )
        .map_err(|e| Refusal::os("watching a container's init", e))?;
        if let Ok(pid) = u32::try_from(pid) {
            owned()?.insert(pid);
        }
        inits.by_token.insert(token, id.clone());
        inits.by_box.insert(
            id.clone(),
            Init {
                pid,
                pidfd: Some(pidfd),
                exit: None,
            },
        );
        inits.reserved = inits.reserved.saturating_sub(1);
        slot.held = false;
        Ok(())
    }

    /// How box `id`'s init ended, if it is watched and has: reaped now if it has ended and the
    /// supervising thread hasn't come to it yet, so inspect never reads a stopped box without its
    /// exit. `None` when it runs or isn't watched.
    pub fn exit(&self, id: &BoxId) -> Result<Option<Exit>, Refusal> {
        let mut inits = self.inits()?;
        self.collect(&mut inits, id)
    }

    /// Stops watching box `id`'s init, which the box's delete is about to end; returns its pidfd, if
    /// it hasn't been reaped, for the delete to reap it once the box is empty. After this nothing
    /// writes the box's record on the init's behalf.
    pub fn forget(&self, id: &BoxId) -> Result<Option<Forgotten>, Refusal> {
        let mut inits = self.inits()?;
        inits.by_token.retain(|_, b| b != id);
        let Some(init) = inits.by_box.remove(id) else {
            return Ok(None);
        };
        let pidfd = init.pidfd;
        if pidfd.is_none() {
            disown(init.pid)?;
        }
        if let Some(fd) = &pidfd {
            // Out of the epoll set, so its end doesn't wake the thread until the delete reaps it.
            epoll::delete(self.epoll.as_fd(), fd.as_fd())
                .map_err(|e| Refusal::os("unwatching a deleted box's init", e))?;
        }
        // Still owned: the delete reaps it ([`reap`]), which disowns it.
        Ok(pidfd.map(|pidfd| Forgotten {
            pid: init.pid,
            pidfd,
        }))
    }

    /// A [`Proc`] whose pid the sweep leaves to it until it has reaped it or is dropped.
    pub fn own(&self, proc: Box<dyn Proc>) -> Result<Box<dyn Proc>, Refusal> {
        let pid = proc.pid();
        owned()?.insert(pid);
        Ok(Box::new(Owned {
            proc,
            pid,
            released: false,
        }))
    }

    /// Owns an exec'd program of box `id` that the agent may have to reap on the box's delete: `proc`
    /// reaps it for the caller, and `pidfd` (the agent's own duplicate) lets the agent reap it instead
    /// if a delete beats the caller. The container's init blocks in the kernel until the programs in its
    /// pid namespace are reaped, so a delete that didn't reap them would never empty the box (decision
    /// 3). Refused past [`PROGRAM_BUDGET`] watched for the box at once.
    pub fn own_program(
        self: &Arc<Self>,
        id: &BoxId,
        proc: Box<dyn Proc>,
        pidfd: OwnedFd,
    ) -> Result<Box<dyn Proc>, Refusal> {
        let pid = proc.pid();
        let exit = Arc::new(Mutex::new(None));
        {
            let mut programs = self.programs.lock().map_err(poisoned)?;
            let watched = programs.entry(id.clone()).or_default();
            if watched.len() >= PROGRAM_BUDGET {
                return Err(Refusal::Exhausted {
                    what: "exec'd programs watched for one box",
                    bound: PROGRAM_BUDGET,
                });
            }
            watched.push(Program {
                pid,
                pidfd,
                exit: Arc::clone(&exit),
            });
        }
        owned()?.insert(pid);
        Ok(Box::new(ProgramProc {
            supervisor: Arc::clone(self),
            id: id.clone(),
            proc,
            pid,
            exit,
            done: false,
        }))
    }

    /// Reaps every exec'd program still watched for box `id`, recording each exit for its caller's
    /// [`Proc`] to read. The box's delete calls this after `cgroup.kill`, so the programs — killed, in
    /// the container's pid namespace — are reaped, letting the init finish exiting and the box empty.
    pub fn reap_programs(&self, id: &BoxId, patience: Duration) -> Result<(), Refusal> {
        let watched = self
            .programs
            .lock()
            .map_err(poisoned)?
            .remove(id)
            .unwrap_or_default();
        let mut owned = owned()?;
        let deadline = Instant::now().checked_add(patience);
        for program in watched {
            // Wait (bounded) for the cgroup.kill'd program to end, then reap it, so the container's init
            // can finish its pid-namespace teardown. Bounded so a process wedged uninterruptibly doesn't
            // hang the delete; wait_empty then reports the box still populated, as it does for any wedge.
            let left = deadline
                .map(|d| d.saturating_duration_since(Instant::now()))
                .unwrap_or(patience);
            let ts = Timespec {
                tv_sec: i64::try_from(left.as_secs()).unwrap_or(i64::MAX),
                tv_nsec: i64::from(left.subsec_nanos()),
            };
            let mut fds = [PollFd::new(&program.pidfd, PollFlags::IN)];
            match rustix::event::poll(&mut fds, Some(&ts)) {
                Ok(_) | Err(Errno::INTR) => {}
                Err(e) => return Err(Refusal::os("waiting on a deleted box's program", e)),
            }
            let exit = match rustix::process::waitid(
                WaitId::PidFd(program.pidfd.as_fd()),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG,
            ) {
                Ok(Some(status)) => Some(exit_of(&status)),
                Ok(None) | Err(Errno::CHILD) => None,
                Err(e) => return Err(Refusal::os("reaping a deleted box's program", e)),
            };
            if let Some(exit) = exit
                && let Ok(mut slot) = program.exit.lock()
            {
                *slot = Some(exit);
            }
            owned.remove(&program.pid);
        }
        Ok(())
    }

    /// Stops watching a program the caller has reaped or let go, freeing the agent's pidfd for it, so
    /// the box's delete no longer looks for it.
    fn forget_program(&self, id: &BoxId, pid: u32) {
        if let Ok(mut programs) = self.programs.lock()
            && let Some(watched) = programs.get_mut(id)
        {
            watched.retain(|p| p.pid != pid);
            if watched.is_empty() {
                programs.remove(id);
            }
        }
    }

    /// Reaps box `id`'s init if it has ended, and records how in its record. Holding `inits`, so a
    /// delete's [`Supervisor::forget`] waits for the record write and nothing writes it after.
    fn collect(&self, inits: &mut Inits, id: &BoxId) -> Result<Option<Exit>, Refusal> {
        let Some(init) = inits.by_box.get_mut(id) else {
            return Ok(None);
        };
        if let Some(pidfd) = &init.pidfd {
            match rustix::process::waitid(
                WaitId::PidFd(pidfd.as_fd()),
                WaitIdOptions::EXITED | WaitIdOptions::NOHANG,
            ) {
                Ok(None) => return Ok(None),
                Ok(Some(status)) => init.exit = Some(exit_of(&status)),
                // Not the agent's child any more: something else in this process reaped it.
                Err(Errno::CHILD) => init.exit = Some(Exit::Unknown),
                Err(e) => return Err(Refusal::os("reaping a container's init", e)),
            }
            // Closing the pidfd takes it out of the epoll set.
            init.pidfd = None;
            if let Ok(pid) = u32::try_from(init.pid) {
                owned()?.remove(&pid);
            }
        }
        let exit = init.exit;
        // Kept in memory until the record holds it; a failed write is tried again on the next
        // inspect or sweep, never dropped (§3). A box with no readable record has none to hold it:
        // inspect refuses such a box as broken, and recovery removes it. One change of the record, so
        // a count the socket writes at the same moment isn't lost.
        self.store.update(id, |record| record.exit = exit)?;
        inits.by_box.remove(id);
        inits.by_token.retain(|_, b| b != id);
        Ok(exit)
    }

    /// The supervising thread's pass over the inits whose pidfds turned readable.
    fn ready(&self, tokens: &[u64]) -> Result<(), Refusal> {
        let mut inits = self.inits()?;
        let mut first_error = None;
        for token in tokens {
            let Some(id) = inits.by_token.get(token).cloned() else {
                continue;
            };
            if let Err(e) = self.collect(&mut inits, &id) {
                first_error.get_or_insert(e);
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Reaps the box processes orphaned to the agent that have ended, and records exits a failed
    /// write left in memory. Under the spawning lock, so no child is taken before it is registered.
    fn sweep(&self) -> Result<usize, Refusal> {
        let _no_spawns = SPAWNING.write().map_err(poisoned)?;
        let mut inits = self.inits()?;
        let unrecorded: Vec<BoxId> = inits
            .by_box
            .iter()
            .filter(|(_, init)| init.exit.is_some())
            .map(|(id, _)| id.clone())
            .collect();
        for id in &unrecorded {
            self.collect(&mut inits, id)?;
        }
        drop(inits);
        let owned = owned()?;
        let mut reaped = 0usize;
        for pid in children()? {
            if u32::try_from(pid).is_ok_and(|p| owned.contains(&p)) {
                continue;
            }
            if self.reap_orphan(pid)? {
                reaped = reaped.saturating_add(1);
            }
        }
        Ok(reaped)
    }

    /// Reaps `pid` if it is an ended process in one of the node's boxes. Its pidfd is opened first,
    /// so the checks and the reap are of the same process.
    fn reap_orphan(&self, pid: i32) -> Result<bool, Refusal> {
        let Some(raw) = Pid::from_raw(pid) else {
            return Ok(false);
        };
        let Ok(pidfd) = rustix::process::pidfd_open(raw, PidfdFlags::empty()) else {
            // Gone already (reaped by its owner since the children were read).
            return Ok(false);
        };
        let ended = Stat::of(pid).is_some_and(|s| s.zombie);
        if !ended || !self.in_boxes(pid) {
            return Ok(false);
        }
        match rustix::process::waitid(
            WaitId::PidFd(pidfd.as_fd()),
            WaitIdOptions::EXITED | WaitIdOptions::NOHANG,
        ) {
            Ok(status) => Ok(status.is_some()),
            Err(Errno::CHILD) => Ok(false),
            Err(e) => Err(Refusal::os("reaping an orphaned box process", e)),
        }
    }

    /// Whether process `pid` is (or, ended, was) in a cgroup under the node's box root. A zombie's
    /// `/proc/<pid>/cgroup` still names the cgroup it ended in.
    fn in_boxes(&self, pid: i32) -> bool {
        let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/cgroup")) else {
            return false;
        };
        text.lines()
            .filter_map(|l| l.strip_prefix("0::"))
            .any(|path| {
                path.strip_prefix(self.box_root.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
            })
    }

    fn note(&self, outcome: Result<(), Refusal>) {
        if let Ok(mut fault) = self.fault.lock() {
            *fault = outcome.err().map(|e| e.to_string());
        }
    }
}

/// The supervising thread: waits on the inits' pidfds, collecting each that ends, and sweeps each
/// [`SWEEP_PERIOD`]; ends once the node is dropped.
fn supervise(epoll: &OwnedFd, supervisor: &Weak<Supervisor>) {
    let period = Timespec {
        tv_sec: i64::try_from(SWEEP_PERIOD.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(SWEEP_PERIOD.subsec_nanos()),
    };
    let mut events = Vec::with_capacity(EVENTS);
    let mut swept = Instant::now();
    loop {
        events.clear();
        let waited = match epoll::wait(epoll.as_fd(), spare_capacity(&mut events), Some(&period)) {
            Ok(_) | Err(Errno::INTR) => Ok(()),
            Err(e) => Err(Refusal::os("waiting on the inits' pidfds", e)),
        };
        let Some(supervisor) = supervisor.upgrade() else {
            return;
        };
        if let Err(e) = waited {
            // epoll_wait fails only on a bad set or buffer (epoll_wait(2)): the supervisor can't go
            // on. The fault stays, so every later create is refused with it (§3).
            supervisor.note(Err(e));
            return;
        }
        let tokens: Vec<u64> = events.iter().map(|e| e.data.u64()).collect();
        let mut outcome = supervisor.ready(&tokens);
        if swept.elapsed() >= SWEEP_PERIOD {
            swept = Instant::now();
            outcome = outcome.and_then(|()| supervisor.sweep().map(|_| ()));
        }
        // A fault is kept for create to refuse on until a pass is clean (§3).
        supervisor.note(outcome);
    }
}

/// A place in the init budget, given back on drop unless [`Supervisor::watch`] took it.
pub struct Slot {
    supervisor: Arc<Supervisor>,
    held: bool,
}

impl Drop for Slot {
    fn drop(&mut self) {
        if self.held
            && let Ok(mut inits) = self.supervisor.inits.lock()
        {
            inits.reserved = inits.reserved.saturating_sub(1);
        }
    }
}

/// A program the node started, its pid kept from the sweep until it is reaped or let go.
struct Owned {
    proc: Box<dyn Proc>,
    pid: u32,
    released: bool,
}

impl Owned {
    fn release(&mut self) {
        if !self.released
            && let Ok(mut owned) = OWNED.lock()
        {
            owned.remove(&self.pid);
            self.released = true;
        }
    }

    fn seen(&mut self, exit: Option<Exit>) -> Option<Exit> {
        if exit.is_some() {
            self.release();
        }
        exit
    }
}

impl Proc for Owned {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        let exit = self.proc.try_wait()?;
        Ok(self.seen(exit))
    }

    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal> {
        let exit = self.proc.wait_for(timeout)?;
        Ok(self.seen(exit))
    }
}

impl Drop for Owned {
    // Let go unreaped, an ended program is the sweep's to reap.
    fn drop(&mut self) {
        self.release();
    }
}

/// A [`Proc`] for an exec'd program the agent watches for its box's delete. It reaps the program itself
/// when the caller waits, unless a delete reaped it first — then it reads the exit the agent recorded.
/// Dropped unreaped, it leaves the program to the sweep (it is the box's) and stops being watched.
struct ProgramProc {
    supervisor: Arc<Supervisor>,
    id: BoxId,
    proc: Box<dyn Proc>,
    pid: u32,
    exit: Arc<Mutex<Option<Exit>>>,
    done: bool,
}

impl ProgramProc {
    /// The exit a delete recorded, if the agent reaped the program.
    fn recorded(&self) -> Option<Exit> {
        self.exit.lock().ok().and_then(|slot| *slot)
    }

    /// Stops watching and leaves [`OWNED`], once the program is reaped or let go.
    fn done(&mut self, exit: Exit) -> Exit {
        if !self.done {
            self.done = true;
            self.supervisor.forget_program(&self.id, self.pid);
            if let Ok(mut owned) = OWNED.lock() {
                owned.remove(&self.pid);
            }
        }
        exit
    }
}

impl Proc for ProgramProc {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn try_wait(&mut self) -> Result<Option<Exit>, Refusal> {
        if let Some(exit) = self.recorded() {
            return Ok(Some(self.done(exit)));
        }
        match self.proc.try_wait() {
            Ok(Some(exit)) => Ok(Some(self.done(exit))),
            Ok(None) => Ok(None),
            // A delete may have reaped it, so the caller's own wait can't; read what the agent recorded.
            Err(e) => match self.recorded() {
                Some(exit) => Ok(Some(self.done(exit))),
                None => Err(e),
            },
        }
    }

    fn wait_for(&mut self, timeout: Duration) -> Result<Option<Exit>, Refusal> {
        if let Some(exit) = self.recorded() {
            return Ok(Some(self.done(exit)));
        }
        match self.proc.wait_for(timeout) {
            Ok(Some(exit)) => Ok(Some(self.done(exit))),
            Ok(None) => match self.recorded() {
                Some(exit) => Ok(Some(self.done(exit))),
                None => Ok(None),
            },
            Err(e) => match self.recorded() {
                Some(exit) => Ok(Some(self.done(exit))),
                None => Err(e),
            },
        }
    }
}

impl Drop for ProgramProc {
    fn drop(&mut self) {
        if !self.done {
            // Let go unreaped: leave it to the sweep (it is the box's) and stop watching it.
            self.supervisor.forget_program(&self.id, self.pid);
            if let Ok(mut owned) = OWNED.lock() {
                owned.remove(&self.pid);
            }
        }
    }
}

/// How a reaped process ended.
fn exit_of(status: &WaitIdStatus) -> Exit {
    match (status.exit_status(), status.terminating_signal()) {
        (_, Some(sig)) => Exit::Signal(sig),
        (Some(code), None) => Exit::Code(code),
        (None, None) => Exit::Unknown,
    }
}

/// A deleted box's init, unwatched but still owned, for the delete to reap.
pub struct Forgotten {
    pid: i32,
    pidfd: OwnedFd,
}

/// Reaps a deleted box's init, which has ended since its box is empty, and disowns it.
pub fn reap(init: &Forgotten) -> Result<(), Refusal> {
    let reaped = match rustix::process::waitid(
        WaitId::PidFd(init.pidfd.as_fd()),
        WaitIdOptions::EXITED | WaitIdOptions::NOHANG,
    ) {
        Ok(_) | Err(Errno::CHILD) => Ok(()),
        Err(e) => Err(Refusal::os("reaping a deleted box's init", e)),
    };
    disown(init.pid)?;
    reaped
}

/// Leaves `pid` to the sweep.
fn disown(pid: i32) -> Result<(), Refusal> {
    if let Ok(pid) = u32::try_from(pid) {
        owned()?.remove(&pid);
    }
    Ok(())
}

/// The agent's children, from each of its threads' `children` file (proc_pid_task(5)), at most
/// [`SWEEP_CHILDREN`] of them.
fn children() -> Result<Vec<i32>, Refusal> {
    let tasks = std::fs::read_dir("/proc/self/task")
        .map_err(|e| Refusal::os("listing the node agent's threads", e))?;
    let mut out = Vec::new();
    for task in tasks.take(SWEEP_TASKS) {
        let task = task.map_err(|e| Refusal::os("listing the node agent's threads", e))?;
        let path: PathBuf = task.path().join("children");
        // A thread that has ended since the listing has no file; it has no children either.
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        for pid in text.split_whitespace().filter_map(|p| p.parse().ok()) {
            if out.len() >= SWEEP_CHILDREN {
                return Ok(out);
            }
            out.push(pid);
        }
    }
    Ok(out)
}

/// The fields of `/proc/<pid>/stat` the supervisor reads (proc_pid_stat(5)).
#[derive(Debug, PartialEq, Eq)]
struct Stat {
    /// State `Z`: ended, waiting to be reaped.
    zombie: bool,
    /// Field 4.
    ppid: i32,
    /// Field 22, clock ticks after boot.
    start_ticks: u64,
}

impl Stat {
    fn of(pid: i32) -> Option<Self> {
        Self::parse(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
    }

    /// The fields after the command name, which is parenthesized and may itself hold spaces or
    /// parentheses: the state (field 3), the parent (4), then eighteen on, the start time (22).
    fn parse(stat: &str) -> Option<Self> {
        let (_, rest) = stat.rsplit_once(')')?;
        let mut fields = rest.split_whitespace();
        let state = fields.next()?;
        let ppid = fields.next()?.parse().ok()?;
        let start_ticks = fields.nth(17)?.parse().ok()?;
        Some(Self {
            zombie: state == "Z",
            ppid,
            start_ticks,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Decision 2: the supervisor reads a process's state, parent and start time from its stat line,
    /// a command name with spaces and parentheses included; a truncated or malformed line is no stat.
    #[test]
    fn a_stat_line_gives_state_parent_and_start_time() {
        let line = "4242 (a (b) c) Z 17 4242 4242 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 987654 0 0";
        assert_eq!(
            Stat::parse(line),
            Some(Stat {
                zombie: true,
                ppid: 17,
                start_ticks: 987_654
            })
        );
        let running = line.replace(") Z ", ") S ");
        assert!(!Stat::parse(&running).unwrap().zombie);
        for bad in [
            "",
            "4242 (x",
            "4242 (x) S",
            "4242 (x) S notapid 1 1",
            "4242 (x) S 1 2 3",
        ] {
            assert_eq!(Stat::parse(bad), None, "{bad:?}");
        }
    }

    /// This process's own stat line parses, its parent is its parent, and its start time is the one
    /// the oci state reads (the same field 22 create records).
    #[test]
    fn this_processs_stat_agrees_with_the_oci_start_time() {
        let me = i32::try_from(std::process::id()).unwrap();
        let stat = Stat::of(me).unwrap();
        assert!(!stat.zombie);
        assert_eq!(
            Some(stat.start_ticks),
            hex_runtime::linux::oci::state::start_time(me)
        );
        assert_eq!(
            stat.ppid,
            rustix::process::getppid().unwrap().as_raw_nonzero().get()
        );
    }
}
