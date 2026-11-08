//! The `process` class on macOS (docs/design/runtime.md, decision "each OS runs its own boxes
//! natively"): a box is a process group Hex spawns (posix_spawn, a new process group), at background QoS
//! when its declared weight is under the default and unclamped otherwise. macOS has no public cap on a process tree's memory or
//! its process count (docs/research/runtime/native-platforms.md §3), so a watcher thread holds the box's
//! bounds by Hex's own rule ([`hex_core::watch`]): it lists the box's members (`proc_listpids` of its
//! process group), reads each footprint (`proc_pid_rusage`), and ends the box (killpg, then the members'
//! own waiters reap) once it passes a bound. The rule is the pure core; this shell does the listing, the
//! reading, the waiting and the killing, from a timeshare thread on `NOTE_CRITICAL` kqueue
//! timers so a box filling every core can't hold its watcher off (RESULT.md).
//!
//! Each member Hex spawns is also armed with a fatal per-process memory cap at the box's whole bound
//! (`posix_spawnattr_setjetsam_ext`, rootless; `sys`, docs/research/runtime/macos-spawn-jetsam-cap.md):
//! the kernel SIGKILLs it at the bound with no overshoot (native plan step 1's `_ext` backstop). So a box
//! holding at most one armed member is bounded by the kernel, and the watcher reads it only when it
//! changes; any fork (an unarmed child) or any unarmed member reverts it to the polling pace at once (the
//! watcher watches each member for its `NOTE_FORK`/`NOTE_EXIT`). [`ProcessBox::usage`] reads the box live,
//! so its accounting is current however long the watcher waits. The cap is per process, doesn't survive a fork, and caps each member at the
//! *whole* bound, not its share, so it doesn't bound a multi-member box's footprint sum — the watcher's
//! rule still does that (step 2's privileged helper apportions a per-member share). A member killed at
//! its cap dies by SIGKILL; the watcher tells that from an ordinary kill by the dead member's peak
//! footprint (`proc_pid_rusage`) having reached the cap, and counts it as a memory end.
//!
//! Rootless: a box is its process group. A member that daemonizes (setsid) leaves the group; the watcher
//! counts it (`Status::escaped`) and no longer holds it, which the next step's box-user closes. A box's
//! leader pid names the group, so a program that outlives the leader and whose leader pid is recycled to a
//! new group leader could be mislisted; this step documents that gap, which the box-user also closes.

use std::collections::BTreeSet;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use hex_api::{Bounds, BoxId, Refusal, Signal, Usage};
use hex_core::watch::{self, Life, Limit, Member, Rates, Verdict, Watch};

use super::sys::{self, Clamp, Event, Kqueue, Timebase};

/// Why a box ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ended {
    /// The watcher found it past a bound and killed it.
    Over(watch::Over),
    /// It was deleted.
    Deleted,
}

/// A box as it is now.
#[derive(Debug, Clone, Default)]
pub struct Status {
    /// Its members still running (not a zombie awaiting reap).
    pub running: u64,
    /// Its processes, as its process bound counts them.
    pub processes: u64,
    /// Its members' footprint, summed, in bytes.
    pub footprint: u64,
    /// The highest footprint the watcher read.
    pub peak: u64,
    /// Times the watcher ended the box for its memory bound.
    pub memory_kills: u64,
    /// Times the watcher ended the box for its process bound.
    pub process_kills: u64,
    /// CPU its members used, in Mach ticks summed.
    pub cpu_ticks: u64,
    pub read_bytes: u64,
    pub written_bytes: u64,
    /// Members that left the box's process group by daemonizing (setsid): counted, no longer held (the
    /// next step's box-user closes this), never dropped silently (CLAUDE.md §3).
    pub escaped: u64,
    /// Set once the box has ended, with why.
    pub ended: Option<Ended>,
    /// Reads the watcher has made of the box.
    pub reads: u64,
    /// Reads that also redid the box's membership work (its kqueue watches, the escape check): one per
    /// change in its running members, not one per read (observability: the watcher's work per read).
    pub membership_passes: u64,
}

/// What every box on the node shares, read once at node start: the machine's timebase and the allocation
/// rate the bound is reckoned against.
#[derive(Debug, Clone)]
pub struct Shared {
    tb: Timebase,
    /// The fastest a box is taken to grow, as this node learns it: seeded by the caller (its machine class's
    /// record, hex_core::watch::seed), raised by any faster growth a watcher sees (watch::LearnedRates).
    rates: Arc<Mutex<watch::LearnedRates>>,
    /// How long a killed group gets to empty, as this node learns it: the record's to start, raised by every
    /// slower drain any box sees (hex_core::drain::Learned; the learned-waits rule, 2026-11-06).
    drain: Arc<Mutex<hex_core::drain::Learned>>,
}

