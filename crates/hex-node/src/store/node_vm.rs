//! Node VM rows (joint plan step 39): each node VM the node runs for itself keeps its record ([`NodeVmRecord`]) in a
//! directory of its own, `.node-vms`, apart from box rows and pool rows, so no box listing reads one by construction
//! (ROOT's ruling of 2026-11-13). Behavior kept here: none is kept.

use hex_api::{NodeRun, Refusal};
use hex_core::record::NodeVmRecord;

use super::Store;

impl Store {
    /// Keeps `record`, its run's row replaced whole. Behavior kept here: nothing is written.
    pub(crate) fn put_node_vm(&self, record: &NodeVmRecord) -> Result<(), Refusal> {
        record.check()
    }

    /// Node VM `run`'s record, `None` where it keeps none. Behavior kept here: none.
    pub(crate) fn node_vm(&self, run: &NodeRun) -> Result<Option<NodeVmRecord>, Refusal> {
        let _ = run;
        Ok(None)
    }

    /// Lets node VM `run`'s record go. Behavior kept here: there is none.
    pub(crate) fn remove_node_vm(&self, run: &NodeRun) -> Result<(), Refusal> {
        let _ = run;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
