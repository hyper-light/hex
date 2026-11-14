//! Node VM rows (joint plan step 39): each node VM the node runs for itself keeps its record ([`NodeVmRecord`]) in a
//! directory of its own, `.node-vms`, one row a run, named by its run's hex, apart from box rows and pool rows, so no
//! box listing reads one by construction (ROOT's ruling of 2026-11-13). Each is published durably as a pool row is
//! (`Store::publish_durable`), under the store's metadata turn, at most [`TURNS`] of them, a run's first row past that
//! refused as a pool's first is.
//!
//! A row's run is its owner's, the node process that wrote it Booting, whose lock on the run's lock file
//! (`<run>.lock`, beside its row) it holds from that write until it writes the row Ended (ROOT, 2026-11-13). Shards
//! ends a run with its owner (D231, shards-api's `image`: "The run ends with its ImageGuest: dropped, stopped, or its
//! channel gone, the VM goes, so no microVM outlives its owner"), and its ImageGuest names no run host's process, so a
//! row whose lock no process holds names a run that has ended: its owner's, or the host's boot. The lock is its owner's
//! alone:
//! - on Unix a POSIX record lock, fcntl(2)'s `F_SETLK` (rustix's `fcntl_lock`), never an open file description's
//!   (`flock(2)`, Linux's `F_OFD_SETLK`): "Record locks are not inherited by a child created via fork(2)" (fcntl(2)),
//!   so no child of the node keeps it, even one forked without an exec, which shares the description; and the kernel
//!   lets it go as its process ends. It goes too as its process closes any descriptor of its file, so the node opens a
//!   run's lock file once, keeps that one handle while it holds the lock, and opens it again never;
//! - on Windows LockFileEx (std's `File::lock`), on a handle no child inherits: std opens none inheritable, and Hex's
//!   CreateProcessW hands its programs their three standard handles alone.
//!
//! As a node starts, it settles the rows an earlier node left ([`Store::settle_node_vms`]): a row whose run ended, or
//! whose lock it can take, is let go with its lock file and its diagnostics; one whose owner still runs stays, its
//! receipt booked, and a thread follows that owner to its end, then lets it go.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::Instant;

use hex_api::{LearnedEnd, NodeRun, NodeVmPhase, Refusal};
use hex_core::patience::Patience;
use hex_core::record::NodeVmRecord;

use super::{Store, TURNS, scoped_paths};

/// `store.node_vm_owner`, W*: the slowest end of a node VM's owner this process has followed, from its start's look at
/// its row (decision 9): past it the run-out is told, and the wait follows the owner by its life to its end.
static OWNER_END: Patience = Patience::measuring("store.node_vm_owner");

/// What a node's start did with a node VM row an earlier node process left (joint plan step 39).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeVmSettled {
    /// Its run had ended, its end kept: its row let go.
    Ended,
    /// No process held its lock, so its owner had ended, or the host's boot had, and its run with it: its row let go.
    OwnerEnded,
    /// Its owner still runs: its receipt stays booked, and a thread follows that owner to its end.
    Followed,
}

impl std::fmt::Display for NodeVmSettled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Ended => "its run had ended, its end kept: its row let go",
            Self::OwnerEnded => {
                "no process held its lock, so its owner had ended, or the host's boot had, and its run with it \
                 (D231: no microVM outlives its owner): its row let go"
            }
            Self::Followed => {
                "its owner, an earlier node process, still runs: its receipt stays booked, and the node follows \
                 that process to its end"
            }
        })
    }
}

/// What a node's start did with the node VM rows an earlier node process left (joint plan step 39).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeVmsSettled {
    /// Each row by its run, with what the start did with it.
    pub rows: Vec<(NodeRun, NodeVmSettled)>,
    /// The last-resort ends the let-go rows kept (C5), each read back before its row went: an Ended row's, and one
    /// whose owner had ended.
    pub ends: Vec<LearnedEnd>,
}

/// What a node's start carried of its node VMs' last-resort ends into its charge learner (joint plan step 39, place 1;
/// ROOT, 2026-11-13): none on record is its cold start, told, never a guess.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NodeVmEnds {
    /// No node VM row it let go kept a last-resort end: the learner's node VM ends begin empty.
    #[default]
    NoneOnRecord,
    /// These, each read back from its row before the row went, in its runs' order.
    Carried(Vec<LearnedEnd>),
}

