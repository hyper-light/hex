//! A node's credential leases over its wire (joint plan step 33; docs/design/credential-grants.md C7): not built yet,
//! so every run of a box granting a credential is refused its lessor here.

use std::sync::Arc;

use hex_api::lease::{Lessor, Lessors};
use hex_api::{BoxId, Refusal};

/// The node's lessors over its wire: none built yet.
pub struct WireLessors;

impl WireLessors {
    pub fn new() -> Arc<Self> {
        Arc::new(Self)
    }
}

impl Lessors for WireLessors {
    fn lessor(&self, id: &BoxId) -> Result<Arc<dyn Lessor>, Refusal> {
        Err(Refusal::Unconfigured {
            what: "lessors for its image VMs' credentials",
            why: format!("box {id}: this node leases over its wire for no connection yet"),
        })
    }
}
