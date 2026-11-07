//! A node over any OS's runtime (native plan step 8): each box's record kept durably in the node's state
//! directory beside the runtime that makes the box ([`crate::store`], the records Linux's node keeps), and the
//! programs the socket's disconnect rule ends counted into it ([`crate::abandoned`]), so a node served on
//! any OS's socket ([`crate::serve`]) answers as Linux's does. Linux's node ([`crate::Node`]) keeps the same
//! records and counts through the same code, with its cgroups and containers beside them; macOS's node is
//! this over its runtime (the `process` class, and Linux images in its Shards guest).

use std::path::Path;

use hex_api::{
    BoxId, BoxInfo, BoxSpec, Class, Proc, Program, Refusal, Runtime, Signal, Stdio, Usage,
};
use hex_core::netns;
use hex_core::record::{BoxRecord, Phase};

use crate::abandoned::Abandoned;
use crate::serve::Host;
use crate::store::Store;

/// A runtime whose boxes' records the node keeps.
pub struct Recorded<R> {
    runtime: R,
    store: Store,
    abandoned: Abandoned,
}

impl<R: Runtime> Recorded<R> {
    /// `runtime`'s boxes, their records in `state_dir`.
    pub fn open(runtime: R, state_dir: &Path) -> Result<Self, Refusal> {
        Ok(Self {
            runtime,
            store: Store::open(state_dir)?,
            abandoned: Abandoned::default(),
        })
    }

    /// `info` with the abandoned programs counted for its box, recorded and not yet.
    fn counted(&self, mut info: BoxInfo) -> Result<BoxInfo, Refusal> {
        let recorded = match self.store.get(&info.id)? {
            Some(Ok(record)) => record.abandoned,
            _ => 0,
        };
        info.abandoned = recorded.saturating_add(self.abandoned.unrecorded(&info.id)?);
        Ok(info)
    }
}

impl<R: Runtime> Runtime for Recorded<R> {
    fn classes(&self) -> Vec<Class> {
        self.runtime.classes()
    }

    /// Records the box as being made before anything is made, so a crash leaves a half-made box known as
    /// one (T-T7.1.2), then as made once the runtime has made it; a refused create leaves no record.
    fn create(&self, spec: BoxSpec) -> Result<BoxInfo, Refusal> {
        let mut record = BoxRecord::new(spec.clone(), Phase::Creating, Vec::new());
        self.store.put(&record)?;
        let info = match self.runtime.create(spec) {
            Ok(info) => info,
            Err(why) => {
                let _ = self.store.remove(&record.spec.id);
                return Err(why);
            }
        };
        record.phase = Phase::Ready;
        self.store.put(&record)?;
        self.counted(info)
    }

    fn start(&self, id: &BoxId) -> Result<(), Refusal> {
        self.runtime.start(id)
    }

    fn exec(&self, id: &BoxId, program: Program, stdio: Stdio) -> Result<Box<dyn Proc>, Refusal> {
        self.runtime.exec(id, program, stdio)
    }

    fn signal(&self, id: &BoxId, signal: Signal) -> Result<(), Refusal> {
        self.runtime.signal(id, signal)
    }

    fn inspect(&self, id: &BoxId) -> Result<BoxInfo, Refusal> {
        self.counted(self.runtime.inspect(id)?)
    }

    fn usage(&self, id: &BoxId) -> Result<Usage, Refusal> {
        self.runtime.usage(id)
    }

    fn list(&self) -> Result<Vec<BoxInfo>, Refusal> {
        self.runtime
            .list()?
            .into_iter()
            .map(|info| self.counted(info))
            .collect()
    }

    /// Records the box as being deleted, deletes it, then removes its record: a crash between leaves a
    /// record recovery finishes.
    fn delete(&self, id: &BoxId) -> Result<(), Refusal> {
        if let Some(Ok(mut record)) = self.store.get(id)? {
            record.phase = Phase::Deleting;
            self.store.put(&record)?;
        }
        self.runtime.delete(id)?;
        self.store.remove(id)?;
        self.abandoned.forget(id);
        Ok(())
    }
}

impl<R: Runtime> Host for Recorded<R> {
    fn abandoned(&self, id: &BoxId) -> Result<(), Refusal> {
        self.abandoned.count(&self.store, id)
    }

    /// Network namespaces are Linux's: a node elsewhere has none to count.
    fn netns(&self) -> Result<netns::Counts, Refusal> {
        Err(Refusal::Platform {
            what: "the node's network namespace counts",
            platform: std::env::consts::OS,
            why: "network namespaces are Linux's; this node makes none".to_owned(),
        })
    }
}
