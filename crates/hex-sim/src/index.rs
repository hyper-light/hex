//! What each node can take, and the fast path's pick over it: the kernel's walk, ported from kiln.
//!
//! A node admits a workload when its hard filters pass (`scheduler-architecture.md` S3) and it has
//! the room: the image's platform the node's, GPUs and devices only where the node has them free and
//! the workload's class can be given them on the node's OS ([`Class::exposes`]), a benchmark never
//! beside a build nor a build beside a benchmark, and an agent session never on a node above 90% CPU
//! (`AC-T7.6.3`). Among the nodes that admit it, best fit is the node with the least free CPU, then
//! least free memory, then fewest free GPUs, then lowest id: one total order ([`Key`]), so the
//! scheduler and its serial specification can't disagree on a tie. Best fit is the baseline the
//! design scores against (Borg's, S1, CM[1]). GPU work goes first to the node whose GPUs are least
//! loaded, as kiln's pick goes to the runner with the most CPU free.

use crate::census;
use crate::error::SimError;
use crate::lb::{self, BUCKETS, Index};
use crate::model::{
    Class, Classes, Demand, Device, DeviceKind, GpuLoad, Held, Kind, NodeId, NodeSpec, Platform,
    Resources, WorkloadId, bind,
};
use crate::start_cost::{self, Costs, Prepared, Rates, StartObservation, Terms};

/// A session is refused a node whose CPU in use, background and placed, is above this share of
/// its CPU, per mille: `AC-T7.6.3`'s 90%.
pub const HOT_PER_MILLE: u64 = 900;

/// A node's place in the best-fit order: its free capacity, then its id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Key {
    pub cpu_milli: u64,
    pub mem_bytes: u64,
    pub gpus: u32,
    pub node: NodeId,
}

impl Key {
    fn of(node: NodeId, free: &Resources) -> Self {
        Self {
            cpu_milli: free.cpu_milli,
            mem_bytes: free.mem_bytes,
            gpus: free.gpus,
            node,
        }
    }
}

/// How a pick ranks the nodes that admit a demand: its scorer, and the workload shapes
/// [`Scorer::Fragment`] keeps usable. A pick reads every node with the room, from the least room that
/// could hold the need up (the kernel's walk, [`crate::lb::Index`]): no sample, so no sample size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pick {
    /// How the nodes that admit the demand are ranked.
    pub scorer: Scorer,
    /// The workload shapes [`Scorer::Fragment`] keeps usable.
    pub shapes: Shapes,
}

/// At most [`Shapes::MAX`] workload shapes, each with the class that asks it, kept in a fixed array
/// so a [`Pick`] stays a plain value. More is refused, typed: the shapes a pick keeps are
/// Pareto-minimal, none fitting wherever another does, so dropping one could leave a leftover only it
/// fits read as stranded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Shapes {
    len: usize,
    items: [(Resources, Class); Shapes::MAX],
}

impl Shapes {
    /// The input-bound rule ([`crate::census`]): twice the most minimal shapes a committed trace's
    /// workload has.
    pub const MAX: usize = census::SHAPES * census::INPUT_MARGIN;

    /// `shapes`, each once, in order (by CPU, then memory, then GPUs, then class); more than
    /// [`Shapes::MAX`] distinct ones refused.
    pub fn of(mut shapes: Vec<(Resources, Class)>) -> Result<Self, SimError> {
        shapes.sort_unstable();
        shapes.dedup();
        if shapes.len() > Self::MAX {
            return Err(SimError::Full {
                what: "a sample's workload shapes",
                bound: Self::MAX,
            });
        }
        let mut out = Self::default();
        for (slot, shape) in out.items.iter_mut().zip(shapes) {
            *slot = shape;
            out.len = out.len.saturating_add(1);
        }
        Ok(out)
    }

    pub fn iter(&self) -> impl Iterator<Item = &(Resources, Class)> {
        self.items.iter().take(self.len)
    }

    /// Whether some shape of a class `classes` holds fits `free`.
    pub fn usable(&self, free: &Resources, classes: Classes) -> bool {
        self.iter()
            .any(|(shape, class)| classes.has(*class) && free.holds(shape))
    }
}

/// How a pick ranks the nodes that admit a demand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scorer {
    /// The least free capacity first ([`Key`]'s order): best fit, the baseline (Borg's, S1).
    #[default]
    BestFit,
    /// Best fit across resources: the node most allocated once the need is placed, CPU and memory
    /// each as a share of the node's and weighted alike, as Kubernetes' `MostAllocated` bin-packing
    /// score is; then [`Key`]'s order. [`Scorer::BestFit`] ranks by free CPU first, which on a
    /// memory-bound workload stops packing the resource that binds.
    MostAllocated,
    /// Tetris's alignment (Grandl et al., SIGCOMM 2014): the largest dot product of the need and the
    /// node's free capacity, each resource taken as a share of the node's capacity, ties by
    /// [`Key`]'s order.
    Align,
    /// Least stranded: the node whose leftover after placing the need is either nothing or still
    /// fits one of the pick's [`Shapes`], so no CPU or memory is left that no workload can use;
    /// then the [`Tie`]; then [`Key`]'s order. [`Scorer::HEX`] is Hex's packing score.
    Fragment(Tie),
}

impl Scorer {
    /// Hex's packing score: least stranded, then the node that runs the fewest classes, then the most
    /// allocated, so a node only some classes can use is spent before one that every class can, and
    /// what isn't stranded packs. On the recorded workload of OCI containers, Firecracker microVMs and
    /// Shards microVMs it strands at least 30% less than either best fit at 90% load on every seed,
    /// for the mix and for each class alone, and passes over fewer of them (hex-sim.md H11).
    pub const HEX: Self = Self::Fragment(Tie::Narrowest);
}

/// What ranks the nodes [`Scorer::Fragment`] finds equally stranded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tie {
    /// Tetris's alignment ([`Scorer::Align`]): the plan's alignment score with a fragmentation term.
    Align,
    /// The node least allocated once the need is placed (Kubernetes' `LeastAllocated`).
    LeastAllocated,
    /// The node most allocated once the need is placed (Kubernetes' `MostAllocated`).
    MostAllocated,
    /// The node that runs the fewest classes, then the most allocated: a node every class can use is
    /// kept for the classes only it can take.
    Narrowest,
}

/// `part` as a share of `whole` in millionths; 0 of a whole of 0, and past u64 saturates.
fn millionths(part: u64, whole: u64) -> u64 {
    u128::from(part)
        .checked_mul(1_000_000)
        .and_then(|x| x.checked_div(u128::from(whole)))
        .map_or(0, |x| u64::try_from(x).unwrap_or(u64::MAX))
}

/// Nodes a scan passed over because a filter refused them though they had the room, by filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Refusals {
    /// GPU work, a node without the GPUs it asks, or of a class that can't be given them there.
    pub gpu: u64,
    /// An image built for another OS or architecture.
    pub platform: u64,
    /// Devices or KVM the node hasn't, or that the workload's class can't be given there.
    pub device: u64,
    /// A benchmark and a build, one already running there.
    pub shares: u64,
    /// A session, a node above 90% CPU.
    pub hot: u64,
    /// A workload of a class the node can't run.
    pub class: u64,
}

/// A pick: the best node that admits the demand, the nodes its walk read, and what the filters
/// refused among them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    pub best: Option<Key>,
    /// Nodes the walk read: every one with the CPU for the need.
    pub walked: u64,
    pub refused: Refusals,
}

/// Why a node doesn't admit a demand, or that it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Admits,
    NoClass,
    /// The image is built for another OS or architecture.
    NoPlatform,
    /// The need asks GPUs the node hasn't, of the model and with the memory free asked, or that its
    /// class can't be given on the node's OS ([`Class::exposes`]).
    NoGpu,
    /// The ask names devices or KVM the node hasn't, or that its class can't be given there.
    NoDevice,
    NoRoom,
    Shares,
    Hot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    capacity: Resources,
    free: Resources,
    background_milli: u64,
    classes: Classes,
    platform: Platform,
    kvm: bool,
    builds: u32,
    benchmarks: u32,
}

/// A GPU's load as one share, in millionths: the larger of its utilisation and its memory in use, as
/// DRF takes a dominant share (Ghodsi et al., NSDI 2011).
fn gpu_busy(d: &Device) -> u64 {
    millionths(d.load.memory_used, d.size)
        .max(u64::from(d.load.busy_per_mille).saturating_mul(1000))
}

/// How idle the GPUs `held` names are: the sum of each one's share free of its load, in millionths.
fn gpu_idle(devices: &[Device], held: &[u16]) -> u64 {
    held.iter()
        .filter_map(|&i| devices.get(usize::from(i)))
        .filter(|d| d.kind == DeviceKind::Gpu)
        .map(|d| 1_000_000u64.saturating_sub(gpu_busy(d)))
        .fold(0, u64::saturating_add)
}