impl Shared {
    /// The node's shared timing. `rate_bytes` and `rate_processes` seed the fastest a box is taken to grow:
    /// the node's machine class's record (hex_core::watch::seed), which its watchers then raise as they see
    /// faster growth.
    pub fn new(
        rate_bytes: std::num::NonZeroU64,
        rate_processes: std::num::NonZeroU64,
    ) -> io::Result<Self> {
        Ok(Self {
            tb: Timebase::get()?,
            rates: Arc::new(Mutex::new(watch::LearnedRates::new(Rates {
                bytes: rate_bytes,
                processes: rate_processes,
            }))),
            drain: Arc::new(Mutex::new(hex_core::drain::Learned::default())),
        })
    }

    /// The rates as learned so far.
    pub fn rates(&self) -> Result<Rates, Refusal> {
        Ok(self
            .rates
            .lock()
            .map_err(|_| poisoned("the node's growth rates"))?
            .rates())
    }
}

/// The state one box shares between the node's threads and its one watcher thread.
#[derive(Debug)]
struct State {
    id: BoxId,
    clamp: Clamp,
    /// The bounds the box was made with, to report on inspect; the watcher holds the [`Limit`] of them it
    /// enforces. Neither changes after create (a new bound is a new box).
    bounds: Bounds,
    /// What the box may hold, read once by its watcher.
    limit: Limit,
    /// The whole-bound fatal cap in bytes, when the box has a memory bound the SPI can take: the watcher
    /// reads a dead armed member's peak footprint against it to tell a cap kill from an ordinary exit
    /// (native plan step 1's `_ext` backstop). `None` leaves the box to the watcher's own rule.
    cap_bytes: Option<u64>,
    /// The box's process group id: the pid of its leader, the first program [`ProcessBox::exec`] spawns.
    pgid: Mutex<Option<i32>>,
    status: Mutex<Status>,
    /// The pids the box armed with the fatal per-process cap — members it spawned, each kernel-bounded.
    /// The watcher reads it to leave a fully-armed one-member box to its events and to attribute a
    /// member's death at its cap to memory; a pid is dropped once gone or accounted, so the set stays
    /// bounded by the box's live member count (CLAUDE.md §2).
    armed: Mutex<BTreeSet<i32>>,
    /// Whether the watcher is ending the box by its own rule (it issued a killpg for a bound). A member's
    /// waiter reads it so a SIGKILL the watcher sent isn't double-counted as a kernel cap kill — the
    /// watcher's End verdict already counted that bound. The watcher mirrors its own `ending` here.
    ending: AtomicBool,
    /// Set by [`ProcessBox::delete`] to end the box; the watcher kills it and stops.
    stop: AtomicBool,
    /// The watcher waits on it; the node triggers it to make a read happen at once (a member started, or
    /// the box is stopped).
    kq: Kqueue,
}

impl State {
    fn status(&self) -> Result<std::sync::MutexGuard<'_, Status>, Refusal> {
        self.status.lock().map_err(|_| poisoned("a box's status"))
    }

    /// Wakes the watcher so it reads now rather than at its next timer. A box with no bound to pace by has
    /// no timer, so a wake that fails is the caller's refusal: the watcher wouldn't see the change.
    fn wake(&self) -> Result<(), Refusal> {
        self.kq
            .trigger()
            .map_err(|e| Refusal::os("waking a box's watcher", e))
    }

    /// Counts member `pid`'s death as a memory end — once — if the box armed it with the fatal cap, isn't
    /// being deleted, and isn't being ended by the watcher's own rule (whose End verdict counts that
    /// bound). The caller establishes the member died by the kernel's hand (a `SIGKILL`: its cap, or
    /// memory pressure on a box member). `peak` is the member's footprint at death when known (its
    /// zombie's, read before it was reaped), else the cap is a sound floor — reaching the fatal cap is
    /// death. Dropping `pid` from the armed set makes the count idempotent across the member's waiter
    /// (on reap) and the watcher (on the exit event): whoever removes it first counts it (native plan
    /// step 1's `_ext` backstop; HEX-MAC-1, the figure HEX-RT-5's memory case and the overshoot bench read).
    fn killed_by_cap(&self, pid: i32, peak: Option<u64>) -> Result<(), Refusal> {
        if self.stop.load(Ordering::SeqCst) || self.ending.load(Ordering::SeqCst) {
            return Ok(());
        }
        let Some(cap) = self.cap_bytes else {
            return Ok(());
        };
        if !self
            .armed
            .lock()
            .map_err(|_| poisoned("a box's armed set"))?
            .remove(&pid)
        {
            // Not an armed member, or already counted: the armed-set removal is the dedup point.
            return Ok(());
        }
        let peak = peak.unwrap_or(cap).max(cap);
        let bound = self.limit.memory.unwrap_or(peak);
        let mut status = self.status()?;
        status.memory_kills = status.memory_kills.saturating_add(1);
        status.peak = status.peak.max(peak);
        if status.ended.is_none() {
            status.ended = Some(Ended::Over(watch::Over::Memory {
                footprint: peak,
                bound,
            }));
        }
        Ok(())
    }
}

