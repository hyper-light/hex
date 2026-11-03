//! The worker around the bounded pure `/dev` pool. Each prepared tmpfs is detached and owned by one
//! create; nothing is cloned or returned after use (goal 5 extended step 3).
use hex_api::Refusal;
use hex_core::devfs::{Counts, MakeFailure, Pool};
use hex_runtime::linux::oci::run::DevMounts;
use hex_runtime::linux::oci::{OciError, devmount::Key, init};
use std::os::fd::OwnedFd;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

pub struct Devfs {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
}
struct Shared {
    pool: Mutex<Pool<Key, OwnedFd>>,
    wake: Condvar,
}
impl Devfs {
    pub fn start(enabled: bool) -> Result<Self, Refusal> {
        let shared = Arc::new(Shared {
            pool: Mutex::new(Pool::new(enabled)),
            wake: Condvar::new(),
        });
        let worker = if enabled {
            let s = Arc::clone(&shared);
            Some(
                std::thread::Builder::new()
                    .name("hex-devfs".to_owned())
                    .spawn(move || refill(&s))
                    .map_err(|e| Refusal::os("starting the prepared /dev worker", e))?,
            )
        } else {
            None
        };
        Ok(Self { shared, worker })
    }
    pub fn counts(&self) -> Result<Counts, Refusal> {
        self.shared
            .pool
            .lock()
            .map(|p| p.counts())
            .map_err(|_| Refusal::Broken("the prepared /dev pool lock was poisoned".to_owned()))
    }
}
impl DevMounts for Devfs {
    fn unpooled(&self, reason: hex_core::devfs::Unpooled) -> Result<(), OciError> {
        self.shared
            .pool
            .lock()
            .map_err(|_| OciError::Broken("the prepared /dev pool lock was poisoned".to_owned()))?
            .unpooled(reason)
            .map_err(|e| OciError::Broken(e.to_string()))
    }

    fn take(&self, key: Key) -> Result<Option<OwnedFd>, OciError> {
        let (taken, discarded) = self
            .shared
            .pool
            .lock()
            .map_err(|_| OciError::Broken("the prepared /dev pool lock was poisoned".to_owned()))?
            .take(key)
            .map_err(|e| OciError::Broken(e.to_string()))?;
        drop(discarded);
        self.shared.wake.notify_one();
        Ok(taken)
    }
}
impl Drop for Devfs {
    fn drop(&mut self) {
        let held = self.shared.pool.lock().map(|mut p| p.shut_down());
        drop(held);
        self.shared.wake.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn refill(shared: &Shared) {
    loop {
        let Ok(mut pool) = shared.pool.lock() else {
            return;
        };
        while !pool.closed() && !pool.wants_work() {
            pool = match shared.wake.wait(pool) {
                Ok(p) => p,
                Err(_) => return,
            };
        }
        if pool.closed() {
            return;
        }
        let Some(job) = pool.next_job() else {
            continue;
        };
        drop(pool);
        let k = job.key();
        let made = init::prepare_dev(&k.source, &k.data, k.flags);
        // A preparation failure is an optimization miss: counted, with no retry until a new take.
        // The requesting create always runs ordinary setup when dry, preserving its typed refusal.
        let Ok(mut pool) = shared.pool.lock() else {
            return;
        };
        let discarded = pool.complete(
            job,
            made.map_err(|e| match e {
                OciError::Os {
                    op: "making a device" | "setting a device's mode",
                    ..
                } => MakeFailure::Device,
                OciError::Os {
                    op: "preparing a /dev link",
                    ..
                } => MakeFailure::Link,
                _ => MakeFailure::Filesystem,
            }),
        );
        drop(pool);
        drop(discarded);
    }
}