impl Slot {
    /// The verdict on `demand` here, `devices` the node's and `free(i)` whether device `i` is free.
    fn verdict(
        &self,
        demand: &Demand,
        devices: &[Device],
        free: impl Fn(usize) -> bool,
    ) -> Verdict {
        let (need, ask, class, os) = (&demand.need, &demand.ask, demand.class, self.platform.os);
        if !self.classes.has(class) {
            return Verdict::NoClass;
        }
        // AC-T7.4.1: VM images describe their Linux guest, independently of the VMM host.
        let image_os = match class {
            Class::Oci => os,
            Class::Shards | Class::Firecracker => crate::model::Os::Linux,
        };
        if ask.os.is_some_and(|o| o != image_os)
            || ask.arch.is_some_and(|a| a != self.platform.arch)
        {
            return Verdict::NoPlatform;
        }
        let gpus_only = crate::model::Ask {
            vfio: 0,
            block: 0,
            net: 0,
            kvm: false,
            ..*ask
        };
        if need.gpus > 0
            && (!class.exposes(os, DeviceKind::Gpu)
                || bind(devices, |_| true, need.gpus, &gpus_only).is_none())
        {
            return Verdict::NoGpu;
        }
        if ask.devices() {
            let exposed = [
                (ask.vfio, DeviceKind::Vfio),
                (ask.block, DeviceKind::Block),
                (ask.net, DeviceKind::Net),
            ]
            .iter()
            .all(|&(n, kind)| n == 0 || class.exposes(os, kind));
            let kvm = !ask.kvm || (self.kvm && class.gives_kvm(os));
            if !exposed || !kvm || bind(devices, |_| true, 0, ask).is_none() {
                return Verdict::NoDevice;
            }
        }
        if !self.free.holds(need) {
            return Verdict::NoRoom;
        }
        if (need.gpus > 0 || ask.devices()) && bind(devices, free, need.gpus, ask).is_none() {
            return Verdict::NoRoom;
        }
        match demand.kind {
            Kind::Build if self.benchmarks > 0 => Verdict::Shares,
            Kind::Benchmark if self.builds > 0 => Verdict::Shares,
            Kind::Session if self.hot() => Verdict::Hot,
            Kind::Plain | Kind::Build | Kind::Benchmark | Kind::Session => Verdict::Admits,
        }
    }

    /// Tetris's alignment of `need` with this node's free capacity: the sum over CPU, memory and
    /// GPUs of (need / capacity) * (free / capacity), each share in millionths, so each term is at
    /// most 10^12 and the sum fits u64. A resource the node has none of adds nothing.
    fn alignment(&self, need: &Resources) -> u64 {
        let term =
            |n: u64, f: u64, c: u64| millionths(n, c).checked_mul(millionths(f, c)).unwrap_or(0);
        let (c, f) = (&self.capacity, &self.free);
        [
            term(need.cpu_milli, f.cpu_milli, c.cpu_milli),
            term(need.mem_bytes, f.mem_bytes, c.mem_bytes),
            term(u64::from(need.gpus), u64::from(f.gpus), u64::from(c.gpus)),
        ]
        .into_iter()
        .fold(0u64, u64::saturating_add)
    }

    /// What placing `need` here would leave that no shape can use: when the leftover fits no shape,
    /// its CPU and its memory, each as a share of the node's in millionths, summed; else nothing.
    fn stranded_after(&self, need: &Resources, shapes: &Shapes) -> u64 {
        let Some(left) = self.free.checked_sub(need) else {
            return u64::MAX;
        };
        if shapes.usable(&left, self.classes) {
            0
        } else {
            let c = &self.capacity;
            millionths(left.cpu_milli, c.cpu_milli)
                .saturating_add(millionths(left.mem_bytes, c.mem_bytes))
        }
    }

    /// The node's CPU and memory allocated once `need` is placed, each as a share of the node's in
    /// millionths, summed: Kubernetes' `MostAllocated` score with equal weights, and the inverse of
    /// its `LeastAllocated`.
    fn allocated_after(&self, need: &Resources) -> u64 {
        let (c, f) = (&self.capacity, &self.free);
        let used = |cap: u64, free: u64, more: u64| cap.saturating_sub(free).saturating_add(more);
        millionths(used(c.cpu_milli, f.cpu_milli, need.cpu_milli), c.cpu_milli).saturating_add(
            millionths(used(c.mem_bytes, f.mem_bytes, need.mem_bytes), c.mem_bytes),
        )
    }

    /// What `pick`'s scorer gives placing `need` here: higher is better.
    fn score(&self, need: &Resources, pick: &Pick) -> u128 {
        match pick.scorer {
            Scorer::BestFit => 0,
            Scorer::MostAllocated => u128::from(self.allocated_after(need)),
            Scorer::Align => u128::from(self.alignment(need)),
            Scorer::Fragment(tie) => {
                // Lexicographic in one number: least stranded capacity in the high word, then the tie
                // in the low word.
                let stranded = self.stranded_after(need, &pick.shapes);
                let then = match tie {
                    Tie::Align => self.alignment(need),
                    Tie::LeastAllocated => u64::MAX.saturating_sub(self.allocated_after(need)),
                    Tie::MostAllocated => self.allocated_after(need),
                    // Fewer classes in the high bits; the allocation (at most 2,000,000) below.
                    Tie::Narrowest => {
                        u64::from(Classes::ALL.count().saturating_sub(self.classes.count()))
                            .checked_shl(32)
                            .unwrap_or(0)
                            | self.allocated_after(need)
                    }
                };
                u128::from(u64::MAX.saturating_sub(stranded))
                    .checked_mul(1 << 64)
                    .unwrap_or(0)
                    | u128::from(then)
            }
        }
    }

    /// CPU in use, background and placed, above 90% of the node's CPU. Compared as
    /// busy * 1000 > capacity * 900 in u128, where neither side can overflow; a busy count past u64
    /// is hot.
    fn hot(&self) -> bool {
        let busy = self
            .capacity
            .cpu_milli
            .checked_sub(self.free.cpu_milli)
            .and_then(|placed| placed.checked_add(self.background_milli));
        let Some(busy) = busy else {
            return true;
        };
        let lhs = u128::from(busy).checked_mul(1000);
        let rhs = u128::from(self.capacity.cpu_milli).checked_mul(u128::from(HOT_PER_MILLE));
        matches!((lhs, rhs), (Some(l), Some(r)) if l > r)
    }
}

/// The nodes' room and filter state, and the kernel's index of them by free whole CPUs
/// ([`crate::lb::Index`], kiln's `lb/index.rs`): a pick walks it from the need's bucket up, so it
/// reads only nodes with the CPU for the need, every one of them.
#[derive(Debug, Clone)]
pub struct FitIndex {
    costs: Costs,
    /// By node id; ids are dense from 0.
    slots: Vec<Slot>,
    /// Each node's devices, by node id.
    devices: Vec<Vec<Device>>,
    /// The workload holding each of a node's devices, until it is deleted.
    held: Vec<Vec<Option<WorkloadId>>>,
    ix: Index,
    /// The highest bucket: the most whole CPUs any node has.
    last: u32,
}

/// A node's bucket in the kernel's index: its free whole CPUs, at most `last`.
fn bucket(free: &Resources, last: u32) -> u32 {
    let whole = free.cpu_milli.checked_div(lb::CPU).unwrap_or(0);
    u32::try_from(whole).unwrap_or(u32::MAX).min(last)
}

/// A node's capacity as the kernel's amounts: CPU, memory, disk, GPUs.
fn amounts(r: &Resources) -> lb::Amounts {
    [r.cpu_milli, r.mem_bytes, r.disk_bytes, u64::from(r.gpus)]
}

