//! Sizes the node learns from the demand it sees, and the bounds over them (Ada's rule, 2026-11-06: "We do not
//! support arbitrary constants. We support learned and data derived values"; docs/design/runtime-architecture.md,
//! "Pool sizes learned from demand"). Latency and throughput come before memory, so a learned size starts at no
//! less than the host's own parallelism, grows ahead of the demand seen, and never shrinks: no rule that shrinks
//! a pool can be shown not to cause a take to find it short under the demand already seen. Each size stops at
//! its bound, derived from the memory the node may hold (CLAUDE.md §2); past it a take finds the pool short and
//! is counted, as every pool's cold take is.
//!
//! Pure (CLAUDE.md §9): the node's shell reads the host's facts ([`mem_available`], [`cgroup_headroom`]) and
//! reports what its pools see; this decides the sizes.

use std::collections::BTreeMap;

/// A size learned from demand: at least its start, at most its bound, raised to each demand reported, never
/// lowered. The default holds none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Learned {
    size: usize,
    cap: usize,
}

impl Learned {
    /// A size starting at `start`, held at or under `cap`.
    pub fn new(start: usize, cap: usize) -> Self {
        Self {
            size: start.min(cap),
            cap,
        }
    }

    /// Demand of `n` seen: the size is raised to it, up to the bound. Whether it rose.
    pub fn observe(&mut self, n: usize) -> bool {
        let to = n.min(self.cap);
        let rose = to > self.size;
        self.size = self.size.max(to);
        rose
    }

    pub fn size(&self) -> usize {
        self.size
    }

    pub fn cap(&self) -> usize {
        self.cap
    }
}

/// A pool made ahead's size, learned from its draws: each take since the pool last held its size is one drawn,
/// and a take that finds the pool empty (a cold take) raises the size to what was drawn plus every cold take of
/// that draw, so the next draw as deep finds the pool full with that many to spare. The headroom grows with each
/// take found short, as the lead's condition of 2026-11-06 asks, and the size never falls. The default holds none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Depth {
    size: Learned,
    drawn: usize,
    cold: usize,
}

impl Depth {
    pub fn new(start: usize, cap: usize) -> Self {
        Self {
            size: Learned::new(start, cap),
            drawn: 0,
            cold: 0,
        }
    }

    /// A take: `cold` when it found the pool empty. Whether the size rose.
    pub fn took(&mut self, cold: bool) -> bool {
        self.drawn = self.drawn.saturating_add(1);
        if !cold {
            return false;
        }
        self.cold = self.cold.saturating_add(1);
        self.size.observe(self.drawn.saturating_add(self.cold))
    }

    /// The pool holds its size again: the next take starts a new draw.
    pub fn full(&mut self) {
        self.drawn = 0;
        self.cold = 0;
    }

    pub fn size(&self) -> usize {
        self.size.size()
    }

    pub fn cap(&self) -> usize {
        self.size.cap()
    }
}

/// The most idle items any one owner keeps, learned from regret, as ARC adapts its lists' target (Megiddo and
/// Modha, "ARC: A Self-Tuning, Low Overhead Replacement Cache", FAST 2003, §4.3: a hit in a list's ghost
/// directory raises that list's target). Each item an owner's bound put out is remembered as a ghost of that
/// owner; a take that then finds the owner with none idle spends one and raises the bound by one, so a burst
/// one round lost to the bound is kept whole the next. The ghosts are bounded: at most `cap` owners, the one
/// whose ghost came longest ago forgotten past it, and at most `cap` ghosts an owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ghosts {
    bound: Learned,
    /// Each owner's ghosts and when its last came.
    owners: BTreeMap<String, (u64, usize)>,
    /// The owners by when their last ghost came, the oldest first.
    order: BTreeMap<u64, String>,
    seq: u64,
}

impl Ghosts {
    pub fn new(start: usize, cap: usize) -> Self {
        Self {
            bound: Learned::new(start, cap),
            owners: BTreeMap::new(),
            order: BTreeMap::new(),
            seq: 0,
        }
    }

    /// The bound now.
    pub fn bound(&self) -> usize {
        self.bound.size()
    }

    pub fn cap(&self) -> usize {
        self.bound.cap()
    }

    /// One of `owner`'s idle items was put out by the bound.
    pub fn evicted(&mut self, owner: &str) {
        let cap = self.bound.cap();
        if cap == 0 {
            return;
        }
        self.seq = self.seq.saturating_add(1);
        let count = match self.owners.remove(owner) {
            Some((seq, count)) => {
                self.order.remove(&seq);
                count.saturating_add(1).min(cap)
            }
            None => 1,
        };
        self.owners.insert(owner.to_owned(), (self.seq, count));
        self.order.insert(self.seq, owner.to_owned());
        // One insert a call, so at most one owner past the bound.
        if self.owners.len() > cap
            && let Some((_, oldest)) = self.order.pop_first()
        {
            self.owners.remove(&oldest);
        }
    }

    /// A take of `owner`'s found none of its items idle: one of its ghosts, if any, is spent and the bound
    /// raised by one. Whether it rose.
    pub fn missed(&mut self, owner: &str) -> bool {
        let Some((seq, count)) = self.owners.get_mut(owner) else {
            return false;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            let seq = *seq;
            self.owners.remove(owner);
            self.order.remove(&seq);
        }
        self.bound.observe(self.bound.size().saturating_add(1))
    }

    /// How many owners have ghosts.
    pub fn owners(&self) -> usize {
        self.owners.len()
    }
}

/// `MemAvailable` from `/proc/meminfo`, in bytes: the kernel's estimate of the memory available for starting new
/// work without swapping (Linux Documentation/filesystems/proc.rst, "meminfo"). `None` when the text has no such
/// line in kB or its number doesn't fit.
pub fn mem_available(meminfo: &str) -> Option<u64> {
    meminfo.lines().find_map(|line| {
        let rest = line.strip_prefix("MemAvailable:")?;
        let mut words = rest.split_whitespace();
        let kib: u64 = words.next()?.parse().ok()?;
        if words.next()? != "kB" || words.next().is_some() {
            return None;
        }
        kib.checked_mul(1024)
    })
}

/// What a cgroup's `memory.max` leaves past its `memory.current`, in bytes (Linux
/// Documentation/admin-guide/cgroup-v2.rst, "memory.max"): `None` where it sets no bound (`max`) or a file
/// doesn't parse; 0 where it is reached.
pub fn cgroup_headroom(max: &str, current: &str) -> Option<u64> {
    let max: u64 = max.trim().parse().ok()?;
    let current: u64 = current.trim().parse().ok()?;
    Some(max.saturating_sub(current))
}

/// How many items of `each` bytes fit in an even share of `memory` among `sharers` pools: the pools that hold
/// the node's memory split it evenly, so together they never hold past it. 0 for no sharers or items of no
/// size, which nothing should name.
pub fn share(memory: u64, sharers: usize, each: u64) -> usize {
    let Ok(sharers) = u64::try_from(sharers) else {
        return 0;
    };
    memory
        .checked_div(sharers)
        .and_then(|part| part.checked_div(each))
        .map_or(0, |n| usize::try_from(n).unwrap_or(usize::MAX))
}

#[cfg(test)]
mod tests;