/// A handle to a box's shared state that its members' waiters hold, so a member killed at the fatal cap
/// is counted the instant its waiter reaps it — before the caller that waited reads the box's usage, not
/// only when the watcher next polls (native plan step 1's `_ext` backstop). Cloneable.
#[derive(Clone)]
pub struct Members(Arc<State>);

impl Members {
    /// Tells the box a member ended: when `sigkill` (the kernel killed it, at its cap or under memory
    /// pressure), counts a memory end once ([`State::killed_by_cap`]). A clean exit or another signal is
    /// not a memory end. The member is reaped, so its exact peak is gone and the cap is the floor.
    pub fn ended(&self, pid: i32, sigkill: bool) -> Result<(), Refusal> {
        if sigkill {
            self.0.killed_by_cap(pid, None)
        } else {
            Ok(())
        }
    }
}

/// A macOS `process` box. Dropping it ends the box and joins its watcher.
pub struct ProcessBox {
    state: Arc<State>,
    watcher: Mutex<Option<JoinHandle<()>>>,
    /// The node's learned drain bound, shared by every box ([`Shared`]).
    drain: Arc<Mutex<hex_core::drain::Learned>>,
    /// The machine's timebase, for a live read's CPU.
    tb: Timebase,
}

impl ProcessBox {
    /// A box with `bounds`, its members clamped by `weight` (a low weight runs the box at background
    /// priority), watched at the node's `rates`. Nothing runs in it until [`ProcessBox::exec`].
    pub fn create(id: BoxId, bounds: &Bounds, shared: &Shared) -> Result<Self, Refusal> {
        let state = Arc::new(State {
            id,
            clamp: clamp(bounds),
            bounds: bounds.clone(),
            limit: limit(bounds),
            cap_bytes: bounds.memory_max.and_then(sys::jetsam_cap_bytes),
            pgid: Mutex::new(None),
            status: Mutex::new(Status::default()),
            armed: Mutex::new(BTreeSet::new()),
            ending: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            kq: Kqueue::new().map_err(|e| Refusal::os("making a box's kqueue", e))?,
        });
        let watcher = Watcher::new(Arc::clone(&state), shared.clone())
            .map_err(|e| Refusal::os("sizing a box's watcher", e))?
            .start()
            .map_err(|e| Refusal::os("starting a box's watcher", e))?;
        Ok(Self {
            state,
            watcher: Mutex::new(Some(watcher)),
            drain: Arc::clone(&shared.drain),
            tb: shared.tb,
        })
    }

    pub fn id(&self) -> &BoxId {
        &self.state.id
    }

    /// A handle for a member's waiter to tell the box the member ended, so a kill at the fatal cap is
    /// counted the instant the waiter reaps it (native plan step 1's `_ext` backstop).
    pub fn members(&self) -> Members {
        Members(Arc::clone(&self.state))
    }

    /// The bounds the box was made with.
    pub fn bounds(&self) -> Bounds {
        self.state.bounds.clone()
    }

    /// Spawns `program` into the box: the first into a new process group it leads, each later one into that
    /// group, so every member and everything it forks is in the box from its first instruction. Once every
    /// member has ended the group is gone and none can join it, so the next program leads a new one, as a
    /// box on any other OS takes a program after its last ended (HEX-DIFF-1). Returns its pid.
    pub fn exec(&self, spawn: sys::Spawn) -> Result<i32, Refusal> {
        if self.state.stop.load(Ordering::SeqCst) {
            return Err(Refusal::NoSuchBox(self.state.id.clone()));
        }
        let mut pgid = self
            .state
            .pgid
            .lock()
            .map_err(|_| poisoned("a box's group"))?;
        let group = match *pgid {
            Some(g) if live(g)? => g,
            _ => 0,
        };
        let carries_cap = spawn.jetsam_mib.is_some();
        let pid = sys::spawn(&spawn, group, self.state.clamp)
            .map_err(|e| Refusal::os("spawning a program in a box", e))?;
        if group == 0 {
            *pgid = Some(pid);
        }
        drop(pgid);
        // The member carries the fatal kernel cap: track it so the watcher paces by it and attributes a
        // death at the cap to memory (native plan step 1's `_ext` backstop).
        if carries_cap {
            self.state
                .armed
                .lock()
                .map_err(|_| poisoned("a box's armed set"))?
                .insert(pid);
        }
        self.state.wake()?;
        Ok(pid)
    }

