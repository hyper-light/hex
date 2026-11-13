//! Node VM rows (joint plan step 39): each node VM the node runs for itself keeps its record ([`NodeVmRecord`]) in a
//! directory of its own, `.node-vms`, one row a run, named by its run's hex, apart from box rows and pool rows, so no
//! box listing reads one by construction (ROOT's ruling of 2026-11-13). Each is published durably as a pool row is
//! (`Store::publish_durable`), under the store's metadata turn.

use std::fs::{self, File};
use std::path::PathBuf;

use hex_api::{NodeRun, Refusal};
use hex_core::record::NodeVmRecord;

use super::{Store, TURNS, scoped_paths};

/// What a node's start did with a node VM row an earlier node process left (joint plan step 39).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeVmSettled {
    /// Its run had ended, its end kept: its row let go.
    Ended,
    /// No process held its lock, so its owner had ended, or the host's boot had, and its run with it: its row let go.
    OwnerEnded,
    /// Its owner still runs: its receipt stays booked, and a thread follows that owner to its end.
    Followed,
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

    /// Keeps `record`, its run's row replaced whole, published durably.
    pub(crate) fn put_node_vm(&self, record: &NodeVmRecord) -> Result<(), Refusal> {
        let bytes = record.encode()?;
        let _metadata = self.changing_metadata()?;
        let directory = self.node_vm_directory();
        fs::create_dir_all(&directory)
            .map_err(|e| Refusal::os("making the node VM records' directory", e))?;
        let (temporary, final_path) = scoped_paths(&directory, &name(&record.run)?)?;
        self.publish_durable(&directory, &temporary, &final_path, &bytes)
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
    /// 2026-11-13): says what it did with each. Original (red): nothing.
    pub(crate) fn settle_node_vms(&self) -> Result<Vec<(NodeRun, NodeVmSettled)>, Refusal> {
        Ok(Vec::new())
    }

    /// The handle of node VM `run`'s lock this process holds, a second descriptor of its one open file description:
    /// for a test's child to share it as a forked child would. Original (red): none is held.
    #[cfg(test)]
    fn node_vm_lock(&self, run: &NodeRun) -> Option<File> {
        let _ = run;
        None
    }

    /// Lets node VM `run`'s record go, its directory flushed after.
    pub(crate) fn remove_node_vm(&self, run: &NodeRun) -> Result<(), Refusal> {
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