impl NodeVmEnds {
    /// What `ends`, read back from the let-go rows, tell: none, a cold start; else each.
    pub fn of(ends: &[LearnedEnd]) -> Result<Self, Refusal> {
        if ends.is_empty() {
            return Ok(Self::NoneOnRecord);
        }
        let mut carried = Vec::new();
        carried
            .try_reserve_exact(ends.len())
            .map_err(|_| allocation(ends.len()))?;
        for end in ends {
            carried.push(end.try_clone()?);
        }
        Ok(Self::Carried(carried))
    }
}

impl std::fmt::Display for NodeVmEnds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoneOnRecord => f.write_str("node VM: no last-resort end on record"),
            Self::Carried(ends) => write!(
                f,
                "node VM: {} last-resort end(s) on record, carried into the charge learner's ends",
                ends.len()
            ),
        }
    }
}

fn allocation(bound: usize) -> Refusal {
    Refusal::Exhausted {
        what: "node VM record storage",
        bound,
    }
}

fn damaged(why: &str) -> Refusal {
    Refusal::Invalid {
        what: "a node VM's row",
        why: why.to_owned(),
    }
}

/// Run `run`'s row's name: its 16 bytes in lowercase hex.
fn name(run: &NodeRun) -> Result<String, Refusal> {
    let bytes = run.bytes();
    let count = bytes
        .len()
        .checked_mul(2)
        .ok_or_else(|| allocation(usize::MAX))?;
    let mut name = String::new();
    name.try_reserve_exact(count)
        .map_err(|_| allocation(count))?;
    for byte in bytes {
        for digit in [byte >> 4, byte & 15] {
            let digit = b"0123456789abcdef"
                .get(usize::from(digit))
                .copied()
                .ok_or_else(|| allocation(count))?;
            name.push(char::from(digit));
        }
    }
    Ok(name)
}

/// The run a row's file is named for: its 16 bytes in lowercase hex, then `.json`. None for any other name.
fn run_of(name: &std::ffi::OsStr) -> Option<NodeRun> {
    let text = name.to_str()?.strip_suffix(".json")?;
    let (pairs, tail) = text.as_bytes().as_chunks::<2>();
    if text.len() != 32 || !tail.is_empty() {
        return None;
    }
    let mut run = [0; 16];
    for (byte, pair) in run.iter_mut().zip(pairs) {
        let pair = std::str::from_utf8(pair).ok()?;
        if !pair
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
        {
            return None;
        }
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(NodeRun::from_bytes(run))
}

/// Run `run`'s lock file in `directory`: its row's name, `.lock` for `.json`.
fn lock_path(directory: &Path, run: &NodeRun) -> Result<PathBuf, Refusal> {
    let (_, row) = scoped_paths(directory, &name(run)?)?;
    Ok(row.with_extension("lock"))
}

/// Run `run`'s lock file, made where missing: a handle the lock is taken on, never truncating what it opens.
fn open_lock(directory: &Path, run: &NodeRun) -> Result<File, Refusal> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path(directory, run)?)
        .map_err(|e| Refusal::os("opening a node VM's lock file", e))
}

/// Takes `file`'s owner-only lock where no process holds it: whether it did. POSIX lets a lock another process holds
/// answer `F_SETLK` with either EACCES or EAGAIN (fcntl(2)).
#[cfg(unix)]
fn try_own(file: &File) -> Result<bool, Refusal> {
    match rustix::fs::fcntl_lock(file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(true),
        Err(rustix::io::Errno::ACCESS | rustix::io::Errno::AGAIN) => Ok(false),
        Err(e) => Err(Refusal::os("taking a node VM's lock", e)),
    }
}

#[cfg(windows)]
fn try_own(file: &File) -> Result<bool, Refusal> {
    match file.try_lock() {
        Ok(()) => Ok(true),
        Err(std::fs::TryLockError::WouldBlock) => Ok(false),
        Err(std::fs::TryLockError::Error(e)) => Err(Refusal::os("taking a node VM's lock", e)),
    }
}

/// Takes `file`'s owner-only lock once the process that holds it lets it go, as it ends: `F_SETLKW`, which waits on
/// the holder alone. A signal's interruption is no answer, so the wait goes on, following its holder's life.
#[cfg(unix)]
fn own(file: &File) -> Result<(), Refusal> {
    loop {
        match rustix::fs::fcntl_lock(file, rustix::fs::FlockOperation::LockExclusive) {
            Ok(()) => return Ok(()),
            Err(rustix::io::Errno::INTR) => {}
            Err(e) => return Err(Refusal::os("following a node VM's owner", e)),
        }
    }
}