    /// The box as it is now.
    pub fn status(&self) -> Result<Status, Refusal> {
        self.state.status().map(|s| s.clone())
    }

    /// Whether the box has a live member right now, read live (not from the watcher's last poll, which
    /// lags), as the Linux class reads its cgroup's population for inspect.
    pub fn running_now(&self) -> Result<bool, Refusal> {
        match *self
            .state
            .pgid
            .lock()
            .map_err(|_| poisoned("a box's group"))?
        {
            Some(pgid) => live(pgid),
            None => Ok(false),
        }
    }

    /// What the box has used and holds now, as a [`Usage`]: its members read live (the group listed, each
    /// one's rusage), as the Linux class reads its cgroup's files, so it is current however long the
    /// watcher waits (a box with no bound to pace by is read only when it changes). The kill counts and
    /// the peak are the watcher's, the peak raised to what this read finds.
    pub fn usage(&self) -> Result<Usage, Refusal> {
        let pgid = *self
            .state
            .pgid
            .lock()
            .map_err(|_| poisoned("a box's group"))?;
        let live = match pgid {
            Some(pgid) => Some(read_live(pgid)?),
            None => None,
        };
        let mut s = self.state.status()?;
        if let Some(live) = live {
            s.footprint = live.footprint;
            s.processes = live.processes;
            s.peak = s.peak.max(live.footprint);
            s.cpu_ticks = self.tb.nanos(live.cpu) / 1000;
            s.read_bytes = s.read_bytes.max(live.read);
            s.written_bytes = s.written_bytes.max(live.written);
        }
        Ok(Usage {
            memory_bytes: s.footprint,
            memory_peak_bytes: s.peak,
            oom_kills: s.memory_kills,
            pids: s.processes,
            pids_refused: s.process_kills,
            // Mach ticks are reported as microseconds by [`sys::Timebase`] when the node reads usage; the
            // raw ticks are kept here, converted where the timebase is to hand.
            cpu_usec: s.cpu_ticks,
            io_read_bytes: s.read_bytes,
            io_write_bytes: s.written_bytes,
        })
    }

    /// Sends `signal` to every member (killpg). KILL is the rule's own end; any other is passed on. Refused
    /// as not running when no member is alive, as every class refuses it (HEX-DIFF-2).
    pub fn signal(&self, signal: Signal) -> Result<(), Refusal> {
        let macos = hex_core::darwin::signal_to_macos(signal.number()).ok_or(Refusal::Invalid {
            what: "signal",
            why: format!("signal {} has no macOS number", signal.number()),
        })?;
        let not_running = || Refusal::NotRunning(self.state.id.clone(), "signal");
        match *self
            .state
            .pgid
            .lock()
            .map_err(|_| poisoned("a box's group"))?
        {
            Some(pgid) if live(pgid)? => {
                if killpg(pgid, macos)? {
                    Ok(())
                } else {
                    Err(not_running())
                }
            }
            _ => Err(not_running()),
        }
    }

    /// Ends the box: stops its watcher, kills every member (killpg KILL) and waits for them to die, leaving
    /// each member's own waiter to reap it. Idempotent.
    pub fn delete(&self) -> Result<(), Refusal> {
        self.state.stop.store(true, Ordering::SeqCst);
        self.state.wake()?;
        if let Some(handle) = self
            .watcher
            .lock()
            .map_err(|_| poisoned("a box's watcher"))?
            .take()
        {
            let _ = handle.join();
        }
        // The watcher has killed the group and set `ended`; make sure no member still runs.
        if let Some(pgid) = *self
            .state
            .pgid
            .lock()
            .map_err(|_| poisoned("a box's group"))?
        {
            let mut pids = sys::listing().map_err(|e| Refusal::os("listing a box's group", e))?;
            // The group's size sets how long it gets to empty: the measured kill-to-empty envelope, doubled,
            // as this node has learned it (hex_core::drain). Past it the kill didn't take, a fault named
            // (CLAUDE.md §4); a delete has no other box to try, so the caller gets it.
            let members = alive_in(pgid, &mut pids)?;
            let patience = self
                .drain
                .lock()
                .map_err(|_| poisoned("the node's drain bound"))?
                .bound(members);
            let killed = Instant::now();
            let _ = killpg(
                pgid,
                hex_core::darwin::signal_to_macos(Signal::KILL.number()).unwrap_or(9),
            );
            let deadline = killed.checked_add(patience);
            loop {
                if alive_in(pgid, &mut pids)? == 0 {
                    if let Ok(mut learned) = self.drain.lock() {
                        learned.observe(members, killed.elapsed());
                    }
                    return Ok(());
                }
                if deadline.is_none_or(|d| Instant::now() >= d) {
                    return Err(Refusal::os(
                        "emptying a box",
                        io::Error::other(format!(
                            "box {} still holds a live member",
                            self.state.id
                        )),
                    ));
                }
                std::thread::yield_now();
            }
        }
        Ok(())
    }
}

