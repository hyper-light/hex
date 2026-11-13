//! Node VM rows (joint plan step 39): each node VM the node runs for itself keeps its record ([`NodeVmRecord`]) in a
//! directory of its own, `.node-vms`, one row a run, named by its run's hex, apart from box rows and pool rows, so no
//! box listing reads one by construction (ROOT's ruling of 2026-11-13). Each is published durably as a pool row is
//! (`Store::publish_durable`), under the store's metadata turn.

use std::fs::{self, File};
use std::path::PathBuf;

use hex_api::{NodeRun, Refusal};
use hex_core::record::NodeVmRecord;

use super::{Store, scoped_paths};

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
