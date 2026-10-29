//! The node agent's pool of network namespaces made ahead (decision 4 of docs/design/runtime-architecture.md,
//! goal 4 plan step 3): how many it holds, when it asks for more, what a create's take is answered with, and
//! what it counts. Pure: the agent's refill thread makes each namespace off the path (`unshare(CLONE_NEWNET)` on
//! that thread, loopback brought up inside it) and keeps it alive by its descriptor, the shell's `T`; a create
//! whose container makes a network namespace of its own takes one, and its container joins it with `setns`. This
//! decides which, and refuses by type what can't be done.
//!
//! - **The bound**: the pool holds at most its bound, decision 4's [`BOUND`] unless the node is given another. A
//!   namespace made past it is refused ([`Refused::Full`]) and handed back for the shell to close.
//! - **The refill**: the refill mark is one below the bound. The pool asks for more as soon as it holds fewer
//!   than its bound ([`Pool::wanted`]), as decision 4's one thread refills whenever the pool is below it. A make
//!   that fails stops the asking until the next take, so a kernel that refuses every make costs that thread one
//!   failed make a take, never a loop of them (CLAUDE.md §2).
//! - **Never reused**: a take moves its namespace out to the caller and nothing hands one back, so nothing of
//!   one container's network reaches the next and none needs cleaning. Takes go oldest first, so under churn
//!   every namespace is handed out within `bound` takes of its making, none left in the pool while others
//!   come and go.
//! - **Dry**: a take from an empty pool is answered [`Take::MakeOnPath`] and counted; the create makes its
//!   namespace on the path, as with no pool. A create the pool can't serve, a container with a user namespace
//!   of its own (decision 4), makes its namespace on the path too, and [`Pool::on_path`] counts it the same.
//! - **Shutdown**: [`Pool::shut_down`] hands back what the pool holds, counted as discarded, and every take after
//!   it is refused ([`ShutDown`]); a namespace made after it is handed back ([`Refused::ShutDown`]).
//!
//! [`Counts`] is what the bench and the node's metrics read.

use std::collections::VecDeque;

/// The most namespaces decision 4's pool holds: the most creates the bench's busiest phase has in flight at once
/// (concurrency 32), so a burst of its whole width finds a namespace each, at most 24 MiB of kernel memory on
/// the heavier machine measured (`bench/competitive/probes/netns_pool.md`).
pub const BOUND: usize = 32;

/// What a take is answered with.
#[derive(Debug, PartialEq, Eq)]
pub enum Take<T> {
    /// A namespace made ahead, now the caller's alone: the pool never has it again.
    Pooled(T),
    /// The pool is dry: the create makes its container's namespace on the path. Counted in
    /// [`Counts::made_on_path`].
    MakeOnPath,
}

/// A take refused: the pool is shut down, so the node is going away and starts no container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the network-namespace pool is shut down")]
pub struct ShutDown;

/// A namespace the pool didn't take in, handed back for the shell to close, and why.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refused<T> {
    /// The pool holds its bound already. A shell that makes only what [`Pool::wanted`] asks for, one at a time,
    /// never makes one past it.
    #[error("the network-namespace pool holds its most ({bound})")]
    Full { bound: usize, ns: T },
    /// The pool is shut down: the make was under way when it was.
    #[error("the network-namespace pool is shut down")]
    ShutDown { ns: T },
}

/// What the pool has done, as the bench and the node's metrics read it. The pool counts only namespaces it took
/// in, so every one made ahead was taken, is held or was discarded: `made_ahead` is `taken`, plus
/// [`Pool::held`], plus `discarded`. A count stops at `u64::MAX`, which at one a nanosecond takes 584 years to
/// reach.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// Namespaces made ahead and taken into the pool.
    pub made_ahead: u64,
    /// Namespaces handed to creates.
    pub taken: u64,
    /// Namespaces made on the path: takes the pool was dry for, and creates it can't serve
    /// ([`Pool::on_path`]).
    pub made_on_path: u64,
    /// Namespaces the pool held when it was shut down, handed back to close, never used.
    pub discarded: u64,
    /// Makes that failed.
    pub failed: u64,
}