impl Drop for ProcessBox {
    fn drop(&mut self) {
        let _ = self.delete();
    }
}

/// A box's group as one live read finds it: its footprint and processes as the watcher's rule counts them,
/// and its members' summed CPU (Mach ticks) and I/O.
struct Live {
    footprint: u64,
    processes: u64,
    cpu: u64,
    read: u64,
    written: u64,
}

/// Reads process group `pgid` now: each listed member's rusage, a pid gone since the listing (ESRCH) left
/// out, and the leader's zombie not counted as a process while it is held for the group's id, as
/// [`watch::Watch::read`] counts.
fn read_live(pgid: i32) -> Result<Live, Refusal> {
    let mut pids = sys::listing().map_err(|e| Refusal::os("listing a box's group", e))?;
    sys::group(pgid, &mut pids).map_err(|e| Refusal::os("listing a box's group", e))?;
    let mut live = Live {
        footprint: 0,
        processes: 0,
        cpu: 0,
        read: 0,
        written: 0,
    };
    for &pid in &pids {
        let Some(r) = sys::rusage(pid).map_err(|e| Refusal::os("reading a member's rusage", e))?
        else {
            continue;
        };
        if !(r.ended && pid == pgid) {
            live.processes = live.processes.saturating_add(1);
        }
        live.footprint = live.footprint.saturating_add(r.footprint);
        live.cpu = live.cpu.saturating_add(r.cpu);
        live.read = live.read.saturating_add(r.read_bytes);
        live.written = live.written.saturating_add(r.written_bytes);
    }
    Ok(live)
}

/// Whether process group `pgid` has a member alive (not a zombie).
fn live(pgid: i32) -> Result<bool, Refusal> {
    let mut pids = sys::listing().map_err(|e| Refusal::os("listing a box's group", e))?;
    Ok(alive_in(pgid, &mut pids)? > 0)
}

/// How many of process group `pgid`'s members are alive (not a zombie), by reading each one's rusage.
fn alive_in(pgid: i32, pids: &mut Vec<i32>) -> Result<usize, Refusal> {
    sys::group(pgid, pids).map_err(|e| Refusal::os("listing a box's group", e))?;
    let mut alive = 0usize;
    for pid in pids.iter() {
        match sys::rusage(*pid).map_err(|e| Refusal::os("reading a member's rusage", e))? {
            Some(r) if !r.ended => alive = alive.saturating_add(1),
            _ => {}
        }
    }
    Ok(alive)
}

/// The watcher of one box: a thread that reads the box every (bound − footprint) /
/// rate, holds its bounds, and ends it when it passes one.
struct Watcher {
    state: Arc<State>,
    shared: Shared,
    watch: Watch,
    /// The members seen at the last read, to find which went and which are new (escapes, [`watch::changes`]).
    last: Vec<Member>,
    /// A cheap signature of the last read's running members (summed pids and start times): the escape check
    /// — which allocates — runs only when it changes, so a box's steady members cost nothing between reads.
    last_sig: u64,
    /// The running members watched on the box's kqueue for their forks and ends, so a fork (an unarmed
    /// child) or a member's death wakes the watcher at once rather than at its next timer.
    watched: BTreeSet<i32>,
    pids: Vec<i32>,
    /// The last read's listing, to tell whether this one changed.
    last_listing: Vec<i32>,
    events: Vec<Event>,
    /// When the last read was and what it found (footprint, processes): the growth to the next read is what
    /// the node's rates learn from.
    last_read: Option<(Instant, u64, u64)>,
}

impl Watcher {
    fn new(state: Arc<State>, shared: Shared) -> io::Result<Self> {
        let rates = shared
            .rates()
            .map_err(|e| io::Error::other(e.to_string()))?;
        let watch = Watch::new(state.limit, rates);
        Ok(Self {
            state,
            shared,
            watch,
            last: Vec::new(),
            last_sig: 0,
            watched: BTreeSet::new(),
            pids: sys::listing()?,
            events: Vec::with_capacity(sys::EVENTS),
            last_listing: Vec::new(),
            last_read: None,
        })
    }

