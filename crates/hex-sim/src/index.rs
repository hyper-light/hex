//! What each node can take, and the fast path's pick over it: the kernel's walk, ported from kiln.
//!
//! A node admits a workload when its hard filters pass (`scheduler-architecture.md` S3) and it has
//! the room: GPU work only on a node with GPUs, a benchmark never beside a build nor a build beside a
//! benchmark, and an agent session never on a node above 90% CPU (`AC-T7.6.3`). Among the nodes that
//! admit it, best fit is the node with the least free CPU, then least free memory, then fewest free
//! GPUs, then lowest id: one total order ([`Key`]), so the scheduler and its serial specification
//! can't disagree on a tie. Best fit is the baseline the design scores against (Borg's, S1, CM[1]).

use crate::census;
use crate::error::SimError;
use crate::lb::{self, BUCKETS, Index};
use crate::model::{Class, Classes, Demand, Kind, NodeId, NodeSpec, Resources};

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
    /// GPU work, a node without GPUs.
    pub gpu: u64,
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
    /// The need asks a GPU the node hasn't, or that its class can't be given ([`Class::takes_gpus`]).
    NoGpu,
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
    builds: u32,
    benchmarks: u32,
}

impl Slot {
    fn verdict(&self, demand: &Demand) -> Verdict {
        let need = &demand.need;
        if !self.classes.has(demand.class) {
            return Verdict::NoClass;
        }
        if need.gpus > 0 && (self.capacity.gpus == 0 || !demand.class.takes_gpus()) {
            return Verdict::NoGpu;
        }
        if !self.free.holds(need) {
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
    /// By node id; ids are dense from 0.
    slots: Vec<Slot>,
    ix: Index,
    /// The highest bucket: the most whole CPUs any node has.
    last: u32,
}

/// A node's bucket in the kernel's index: its free whole CPUs, at most `last`.
fn bucket(free: &Resources, last: u32) -> u32 {
    let whole = free.cpu_milli.checked_div(lb::CPU).unwrap_or(0);
    u32::try_from(whole).unwrap_or(u32::MAX).min(last)
}

/// A node's capacity as the kernel's amounts: CPU, memory, disk (none here), GPUs.
fn amounts(r: &Resources) -> lb::Amounts {
    [r.cpu_milli, r.mem_bytes, 0, u64::from(r.gpus)]
}

impl FitIndex {
    /// Nodes' ids must be dense from 0, in order.
    pub fn new(nodes: &[NodeSpec]) -> Result<Self, SimError> {
        let mut slots = Vec::with_capacity(nodes.len());
        for (at, node) in nodes.iter().enumerate() {
            if usize::try_from(node.id.0).ok() != Some(at) {
                return Err(SimError::Config("node ids must be dense from 0, in order"));
            }
            slots.push(Slot {
                capacity: node.capacity,
                free: node.capacity,
                background_milli: node.background_milli,
                classes: node.classes,
                builds: 0,
                benchmarks: 0,
            });
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
        Ok(Self { slots, ix, last })
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
        Ok(self.slot(node)?.verdict(demand))
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
            .any(|slot| slot.verdict(demand) == Verdict::Admits)
    }

    /// The best node for `demand` by `pick`'s scorer, ties broken by [`Key`]'s order: the kernel's
    /// walk ([`crate::lb::Index`], kiln's `owner.rs` walk) from the bucket of the need's whole CPUs up,
    /// every node in it read and checked exactly. Kiln's walk stops at the first runner that fits,
    /// the least loaded; Hex's reads them all, since the packing score (H11) ranks by what each node
    /// would have left. Each node is read once, so the node count bounds it.
    pub fn fit(&self, demand: &Demand, pick: &Pick) -> Fit {
        let floor = u32::try_from(demand.need.cpu_milli.checked_div(lb::CPU).unwrap_or(0))
            .unwrap_or(u32::MAX);
        let (mut best, mut best_score) = (None::<Key>, 0u128);
        let mut refused = Refusals::default();
        let mut walked = 0u64;
        let mut b = self.ix.at_or_above(floor.min(self.last));
        while let Some(bb) = b {
            for r in self.ix.bucket(bb) {
                walked = walked.saturating_add(1);
                let Some(slot) = self.slots.get(lb::ix(r)) else {
                    continue;
                };
                match slot.verdict(demand) {
                    Verdict::Admits => {}
                    Verdict::NoRoom => continue,
                    Verdict::NoClass => {
                        refused.class = refused.class.saturating_add(1);
                        continue;
                    }
                    Verdict::NoGpu => {
                        refused.gpu = refused.gpu.saturating_add(1);
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
                let score = slot.score(&demand.need, pick);
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
        Fit {
            best,
            walked,
            refused,
        }
    }

    /// Places `demand` on `node`: its need taken from the free capacity, its kind counted; the
    /// node's new key.
    pub fn take(&mut self, node: NodeId, demand: &Demand) -> Result<Key, SimError> {
        self.change(
            node,
            demand,
            |free| free.checked_sub(&demand.need),
            |n| n.checked_add(1),
        )
    }

    /// Ends `demand` on `node`; the node's new key.
    pub fn give(&mut self, node: NodeId, demand: &Demand) -> Result<Key, SimError> {
        self.change(
            node,
            demand,
            |free| free.checked_add(&demand.need),
            |n| n.checked_sub(1),
        )
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

    fn res(cpu: u64, mem: u64, gpus: u32) -> Resources {
        Resources {
            cpu_milli: cpu,
            mem_bytes: mem,
            gpus,
        }
    }

    fn plain(need: Resources) -> Demand {
        Demand {
            need,
            kind: Kind::Plain,
            class: Class::Oci,
        }
    }

    fn nodes(caps: &[Resources]) -> Vec<NodeSpec> {
        caps.iter()
            .enumerate()
            .map(|(i, c)| NodeSpec {
                id: NodeId(u32::try_from(i).unwrap()),
                capacity: *c,
                background_milli: 0,
                classes: Classes::ALL,
            })
            .collect()
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
        };
        assert_eq!(index.verdict(NodeId(0), &session).unwrap(), Verdict::Hot);
        assert_eq!(index.verdict(NodeId(1), &session).unwrap(), Verdict::Admits);
        let gpu = plain(res(1, 1, 1));
        assert_eq!(index.verdict(NodeId(0), &gpu).unwrap(), Verdict::NoGpu);
        let build = Demand {
            need: res(1, 1, 0),
            kind: Kind::Build,
            class: Class::Oci,
        };
        let bench = Demand {
            need: res(1, 1, 0),
            kind: Kind::Benchmark,
            class: Class::Oci,
        };
        index.take(NodeId(1), &build).unwrap();
        assert_eq!(index.verdict(NodeId(1), &bench).unwrap(), Verdict::Shares);
        index.give(NodeId(1), &build).unwrap();
        index.take(NodeId(1), &bench).unwrap();
        assert_eq!(index.verdict(NodeId(1), &build).unwrap(), Verdict::Shares);
        let s = index.fit(&build, &Pick::default());
        assert_eq!(
            (s.best.map(|k| k.node), s.refused.shares),
            (Some(NodeId(0)), 1)
        );
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
            index.take(NodeId(0), &plain(res(2000, 1, 0))),
            Err(SimError::Resources(NodeId(0)))
        );
        assert_eq!(index.key(NodeId(0)).unwrap().cpu_milli, 1000);
        assert_eq!(
            index.take(NodeId(3), &plain(res(1, 1, 0))),
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
                let _ = index.take(node, &Demand { need: res(c, m, g), kind: kind(k), class: Class::Oci });
            }
            for (c, m, g, k) in needs {
                let demand = Demand { need: res(c, m, g), kind: kind(k), class: Class::Oci };
                proptest::prop_assert_eq!(index.fit(&demand, &Pick::default()).best, oracle(&index, &demand));
            }
        }
    }
}
