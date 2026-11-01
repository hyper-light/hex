//! The macOS box helper daemon (native plan step 2). A box runs as a user of its own from a pool the
//! installer makes; the privileged launchd daemon starts each box's programs as that user, arms their
//! physical-footprint ledger limits and catches their Mach exceptions, and ends a box by killing every
//! process of its user. The node drives it over the [`hex_core::helper`] protocol.
//!
//! This module is the daemon's membership and bookkeeping: the live members of a box's pool user and
//! their footprints (`proc_listpids(PROC_UID_ONLY)` through [`super::sys::user_pids`]), whether the user
//! is empty (delete frees the user back to the pool only once it is), and killing every process of the
//! user. A box's process count is the kernel's — `RLIMIT_NPROC` of its user, set when the leader starts.
//! The privileged spawn and the ledger/exception arming are [`super::sys`]'s (`unsafe`, CLAUDE.md §12);
//! the apportionment is [`hex_core::boxuser`].

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use hex_api::{BoxId, Program, Refusal, Signal};
use hex_core::boxuser::{self, Bound, Use};
use hex_core::darwin;
use hex_core::helper::{self, Answer, Refused, Request, START_FDS, State};

use super::helper_client::{recv_framed, send_framed};
use super::sys;

/// How long [`Daemon::end`] waits for a killed box's user to empty before it reports the kill didn't take
/// (a `SIGKILL` ends a user's processes in milliseconds; missing a generous bound is a fault, not a limit
/// to raise, CLAUDE.md §4). Freeing the uid before it is empty would hand a live process to the next box.
const DRAIN_PATIENCE: Duration = Duration::from_secs(10);

/// One live member of a box's user: its pid and its physical footprint in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Member {
    pub pid: i32,
    pub footprint: u64,
}

/// The box user's live members, each with its footprint, through `proc_listpids(PROC_UID_ONLY)` and
/// `proc_pid_rusage`. Finds every process of the user — including one that daemonized (`setsid`) out of
/// its process group, since a user id is inherited and can't be dropped unprivileged
/// (docs/research/runtime/native-platforms.md §3). A process gone since it was listed (`ESRCH`) or ended
/// and awaiting reap is left out; its footprint would be 0.
pub fn members(uid: u32) -> io::Result<Vec<Member>> {
    let mut pids = Vec::with_capacity(sys::max_processes().unwrap_or(4096));
    sys::user_pids(uid, &mut pids)?;
    let mut out = Vec::with_capacity(pids.len());
    for &pid in &pids {
        if let Some(r) = sys::rusage(pid)?
            && !r.ended
        {
            out.push(Member {
                pid,
                footprint: r.footprint,
            });
        }
    }
    Ok(out)
}