    fn start(mut self) -> io::Result<JoinHandle<()>> {
        std::thread::Builder::new()
            .name(format!("hex-box-{}", self.state.id))
            .spawn(move || self.run())
    }

    fn run(&mut self) {
        // A plain timeshare thread: the watcher holds a box's bound by reading it, never by taking priority
        // over other programs (Ada's rule, 2026-11-03; the lead, 2026-11-06).
        loop {
            if self.state.stop.load(Ordering::SeqCst) {
                self.finish();
                return;
            }
            // Account any member the kernel killed at its fatal cap, from the exit events the last wait
            // delivered, before this read prunes the armed set of pids now gone.
            if self.account_exit_events().is_err() {
                return;
            }
            let next = match self.step() {
                Ok(next) => next,
                // A read failed (a lock poisoned, a call errored): stop watching rather than spin. The box
                // is left as it is for delete to clean up.
                Err(_) => return,
            };
            // A timer only where a bound needs one; otherwise the box's next change wakes it: a member's
            // fork or end, a start (exec) or a delete, each a kqueue event.
            let _ = self.state.kq.wait(next, None, &mut self.events);
        }
    }

    /// One read and its decision; returns the wait before the next read, or `None` to wait until the box
    /// changes (it is idle).
    fn step(&mut self) -> Result<Option<Duration>, Refusal> {
        let Some(pgid) = *self
            .state
            .pgid
            .lock()
            .map_err(|_| poisoned("a box's group"))?
        else {
            return Ok(None);
        };
        let rates = self.shared.rates()?;
        self.watch.rated(rates);
        let reading = self.read_members(pgid)?;
        // The membership work (the escape check, which diffs the member lists and allocates, and the
        // kqueue rewatch) runs only when the running set changed: a box with steady members costs a listing
        // and a rusage a member per read, nothing more (the lead, 2026-11-06: less work per read).
        let membership_changed = reading.sig != self.last_sig;
        if membership_changed {
            self.count_escapes(&reading.members)?;
            self.last_sig = reading.sig;
        }
        let read = self.watch.read(&reading.members);
        // The growth since the last read: a box faster than the node's rates raises them.
        let now = Instant::now();
        if let Some((then, footprint, processes)) = self.last_read
            && let Ok(mut learned) = self.shared.rates.lock()
        {
            learned.observe(
                read.footprint.saturating_sub(footprint),
                read.processes.saturating_sub(processes),
                now.saturating_duration_since(then),
            );
        }
        self.last_read = Some((now, read.footprint, read.processes));
        self.record(&read, &reading.totals)?;
        // Mirror the watcher's own ending for a member's waiter: a SIGKILL the watcher sent isn't a
        // separate kernel cap kill to count.
        self.state
            .ending
            .store(self.watch.ending().is_some(), Ordering::SeqCst);
        // Watch the current members for their forks and ends, so the next membership change wakes at once.
        if membership_changed {
            self.rewatch(&reading.members)?;
        }
        self.last = reading.members;
        match read.verdict {
            Verdict::Wait(d) => Ok(Some(d)),
            Verdict::End(_) | Verdict::Kill => {
                let _ = killpg(pgid, kill_signal());
                Ok(Some(watch::FLOOR))
            }
            Verdict::Idle | Verdict::Events => Ok(None),
        }
    }

