//! A runtime of the Shards class on Agentfile images alone (joint plan step 12's item B; kiln's step 17, part 7: launch
//! through Hex): no guest of Hex's, no PID 1 and no guest configuration, which stay for every other host. Its boxes, each
//! a Shards image VM booted under shards-init (D231), are the image class's ([`ImageClass`]); their run hosts the
//! `shards` binary given, with the Shards home and cache given; their records a node Store of its own under the
//! directory given, changed one at a time; and each run host's diagnostics a file beside them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, HostUse, Member, Proc, Program, Refusal, Runtime, Signal,
    Stdio, Usage,
};
use hex_core::record::{BoxRecord, PoolRecord};
use hex_runtime::devices::{Books, PoolPreparationCheck, PoolRecords, Records, Transaction};
use hex_runtime::guest::{ImageClass, ImageWaits, host_images};
use shards_api::image::Images;

use crate::store::Store;

/// The Shards class on Agentfile images as a runtime of its own: its record authority this runtime's own Store.
pub struct ImageRuntime {
    store: Store,
    /// A box's record is read and changed by one transaction at a time.
    turn: Mutex<()>,
    class: ImageClass,
}

impl ImageRuntime {
    /// The class whose run hosts are the `shards` binary `shards` (built at Hex's vendored Shards revision, so it speaks
    /// the image channel Hex's `shards_api` does), their Shards home and cache `paths`, whose store holds the images,
    /// its boxes' records under `records` (`records/state` and `records/run`, made where missing), and each run host's
    /// diagnostics in `records/image-diagnostics`. Shards' image boot as every host runs it ([`host_images`]), its waits
    /// learned from their own answers (decision 9).
    pub fn new(
        shards: PathBuf,
        paths: shards_api::RuntimePaths,
        records: &Path,
    ) -> Result<Arc<Self>, Refusal> {
        Self::with_images(shards, paths, records, host_images(), Ok(ImageWaits::new()))
    }

    /// [`ImageRuntime::new`] over the image boot `images` and the waits `waits` given.
    pub fn with_images(
        shards: PathBuf,
        paths: shards_api::RuntimePaths,
        records: &Path,
        images: Result<Arc<dyn Images>, Refusal>,
        waits: Result<ImageWaits, Refusal>,
    ) -> Result<Arc<Self>, Refusal> {
        let _ = (shards, paths, records, images, waits);
        Err(Refusal::Platform {
            what: "Shards image VMs",
            platform: std::env::consts::OS,
            why: "no runtime of the image class alone is built yet".to_owned(),
        })
    }
}

impl Books for ImageRuntime {
    fn records(&self) -> Result<Records, Refusal> {
        Err(unbuilt())
    }
    fn pool_records(&self) -> Result<PoolRecords, Refusal> {
        Err(unbuilt())
    }
    fn prepare_pool(
        &self,
        _: [u8; 32],
        _: hex_api::Execution,
        _: hex_api::RuntimeCost,
        _: &[hex_api::DeviceId],
        _: (&[u8; 32], &[hex_api::DeviceId], Option<&PoolRecord>),
        _: &mut PoolPreparationCheck<'_>,
    ) -> Result<PoolRecord, Refusal> {
        Err(unbuilt())
    }
    fn capacity(&self) -> Result<hex_api::Capacity, Refusal> {
        Err(unbuilt())
    }
    fn physical_devices(&self) -> Result<Vec<hex_api::DeviceInfo>, Refusal> {
        Err(unbuilt())
    }
    fn with_transaction(
        &self,
        _: &BoxId,
        _: &[hex_api::DeviceId],
        _: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        Err(unbuilt())
    }
    fn with_pool_transaction(
        &self,
        _: &hex_api::BackingKey,
        _: &[hex_api::DeviceId],
        _: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        Err(unbuilt())
    }
    fn with_handoff_transaction(
        &self,
        _: &BoxId,
        _: &hex_api::BackingKey,
        _: &[hex_api::DeviceId],
        _: &mut dyn FnMut(&dyn Transaction) -> Result<(), Refusal>,
    ) -> Result<(), Refusal> {
        Err(unbuilt())
    }
}

fn unbuilt() -> Refusal {
    Refusal::Broken("no runtime of the image class alone is built yet".to_owned())
}

impl Runtime for ImageRuntime {
    fn classes(&self) -> Vec<Class> {
        Vec::new()
    }
    fn create(&self, _: BoxSpec) -> Result<BoxInfo, Refusal> {
        Err(unbuilt())
    }
    fn start(&self, _: &BoxId) -> Result<(), Refusal> {
        Err(unbuilt())
    }
    fn exec(&self, _: &BoxId, _: Program, _: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        Err(unbuilt())
    }
    fn run(&self, _: BoxSpec, _: Program, _: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        Err(unbuilt())
    }
    fn signal(&self, _: &BoxId, _: Signal) -> Result<(), Refusal> {
        Err(unbuilt())
    }
    fn inspect(&self, _: &BoxId) -> Result<BoxInfo, Refusal> {
        Err(unbuilt())
    }
    fn usage(&self, _: &BoxId) -> Result<Usage, Refusal> {
        Err(unbuilt())
    }
    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        Err(unbuilt())
    }
    fn delete(&self, _: &BoxId) -> Result<(), Refusal> {
        Err(unbuilt())
    }
    fn member(&self, _: &BoxId, _: &Member) -> Result<Box<dyn Proc>, Refusal> {
        Err(unbuilt())
    }
    fn room(&self, _: &BoxId, _: Option<i64>) -> Result<Option<u64>, Refusal> {
        Err(unbuilt())
    }
    fn rate(
        &self,
        _: &BoxId,
        _: Option<std::num::NonZeroU64>,
    ) -> Result<Option<std::num::NonZeroU64>, Refusal> {
        Err(unbuilt())
    }
    fn host_use(&self, _: &BoxId) -> Result<HostUse, Refusal> {
        Err(unbuilt())
    }
}

#[cfg(test)]
#[path = "images/tests.rs"]
mod tests;