/// Sends `signal` (a Linux signal number, mapped to macOS's as the rest of Hex maps them) to every
/// process of the box's user. Returns how many it signalled. Delete sends `KILL` then waits for the user
/// to empty ([`members`] goes empty) before freeing it; [`hex_core::helper::Request::Signal`] sends the
/// caller's signal.
pub fn kill_user(uid: u32, linux_signal: i32) -> io::Result<usize> {
    let macos = darwin::signal_to_macos(linux_signal)
        .ok_or_else(|| io::Error::other(format!("signal {linux_signal} has no macOS number")))?;
    let sig = rustix::process::Signal::from_named_raw(macos)
        .ok_or_else(|| io::Error::other(format!("{macos} isn't a signal this kernel names")))?;
    let mut pids = Vec::with_capacity(sys::max_processes().unwrap_or(4096));
    sys::user_pids(uid, &mut pids)?;
    let mut signalled = 0usize;
    for &pid in &pids {
        if let Some(p) = rustix::process::Pid::from_raw(pid) {
            match rustix::process::kill_process(p, sig) {
                Ok(()) => signalled = signalled.saturating_add(1),
                // Gone between the listing and the kill: not an error.
                Err(rustix::io::Errno::SRCH) => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    Ok(signalled)
}

/// What the daemon must do to a box's kernel limits after a read of its members: arm some at new ledger
/// limits, or end the box because its tree reached the bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// (Re)arm each `(pid, limit)`: a member new since the last read, or one whose apportioned share
    /// changed. A member whose share is unchanged keeps its current kernel limit and isn't re-armed.
    Arm(Vec<(i32, u64)>),
    /// The members' footprints reached what may be apportioned: end the box (its footprint and bound).
    End { footprint: u64, bound: u64 },
}

/// One box the helper manages: its pool user, its bound and reserve, and the ledger limit each member is
/// armed at now. The daemon apportions the bound across the box's members ([`hex_core::boxuser`]) and
/// arms the kernel limits ([`super::sys`], privileged); this tracks what is armed so a read only re-arms
/// what changed, and keeps the box's peak footprint and how often a member crossed its share.
#[derive(Debug, Clone)]
pub struct Tracked {
    uid: u32,
    bound: Bound,
    /// Each live member's pid and the ledger limit it is armed at.
    armed: BTreeMap<i32, u64>,
    peak: u64,
    crossings: u64,
    ended_over: bool,
}

impl Tracked {
    /// A box on pool user `uid`, bounded at `bound` bytes with `reserve` bytes held back for the fork
    /// arming window (`hex_core::boxuser::Bound`).
    pub fn new(uid: u32, bound: u64, reserve: u64) -> Self {
        Self {
            uid,
            bound: Bound {
                limit: bound,
                reserve,
            },
            armed: BTreeMap::new(),
            peak: 0,
            crossings: 0,
            ended_over: false,
        }
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// Apportions the bound across `members` (their footprints now) and returns what to arm: the members
    /// new or whose share changed. Forgets members gone since the last read. Ends the box if the tree
    /// reached the apportionable bound (HEX-MAC-14).
    pub fn reapportion(&mut self, members: &[Member]) -> Plan {
        let footprint = members
            .iter()
            .fold(0u64, |s, m| s.saturating_add(m.footprint));
        self.peak = self.peak.max(footprint);
        let uses: Vec<Use> = members
            .iter()
            .filter_map(|m| {
                u32::try_from(m.pid).ok().map(|pid| Use {
                    pid,
                    footprint: m.footprint,
                })
            })
            .collect();
        match boxuser::apportion(self.bound, &uses) {
            boxuser::Plan::End { footprint, bound } => {
                self.ended_over = true;
                Plan::End { footprint, bound }
            }
            boxuser::Plan::Arm(shares) => {
                // Forget members gone since the last read, so `armed` tracks only the live ones.
                let live: BTreeSet<i32> = members.iter().map(|m| m.pid).collect();
                self.armed.retain(|pid, _| live.contains(pid));
                let mut to_arm = Vec::new();
                for s in shares {
                    let pid = i32::try_from(s.pid).unwrap_or(-1);
                    if self.armed.get(&pid) != Some(&s.limit) {
                        self.armed.insert(pid, s.limit);
                        to_arm.push((pid, s.limit));
                    }
                }
                Plan::Arm(to_arm)
            }
        }
    }

    /// Records that a member crossed its ledger share (an `EXC_RESOURCE` the helper caught); the next
    /// [`Tracked::reapportion`] decides whether to rebalance or end.
    pub fn crossed(&mut self) {
        self.crossings = self.crossings.saturating_add(1);
    }

    /// The ledger cap, in whole MiB, to arm a just-started member at before the box is re-apportioned: the
    /// whole apportionable bound (a single member may use it all until others claim their shares). Rounded
    /// down, so a sum of such caps never passes the bound.
    pub fn provisional_cap_mib(&self) -> u32 {
        u32::try_from(self.bound.apportionable() >> 20).unwrap_or(u32::MAX)
    }

    /// What the box holds now, for an inspect answer.
    pub fn state(&self, members: &[Member]) -> State {
        let footprint = members
            .iter()
            .fold(0u64, |s, m| s.saturating_add(m.footprint));
        State {
            members: u32::try_from(members.len()).unwrap_or(u32::MAX),
            footprint,
            peak: self.peak.max(footprint),
            processes: u32::try_from(members.len()).unwrap_or(u32::MAX),
            crossings: self.crossings,
            ended_over: self.ended_over,
        }
    }
}

/// The privileged operations the daemon needs root for, behind a seam (CLAUDE.md §9) so the dispatch runs
/// against a simulator in tests and the real syscalls ([`super::helper_sys`], `unsafe`, CLAUDE.md §12)
/// swap in. The membership reads and kills ([`members`], [`kill_user`]) need no privilege and are not here.
pub trait Privileged {
    /// Start `program` as user `uid` (a new session if `leader`), its stdio the three `fds`, its process
    /// count capped by `nproc` (`RLIMIT_NPROC` of the user) and its physical footprint armed at `cap_mib`
    /// with its `EXC_RESOURCE` routed to the daemon. Returns its pid.
    fn spawn(
        &self,
        uid: u32,
        program: &Program,
        fds: [OwnedFd; START_FDS],
        leader: bool,
        cap_mib: u32,
        nproc: u32,
    ) -> io::Result<i32>;

    /// Re-arm a running member's footprint at `cap_mib` (a forked member, or a rebalance of its share).
    fn arm(&self, pid: i32, cap_mib: u32) -> io::Result<()>;

    /// The box user's live members (needs no privilege; defaults to [`members`]).
    fn list(&self, uid: u32) -> io::Result<Vec<Member>> {
        members(uid)
    }

    /// Signal every process of the box user (needs no privilege when the node owns the pool; defaults to
    /// [`kill_user`]). Returns how many were signalled.
    fn kill(&self, uid: u32, signal: i32) -> io::Result<usize> {
        kill_user(uid, signal)
    }
}

/// The macOS box helper daemon over a [`Privileged`] backend: a pool of box users, the boxes it manages,
/// and the node's connection. A box's process count is `RLIMIT_NPROC` of its user (`nproc`), set when its
/// leader starts.
pub struct Daemon<P: Privileged> {
    ops: P,
    /// The free pool users the installer made (native-platforms.md §3); a box takes one on acquire and
    /// returns it on delete once its user is empty.
    pool: Mutex<Vec<u32>>,
    boxes: Mutex<BTreeMap<BoxId, Tracked>>,
    nproc: u32,
}

impl<P: Privileged> Daemon<P> {
    /// A daemon over `ops`, with the free `pool` users and a per-user process cap `nproc`.
    pub fn new(ops: P, pool: Vec<u32>, nproc: u32) -> Self {
        Self {
            ops,
            pool: Mutex::new(pool),
            boxes: Mutex::new(BTreeMap::new()),
            nproc,
        }
    }

    /// Serves one node connection until it closes: reads each framed request and answers it. A request it
    /// can't read is refused by a typed answer (CLAUDE.md §3); the connection closing (a read of nothing)
    /// ends the loop. The caller ends the boxes this connection held (the node's disconnect rule).
    pub fn serve(&self, fd: BorrowedFd<'_>) -> Result<(), Refusal> {
        loop {
            let (bytes, fds) = match recv_framed(fd) {
                Ok(m) => m,
                // The node closed (or the link broke): stop serving it.
                Err(_) => return Ok(()),
            };
            let (version, id, answer) = match helper::read_request(&bytes, fds.len()) {
                Ok((version, id, request)) => (version, id, self.handle(request, fds)),
                // Malformed: refuse it by name, in the first version (its own may be unreadable).
                Err(fault) => (
                    helper::FIRST,
                    0,
                    Answer::Refused(Refused::Malformed(fault.to_string())),
                ),
            };
            let out = helper::encode_answer(version, id, &answer)
                .map_err(|e| Refusal::Broken(format!("encoding a helper answer: {e}")))?;
            send_framed(fd, &out, &[])?;
        }
    }

    fn handle(&self, request: Request, fds: Vec<OwnedFd>) -> Answer {
        match request {
            Request::Hello { .. } => Answer::Hello {
                version: helper::VERSION,
            },
            Request::Acquire {
                box_id,
                bound,
                reserve,
            } => self.acquire(box_id, bound, reserve),
            Request::Start {
                box_id,
                program,
                leader,
            } => self.start(&box_id, &program, fds, leader),
            Request::End { box_id } => self.end(&box_id),
            Request::Inspect { box_id } => self.inspect(&box_id),
            Request::Signal { box_id, signal } => self.signal(&box_id, signal),
        }
    }

    fn boxes(&self) -> Result<std::sync::MutexGuard<'_, BTreeMap<BoxId, Tracked>>, Answer> {
        self.boxes
            .lock()
            .map_err(|_| poisoned("the daemon's boxes"))
    }

    fn acquire(&self, box_id: BoxId, bound: u64, reserve: u64) -> Answer {
        let mut boxes = match self.boxes() {
            Ok(b) => b,
            Err(a) => return a,
        };
        if boxes.contains_key(&box_id) {
            return Answer::Refused(Refused::AlreadyAcquired(box_id));
        }
        let Ok(mut pool) = self.pool.lock() else {
            return poisoned("the daemon's pool");
        };
        let Some(uid) = pool.pop() else {
            return Answer::Refused(Refused::NoFreeUser);
        };
        drop(pool);
        boxes.insert(box_id, Tracked::new(uid, bound, reserve));
        Answer::Acquired { uid }
    }

    fn start(&self, box_id: &BoxId, program: &Program, fds: Vec<OwnedFd>, leader: bool) -> Answer {
        let Ok([stdin, stdout, stderr]) = <[OwnedFd; START_FDS]>::try_from(fds) else {
            return Answer::Refused(Refused::Malformed(
                "a start without its three stdio descriptors".to_owned(),
            ));
        };
        let (uid, cap_mib) = {
            let boxes = match self.boxes() {
                Ok(b) => b,
                Err(a) => return a,
            };
            match boxes.get(box_id) {
                Some(t) => (t.uid(), t.provisional_cap_mib()),
                None => return Answer::Refused(Refused::UnknownBox(box_id.clone())),
            }
        };
        // Spawn generously at the whole apportionable bound (the only member may use it all), then
        // re-apportion against the live membership and arm each member down to its share.
        let pid = match self.ops.spawn(
            uid,
            program,
            [stdin, stdout, stderr],
            leader,
            cap_mib,
            self.nproc,
        ) {
            Ok(pid) => pid,
            Err(e) => return Answer::Refused(Refused::StartFailed(e.to_string())),
        };
        if let Err(a) = self.rearm(box_id, uid) {
            return a;
        }
        Answer::Started { pid }
    }

    /// Re-apportions a box against its live members and arms each one whose share changed; ends the box if
    /// its tree reached the bound. Called after a member starts (and, with the exception server, on a
    /// crossing).
    fn rearm(&self, box_id: &BoxId, uid: u32) -> Result<(), Answer> {
        let live = self
            .ops
            .list(uid)
            .map_err(|e| os("reading a box's members", &e))?;
        let plan = {
            let mut boxes = self.boxes()?;
            let Some(tracked) = boxes.get_mut(box_id) else {
                return Ok(());
            };
            tracked.reapportion(&live)
        };
        match plan {
            Plan::Arm(to_arm) => {
                for (pid, limit) in to_arm {
                    let mib = u32::try_from(limit >> 20).unwrap_or(u32::MAX);
                    if let Err(e) = self.ops.arm(pid, mib) {
                        return Err(os("arming a box member", &e));
                    }
                }
                Ok(())
            }
            // The tree reached the bound: end the box. The caller's answer still reports the start that
            // tipped it; the next read finds the box gone.
            Plan::End { .. } => {
                let _ = self.end(box_id);
                Ok(())
            }
        }
    }

    fn end(&self, box_id: &BoxId) -> Answer {
        let uid = {
            let mut boxes = match self.boxes() {
                Ok(b) => b,
                Err(a) => return a,
            };
            match boxes.remove(box_id) {
                Some(t) => t.uid(),
                None => return Answer::Refused(Refused::UnknownBox(box_id.clone())),
            }
        };
        // Kill every process of the user, then wait for the user to empty before freeing it, so the next
        // box never inherits a live process. KILL is 9; mapped through the table like the rest of Hex.
        if let Err(e) = self.ops.kill(uid, Signal::KILL.number()) {
            return os("ending a box", &e);
        }
        let deadline = Instant::now().checked_add(DRAIN_PATIENCE);
        loop {
            match self.ops.list(uid).map(|m| m.is_empty()) {
                Ok(true) => break,
                Ok(false) => {
                    if deadline.is_none_or(|d| Instant::now() >= d) {
                        return os(
                            "emptying a box's user",
                            &io::Error::other(format!("user {uid} still holds a live process")),
                        );
                    }
                    let _ = self.ops.kill(uid, Signal::KILL.number());
                    std::hint::spin_loop();
                }
                Err(e) => return os("checking a box's user is empty", &e),
            }
        }
        if let Ok(mut pool) = self.pool.lock() {
            pool.push(uid);
        }
        Answer::Ended
    }

    fn inspect(&self, box_id: &BoxId) -> Answer {
        let uid = {
            let boxes = match self.boxes() {
                Ok(b) => b,
                Err(a) => return a,
            };
            match boxes.get(box_id) {
                Some(t) => t.uid(),
                None => return Answer::Refused(Refused::UnknownBox(box_id.clone())),
            }
        };
        let live = match self.ops.list(uid) {
            Ok(m) => m,
            Err(e) => return os("reading a box's members", &e),
        };
        let mut boxes = match self.boxes() {
            Ok(b) => b,
            Err(a) => return a,
        };
        match boxes.get_mut(box_id) {
            Some(tracked) => Answer::State(tracked.state(&live)),
            None => Answer::Refused(Refused::UnknownBox(box_id.clone())),
        }
    }

    fn signal(&self, box_id: &BoxId, signal: i32) -> Answer {
        let uid = {
            let boxes = match self.boxes() {
                Ok(b) => b,
                Err(a) => return a,
            };
            match boxes.get(box_id) {
                Some(t) => t.uid(),
                None => return Answer::Refused(Refused::UnknownBox(box_id.clone())),
            }
        };
        match self.ops.kill(uid, signal) {
            Ok(_) => Answer::Signalled,
            Err(e) => os("signalling a box", &e),
        }
    }
}

fn poisoned(what: &str) -> Answer {
    Answer::Refused(Refused::Malformed(format!("{what} lock is poisoned")))
}

fn os(op: &str, e: &io::Error) -> Answer {
    Answer::Refused(Refused::StartFailed(format!("{op}: {e}")))
}

#[cfg(test)]
mod tests;
