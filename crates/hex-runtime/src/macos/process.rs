//! The `process` class on macOS (docs/design/runtime.md, decision "each OS runs its own boxes
//! natively"): a box is a process group Hex spawns (posix_spawn, a new process group) under a QoS clamp,
//! with background priority when its weight is low. macOS has no public cap on a process tree's memory or
//! its process count (docs/research/runtime/native-platforms.md §3), so a watcher thread holds the box's
//! bounds by Hex's own rule ([`hex_core::watch`]): it lists the box's members (`proc_listpids` of its
//! process group), reads each footprint (`proc_pid_rusage`), and ends the box (killpg, then the members'
//! own waiters reap) once it passes a bound. The rule is the pure core; this shell does the listing, the
//! reading, the waiting and the killing, from a Mach time-constraint thread on `NOTE_CRITICAL` kqueue
//! timers so a box filling every core can't hold its watcher off (RESULT.md).
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

/// How long [`ProcessBox::delete`] waits for a killed box's members to die before it reports the kill
/// didn't take: a `SIGKILL` to a process group ends its members in milliseconds, so missing a generous
/// bound is a fault to report, not a limit to raise (CLAUDE.md §4).
const DRAIN_PATIENCE: Duration = Duration::from_secs(10);

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
}

/// What every box on the node shares, read once at node start: the machine's timebase and the allocation
/// rate the bound is reckoned against.
#[derive(Debug, Clone)]
pub struct Shared {
    tb: Timebase,
    rates: Rates,
}

impl Shared {
    /// The node's shared timing. `rate_bytes` and `rate_processes` are the fastest a box is taken to grow;
    /// measured at node start or cited (CLAUDE.md §5), the caller says which.
    pub fn new(
        rate_bytes: std::num::NonZeroU64,
        rate_processes: std::num::NonZeroU64,
    ) -> io::Result<Self> {
        Ok(Self {
            tb: Timebase::get()?,
            rates: Rates {
                bytes: rate_bytes,
                processes: rate_processes,
            },
        })
    }