/// The pool: the namespaces it holds as the shell's `T`, whether it asks for more, and what it has done. The
/// shell keeps it behind its lock; a take neither allocates nor waits.
#[derive(Debug)]
pub struct Pool<T> {
    bound: usize,
    /// Fresh namespaces, oldest first, never more than `bound`.
    held: VecDeque<T>,
    /// False from a failed make to the next take: the bound on the refill's retries.
    asking: bool,
    shut: bool,
    counts: Counts,
}

impl<T> Pool<T> {
    /// An empty pool that holds at most `bound` namespaces and asks for all of them. With a bound of 0 it holds
    /// none, and every take is made on the path, as on a node without a pool.
    pub fn new(bound: usize) -> Self {
        Self {
            bound,
            held: VecDeque::new(),
            asking: true,
            shut: false,
            counts: Counts::default(),
        }
    }

    /// How many namespaces the pool asks its shell to make now: the room below its bound, while it is open and
    /// no make has failed since the last take. The shell makes them off the path, one at a time, and hands each
    /// in with [`made`](Self::made) or reports it [`failed`](Self::failed). The pool counts no make under way,
    /// so a second maker could make one past the bound, which is refused.
    pub fn wanted(&self) -> usize {
        if self.shut || !self.asking {
            return 0;
        }
        // `held` never passes the bound, so this is the room left.
        self.bound.saturating_sub(self.held.len())
    }

    /// A namespace the shell made ahead: taken in and counted, or, past the bound or after shutdown, refused
    /// and handed back.
    pub fn made(&mut self, ns: T) -> Result<(), Refused<T>> {
        if self.shut {
            return Err(Refused::ShutDown { ns });
        }
        if self.held.len() >= self.bound {
            return Err(Refused::Full {
                bound: self.bound,
                ns,
            });
        }
        self.held.push_back(ns);
        self.counts.made_ahead = self.counts.made_ahead.saturating_add(1);
        Ok(())
    }

    /// A make that failed (the shell reports why): counted, and the pool asks for no more until the next take.
    pub fn failed(&mut self) {
        self.counts.failed = self.counts.failed.saturating_add(1);
        self.asking = false;
    }

    /// A create's namespace: the oldest made ahead, now the caller's alone, or, when the pool is dry, word to
    /// make it on the path, counted. Either way the pool asks for more. Refused once the pool is shut down.
    pub fn take(&mut self) -> Result<Take<T>, ShutDown> {
        if self.shut {
            return Err(ShutDown);
        }
        self.asking = true;
        Ok(match self.held.pop_front() {
            Some(ns) => {
                self.counts.taken = self.counts.taken.saturating_add(1);
                Take::Pooled(ns)
            }
            None => {
                self.counts.made_on_path = self.counts.made_on_path.saturating_add(1);
                Take::MakeOnPath
            }
        })
    }

    /// A create the pool can't serve, which makes its namespace on the path: its container has a user namespace
    /// of its own, and a namespace belongs to its maker's (decision 4). Counted as a dry take is; nothing is
    /// taken, so the asking stays as it was. Refused once the pool is shut down, as a take is.
    pub fn on_path(&mut self) -> Result<(), ShutDown> {
        if self.shut {
            return Err(ShutDown);
        }
        self.counts.made_on_path = self.counts.made_on_path.saturating_add(1);
        Ok(())
    }

    /// Shuts the pool down and hands back what it holds for the shell to close, counted as discarded. Every
    /// take after is refused and nothing more is asked for; shutting down again hands back nothing. The shell
    /// shuts the pool down before it lets it go, so nothing it held goes uncounted.
    #[must_use = "the namespaces handed back are the shell's to close"]
    pub fn shut_down(&mut self) -> Vec<T> {
        self.shut = true;
        let held = Vec::from(std::mem::take(&mut self.held));
        let n = u64::try_from(held.len()).unwrap_or(u64::MAX);
        self.counts.discarded = self.counts.discarded.saturating_add(n);
        held
    }

    /// How many namespaces the pool holds.
    pub fn held(&self) -> usize {
        self.held.len()
    }

    /// Whether the pool is shut down: the refill thread's word to end.
    pub fn is_shut_down(&self) -> bool {
        self.shut
    }

    /// What the pool has done.
    pub fn counts(&self) -> Counts {
        self.counts
    }
}

#[cfg(test)]
mod tests;