#[cfg(windows)]
fn own(file: &File) -> Result<(), Refusal> {
    file.lock()
        .map_err(|e| Refusal::os("following a node VM's owner", e))
}

/// Takes `file`'s lock once its owner ends, as decision 9 waits: within W* ([`OWNER_END`]) the lock is the answer;
/// past it the run-out is told, and the wait follows the owner by its life, its thread's blocked lock answering only as
/// the owner ends. Refused where that thread ends with no answer.
fn follow_owner(file: File) -> Result<File, Refusal> {
    let began = Instant::now();
    let (said, answer) = hex_wait::mpsc::sync_channel(1)
        .map_err(|e| Refusal::os("making a node VM owner's follow", e))?;
    std::thread::Builder::new()
        .name("hex-node-vm-lock".to_owned())
        .spawn(move || {
            let _ = said.try_send(own(&file).map(|()| file));
        })
        .map_err(|e| Refusal::os("starting a node VM owner's follow", e))?;
    let got = match OWNER_END.deadline() {
        Some(bound) => match answer.recv_timeout(bound) {
            Err(hex_wait::mpsc::RecvTimeoutError::Timeout) => {
                OWNER_END.ran_out(bound);
                answer
                    .recv()
                    .map_err(hex_wait::mpsc::RecvTimeoutError::from)
            }
            got => got,
        },
        None => answer
            .recv()
            .map_err(hex_wait::mpsc::RecvTimeoutError::from),
    };
    match got {
        Ok(Ok(file)) => {
            OWNER_END.answered(began.elapsed());
            Ok(file)
        }
        Ok(Err(why)) => {
            OWNER_END.refused(began.elapsed());
            Err(why)
        }
        Err(hex_wait::mpsc::RecvTimeoutError::Failed(why)) => {
            Err(Refusal::os("waiting for a node VM owner's end", why))
        }
        // Disconnected: its lock's thread ended with no answer (a Timeout comes from the bounded wait alone).
        Err(_) => Err(Refusal::Broken(
            "the thread following a node VM's owner ended with no answer".to_owned(),
        )),
    }
}

/// Whether `path` names a file.
fn path_exists(path: &Path) -> Result<bool, Refusal> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(Refusal::os("looking for a node VM's row", e)),
    }
}

/// Removes `path`, `what`, where it is.
fn remove_if_there(path: &Path, what: &'static str) -> Result<(), Refusal> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Refusal::Os {
            op: "removing a node VM's file",
            why: format!("{what}: {e}"),
        }),
    }
}

fn poisoned() -> Refusal {
    Refusal::Broken("the node VM owners' lock is poisoned".to_owned())
}

fn read_row(file: File, run: &NodeRun) -> Result<NodeVmRecord, Refusal> {
    let record = NodeVmRecord::decode(&super::read_record_bytes(file)?)?;
    if record.run != *run {
        return Err(damaged("its row names another run"));
    }
    Ok(record)
}

impl Store {
    fn node_vm_directory(&self) -> PathBuf {
        self.disk.join(".node-vms")
    }

    /// Keeps `record`, its run's row replaced whole but for its last-resort end and its start place, published durably.
    /// A run's first row is refused past [`TURNS`] rows, and is given the next place in the node's start order (C3), which
    /// every later write keeps. A Booting or Ready row of a run this process doesn't hold yet takes the run's lock first,
    /// so no live run's row is ever written unlocked; an Ended row lets the lock go once written, its run its owner's no
    /// more. The end is its learner's to write ([`Store::update_node_vm`]): a phase write, made from a read before that
    /// write, keeps the row's (C5, intent first).
    pub(crate) fn put_node_vm(&self, record: &NodeVmRecord) -> Result<(), Refusal> {
        record.check()?;
        let _metadata = self.changing_metadata()?;
        let directory = self.node_vm_directory();
        fs::create_dir_all(&directory)
            .map_err(|e| Refusal::os("making the node VM records' directory", e))?;
        let (temporary, final_path) = scoped_paths(&directory, &name(&record.run)?)?;
        if !path_exists(&final_path)? && self.all_node_vms()?.len() >= TURNS {
            return Err(allocation(TURNS));
        }
        let mut owners = self.node_vm_owners.lock().map_err(|_| poisoned())?;
        let (last_resort, started) = match self.node_vm(&record.run)? {
            Some(row) => (row.last_resort, row.started),
            None => (None, Some(self.next_start()?)),
        };
        let bytes = NodeVmRecord {
            last_resort,
            started,
            ..record.clone()
        }
        .encode()?;
        let taken = if record.phase != NodeVmPhase::Ended && !owners.contains_key(&record.run) {
            let file = open_lock(&directory, &record.run)?;
            if !try_own(&file)? {
                return Err(damaged(
                    "another process holds the lock of a run this node writes Booting",
                ));
            }
            Some(file)
        } else {
            None
        };
        if let Err(why) = self.publish_durable(&directory, &temporary, &final_path, &bytes) {
            // Its row unwritten, a lock just taken holds nothing: its file goes, and the lock with its handle.
            if let Some(file) = taken {
                remove_if_there(
                    &lock_path(&directory, &record.run)?,
                    "a node VM's lock file",
                )?;
                drop(file);
            }
            return Err(why);
        }
        if let Some(file) = taken {
            owners.insert(record.run, file);
        }
        if record.phase == NodeVmPhase::Ended {
            owners.remove(&record.run);
        }
        Ok(())
    }