    pub fn rates(&self) -> Rates {
        self.rates
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
    /// The box's process group id: the pid of its leader, the first program [`ProcessBox::exec`] spawns.
    pgid: Mutex<Option<i32>>,
    status: Mutex<Status>,
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

    /// Wakes the watcher so it reads now rather than at its next timer.
    fn wake(&self) {
        let _ = self.kq.trigger();
    }
}

/// A macOS `process` box. Dropping it ends the box and joins its watcher.
pub struct ProcessBox {
    state: Arc<State>,
    watcher: Mutex<Option<JoinHandle<()>>>,
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
            pgid: Mutex::new(None),
            status: Mutex::new(Status::default()),
            stop: AtomicBool::new(false),
            kq: Kqueue::new().map_err(|e| Refusal::os("making a box's kqueue", e))?,
        });
        let watcher = Watcher::new(Arc::clone(&state), shared.clone())
            .start()
            .map_err(|e| Refusal::os("starting a box's watcher", e))?;
        Ok(Self {
            state,
            watcher: Mutex::new(Some(watcher)),
        })
    }

    pub fn id(&self) -> &BoxId {
        &self.state.id
    }

    /// The bounds the box was made with.
    pub fn bounds(&self) -> Bounds {
        self.state.bounds.clone()
    }

    /// Spawns `program` into the box: the first into a new process group it leads, each later one into that
    /// group, so every member and everything it forks is in the box from its first instruction. Returns
    /// its pid.
    pub fn exec(&self, spawn: sys::Spawn) -> Result<i32, Refusal> {
        if self.state.stop.load(Ordering::SeqCst) {
            return Err(Refusal::NoSuchBox(self.state.id.clone()));
        }
        let mut pgid = self
            .state
            .pgid
            .lock()
            .map_err(|_| poisoned("a box's group"))?;
        let group = pgid.unwrap_or(0);
        let pid = sys::spawn(&spawn, group, self.state.clamp)
            .map_err(|e| Refusal::os("spawning a program in a box", e))?;
        if pgid.is_none() {
            *pgid = Some(pid);
        }
        drop(pgid);
        self.state.wake();
        Ok(pid)
    }

    /// The box as it is now.
    pub fn status(&self) -> Result<Status, Refusal> {
        self.state.status().map(|s| s.clone())
    }

    /// Whether the box has a live member right now, read live (not from the watcher's last poll, which
    /// lags), as the Linux class reads its cgroup's population for inspect.
    pub fn running_now(&self) -> Result<bool, Refusal> {
        let Some(pgid) = *self
            .state
            .pgid
            .lock()
            .map_err(|_| poisoned("a box's group"))?
        else {
            return Ok(false);
        };
        let mut pids = Vec::with_capacity(sys::max_processes().unwrap_or(4096));
        Ok(alive_in(pgid, &mut pids)? > 0)
    }

    /// What the box has used and holds now, as a [`Usage`].
    pub fn usage(&self) -> Result<Usage, Refusal> {
        let s = self.state.status()?;
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

    /// Sends `signal` to every member (killpg). KILL is the rule's own end; any other is passed on.
    pub fn signal(&self, signal: Signal) -> Result<(), Refusal> {
        let macos = hex_core::darwin::signal_to_macos(signal.number()).ok_or(Refusal::Invalid {
            what: "signal",
            why: format!("signal {} has no macOS number", signal.number()),
        })?;
        match *self
            .state
            .pgid
            .lock()
            .map_err(|_| poisoned("a box's group"))?
        {
            Some(pgid) => killpg(pgid, macos),
            None => Err(Refusal::NotRunning(self.state.id.clone(), "signal")),
        }
    }

    /// Ends the box: stops its watcher, kills every member (killpg KILL) and waits for them to die, leaving
    /// each member's own waiter to reap it. Idempotent.
    pub fn delete(&self) -> Result<(), Refusal> {
        self.state.stop.store(true, Ordering::SeqCst);
        self.state.wake();
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
            let _ = killpg(
                pgid,
                hex_core::darwin::signal_to_macos(Signal::KILL.number()).unwrap_or(9),
            );
            let deadline = Instant::now().checked_add(DRAIN_PATIENCE);
            let mut pids = Vec::with_capacity(sys::max_processes().unwrap_or(4096));
            loop {
                if alive_in(pgid, &mut pids)? == 0 {
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

/// The watcher of one box: a Mach time-constraint thread that reads the box every (bound − footprint) /
/// rate, holds its bounds, and ends it when it passes one.
struct Watcher {
    state: Arc<State>,
    shared: Shared,
    watch: Watch,
    /// The members seen at the last read, to find which went and which are new (escapes, [`watch::changes`]).
    last: Vec<Member>,
    /// Members being watched for their forks and ends, so a read happens as soon as the box changes.
    watched: BTreeSet<i32>,
    pids: Vec<i32>,
    events: Vec<Event>,
}

impl Watcher {
    fn new(state: Arc<State>, shared: Shared) -> Self {
        let watch = Watch::new(state.limit, shared.rates);
        Self {
            state,
            shared,
            watch,
            last: Vec::new(),
            watched: BTreeSet::new(),
            pids: Vec::with_capacity(sys::max_processes().unwrap_or(4096)),
            events: Vec::with_capacity(sys::EVENTS),
        }
    }

    fn start(mut self) -> io::Result<JoinHandle<()>> {
        std::thread::Builder::new()
            .name(format!("hex-box-{}", self.state.id))
            .spawn(move || self.run())
    }

    fn run(&mut self) {
        // Best-effort: a real-time thread reads on time even when the box fills every core (RESULT.md).
        // Without it the watcher still runs, only later under full load, so a failure isn't fatal.
        let _ = sys::time_constraint(
            &self.shared.tb,
            watch::FLOOR,
            Duration::from_micros(50),
            watch::FLOOR,
        );
        loop {
            if self.state.stop.load(Ordering::SeqCst) {
                self.finish();
                return;
            }
            let next = match self.step() {
                Ok(next) => next,
                // A read failed (a lock poisoned, a call errored): stop watching rather than spin. The box
                // is left as it is for delete to clean up.
                Err(_) => return,
            };
            let _ = self.state.kq.wait(
                next,
                next.map_or(Some(watch::CEILING), |_| None),
                &mut self.events,
            );
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
        let (members, totals) = self.read_members(pgid)?;
        self.count_escapes(&members)?;
        let read = self.watch.read(&members);
        self.record(&read, &totals)?;
        self.last = members;
        match read.verdict {
            Verdict::Wait(d) => Ok(Some(d)),
            Verdict::End(_) | Verdict::Kill => {
                let _ = killpg(pgid, kill_signal());
                Ok(Some(watch::FLOOR))
            }
            Verdict::Idle => Ok(None),
        }
    }

    /// The box's members now and their summed CPU and I/O: each listed pid with its footprint, start time
    /// and life, read through its rusage, once per member. A pid the listing holds but rusage no longer
    /// has (ESRCH) was reaped since: left out.
    fn read_members(&mut self, pgid: i32) -> Result<(Vec<Member>, Totals), Refusal> {
        sys::group(pgid, &mut self.pids).map_err(|e| Refusal::os("listing a box's group", e))?;
        let mut members = Vec::with_capacity(self.pids.len());
        let mut totals = Totals::default();
        let mut fresh = Vec::new();
        for &pid in &self.pids {
            let Some(r) =
                sys::rusage(pid).map_err(|e| Refusal::os("reading a member's rusage", e))?
            else {
                continue;
            };
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
            if pid != pgid && life == Life::Running && self.watched.insert(pid) {
                fresh.push(pid);
            }
            members.push(Member {
                pid: u32::try_from(pid).unwrap_or(0),
                started: r.started,
                footprint: r.footprint,
                life,
            });
        }
        if !fresh.is_empty() {
            let _ = self.state.kq.watch(&fresh, 0);
        }
        Ok((members, totals))
    }

    /// Counts the members that left the group alive since the last read: each daemonized (setsid), which
    /// the box no longer holds (the next step's box-user does). A member whose rusage is gone or ended just
    /// exited, and isn't an escape.
    fn count_escapes(&mut self, now: &[Member]) -> Result<(), Refusal> {
        let gone = watch::changes(&self.last, now).gone;
        let mut escaped = 0u64;
        for m in gone {
            let pid = i32::try_from(m.pid).unwrap_or(-1);
            self.watched.remove(&pid);
            let _ = self.state.kq.unwatch(pid);
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
        status.peak = self.watch.peak();
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

/// What a box's bounds cap, as the watcher's rule reads them: its memory, and its process count (a thread
/// or a process both count on macOS, as `pids.max` counts both on Linux).
fn limit(bounds: &Bounds) -> Limit {
    Limit {
        memory: bounds.memory_max,
        processes: bounds.pids_max.map(u64::from),
    }
}

/// A box's QoS clamp: background where its weight is low (under the default 100), utility otherwise.
fn clamp(bounds: &Bounds) -> Clamp {
    match bounds.cpu_weight {
        Some(w) if w < 100 => Clamp::Background,
        _ => Clamp::Utility,
    }
}

fn kill_signal() -> i32 {
    // KILL is 9 on both kernels; the table confirms it, falling back to 9 only to satisfy the type.
    hex_core::darwin::signal_to_macos(Signal::KILL.number()).unwrap_or(9)
}

fn poisoned(what: &str) -> Refusal {
    Refusal::Broken(format!("{what} lock is poisoned"))
}

fn killpg(pgid: i32, signal: i32) -> Result<(), Refusal> {
    let sig = rustix::process::Signal::from_named_raw(signal).ok_or(Refusal::Invalid {
        what: "signal",
        why: format!("{signal} isn't a signal this kernel names"),
    })?;
    let pid = rustix::process::Pid::from_raw(pgid).ok_or(Refusal::Invalid {
        what: "pgid",
        why: format!("{pgid} isn't a process group"),
    })?;
    match rustix::process::kill_process_group(pid, sig) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(e) => Err(Refusal::os("killpg", e)),
    }
}
