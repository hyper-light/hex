//! The bounds of the node's pools, derived from the host it starts on (Ada's rule, 2026-11-06: "We do not support
//! arbitrary constants. We support learned and data derived values"; docs/design/runtime-architecture.md, "Pool
//! sizes learned from demand"). Each pool learns its size from the demand it sees ([`hex_core::demand`]), starting
//! at the host's parallelism; these are the bounds that learning stops at, each the count of the pool's items that
//! fits in an even share of the memory the node may hold, at the item's measured cost (CLAUDE.md §2).
//!
//! [`Sizes::of`] is pure; [`Host::now`] reads the host's facts.

use std::num::NonZeroUsize;
use std::sync::OnceLock;

use hex_api::Refusal;
use hex_core::demand::{cgroup_headroom, mem_available, share};

/// An idle cgroup kept for its owner's next box: what the bench's lifecycle leaves charged to it, at most
/// 1,748,992 bytes over 256 idle cgroups of fedora's OCI pass 20261103T044133Z
/// (bench/competitive/probes/reuse_residual.md), and the init parked in it for that box, about 24.7 KB an init with
/// reuse on (bench/competitive/probes/create_tail.md, "What a park charged its cgroup"). The check admits up to
/// [`crate::cgroups::RESIDUAL`] a cgroup; a node whose idle cgroups hold more than measured here is what would
/// falsify this bound.
pub const IDLE_CGROUP_BYTES: u64 = 1_748_992 + 24_700;
/// A cgroup made ahead for a box of no owner with its init parked in it: about 262 KB an init in a fresh cgroup
/// (bench/competitive/probes/create_tail.md, "What a park charged its cgroup", reuse off).
pub const AHEAD_BYTES: u64 = 262_000;
/// A network namespace held, the most the host's Slab and Percpu rose for one: 762.0 KiB on the Mac Studio, the
/// heavier of the two machines measured (bench/competitive/probes/netns_pool.md).
pub const NETNS_BYTES: u64 = 762 * 1024;
/// A create seed, its largest `VmRSS` measured: 2,488 KiB on fedora (bench/competitive/probes/seed_path.md).
pub const SEED_BYTES: u64 = 2_488 * 1024;
/// An ipc and uts namespace pair made ahead, the most the host's Slab rose for one: 14.1 KiB on fedora
/// (bench/competitive/probes/netns_pool.md, goal 5 extended step 4).
pub const IPC_UTS_BYTES: u64 = 14_438;

/// The pools that hold the node's memory, by their items' cost: they split it evenly, so together they hold no
/// more than it.
const SHARERS: [u64; 5] = [
    IDLE_CGROUP_BYTES,
    AHEAD_BYTES,
    NETNS_BYTES,
    SEED_BYTES,
    IPC_UTS_BYTES,
];

/// What the node reads of its host as it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Host {
    /// The memory the node may hold: the host's `MemAvailable`, or less where a cgroup the node is in, or one
    /// above it, leaves less under its `memory.max`.
    pub memory: u64,
    /// The hardware threads the node may run on (`std::thread::available_parallelism`, which reads the process's
    /// affinity and its cgroup's `cpu.max`).
    pub parallelism: NonZeroUsize,
}

impl Host {
    /// The host's facts now. Refused when `/proc/meminfo` gives no `MemAvailable` or the parallelism can't be read:
    /// a node that can't say what it may hold sizes nothing.
    pub fn now() -> Result<Self, Refusal> {
        let meminfo = std::fs::read_to_string("/proc/meminfo")
            .map_err(|e| Refusal::os("reading /proc/meminfo", e))?;
        let available = mem_available(&meminfo).ok_or_else(|| {
            Refusal::Broken("/proc/meminfo gives no MemAvailable in kB".to_owned())
        })?;
        let memory = cgroup_headrooms().into_iter().fold(available, u64::min);
        let parallelism = std::thread::available_parallelism()
            .map_err(|e| Refusal::os("reading the host's parallelism", e))?;
        Ok(Self {
            memory,
            parallelism,
        })
    }
}