    /// The box's members now and their summed CPU and I/O: each listed pid with its footprint, start time,
    /// life and whether the box armed it, read through its rusage, once per member. A pid the listing
    /// holds but rusage no longer has (ESRCH) was reaped since: left out. Prunes the armed set of pids
    /// gone, so it stays bounded by the live members (CLAUDE.md §2).
    fn read_members(&mut self, pgid: i32) -> Result<Reading, Refusal> {
        sys::group(pgid, &mut self.pids).map_err(|e| Refusal::os("listing a box's group", e))?;
        // The armed set's pruning needs the listing's pids as sets: built only when the listing changed.
        let listing_changed = self.pids != self.last_listing;
        let last_pids: BTreeSet<i32> = if listing_changed {
            self.last
                .iter()
                .filter_map(|m| i32::try_from(m.pid).ok())
                .collect()
        } else {
            BTreeSet::new()
        };
        let mut armed = self
            .state
            .armed
            .lock()
            .map_err(|_| poisoned("a box's armed set"))?;
        let mut members = Vec::with_capacity(self.pids.len());
        let mut present = BTreeSet::new();
        let mut totals = Totals::default();
        let mut sig = 0u64;
        for &pid in &self.pids {
            let Some(r) =
                sys::rusage(pid).map_err(|e| Refusal::os("reading a member's rusage", e))?
            else {
                continue;
            };
            if listing_changed {
                present.insert(pid);
            }
            // The leader's zombie, while other members run, is the node holding the pid so the group id
            // can't be recycled: not the box's own process. Any other ended member is counted until reaped,
            // as Linux's `pids.max` counts a task until it is reaped.
            let life = match (r.ended, pid == pgid) {
                (true, true) => Life::Held,
                (true, false) => Life::Ended,
                (false, _) => Life::Running,
            };
            totals.cpu = totals.cpu.saturating_add(r.cpu);
            totals.read = totals.read.saturating_add(r.read_bytes);
            totals.written = totals.written.saturating_add(r.written_bytes);
            if life == Life::Running {
                // Order-independent so the listing's order doesn't matter; a member and its start time, so a
                // recycled pid shifts it. The multiplier is Fibonacci hashing's 2^32/phi (Knuth, TAOCP vol. 3,
                // §6.4), which spreads consecutive pids across the word.
                sig = sig.wrapping_add(
                    (u64::try_from(pid).unwrap_or(0))
                        .wrapping_mul(0x9e37_79b9)
                        .wrapping_add(r.started),
                );
            }
            members.push(Member {
                pid: u32::try_from(pid).unwrap_or(0),
                started: r.started,
                footprint: r.footprint,
                life,
                armed: armed.contains(&pid),
            });
        }
        // Prune armed pids that were listed before and are gone now (reaped): keeps the set bounded by the
        // live members, without dropping a just-spawned pid the listing hasn't caught up with yet.
        if listing_changed {
            armed.retain(|p| present.contains(p) || !last_pids.contains(p));
            self.last_listing.clone_from(&self.pids);
        }
        drop(armed);
        Ok(Reading {
            members,
            totals,
            sig,
        })
    }

    /// Counts the members that left the group alive since the last read: each daemonized (setsid), which
    /// the box no longer holds (the next step's box-user does). A member whose rusage is gone or ended just
    /// exited, and isn't an escape.
    fn count_escapes(&mut self, now: &[Member]) -> Result<(), Refusal> {
        let gone = watch::changes(&self.last, now).gone;
        let mut escaped = 0u64;
        for m in gone {
            let pid = i32::try_from(m.pid).unwrap_or(-1);
            match sys::rusage(pid).map_err(|e| Refusal::os("reading a gone member's rusage", e))? {
                Some(r) if !r.ended => escaped = escaped.saturating_add(1),
                _ => {}
            }
        }
        if escaped > 0 {
            let mut status = self.state.status()?;
            status.escaped = status.escaped.saturating_add(escaped);
        }
        Ok(())
    }

    /// Writes what the read found and decided into the box's status.
    fn record(&self, read: &watch::Read, totals: &Totals) -> Result<(), Refusal> {
        let mut status = self.state.status()?;
        status.running = read.running;
        status.processes = read.processes;
        status.footprint = read.footprint;
        // Max, not assignment: a member the kernel killed at its cap recorded its peak in account_jetsam
        // before this read, and the watcher's own peak (summed live footprints) can be lower — a box read
        // only on its events may never have seen the member near its cap while it was alive.
        status.peak = status.peak.max(self.watch.peak());
        status.reads = self.watch.reads();
        // In microseconds, from the machine's timebase (`cpu` is Mach ticks).
        status.cpu_ticks = self.shared.tb.nanos(totals.cpu) / 1000;
        // Monotonic: a member gone since last read no longer reports its I/O, so keep the most seen.
        status.read_bytes = status.read_bytes.max(totals.read);
        status.written_bytes = status.written_bytes.max(totals.written);
        if let Verdict::End(over) = read.verdict {
            match over {
                watch::Over::Memory { .. } => {
                    status.memory_kills = status.memory_kills.saturating_add(1)
                }
                watch::Over::Processes { .. } => {
                    status.process_kills = status.process_kills.saturating_add(1)
                }
            }
            status.ended = Some(Ended::Over(over));
        }
        Ok(())
    }

    /// Accounts the box's members the kernel killed at their fatal cap, from the exit events the last wait
    /// delivered — the path for a member no waiter reaps in time (e.g. the overshoot bench holds its
    /// [`BoxProc`]s and polls). The node is the member's parent, so the kill signal rides in the event
    /// (`NOTE_EXITSTATUS`): a `SIGKILL` of a box member is the kernel's cap or memory pressure.
    /// [`State::killed_by_cap`] counts it once (guarding a delete or the watcher's own kill) with the
    /// zombie's lifetime peak while it is still readable. The member's own waiter accounts the same kill
    /// on reap, deterministically, so a caller that waited sees the count; whichever comes first wins.
    fn account_exit_events(&self) -> Result<(), Refusal> {
        for e in &self.events {
            if let Event::Exit { pid, status, .. } = *e
                && (status & 0x7f) == kill_signal()
            {
                // The member is a fresh zombie (not yet reaped by its waiter): read its lifetime peak for
                // the exact figure; `killed_by_cap` falls back to the cap when it is gone, and guards the
                // box being deleted or the watcher's own kill, and counts each death once.
                let peak = sys::rusage(pid).ok().flatten().map(|r| r.peak);
                self.state.killed_by_cap(pid, peak)?;
            }
        }
        Ok(())
    }