    /// Changes run `run`'s row by `change` where it keeps one and `change` says it changed it, published durably: read
    /// and written under the lock every node VM row's write takes, so no other write lands between its read and its
    /// write. The learner's write of why its last resort ends the VM (C5, intent first). Whether it changed the row.
    pub(crate) fn update_node_vm(
        &self,
        run: &NodeRun,
        change: impl FnOnce(&mut NodeVmRecord) -> bool,
    ) -> Result<bool, Refusal> {
        let _metadata = self.changing_metadata()?;
        let directory = self.node_vm_directory();
        let (temporary, final_path) = scoped_paths(&directory, &name(run)?)?;
        let _owners = self.node_vm_owners.lock().map_err(|_| poisoned())?;
        let Some(mut record) = self.node_vm(run)? else {
            return Ok(false);
        };
        if !change(&mut record) {
            return Ok(false);
        }
        self.publish_durable(&directory, &temporary, &final_path, &record.encode()?)?;
        Ok(true)
    }

    /// Node VM `run`'s record, `None` where it keeps none.
    pub(crate) fn node_vm(&self, run: &NodeRun) -> Result<Option<NodeVmRecord>, Refusal> {
        let (_, path) = scoped_paths(&self.node_vm_directory(), &name(run)?)?;
        match File::open(path) {
            Ok(file) => read_row(file, run).map(Some),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Refusal::os("opening a node VM's record", e)),
        }
    }

    /// Every node VM's record it keeps, in its runs' order: their receipts' one source (joint plan step 39). Within the
    /// store's scan bound for a kind of row, as its pool rows are: a row past it, or a record file whose name is no
    /// run's, is refused, never passed over.
    pub(crate) fn all_node_vms(&self) -> Result<Vec<NodeVmRecord>, Refusal> {
        let directory = self.node_vm_directory();
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Refusal::os("listing the node VM records", e)),
        };
        let bound = TURNS.checked_mul(3).ok_or_else(|| allocation(usize::MAX))?;
        let mut records = Vec::new();
        for (seen, entry) in entries.enumerate() {
            if seen >= bound {
                return Err(allocation(bound));
            }
            let entry = entry.map_err(|e| Refusal::os("listing a node VM record", e))?;
            let filename = entry.file_name();
            let Some(run) = run_of(&filename) else {
                if super::retained_json_name(&filename) {
                    return Err(damaged("a row whose name is no run's"));
                }
                continue;
            };
            let file = File::open(entry.path())
                .map_err(|e| Refusal::os("opening a node VM's record", e))?;
            let record = read_row(file, &run)?;
            records.try_reserve(1).map_err(|_| allocation(bound))?;
            records.push(record);
        }
        records.sort_unstable_by_key(|record| record.run);
        Ok(records)
    }

    /// A run for a node VM about to be booked: 16 bytes from the OS's random numbers, which no row it keeps names (a
    /// run met twice is refused, never drawn again: 2^-128 apart).
    pub(crate) fn mint_node_run(&self) -> Result<NodeRun, Refusal> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes)
            .map_err(|error| Refusal::os("minting a node VM's run", error))?;
        let run = NodeRun::from_bytes(bytes);
        if self.node_vm(&run)?.is_some() {
            return Err(damaged("a run minted names a row already kept"));
        }
        Ok(run)
    }

    /// Settles each node VM row a node process before this one left, as this node starts (joint plan step 39; ROOT,
    /// 2026-11-13), so none is booked for good: an Ended row is let go; one whose lock this process can take is let go,
    /// its owner having ended, or the host's boot having, and its run with it (D231); one whose owner still runs stays,
    /// its receipt booked, and a thread follows that owner to its end (decision 9, no timer), then lets it go. A row
    /// this process holds is its own, left as it is, its lock file never opened again. A follow is two threads, its
    /// lock's and its W*'s, one follow a row, so at most [`TURNS`] of them. Says what it did with each, and the last-resort
    /// end each row it let go kept, read back before the row went (place 1).
    pub(crate) fn settle_node_vms(&self) -> Result<NodeVmsSettled, Refusal> {
        let records = self.all_node_vms()?;
        let mut settled = Vec::new();
        settled
            .try_reserve_exact(records.len())
            .map_err(|_| allocation(records.len()))?;
        let mut ends = Vec::new();
        ends.try_reserve_exact(records.len())
            .map_err(|_| allocation(records.len()))?;
        let directory = self.node_vm_directory();
        for mut record in records {
            if record.phase == NodeVmPhase::Ended {
                let end = record.last_resort.take();
                self.let_go(&record)?;
                settled.push((record.run, NodeVmSettled::Ended));
                ends.extend(end);
                continue;
            }
            if self
                .node_vm_owners
                .lock()
                .map_err(|_| poisoned())?
                .contains_key(&record.run)
            {
                continue;
            }
            let file = open_lock(&directory, &record.run)?;
            if try_own(&file)? {
                let end = record.last_resort.take();
                self.let_go(&record)?;
                settled.push((record.run, NodeVmSettled::OwnerEnded));
                ends.extend(end);
            } else {
                self.follow(record.run, file)?;
                settled.push((record.run, NodeVmSettled::Followed));
            }
        }
        Ok(NodeVmsSettled {
            rows: settled,
            ends,
        })
    }

    /// Follows run `run`'s owner, a live node process, to its end on a thread of its own, its row booked meanwhile, then
    /// lets its row go. Where the follow is refused, the row stays booked until the node's next start, and why is said
    /// on `hex.guest`, the node VM being the node's guest, as the event `node_vm_unfollowed`.
    fn follow(&self, run: NodeRun, file: File) -> Result<(), Refusal> {
        let store = self.clone();
        std::thread::Builder::new()
            .name("hex-node-vm-owner".to_owned())
            .spawn(move || {
                let ended = follow_owner(file).and_then(|file| {
                    let row = store.node_vm(&run)?;
                    let let_go = row.map_or(Ok(()), |row| store.let_go(&row));
                    drop(file);
                    let_go
                });
                if let Err(why) = ended {
                    use std::io::Write as _;
                    let _ = std::io::stderr().lock().write_all(
                        format!(
                            "hex.guest event=node_vm_unfollowed run={} why=\"{why}\"\n",
                            name(&run).unwrap_or_default()
                        )
                        .as_bytes(),
                    );
                }
            })
            .map_err(|e| Refusal::os("starting a node VM owner's follow", e))?;
        Ok(())
    }

    /// Lets node VM `record`'s row go, its run ended: its diagnostics and its lock file first, its row last, so a crash
    /// between leaves the row for the next start to let go again.
    fn let_go(&self, record: &NodeVmRecord) -> Result<(), Refusal> {
        if let Some(diagnostics) = &record.diagnostics {
            remove_if_there(diagnostics, "a node VM's diagnostics")?;
        }
        remove_if_there(
            &lock_path(&self.node_vm_directory(), &record.run)?,
            "a node VM's lock file",
        )?;
        self.remove_node_vm(&record.run)
    }

    /// The handle of node VM `run`'s lock this process holds, a second descriptor of its one open file description:
    /// for a test's child to share it as a forked child would, Unix's case.
    #[cfg(all(test, unix))]
    fn node_vm_lock(&self, run: &NodeRun) -> Option<File> {
        self.node_vm_owners
            .lock()
            .ok()?
            .get(run)
            .and_then(|file| file.try_clone().ok())
    }

    /// Lets node VM `run`'s record go, its directory flushed after.
    fn remove_node_vm(&self, run: &NodeRun) -> Result<(), Refusal> {
        let _metadata = self.changing_metadata()?;
        let directory = self.node_vm_directory();
        let (_, path) = scoped_paths(&directory, &name(run)?)?;
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(Refusal::os("removing a node VM's record", e)),
        }
        self.flush_pool_directory(&directory)
    }
}

#[cfg(test)]
mod tests;