/// What each cgroup from the node's own up to the root leaves under its `memory.max`, where it sets one: a walk up
/// the path `/proc/self/cgroup` names, one step a component, so it ends.
fn cgroup_headrooms() -> Vec<u64> {
    let Ok(own) = std::fs::read_to_string("/proc/self/cgroup") else {
        return Vec::new();
    };
    let Some(path) = own.lines().find_map(|l| l.strip_prefix("0::")) else {
        return Vec::new();
    };
    let mut dir = std::path::Path::new("/sys/fs/cgroup").join(path.trim_start_matches('/'));
    let mut out = Vec::new();
    loop {
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok();
        if let (Some(max), Some(current)) = (read("memory.max"), read("memory.current"))
            && let Some(room) = cgroup_headroom(&max, &current)
        {
            out.push(room);
        }
        if dir.as_os_str() == "/sys/fs/cgroup" || !dir.pop() {
            return out;
        }
    }
}

/// The node's pools' bounds, and where their learned sizes start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    /// Where each learned size starts, before any demand: the host's parallelism (32 on fedora, the bench host,
    /// so no pool starts smaller there than the constant it replaces).
    pub start: usize,
    /// The most idle cgroups the node keeps, all owners' together, and so the most any owner keeps.
    pub idle_cgroups: usize,
    /// The most idle network namespaces the node keeps, all owners' together.
    pub idle_netns: usize,
    /// The most create seeds.
    pub seeds: NonZeroUsize,
    /// The most cgroups made ahead for boxes of no owner. No more creates take one at once than hold a seed.
    pub ahead: usize,
    /// The most ipc and uts pairs made ahead. No more creates take one at once than hold a seed.
    pub ipc_uts: usize,
}

impl Sizes {
    /// The bounds on `host`.
    pub fn of(host: Host) -> Self {
        let each = |cost| share(host.memory, SHARERS.len(), cost);
        let seeds = NonZeroUsize::new(each(SEED_BYTES)).unwrap_or(NonZeroUsize::MIN);
        Self {
            start: host.parallelism.get(),
            idle_cgroups: each(IDLE_CGROUP_BYTES),
            idle_netns: each(NETNS_BYTES),
            seeds,
            ahead: each(AHEAD_BYTES).min(seeds.get()),
            ipc_uts: each(IPC_UTS_BYTES).min(seeds.get()),
        }
    }

    /// The bounds on this host, from its facts as this process first read them: every node a process opens is
    /// bounded alike, and what a caller reckons from them (the descriptors at the bounds,
    /// [`crate::netns::FilesBound`]) is what its node holds.
    pub fn now() -> Result<Self, Refusal> {
        static READ: OnceLock<Sizes> = OnceLock::new();
        if let Some(sizes) = READ.get() {
            return Ok(*sizes);
        }
        let sizes = Host::now().map(Self::of)?;
        Ok(*READ.get_or_init(|| sizes))
    }

    /// The most procs alive at once with parked inits (decision 8, parked procs): one an init parked, in an idle
    /// cgroup or one made ahead.
    pub fn procs_alive(&self) -> usize {
        self.idle_cgroups.saturating_add(self.ahead)
    }

    /// The memory the pools at these bounds hold at most.
    pub fn bytes(&self) -> u64 {
        let of = |n: usize, each: u64| u64::try_from(n).unwrap_or(u64::MAX).saturating_mul(each);
        [
            of(self.idle_cgroups, IDLE_CGROUP_BYTES),
            of(self.ahead, AHEAD_BYTES),
            of(self.idle_netns, NETNS_BYTES),
            of(self.seeds.get(), SEED_BYTES),
            of(self.ipc_uts, IPC_UTS_BYTES),
        ]
        .into_iter()
        .fold(0, u64::saturating_add)
    }
}

#[cfg(test)]
mod tests;