    /// Watches the box's running members for their forks and ends on the box's kqueue, so a fork (an
    /// unarmed child) or a death wakes the watcher at once — to put an events-paced box back on headroom, or
    /// to read a cap kill's peak before the member's own waiter reaps it — rather than at its next timer.
    /// Members gone since the last read are unwatched (the kernel already dropped an exited member's knote,
    /// so that unwatch is a no-op it tolerates).
    fn rewatch(&mut self, members: &[Member]) -> Result<(), Refusal> {
        {
            let mut status = self.state.status()?;
            status.membership_passes = status.membership_passes.saturating_add(1);
        }
        let running: BTreeSet<i32> = members
            .iter()
            .filter(|m| m.life == Life::Running)
            .filter_map(|m| i32::try_from(m.pid).ok())
            .collect();
        let to_watch: Vec<i32> = running.difference(&self.watched).copied().collect();
        if !to_watch.is_empty() {
            self.state
                .kq
                .watch(&to_watch, 0)
                .map_err(|e| Refusal::os("watching a box's members", e))?;
        }
        let to_unwatch: Vec<i32> = self.watched.difference(&running).copied().collect();
        for pid in to_unwatch {
            self.state
                .kq
                .unwatch(pid)
                .map_err(|e| Refusal::os("unwatching a box's member", e))?;
        }
        self.watched = running;
        Ok(())
    }

    /// On stop: end the box. Kill the group and mark it deleted if the rule hadn't already ended it.
    fn finish(&mut self) {
        if let Ok(Some(pgid)) = self.state.pgid.lock().map(|g| *g) {
            let _ = killpg(pgid, kill_signal());
        }
        if let Ok(mut status) = self.state.status.lock()
            && status.ended.is_none()
        {
            status.ended = Some(Ended::Deleted);
        }
    }
}

/// A read's summed CPU (Mach ticks) and I/O over the box's members.
#[derive(Debug, Clone, Copy, Default)]
struct Totals {
    cpu: u64,
    read: u64,
    written: u64,
}

/// What one read of a box found: its members (each with its life and whether the box armed it), the
/// summed CPU/IO totals, and a signature of the running set (for the escape check).
struct Reading {
    members: Vec<Member>,
    totals: Totals,
    sig: u64,
}

/// What a box's bounds cap, as the watcher's rule reads them: its memory, and its process count (a thread
/// or a process both count on macOS, as `pids.max` counts both on Linux).
fn limit(bounds: &Bounds) -> Limit {
    Limit {
        memory: bounds.memory_max,
        processes: bounds.pids_max.map(u64::from),
    }
}

/// A box's QoS clamp, from its declared CPU weight as `cpu.weight` sets a box's share on Linux: background
/// where its weight is under the default 100, none otherwise (the lead, 2026-11-06: no clamp to keep the
/// watcher ahead of a box).
fn clamp(bounds: &Bounds) -> Clamp {
    match bounds.cpu_weight {
        Some(w) if w < 100 => Clamp::Background,
        _ => Clamp::Unclamped,
    }
}

fn kill_signal() -> i32 {
    // KILL is 9 on both kernels; the table confirms it, falling back to 9 only to satisfy the type.
    hex_core::darwin::signal_to_macos(Signal::KILL.number()).unwrap_or(9)
}

fn poisoned(what: &str) -> Refusal {
    Refusal::Broken(format!("{what} lock is poisoned"))
}

/// Signals process group `pgid`; false when the group has no member left (ESRCH).
fn killpg(pgid: i32, signal: i32) -> Result<bool, Refusal> {
    let sig = rustix::process::Signal::from_named_raw(signal).ok_or(Refusal::Invalid {
        what: "signal",
        why: format!("{signal} isn't a signal this kernel names"),
    })?;
    let pid = rustix::process::Pid::from_raw(pgid).ok_or(Refusal::Invalid {
        what: "pgid",
        why: format!("{pgid} isn't a process group"),
    })?;
    match rustix::process::kill_process_group(pid, sig) {
        Ok(()) => Ok(true),
        Err(rustix::io::Errno::SRCH) => Ok(false),
        Err(e) => Err(Refusal::os("killpg", e)),
    }
}
