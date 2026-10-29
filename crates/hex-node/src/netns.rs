//! The node agent's network namespaces made ahead (decision 4 of docs/design/runtime-architecture.md,
//! goal 4 plan step 4): the shell around [`hex_core::netns::Pool`]. One thread of the agent makes each
//! namespace off the create path whenever the pool asks for more ([`net::make_ahead`]: unshared on that
//! thread, its loopback brought up inside it, the thread back home after). A create asks the pool once
//! the runtime has read its spec ([`Networks`]): a container that makes a network namespace of its own
//! in the agent's user namespace takes one, which its shim joins, and the pool counts every container's
//! namespace, taken or made on the path. A namespace taken is the container's alone, so it ends with the
//! container, and the kernel's deferred cleanup tears it down after delete returns.

use std::os::fd::{AsFd, OwnedFd};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;

use hex_api::Refusal;
use hex_core::netns::{Counts, Pool, Take};
use hex_runtime::linux::oci::OciError;
use hex_runtime::linux::oci::net;
use hex_runtime::linux::oci::run::Networks;

/// The pool and the thread that fills it. Dropped, it shuts the pool down, which closes what it held,
/// and waits for the thread, which ends once the make it has under way is done.
pub struct Netns {
    shared: Arc<Shared>,
    refill: Option<JoinHandle<()>>,
}

struct Shared {
    pool: Mutex<Pool<OwnedFd>>,
    /// The refill thread waits on it while the pool wants nothing. A take, which may leave the pool
    /// wanting one, and shutdown wake it; nothing else changes what it waits for.
    wake: Condvar,
}

impl Shared {
    fn pool(&self) -> Result<MutexGuard<'_, Pool<OwnedFd>>, OciError> {
        self.pool.lock().map_err(|_| {
            OciError::Broken("the network-namespace pool's lock was poisoned".to_owned())
        })
    }
}

impl Netns {
    /// A pool that holds at most `bound` namespaces, and the thread that starts filling it at once.
    /// With a bound of 0 there is no thread, and every container that makes a network namespace makes
    /// it on its create's path, counted, as a rootless node's containers do (decision 4).
    pub fn start(bound: usize) -> Result<Self, Refusal> {
        let shared = Arc::new(Shared {
            pool: Mutex::new(Pool::new(bound)),
            wake: Condvar::new(),
        });
        if bound == 0 {
            return Ok(Self {
                shared,
                refill: None,
            });
        }
        let filling = Arc::clone(&shared);
        let refill = std::thread::Builder::new()
            .name("hex-netns".to_owned())
            .spawn(move || refill(&filling))
            .map_err(|e| Refusal::os("starting the network-namespace thread", e))?;
        Ok(Self {
            shared,
            refill: Some(refill),
        })
    }

    /// What the pool has done. Counted at the take, so a create that fails after it took a namespace,
    /// or found the pool dry, is counted all the same.
    pub fn counts(&self) -> Result<Counts, Refusal> {
        Ok(self.shared.pool().map_err(Refusal::from)?.counts())
    }
}

impl Networks for Netns {
    fn take(&self) -> Result<Option<OwnedFd>, OciError> {
        let taken = self
            .shared
            .pool()?
            .take()
            .map_err(|e| OciError::Broken(e.to_string()))?;
        self.shared.wake.notify_one();
        Ok(match taken {
            Take::Pooled(ns) => Some(ns),
            Take::MakeOnPath => None,
        })
    }

    fn on_path(&self) -> Result<(), OciError> {
        self.shared
            .pool()?
            .on_path()
            .map_err(|e| OciError::Broken(e.to_string()))
    }
}

impl Drop for Netns {
    fn drop(&mut self) {
        if let Ok(mut pool) = self.shared.pool.lock() {
            // Closed as they go: namespaces never used.
            drop(pool.shut_down());
        }
        self.shared.wake.notify_all();
        if let Some(refill) = self.refill.take() {
            let _ = refill.join();
        }
    }
}

/// The refill thread: makes a namespace whenever the pool asks for one and hands it in, one at a time,
/// as the pool's count of what it asks for needs, and ends once the pool is shut down. It opens its
/// home namespace first; a thread that can't makes none, each make it is asked for failing, counted.
/// A namespace the pool refuses (past its bound, which one maker never reaches, or after shutdown) is
/// closed here. A poisoned lock ends it, and every create then refuses on the same lock.
fn refill(shared: &Shared) {
    let home = net::thread_network();
    loop {
        let Ok(mut pool) = shared.pool.lock() else {
            return;
        };
        while !pool.is_shut_down() && pool.wanted() == 0 {
            pool = match shared.wake.wait(pool) {
                Ok(pool) => pool,
                Err(_) => return,
            };
        }
        if pool.is_shut_down() {
            return;
        }
        // Made with the lock let go, so a take never waits on a make.
        drop(pool);
        let made = match &home {
            Ok(home) => net::make_ahead(home.as_fd()),
            Err(e) => Err(e.clone()),
        };
        let Ok(mut pool) = shared.pool.lock() else {
            return;
        };
        match made {
            Ok(ns) => {
                let _ = pool.made(ns);
            }
            Err(_) => pool.failed(),
        }
    }
}

#[cfg(test)]
mod tests;