impl FitIndex {
    /// Nodes' ids must be dense from 0, in order.
    pub fn new(nodes: &[NodeSpec]) -> Result<Self, SimError> {
        let mut slots = Vec::with_capacity(nodes.len());
        let (mut devices, mut held) = (
            Vec::with_capacity(nodes.len()),
            Vec::with_capacity(nodes.len()),
        );
        for (at, node) in nodes.iter().enumerate() {
            if usize::try_from(node.id.0).ok() != Some(at) {
                return Err(SimError::Config("node ids must be dense from 0, in order"));
            }
            if !node.devices_agree() {
                return Err(SimError::Config(
                    "a node's GPUs listed aren't its capacity's, or it lists more devices than a PCI segment has",
                ));
            }
            slots.push(Slot {
                capacity: node.capacity,
                free: node.capacity,
                background_milli: node.background_milli,
                classes: node.classes,
                platform: node.platform,
                kvm: node.virt.kvm,
                builds: 0,
                benchmarks: 0,
            });
            devices.push(node.devices.clone());
            held.push(vec![None; node.devices.len()]);
        }
        let whole = nodes
            .iter()
            .map(|n| n.capacity.cpu_milli.checked_div(lb::CPU).unwrap_or(0))
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let buckets = u32::try_from(whole).unwrap_or(BUCKETS).min(BUCKETS);
        let last = buckets.saturating_sub(1);
        let count = u32::try_from(nodes.len()).map_err(|_| SimError::Full {
            what: "the index's nodes",
            bound: usize::try_from(u32::MAX).unwrap_or(usize::MAX),
        })?;
        let mut ix = Index::new(0);
        if !ix.grow(buckets, count) {
            return Err(SimError::Config("the index can't grow to the cluster"));
        }
        for (slot, id) in slots.iter().zip(0u32..) {
            if !ix.join(id, &amounts(&slot.capacity)) {
                return Err(SimError::Config("the index can't hold the cluster"));
            }
            ix.push(id, bucket(&slot.free, last));
        }
        Ok(Self {
            costs: Costs::new(nodes.len()),
            slots,
            devices,
            held,
            ix,
            last,
        })
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn slot(&self, node: NodeId) -> Result<&Slot, SimError> {
        usize::try_from(node.0)
            .ok()
            .and_then(|at| self.slots.get(at))
            .ok_or(SimError::UnknownNode(node))
    }

    pub fn key(&self, node: NodeId) -> Result<Key, SimError> {
        Ok(Key::of(node, &self.slot(node)?.free))
    }

    pub fn verdict(&self, node: NodeId, demand: &Demand) -> Result<Verdict, SimError> {
        let at = usize::try_from(node.0).map_err(|_| SimError::UnknownNode(node))?;
        Ok(self.judge(at, self.slot(node)?, demand))
    }

    /// The verdict on `demand` at node `at`, its slot `slot`.
    fn judge(&self, at: usize, slot: &Slot, demand: &Demand) -> Verdict {
        let (devices, held) = (
            self.devices.get(at).map_or(&[][..], Vec::as_slice),
            self.held.get(at).map_or(&[][..], Vec::as_slice),
        );
        slot.verdict(demand, devices, |i| {
            held.get(i).is_some_and(Option::is_none)
        })
    }

    /// The devices `demand` would hold on node `at` now, least loaded GPUs first.
    fn bound(&self, at: usize, demand: &Demand) -> Option<Held> {
        let devices = self.devices.get(at)?;
        let held = self.held.get(at)?;
        bind(
            devices,
            |i| held.get(i).is_some_and(Option::is_none),
            demand.need.gpus,
            &demand.ask,
        )
    }

    /// The devices `workload` holds on `node`.
    pub fn holds(&self, node: NodeId, workload: WorkloadId) -> Held {
        let at = usize::try_from(node.0).unwrap_or(usize::MAX);
        self.held
            .get(at)
            .into_iter()
            .flatten()
            .zip(0u16..)
            .filter(|(h, _)| **h == Some(workload))
            .map(|(_, i)| i)
            .collect()
    }

    /// Live node facts, preserving reservations. Held devices retain their index/kind/model/size;
    /// capacity cannot shrink below the amounts already held. Failure changes nothing.
    pub(crate) fn check_refresh_node(&self, node: &NodeSpec) -> Result<(), SimError> {
        let at = usize::try_from(node.id.0).map_err(|_| SimError::UnknownNode(node.id))?;
        let old = self.slot(node.id)?;
        if !node.devices_agree() {
            return Err(SimError::Config("refreshed devices disagree with capacity"));
        }
        let used = old
            .capacity
            .checked_sub(&old.free)
            .ok_or(SimError::Resources(node.id))?;
        node.capacity
            .checked_sub(&used)
            .ok_or(SimError::Resources(node.id))?;
        let old_devices = self.devices.get(at).ok_or(SimError::UnknownNode(node.id))?;
        let holders = self.held.get(at).ok_or(SimError::UnknownNode(node.id))?;
        for (i, holder) in holders.iter().enumerate() {
            if holder.is_some()
                && !old_devices
                    .get(i)
                    .zip(node.devices.get(i))
                    .is_some_and(|(old, new)| {
                        old.kind == new.kind && old.model == new.model && old.size == new.size
                    })
            {
                return Err(SimError::Config(
                    "refreshed facts replace or remove a held device",
                ));
            }
        }
        Ok(())
    }

    pub fn refresh_node(&mut self, node: &NodeSpec) -> Result<(), SimError> {
        self.check_refresh_node(node)?;
        let at = usize::try_from(node.id.0).map_err(|_| SimError::UnknownNode(node.id))?;
        let old = self.slot(node.id)?;
        let used = old
            .capacity
            .checked_sub(&old.free)
            .ok_or(SimError::Resources(node.id))?;
        let free = node
            .capacity
            .checked_sub(&used)
            .ok_or(SimError::Resources(node.id))?;
        let holders = self.held.get(at).ok_or(SimError::UnknownNode(node.id))?;
        let mut held = vec![None; node.devices.len()];
        for (new, old) in held.iter_mut().zip(holders) {
            *new = *old;
        }
        let (old_capacity, old_bucket) = (old.capacity, bucket(&old.free, self.last));
        let builds = old.builds;
        let benchmarks = old.benchmarks;
        let slot = self
            .slots
            .get_mut(at)
            .ok_or(SimError::UnknownNode(node.id))?;
        *slot = Slot {
            capacity: node.capacity,
            free,
            background_milli: node.background_milli,
            classes: node.classes,
            platform: node.platform,
            kvm: node.virt.kvm,
            builds,
            benchmarks,
        };
        if let Some(devices) = self.devices.get_mut(at) {
            *devices = node.devices.clone();
        }
        if let Some(holders) = self.held.get_mut(at) {
            *holders = held;
        }
        self.ix
            .reshape(&amounts(&old_capacity), &amounts(&node.capacity));
        self.ix
            .relink(node.id.0, old_bucket, bucket(&free, self.last));
        Ok(())
    }

    /// A GPU's load from what isn't Hex's, as its node's agent reads it (hex-gpu's `sample()`).
    pub fn set_gpu_load(
        &mut self,
        node: NodeId,
        device: usize,
        load: GpuLoad,
    ) -> Result<(), SimError> {
        let d = usize::try_from(node.0)
            .ok()
            .and_then(|at| self.devices.get_mut(at))
            .and_then(|ds| ds.get_mut(device))
            .filter(|d| d.kind == DeviceKind::Gpu)
            .ok_or(SimError::UnknownNode(node))?;
        d.load = load;
        Ok(())
    }

    /// Whether `node`'s CPU in use is above 90% of its CPU.
    pub fn hot(&self, node: NodeId) -> Result<bool, SimError> {
        Ok(self.slot(node)?.hot())
    }

    /// Whether `node` runs `class`: a change to it can change a pick for that class.
    pub fn runs(&self, node: NodeId, class: Class) -> bool {
        self.slot(node).is_ok_and(|s| s.classes.has(class))
    }

    /// Whether any node admits `demand` now.
    pub fn admitted_anywhere(&self, demand: &Demand) -> bool {
        self.slots
            .iter()
            .enumerate()
            .any(|(at, slot)| self.judge(at, slot, demand) == Verdict::Admits)
    }

    /// The best node for `demand` by `pick`'s scorer, ties broken by [`Key`]'s order: the kernel's
    /// walk ([`crate::lb::Index`], kiln's `owner.rs` walk) from the bucket of the need's whole CPUs up,
    /// every node in it read and checked exactly. Kiln's walk stops at the first runner that fits,
    /// the least loaded; Hex's reads them all, since the packing score (H11) ranks by what each node
    /// would have left. Each node is read once, so the node count bounds it. GPU work ranks first by
    /// how idle the GPUs it would hold are ([`gpu_busy`]), as kiln ranks by CPU free: the less-loaded
    /// GPU wins.
    pub fn fit(&self, demand: &Demand, pick: &Pick) -> Fit {
        self.fit_excluding(demand, pick, &std::collections::BTreeSet::new())
    }

    /// The same kernel walk and scoring, excluding peers that explicitly refused this attempt.
    pub fn fit_excluding(
        &self,
        demand: &Demand,
        pick: &Pick,
        excluded: &std::collections::BTreeSet<NodeId>,
    ) -> Fit {
        self.fit_scored(demand, pick, None, excluded)
    }

    /// CP-T7.4/B4: exact kiln per-ask terms, sharing the same admission and packing core. No
    /// context uses the original path. Image identity stays in the caller's borrowed warm facts.
    pub fn fit_with_terms(
        &self,
        demand: &Demand,
        pick: &Pick,
        terms: &Terms<'_>,
    ) -> Result<Fit, SimError> {
        self.fit_with_terms_excluding(demand, pick, terms, &std::collections::BTreeSet::new())
    }

    pub fn fit_with_terms_excluding(
        &self,
        demand: &Demand,
        pick: &Pick,
        terms: &Terms<'_>,
        excluded: &std::collections::BTreeSet<NodeId>,
    ) -> Result<Fit, SimError> {
        let prepared = self.costs.prepare(terms)?;
        // No measured class service and no modeled component means no time signal. Keep exactly
        // the original single walk; a measured zero still counts as a real observation.
        if !terms.costs() && !self.costs.observed_class(demand.class) {
            return Ok(self.fit_excluding(demand, pick, excluded));
        }
        Ok(self.fit_scored(demand, pick, Some(&prepared), excluded))
    }

    /// Measured rates, converted off the pick path just as kiln converts a node's new word/link.
    pub fn set_rates(&mut self, node: NodeId, rates: Rates) -> Result<(), SimError> {
        self.costs.set_rates(node, rates)
    }

    /// Observe actual complete create/start service, including opaque image staging, for this
    /// node/class. Explicit modeled Terms omit this combined elapsed time, preventing double charge.
    pub fn observe_start(
        &mut self,
        node: NodeId,
        class: Class,
        elapsed_ns: u64,
    ) -> Result<(), SimError> {
        self.costs.observe(node, class, elapsed_ns)
    }

    pub fn start_observations(&self) -> Vec<StartObservation> {
        self.costs.snapshot()
    }

    /// Atomically restore at most one checked aggregate per node/class from the shell's durable record.
    pub fn restore_start_observations(
        &mut self,
        rows: &[StartObservation],
    ) -> Result<(), SimError> {
        self.costs.restore(rows)
    }

    fn fit_scored(
        &self,
        demand: &Demand,
        pick: &Pick,
        terms: Option<&Prepared<'_>>,
        excluded: &std::collections::BTreeSet<NodeId>,
    ) -> Fit {
        let floor = u32::try_from(demand.need.cpu_milli.checked_div(lb::CPU).unwrap_or(0))
            .unwrap_or(u32::MAX);
        let (mut best, mut best_score) = (None::<Key>, (0u64, 0u128));
        let mut soonest = None::<u128>;
        let mut gpu_rank = 0;
        let mut refused = Refusals::default();
        let mut walked = 0u64;
        let mut b = self.ix.at_or_above(floor.min(self.last));
        while let Some(bb) = b {
            for r in self.ix.bucket(bb) {
                if excluded.contains(&NodeId(r)) {
                    continue;
                }
                walked = walked.saturating_add(1);
                let Some(slot) = self.slots.get(lb::ix(r)) else {
                    continue;
                };
                match self.judge(lb::ix(r), slot, demand) {
                    Verdict::Admits => {}
                    Verdict::NoRoom => continue,
                    Verdict::NoClass => {
                        refused.class = refused.class.saturating_add(1);
                        continue;
                    }
                    Verdict::NoPlatform => {
                        refused.platform = refused.platform.saturating_add(1);
                        continue;
                    }
                    Verdict::NoGpu => {
                        refused.gpu = refused.gpu.saturating_add(1);
                        continue;
                    }
                    Verdict::NoDevice => {
                        refused.device = refused.device.saturating_add(1);
                        continue;
                    }
                    Verdict::Shares => {
                        refused.shares = refused.shares.saturating_add(1);
                        continue;
                    }
                    Verdict::Hot => {
                        refused.hot = refused.hot.saturating_add(1);
                        continue;
                    }
                }
                let key = Key::of(NodeId(r), &slot.free);
                let idle = if demand.need.gpus > 0 {
                    let devices = self.devices.get(lb::ix(r)).map_or(&[][..], Vec::as_slice);
                    self.bound(lb::ix(r), demand)
                        .map_or(0, |held| gpu_idle(devices, &held))
                } else {
                    0
                };
                let score = (idle, slot.score(&demand.need, pick));
                if let Some(terms) = terms {
                    let finish = self.costs.finish(
                        NodeId(r),
                        demand.class,
                        demand.need.cpu_milli,
                        slot.free.cpu_milli,
                        terms,
                    );
                    if soonest.is_none() || idle > gpu_rank {
                        gpu_rank = idle;
                        soonest = Some(finish);
                    } else if idle == gpu_rank {
                        soonest = soonest.map(|old| old.min(finish));
                    }
                }
                // A higher score wins; between equal scores, the least key.
                let wins =
                    best.is_none_or(|k| score > best_score || (score == best_score && key < k));
                if wins {
                    best = Some(key);
                    best_score = score;
                }
            }
            b = self.ix.above(bb);
        }
        if let (Some(terms), Some(soonest)) = (terms, soonest) {
            // Kiln's within_bound: packing decides inside the exact 1/64 finish band. Hex's
            // established GPU-load preference remains first; no scalar weights join these units.
            let bound = start_cost::within_bound(soonest);
            best = None;
            let mut packing = 0;
            let mut b = self.ix.at_or_above(floor.min(self.last));
            while let Some(bb) = b {
                for r in self.ix.bucket(bb) {
                    if excluded.contains(&NodeId(r)) {
                        continue;
                    }
                    walked = walked.saturating_add(1);
                    let Some(slot) = self.slots.get(lb::ix(r)) else {
                        continue;
                    };
                    if self.judge(lb::ix(r), slot, demand) != Verdict::Admits {
                        continue;
                    }
                    let idle = if demand.need.gpus > 0 {
                        let devices = self.devices.get(lb::ix(r)).map_or(&[][..], Vec::as_slice);
                        self.bound(lb::ix(r), demand)
                            .map_or(0, |held| gpu_idle(devices, &held))
                    } else {
                        0
                    };
                    if idle != gpu_rank
                        || self.costs.finish(
                            NodeId(r),
                            demand.class,
                            demand.need.cpu_milli,
                            slot.free.cpu_milli,
                            terms,
                        ) > bound
                    {
                        continue;
                    }
                    let key = Key::of(NodeId(r), &slot.free);
                    let score = slot.score(&demand.need, pick);
                    if best.is_none_or(|k| score > packing || (score == packing && key < k)) {
                        best = Some(key);
                        packing = score;
                    }
                }
                b = self.ix.above(bb);
            }
        }
        Fit {
            best,
            walked,
            refused,
        }
    }

    /// Places `workload`, of `demand`, on `node`: its need taken from the free capacity, its devices
    /// held by it ([`bind`]), its kind counted; the node's new key. Refused, changing nothing, where
    /// the room or the devices aren't free.
    pub fn take(
        &mut self,
        node: NodeId,
        workload: WorkloadId,
        demand: &Demand,
    ) -> Result<Key, SimError> {
        let at = usize::try_from(node.0).map_err(|_| SimError::UnknownNode(node))?;
        self.slot(node)?;
        let held = self.bound(at, demand).ok_or(SimError::Resources(node))?;
        let key = self.change(
            node,
            demand,
            |free| free.checked_sub(&demand.need),
            |n| n.checked_add(1),
        )?;
        if let Some(hs) = self.held.get_mut(at) {
            for i in held {
                if let Some(h) = hs.get_mut(usize::from(i)) {
                    *h = Some(workload);
                }
            }
        }
        Ok(key)
    }

    /// Ends `workload`, of `demand`, on `node`: its devices free again; the node's new key.
    pub fn give(
        &mut self,
        node: NodeId,
        workload: WorkloadId,
        demand: &Demand,
    ) -> Result<Key, SimError> {
        let key = self.change(
            node,
            demand,
            |free| free.checked_add(&demand.need),
            |n| n.checked_sub(1),
        )?;
        let at = usize::try_from(node.0).map_err(|_| SimError::UnknownNode(node))?;
        for h in self.held.get_mut(at).into_iter().flatten() {
            if *h == Some(workload) {
                *h = None;
            }
        }
        Ok(key)
    }

    fn change(
        &mut self,
        node: NodeId,
        demand: &Demand,
        resources: impl FnOnce(&Resources) -> Option<Resources>,
        count: impl FnOnce(u32) -> Option<u32>,
    ) -> Result<Key, SimError> {
        let at = usize::try_from(node.0).map_err(|_| SimError::UnknownNode(node))?;
        let slot = self.slots.get_mut(at).ok_or(SimError::UnknownNode(node))?;
        let mut next = *slot;
        next.free = resources(&slot.free).ok_or(SimError::Resources(node))?;
        match demand.kind {
            Kind::Build => next.builds = count(slot.builds).ok_or(SimError::Resources(node))?,
            Kind::Benchmark => {
                next.benchmarks = count(slot.benchmarks).ok_or(SimError::Resources(node))?;
            }
            Kind::Plain | Kind::Session => {}
        }
        let (from, to) = (bucket(&slot.free, self.last), bucket(&next.free, self.last));
        *slot = next;
        self.ix.relink(node.0, from, to);
        Ok(Key::of(node, &next.free))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RuntimePool;

    fn res(cpu: u64, mem: u64, gpus: u32) -> Resources {
        Resources {
            cpu_milli: cpu,
            mem_bytes: mem,
            gpus,
            disk_bytes: 0,
        }
    }

    /// A workload's id where only one is placed at a time.
    const W: WorkloadId = WorkloadId(0);

    fn plain(need: Resources) -> Demand {
        Demand {
            need,
            kind: Kind::Plain,
            class: Class::Oci,
            ask: Default::default(),
        }
    }

    fn nodes(caps: &[Resources]) -> Vec<NodeSpec> {
        caps.iter()
            .enumerate()
            .map(|(i, c)| NodeSpec::new(NodeId(u32::try_from(i).unwrap()), *c))
            .collect()
    }

    fn pool_node(classes: Classes, mem: u64) -> NodeSpec {
        let mut node = NodeSpec::new(NodeId(0), res(4_000, 1_024 << 20, 0));
        node.runtime_pools = vec![RuntimePool {
            classes,
            capacity: res(4_000, mem, 0),
        }];
        node
    }

    fn pool_demand(class: Class, mem: u64) -> Demand {
        Demand {
            class,
            ..plain(res(100, mem, 0))
        }
    }

    /// AC-T7.4.1: the real 633MiB-in-632MiB counterexample must fail before a guest create.
    #[test]
    fn runtime_pool_r329_guest_bound_refuses_633_mib_but_admits_64() {
        let node = pool_node(Classes::NONE.with(Class::Shards), 632 << 20);
        let mut index = FitIndex::new(&[node]).unwrap();
        let before = index.key(NodeId(0)).unwrap();
        let oversized = pool_demand(Class::Shards, 633 << 20);
        assert_eq!(
            index.verdict(NodeId(0), &oversized).unwrap(),
            Verdict::NoRoom
        );
        assert!(index.fit(&oversized, &Pick::default()).best.is_none());
        assert!(index.take(NodeId(0), W, &oversized).is_err());
        assert_eq!(index.key(NodeId(0)).unwrap(), before);
        let valid = pool_demand(Class::Shards, 64 << 20);
        index.take(NodeId(0), W, &valid).unwrap();
        index.give(NodeId(0), W, &valid).unwrap();
        assert_eq!(index.key(NodeId(0)).unwrap(), before);
    }

    /// AC-T7.4.1: a guest cap does not strand unrelated native host capacity; both budgets apply.
    #[test]
    fn runtime_pool_keeps_unpooled_native_room_and_global_host_limit() {
        let node = pool_node(Classes::NONE.with(Class::Shards), 632 << 20);
        let mut index = FitIndex::new(&[node]).unwrap();
        let native = pool_demand(Class::Oci, 900 << 20);
        assert_eq!(index.verdict(NodeId(0), &native).unwrap(), Verdict::Admits);
        index.take(NodeId(0), WorkloadId(1), &native).unwrap();
        assert_eq!(
            index
                .verdict(NodeId(0), &pool_demand(Class::Shards, 128 << 20))
                .unwrap(),
            Verdict::NoRoom
        );
        assert_eq!(
            index
                .verdict(NodeId(0), &pool_demand(Class::Shards, 64 << 20))
                .unwrap(),
            Verdict::Admits
        );
    }

    /// AC-T7.4.1: two class names backed by one runtime share one total, and a refused take is atomic.
    #[test]
    fn runtime_pool_shared_classes_aggregate_and_delete_frees_exactly() {
        let classes = Classes::NONE.with(Class::Shards).with(Class::Firecracker);
        let mut index = FitIndex::new(&[pool_node(classes, 100 << 20)]).unwrap();
        let original = index.key(NodeId(0)).unwrap();
        let shards = pool_demand(Class::Shards, 60 << 20);
        index.take(NodeId(0), WorkloadId(1), &shards).unwrap();
        let before = index.key(NodeId(0)).unwrap();
        let mut fail = pool_demand(Class::Firecracker, 41 << 20);
        fail.kind = Kind::Build;
        assert!(index.take(NodeId(0), WorkloadId(2), &fail).is_err());
        assert_eq!(index.key(NodeId(0)).unwrap(), before);
        let benchmark = Demand {
            kind: Kind::Benchmark,
            ..pool_demand(Class::Oci, 1)
        };
        assert_eq!(
            index.verdict(NodeId(0), &benchmark).unwrap(),
            Verdict::Admits
        );
        let firecracker = pool_demand(Class::Firecracker, 40 << 20);
        index.take(NodeId(0), WorkloadId(3), &firecracker).unwrap();
        assert_eq!(
            index
                .verdict(NodeId(0), &pool_demand(Class::Shards, 1))
                .unwrap(),
            Verdict::NoRoom
        );
        index.give(NodeId(0), WorkloadId(1), &shards).unwrap();
        assert_eq!(index.verdict(NodeId(0), &shards).unwrap(), Verdict::Admits);
        index.give(NodeId(0), WorkloadId(3), &firecracker).unwrap();
        assert_eq!(index.key(NodeId(0)).unwrap(), original);
        assert!(index.give(NodeId(0), WorkloadId(3), &firecracker).is_err());
        assert_eq!(index.key(NodeId(0)).unwrap(), original);
    }

    /// AC-T7.4.1: refresh may split a shared pool, but never remove/shrink a held runtime budget.
    #[test]
    fn runtime_pool_refresh_preserves_class_charges_and_refuses_invalid_shrink_atomically() {
        let classes = Classes::NONE.with(Class::Shards).with(Class::Firecracker);
        let mut node = pool_node(classes, 100 << 20);
        let mut index = FitIndex::new(&[node.clone()]).unwrap();
        let shards = pool_demand(Class::Shards, 60 << 20);
        let firecracker = pool_demand(Class::Firecracker, 30 << 20);
        index.take(NodeId(0), WorkloadId(1), &shards).unwrap();
        index.take(NodeId(0), WorkloadId(2), &firecracker).unwrap();
        node.runtime_pools = vec![
            RuntimePool {
                classes: Classes::NONE.with(Class::Shards),
                capacity: res(4_000, 70 << 20, 0),
            },
            RuntimePool {
                classes: Classes::NONE.with(Class::Firecracker),
                capacity: res(4_000, 30 << 20, 0),
            },
        ];
        index.refresh_node(&node).unwrap();
        let before = index.key(NodeId(0)).unwrap();
        let mut shrink = node.clone();
        shrink.capacity.cpu_milli = 10_000;
        shrink.runtime_pools[0].capacity.mem_bytes = 59 << 20;
        assert!(index.refresh_node(&shrink).is_err());
        assert_eq!(index.key(NodeId(0)).unwrap(), before);
        assert_eq!(
            index
                .verdict(NodeId(0), &pool_demand(Class::Shards, 10 << 20))
                .unwrap(),
            Verdict::Admits
        );
        assert_eq!(
            index
                .verdict(NodeId(0), &pool_demand(Class::Firecracker, 1))
                .unwrap(),
            Verdict::NoRoom
        );
        let mut removed = node.clone();
        removed.runtime_pools.clear();
        assert!(index.refresh_node(&removed).is_err());
        index.give(NodeId(0), WorkloadId(1), &shards).unwrap();
        index.give(NodeId(0), WorkloadId(2), &firecracker).unwrap();
        index.refresh_node(&removed).unwrap();
        assert_eq!(
            index
                .verdict(NodeId(0), &pool_demand(Class::Shards, 900 << 20))
                .unwrap(),
            Verdict::Admits
        );
    }

    /// AC-T7.4.1: a newly declared pool must cover already held amounts of every included class.
    #[test]
    fn runtime_pool_new_pool_checks_preexisting_host_backed_class_holds() {
        let mut node = NodeSpec::new(NodeId(0), res(4_000, 1_024 << 20, 0));
        let mut index = FitIndex::new(&[node.clone()]).unwrap();
        let demand = pool_demand(Class::Oci, 64 << 20);
        index.take(NodeId(0), W, &demand).unwrap();
        let before = index.key(NodeId(0)).unwrap();
        node.runtime_pools = vec![RuntimePool {
            classes: Classes::NONE.with(Class::Oci),
            capacity: res(4_000, 63 << 20, 0),
        }];
        assert!(index.refresh_node(&node).is_err());
        assert_eq!(index.key(NodeId(0)).unwrap(), before);
        node.runtime_pools[0].capacity.mem_bytes = 64 << 20;
        index.refresh_node(&node).unwrap();
        assert_eq!(
            index
                .verdict(NodeId(0), &pool_demand(Class::Oci, 1))
                .unwrap(),
            Verdict::NoRoom
        );
    }

    /// AC-T7.4.1: pool masks are nonempty, disjoint and within the node's declared class set.
    #[test]
    fn runtime_pool_invalid_masks_and_excess_count_are_typed_refusals() {
        let one = RuntimePool {
            classes: Classes::NONE.with(Class::Shards),
            capacity: res(4_000, 100 << 20, 0),
        };
        let mut node = pool_node(Classes::NONE, 100 << 20);
        assert!(FitIndex::new(&[node.clone()]).is_err());
        node.runtime_pools = vec![one, one];
        assert!(FitIndex::new(&[node.clone()]).is_err());
        node.runtime_pools = vec![one];
        node.classes = Classes::NONE.with(Class::Oci);
        assert!(FitIndex::new(&[node.clone()]).is_err());
        node.classes = Classes::ALL;
        node.runtime_pools = vec![one; Class::COUNT + 1];
        assert!(matches!(FitIndex::new(&[node]), Err(SimError::Full { .. })));
    }

    /// AC-T7.4.1: absent pools retain existing host-backed admission, including after reconstruction.
    #[test]
    fn runtime_pool_empty_defaults_keep_host_model_and_replay_charges_on_restart() {
        let node = NodeSpec::new(NodeId(0), res(4_000, 1_024 << 20, 0));
        assert!(node.runtime_pools.is_empty());
        let demand = pool_demand(Class::Shards, 633 << 20);
        let mut first = FitIndex::new(&[node.clone()]).unwrap();
        first.take(NodeId(0), W, &demand).unwrap();
        let mut rebooted = FitIndex::new(&[node]).unwrap();
        rebooted.take(NodeId(0), W, &demand).unwrap();
        assert_eq!(
            first.key(NodeId(0)).unwrap(),
            rebooted.key(NodeId(0)).unwrap()
        );
    }

    /// Brute force: the least key among the nodes that admit the demand.
    fn oracle(index: &FitIndex, demand: &Demand) -> Option<Key> {
        (0..index.len())
            .map(|i| NodeId(u32::try_from(i).unwrap()))
            .filter(|&n| index.verdict(n, demand).unwrap() == Verdict::Admits)
            .map(|n| index.key(n).unwrap())
            .min()
    }

    /// T-T7.4.1: a whole-cluster sample is the least-free node that admits the need; GPU work goes
    /// only on a GPU node.
    #[test]
    fn whole_best_fit_is_the_least_free_node_that_admits() {
        let index = FitIndex::new(&nodes(&[
            res(8000, 16, 0),
            res(2000, 16, 0),
            res(4000, 4, 1),
        ]))
        .unwrap();
        let fit = |need| {
            index
                .fit(&plain(need), &Pick::default())
                .best
                .map(|k| k.node)
        };
        assert_eq!(fit(res(1000, 8, 0)), Some(NodeId(1)));
        assert_eq!(fit(res(3000, 8, 0)), Some(NodeId(0)));
        assert_eq!(fit(res(1000, 1, 1)), Some(NodeId(2)));
        assert_eq!(fit(res(1000, 8, 1)), None);
    }

    /// CP-T7.4/B4: measured startup can move a pick, packing still wins inside kiln's 1/64
    /// finish band, and the original no-context core is unaffected by learned timing state.
    #[test]
    fn start_cost_changes_the_actual_pick_but_retains_packing_ties() {
        for (times, expected) in [([1000, 990], NodeId(0)), ([1000, 500], NodeId(1))] {
            let mut index = FitIndex::new(&nodes(&[res(2000, 8, 0), res(4000, 8, 0)])).unwrap();
            let demand = plain(res(1000, 1, 0));
            for (node, ns) in [NodeId(0), NodeId(1)].into_iter().zip(times) {
                index.observe_start(node, Class::Oci, ns).unwrap();
            }
            assert_eq!(
                index
                    .fit_with_terms(&demand, &Pick::default(), &Terms::default())
                    .unwrap()
                    .best
                    .map(|k| k.node),
                Some(expected)
            );
            assert_eq!(
                index.fit(&demand, &Pick::default()).best.map(|k| k.node),
                Some(NodeId(0))
            );
            let shards = Demand {
                class: Class::Shards,
                ..demand
            };
            assert_eq!(
                index
                    .fit_with_terms(&shards, &Pick::default(), &Terms::default())
                    .unwrap()
                    .best
                    .map(|k| k.node),
                Some(NodeId(0))
            );
        }
    }

    /// CP-T7.4: calling the production terms path before learning anything costs the same single
    /// candidate walk; another class's observations do not activate a model for this request.
    #[test]
    fn start_cost_empty_context_keeps_the_original_single_walk() {
        let mut index =
            FitIndex::new(&nodes(&[res(2000, 8, 0), res(4000, 8, 0), res(8000, 8, 0)])).unwrap();
        let demand = plain(res(1000, 1, 0));
        let pick = Pick::default();
        let original = index.fit(&demand, &pick);
        let cold = index
            .fit_with_terms(&demand, &pick, &Terms::default())
            .unwrap();
        assert_eq!(
            (cold.best, cold.walked, cold.refused),
            (original.best, original.walked, original.refused)
        );
        index.observe_start(NodeId(0), Class::Shards, 100).unwrap();
        assert_eq!(
            index
                .fit_with_terms(&demand, &pick, &Terms::default())
                .unwrap()
                .walked,
            original.walked
        );
        index.observe_start(NodeId(0), Class::Oci, 100).unwrap();
        let learned = index
            .fit_with_terms(&demand, &pick, &Terms::default())
            .unwrap();
        assert_eq!(learned.walked, original.walked * 2);
    }

    /// CP-T7.4: requested image warmth is borrowed per ask, never retained under a hash or
    /// leaked to the next image; refused nodes remain excluded even when their start is free.
    #[test]
    fn start_cost_image_terms_and_exclusions_share_the_admission_core() {
        use crate::start_cost::{Link, Warm};
        let mut index = FitIndex::new(&nodes(&[res(2000, 8, 0), res(4000, 8, 0)])).unwrap();
        for node in [NodeId(0), NodeId(1)] {
            index
                .set_rates(
                    node,
                    Rates {
                        link: Link {
                            rate: 1_000_000_000,
                            ..Link::default()
                        },
                        ..Rates::default()
                    },
                )
                .unwrap();
        }
        let demand = plain(res(1000, 1, 0));
        let warm = [Warm {
            node: NodeId(1),
            ..Warm::default()
        }];
        let image = Terms {
            bytes: 1000,
            warm: &warm,
            ..Terms::default()
        };
        assert_eq!(
            index
                .fit_with_terms(&demand, &Pick::default(), &image)
                .unwrap()
                .best
                .map(|k| k.node),
            Some(NodeId(1))
        );
        let excluded = [NodeId(1)].into_iter().collect();
        assert_eq!(
            index
                .fit_with_terms_excluding(&demand, &Pick::default(), &image, &excluded)
                .unwrap()
                .best
                .map(|k| k.node),
            Some(NodeId(0))
        );
        let another_image = Terms { warm: &[], ..image };
        assert_eq!(
            index
                .fit_with_terms(&demand, &Pick::default(), &another_image)
                .unwrap()
                .best
                .map(|k| k.node),
            Some(NodeId(0))
        );
    }

    /// CP-T7.4: learned time never overrides Hex's existing least-loaded GPU preference.
    #[test]
    fn start_cost_preserves_the_existing_gpu_load_preference() {
        use crate::model::{Ask, Os};
        let mut index = FitIndex::new(&[
            gpu_node(0, 4_000, Os::Linux, &[(1, 8 << 30, 700)]),
            gpu_node(1, 8_000, Os::Linux, &[(1, 8 << 30, 100)]),
        ])
        .unwrap();
        index.observe_start(NodeId(0), Class::Oci, 1).unwrap();
        index.observe_start(NodeId(1), Class::Oci, 1000).unwrap();
        let demand = asking(1, Class::Oci, Ask::default());
        assert_eq!(
            index
                .fit_with_terms(&demand, &Pick::default(), &Terms::default())
                .unwrap()
                .best
                .map(|k| k.node),
            Some(NodeId(1))
        );
    }

    /// S3's filters in Hex's kinds (AC-T7.4.3's workload): do a Firecracker microVM where one node
    /// runs only OCI containers and the other every class; expect the first to refuse it as NoClass
    /// and the scan to count that refusal, and an OCI container still to fit both.
    #[test]
    fn a_class_the_node_cant_run_is_refused() {
        let mut nodes = nodes(&[res(8_000, 8, 0), res(8_000, 8, 0)]);
        nodes[0].classes = Classes::NONE.with(Class::Oci);
        let index = FitIndex::new(&nodes).unwrap();
        let microvm = Demand {
            need: res(1_000, 1, 0),
            kind: Kind::Plain,
            class: Class::Firecracker,
            ask: Default::default(),
        };
        assert_eq!(
            index.verdict(NodeId(0), &microvm).unwrap(),
            Verdict::NoClass
        );
        assert_eq!(index.verdict(NodeId(1), &microvm).unwrap(), Verdict::Admits);
        let s = index.fit(&microvm, &Pick::default());
        assert_eq!(
            (s.best.map(|k| k.node), s.refused.class),
            (Some(NodeId(1)), 1)
        );
        let container = plain(res(1_000, 1, 0));
        assert_eq!(
            index.verdict(NodeId(0), &container).unwrap(),
            Verdict::Admits
        );
    }

    /// S3's GPU filter in Hex's kinds: do an OCI container, a Firecracker microVM and a Shards
    /// microVM, each asking a GPU, on a GPU node that runs every class; expect the container admitted
    /// and each microVM refused as NoGpu (neither can be given a GPU), so no node admits it and
    /// admission turns it away.
    #[test]
    fn only_an_oci_container_is_given_a_gpu() {
        let index = FitIndex::new(&nodes(&[res(8_000, 8, 2)])).unwrap();
        let asking = |class| Demand {
            need: res(1_000, 1, 1),
            kind: Kind::Plain,
            class,
            ask: Default::default(),
        };
        assert_eq!(
            index.verdict(NodeId(0), &asking(Class::Oci)).unwrap(),
            Verdict::Admits
        );
        for class in [Class::Firecracker, Class::Shards] {
            assert_eq!(
                index.verdict(NodeId(0), &asking(class)).unwrap(),
                Verdict::NoGpu
            );
            assert!(!index.admitted_anywhere(&asking(class)));
        }
    }

    /// AC-T7.6.3 and S3's filters, each alone: a benchmark refused beside a build and a build beside a
    /// benchmark; a session refused a node above 90% CPU, counting its background; GPU work refused a
    /// node without GPUs.
    #[test]
    fn each_filter_refuses_what_it_names() {
        let mut ns = nodes(&[res(10_000, 100, 0), res(10_000, 100, 2)]);
        ns[0].background_milli = 9_100;
        let mut index = FitIndex::new(&ns).unwrap();
        let session = Demand {
            need: res(1_000, 1, 0),
            kind: Kind::Session,
            class: Class::Oci,
            ask: Default::default(),
        };
        assert_eq!(index.verdict(NodeId(0), &session).unwrap(), Verdict::Hot);
        assert_eq!(index.verdict(NodeId(1), &session).unwrap(), Verdict::Admits);
        let gpu = plain(res(1, 1, 1));
        assert_eq!(index.verdict(NodeId(0), &gpu).unwrap(), Verdict::NoGpu);
        let build = Demand {
            need: res(1, 1, 0),
            kind: Kind::Build,
            class: Class::Oci,
            ask: Default::default(),
        };
        let bench = Demand {
            need: res(1, 1, 0),
            kind: Kind::Benchmark,
            class: Class::Oci,
            ask: Default::default(),
        };
        index.take(NodeId(1), W, &build).unwrap();
        assert_eq!(index.verdict(NodeId(1), &bench).unwrap(), Verdict::Shares);
        index.give(NodeId(1), W, &build).unwrap();
        index.take(NodeId(1), W, &bench).unwrap();
        assert_eq!(index.verdict(NodeId(1), &build).unwrap(), Verdict::Shares);
        let s = index.fit(&build, &Pick::default());
        assert_eq!(
            (s.best.map(|k| k.node), s.refused.shares),
            (Some(NodeId(0)), 1)
        );
    }

    fn gpu_node(id: u32, cpu: u64, os: crate::model::Os, gpus: &[(u32, u64, u16)]) -> NodeSpec {
        use crate::model::{Device, GpuLoad, ModelId, Platform};
        let mut n = NodeSpec::new(NodeId(id), res(cpu, 64, 0));
        n.platform = Platform {
            os,
            arch: crate::model::Arch::X86_64,
        };
        n.classes = os.classes(crate::model::Virt {
            kvm: true,
            nested: false,
            hvf: true,
        });
        n.devices = gpus
            .iter()
            .map(|&(model, memory, busy)| Device {
                load: GpuLoad {
                    busy_per_mille: busy,
                    memory_used: 0,
                },
                ..Device::gpu(ModelId(model), memory)
            })
            .collect();
        n.capacity.gpus = u32::try_from(gpus.len()).unwrap();
        n
    }

    fn asking(gpus: u32, class: Class, ask: crate::model::Ask) -> Demand {
        Demand {
            need: res(1_000, 1, gpus),
            kind: Kind::Plain,
            class,
            ask,
        }
    }

    /// AC-T7.4.1: image OS follows the initialized class; host capability classes stay unchanged.
    #[test]
    fn image_os_matches_vm_guest_and_oci_host() {
        use crate::model::{Arch, Ask, Os, Virt};
        let mut mac = NodeSpec::new(NodeId(0), res(4_000, 16, 0));
        mac.platform = Platform {
            os: Os::Macos,
            arch: Arch::Aarch64,
        };
        mac.virt = Virt {
            hvf: true,
            ..Virt::default()
        };
        mac.classes = Os::Macos.classes(mac.virt);
        let mut windows = NodeSpec::new(NodeId(1), res(4_000, 16, 0));
        windows.platform.os = Os::Windows;
        windows.classes = Os::Windows.classes(windows.virt);
        let linux = NodeSpec::new(NodeId(2), res(4_000, 16, 0));
        let ix = FitIndex::new(&[mac, windows, linux]).unwrap();
        let guest = |os, arch| {
            asking(
                0,
                Class::Shards,
                Ask {
                    os: Some(os),
                    arch: Some(arch),
                    ..Ask::default()
                },
            )
        };
        assert_eq!(
            ix.verdict(NodeId(0), &guest(Os::Linux, Arch::Aarch64))
                .unwrap(),
            Verdict::Admits
        );
        assert_eq!(
            ix.verdict(NodeId(0), &guest(Os::Macos, Arch::Aarch64))
                .unwrap(),
            Verdict::NoPlatform
        );
        assert_eq!(
            ix.verdict(NodeId(0), &guest(Os::Linux, Arch::X86_64))
                .unwrap(),
            Verdict::NoPlatform
        );
        let win = asking(
            0,
            Class::Oci,
            Ask {
                os: Some(Os::Windows),
                ..Ask::default()
            },
        );
        assert_eq!(ix.verdict(NodeId(1), &win).unwrap(), Verdict::Admits);
        assert_eq!(ix.verdict(NodeId(2), &win).unwrap(), Verdict::NoPlatform);
        let mut firecracker = guest(Os::Linux, Arch::X86_64);
        firecracker.class = Class::Firecracker;
        assert_eq!(
            ix.verdict(NodeId(2), &firecracker).unwrap(),
            Verdict::Admits
        );
        firecracker.ask.os = Some(Os::Macos);
        assert_eq!(
            ix.verdict(NodeId(2), &firecracker).unwrap(),
            Verdict::NoPlatform
        );
    }

    /// CP-T7.4, the needs model's filters: do ask an image's platform, a GPU by model and by memory,
    /// and devices of each kind, of each class, on nodes of each OS; expect each node that can't give
    /// it refused for the reason it can't (another platform; no GPU of the model or with the memory;
    /// a class that can't be given the device there: Firecracker and Shards none, OCI on Windows only
    /// a GPU), each refusal counted by the walk (non-vacuity), and the node that can admitting it.
    #[test]
    fn platform_gpus_and_devices_are_matched_against_node_and_class() {
        use crate::model::{Arch, Ask, Device, DeviceKind, GpuLoad, ModelId, Os};
        const GIB: u64 = 1 << 30;
        let mut linux = gpu_node(0, 8_000, Os::Linux, &[(1, 8 * GIB, 0)]);
        linux.devices.push(Device {
            kind: DeviceKind::Vfio,
            model: ModelId::UNNAMED,
            size: 0,
            load: GpuLoad::default(),
        });
        linux.devices.push(Device {
            kind: DeviceKind::Block,
            model: ModelId::UNNAMED,
            size: 100 * GIB,
            load: GpuLoad::default(),
        });
        let windows = gpu_node(1, 8_000, Os::Windows, &[(2, 16 * GIB, 0)]);
        let mut index = FitIndex::new(&[linux, windows]).unwrap();
        let verdicts = |index: &FitIndex, d: &Demand| {
            [NodeId(0), NodeId(1)].map(|n| index.verdict(n, d).unwrap())
        };
        // The image's platform.
        let arm = asking(
            0,
            Class::Oci,
            Ask {
                arch: Some(Arch::Aarch64),
                ..Ask::default()
            },
        );
        assert_eq!(
            verdicts(&index, &arm),
            [Verdict::NoPlatform, Verdict::NoPlatform]
        );
        let win = asking(
            0,
            Class::Oci,
            Ask {
                os: Some(Os::Windows),
                ..Ask::default()
            },
        );
        assert_eq!(
            verdicts(&index, &win),
            [Verdict::NoPlatform, Verdict::Admits]
        );
        // A GPU by model, by memory, and only to OCI.
        let model1 = asking(
            1,
            Class::Oci,
            Ask {
                gpu_model: Some(ModelId(1)),
                ..Ask::default()
            },
        );
        assert_eq!(verdicts(&index, &model1), [Verdict::Admits, Verdict::NoGpu]);
        let big = asking(
            1,
            Class::Oci,
            Ask {
                gpu_memory: 12 * GIB,
                ..Ask::default()
            },
        );
        assert_eq!(verdicts(&index, &big), [Verdict::NoGpu, Verdict::Admits]);
        let shards_gpu = asking(1, Class::Shards, Ask::default());
        assert_eq!(
            verdicts(&index, &shards_gpu),
            [Verdict::NoGpu, Verdict::NoGpu]
        );
        // Devices whole: OCI on Linux alone; Windows' OCI is given only a GPU.
        let vfio = asking(
            0,
            Class::Oci,
            Ask {
                vfio: 1,
                ..Ask::default()
            },
        );
        assert_eq!(
            verdicts(&index, &vfio),
            [Verdict::Admits, Verdict::NoDevice]
        );
        for class in [Class::Firecracker, Class::Shards] {
            assert_eq!(
                index
                    .verdict(
                        NodeId(0),
                        &asking(
                            0,
                            class,
                            Ask {
                                vfio: 1,
                                ..Ask::default()
                            }
                        )
                    )
                    .unwrap(),
                Verdict::NoDevice
            );
        }
        let disk = asking(
            0,
            Class::Oci,
            Ask {
                block: 1,
                block_size: 200 * GIB,
                ..Ask::default()
            },
        );
        assert_eq!(index.verdict(NodeId(0), &disk).unwrap(), Verdict::NoDevice);
        let kvm = asking(
            0,
            Class::Oci,
            Ask {
                kvm: true,
                ..Ask::default()
            },
        );
        assert_eq!(verdicts(&index, &kvm), [Verdict::Admits, Verdict::NoDevice]);
        // Held until deleted: the one VFIO function taken, the next ask finds no room; given back, it
        // does.
        index.take(NodeId(0), WorkloadId(7), &vfio).unwrap();
        assert_eq!(index.holds(NodeId(0), WorkloadId(7)), vec![1]);
        assert_eq!(index.verdict(NodeId(0), &vfio).unwrap(), Verdict::NoRoom);
        assert_eq!(index.fit(&vfio, &Pick::default()).best, None);
        index.give(NodeId(0), WorkloadId(7), &vfio).unwrap();
        assert_eq!(index.holds(NodeId(0), WorkloadId(7)), Vec::<u16>::new());
        assert_eq!(index.verdict(NodeId(0), &vfio).unwrap(), Verdict::Admits);
        // The walk counts each refusal.
        let f = index.fit(&vfio, &Pick::default());
        assert_eq!(
            (f.best.map(|k| k.node), f.refused.device),
            (Some(NodeId(0)), 1)
        );
        let f = index.fit(&win, &Pick::default());
        assert_eq!(
            (f.best.map(|k| k.node), f.refused.platform),
            (Some(NodeId(1)), 1)
        );
        let f = index.fit(&model1, &Pick::default());
        assert_eq!(
            (f.best.map(|k| k.node), f.refused.gpu),
            (Some(NodeId(0)), 1)
        );
    }

    /// CP-T7.4, GPU load: do place a GPU box where two nodes match it, the one packing prefers (less
    /// CPU free) with its GPU 70% busy and the other 10%; expect the less-loaded GPU to win, then,
    /// the loads swapped as hex-gpu's samples would say, the other; and within a node, the idlest of
    /// its GPUs held, memory in use counting as load.
    #[test]
    fn the_less_loaded_gpu_wins() {
        use crate::model::{Ask, GpuLoad, Os};
        const GIB: u64 = 1 << 30;
        let mut index = FitIndex::new(&[
            gpu_node(0, 4_000, Os::Linux, &[(1, 8 * GIB, 700)]),
            gpu_node(1, 8_000, Os::Linux, &[(1, 8 * GIB, 100)]),
        ])
        .unwrap();
        let gpu = asking(1, Class::Oci, Ask::default());
        let cpu_only = asking(0, Class::Oci, Ask::default());
        let pick = Pick {
            scorer: Scorer::HEX,
            ..Pick::default()
        };
        // Packing alone sends work to node 0.
        assert_eq!(
            index.fit(&cpu_only, &pick).best.map(|k| k.node),
            Some(NodeId(0))
        );
        assert_eq!(index.fit(&gpu, &pick).best.map(|k| k.node), Some(NodeId(1)));
        index
            .set_gpu_load(
                NodeId(0),
                0,
                GpuLoad {
                    busy_per_mille: 50,
                    memory_used: 0,
                },
            )
            .unwrap();
        index
            .set_gpu_load(
                NodeId(1),
                0,
                GpuLoad {
                    busy_per_mille: 900,
                    memory_used: 0,
                },
            )
            .unwrap();
        assert_eq!(index.fit(&gpu, &pick).best.map(|k| k.node), Some(NodeId(0)));
        // Memory in use is load too: node 0's GPU idle but 95% of its memory in use.
        index
            .set_gpu_load(
                NodeId(0),
                0,
                GpuLoad {
                    busy_per_mille: 0,
                    memory_used: 8 * GIB / 20 * 19,
                },
            )
            .unwrap();
        assert_eq!(index.fit(&gpu, &pick).best.map(|k| k.node), Some(NodeId(1)));
        // Within a node: the idlest GPU is held.
        let mut two = FitIndex::new(&[gpu_node(
            0,
            8_000,
            Os::Linux,
            &[(1, 8 * GIB, 600), (1, 8 * GIB, 200)],
        )])
        .unwrap();
        two.take(NodeId(0), WorkloadId(1), &gpu).unwrap();
        assert_eq!(two.holds(NodeId(0), WorkloadId(1)), vec![1]);
        two.take(NodeId(0), WorkloadId(2), &gpu).unwrap();
        assert_eq!(two.holds(NodeId(0), WorkloadId(2)), vec![0]);
        two.give(NodeId(0), WorkloadId(1), &gpu).unwrap();
        assert_eq!(two.holds(NodeId(0), WorkloadId(2)), vec![0]);
        assert_eq!(two.verdict(NodeId(0), &gpu).unwrap(), Verdict::Admits);
    }

    #[test]
    fn ids_out_of_order_are_refused() {
        let mut ns = nodes(&[res(1, 1, 0), res(1, 1, 0)]);
        ns[1].id = NodeId(5);
        assert!(matches!(FitIndex::new(&ns), Err(SimError::Config(_))));
    }

    #[test]
    fn taking_more_than_is_free_is_refused_and_changes_nothing() {
        let mut index = FitIndex::new(&nodes(&[res(1000, 10, 0)])).unwrap();
        assert_eq!(
            index.take(NodeId(0), W, &plain(res(2000, 1, 0))),
            Err(SimError::Resources(NodeId(0)))
        );
        assert_eq!(index.key(NodeId(0)).unwrap().cpu_milli, 1000);
        assert_eq!(
            index.take(NodeId(3), W, &plain(res(1, 1, 0))),
            Err(SimError::UnknownNode(NodeId(3)))
        );
    }

    /// CLAUDE.md §3: do give `Shapes::of` as many incomparable shapes as its bound, then one more;
    /// expect the first taken whole and the second refused, typed, never cut to its smallest: none of
    /// them fits wherever another does, so a dropped one could leave a usable leftover read as
    /// stranded.
    #[test]
    fn more_shapes_than_the_bound_are_refused_never_cut() {
        let incomparable = |n: u64| -> Vec<(Resources, Class)> {
            (0..n)
                .map(|i| (res(1 + i, 100 - i, 0), Class::Oci))
                .collect()
        };
        let bound = u64::try_from(Shapes::MAX).unwrap();
        assert_eq!(
            Shapes::of(incomparable(bound)).unwrap().iter().count(),
            Shapes::MAX
        );
        assert_eq!(
            Shapes::of(incomparable(bound + 1)),
            Err(SimError::Full {
                what: "a sample's workload shapes",
                bound: Shapes::MAX
            })
        );
    }

    proptest::proptest! {
        /// T-T7.4.1 (oracle test): after any sequence of takes and gives of any kinds, the whole-cluster
        /// sampled fit equals the brute-force least admitting key.
        #[test]
        fn whole_fit_matches_brute_force(
            caps in proptest::collection::vec((0u64..8, 0u64..8, 0u32..3, 0u64..10), 1..12),
            ops in proptest::collection::vec((0u32..12, 0u64..4, 0u64..4, 0u32..2, 0u8..4), 0..40),
            needs in proptest::collection::vec((0u64..8, 0u64..8, 0u32..3, 0u8..4), 1..12),
        ) {
            let kind = |k: u8| [Kind::Plain, Kind::Build, Kind::Benchmark, Kind::Session][usize::from(k)];
            let mut ns = nodes(&caps.iter().map(|&(c, m, g, _)| res(c, m, g)).collect::<Vec<_>>());
            for (n, &(_, _, _, b)) in ns.iter_mut().zip(&caps) {
                n.background_milli = b;
            }
            let mut index = FitIndex::new(&ns).unwrap();
            for (node, c, m, g, k) in ops {
                let node = NodeId(node % u32::try_from(caps.len()).unwrap());
                let _ = index.take(node, W, &Demand { need: res(c, m, g), kind: kind(k), class: Class::Oci, ask: Default::default(), });
            }
            for (c, m, g, k) in needs {
                let demand = Demand { need: res(c, m, g), kind: kind(k), class: Class::Oci, ask: Default::default(), };
                proptest::prop_assert_eq!(index.fit(&demand, &Pick::default()).best, oracle(&index, &demand));
            }
        }
    }
}
