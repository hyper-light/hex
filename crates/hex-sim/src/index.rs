//! What each node can take, and the fast path's sampled best fit over it.
//!
//! A node admits a workload when its hard filters pass (`scheduler-architecture.md` S3) and it has
//! the room: GPU work only on a node with GPUs, a benchmark never beside a build nor a build beside a
//! benchmark, and an agent session never on a node above 90% CPU (`AC-T7.6.3`). Among the nodes that
//! admit it, best fit is the node with the least free CPU, then least free memory, then fewest free
//! GPUs, then lowest id: one total order ([`Key`]), so the scheduler and its serial specification
//! can't disagree on a tie. Best fit is the baseline the design scores against (Borg's, S1, CM[1]).

use crate::error::SimError;
use crate::model::{Demand, Kind, NodeId, NodeSpec, Resources};
use crate::trace::TraceHash;

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

/// How the fast path samples (`scheduler-architecture.md` S2): a class scans the nodes in id order
/// from a start its demand and `seed` fix, and stops at the `want`-th node that admits it, choosing
/// the best fit among those. `want` is at least `min_feasible` and at least `per_mille` of the nodes,
/// never more than all of them: kube-scheduler's floors, `minFeasibleNodesToFind = 100` and
/// `minFeasibleNodesPercentageToFind = 5` (pkg/scheduler/schedule_one.go:57,62 at v1.37.1), and Borg's
/// relaxed randomization (EuroSys 2015 §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub seed: u64,
    pub min_feasible: u32,
    pub per_mille: u32,
    /// How the sampled nodes are ranked.
    pub scorer: Scorer,
    /// The workload shapes [`Scorer::Fragment`] keeps usable.
    pub shapes: Shapes,
}

/// At most [`Shapes::MAX`] workload shapes, kept in a fixed array so a [`Sample`] stays a plain
/// value. Past the bound, [`Shapes::of`] keeps the smallest (by CPU, then memory, then GPUs): a free
/// capacity a larger shape fits, a smaller one fits too, so the smallest are what decide whether a
/// leftover can still be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Shapes {
    len: usize,
    items: [Resources; Shapes::MAX],
}

impl Shapes {
    pub const MAX: usize = 16;

    pub fn of(mut shapes: Vec<Resources>) -> Self {
        shapes.sort_unstable();
        shapes.dedup();
        let mut out = Self::default();
        for (slot, shape) in out.items.iter_mut().zip(shapes) {
            *slot = shape;
            out.len = out.len.saturating_add(1);
        }
        out
    }

    pub fn iter(&self) -> impl Iterator<Item = &Resources> {
        self.items.iter().take(self.len)
    }

    /// Whether some shape fits `free`.
    fn usable(&self, free: &Resources) -> bool {
        self.iter().any(|s| free.holds(s))
    }
}

/// How a decision ranks the nodes its sample admits.
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
    /// fits one of the sample's [`Shapes`], so no CPU or memory is left that no workload can use;
    /// then the [`Tie`]; then [`Key`]'s order. [`Scorer::HEX`] is Hex's packing score.
    Fragment(Tie),
}

impl Scorer {
    /// Hex's packing score: least stranded, then least allocated, so work spreads over leftovers that
    /// stay usable rather than spending one node's memory beside its idle CPU. On the recorded kiln
    /// workload it strands less than either best fit at 90% load on every seed, where the plan's
    /// alignment tie and a most-allocated tie don't (hex-sim.md H11).
    pub const HEX: Self = Self::Fragment(Tie::LeastAllocated);
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
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            seed: 0,
            min_feasible: 100,
            per_mille: 50,
            scorer: Scorer::BestFit,
            shapes: Shapes {
                len: 0,
                items: [Resources::default(); Shapes::MAX],
            },
        }
    }
}

impl Sample {
    /// A sample that scans every node: whole-cluster best fit.
    pub fn whole() -> Self {
        Self {
            seed: 0,
            min_feasible: u32::MAX,
            per_mille: 0,
            scorer: Scorer::BestFit,
            shapes: Shapes {
                len: 0,
                items: [Resources::default(); Shapes::MAX],
            },
        }
    }

    /// How many admitting nodes a scan over `nodes` stops at.
    pub fn want(&self, nodes: usize) -> usize {
        let share = nodes
            .checked_mul(usize::try_from(self.per_mille).unwrap_or(usize::MAX))
            .map_or(nodes, |x| x.div_ceil(1000));
        let floor = usize::try_from(self.min_feasible).unwrap_or(usize::MAX);
        share.max(floor).max(1).min(nodes)
    }

    /// Where `demand`'s class starts its scan over `nodes` (0 for none).
    pub fn start(&self, demand: &Demand, nodes: usize) -> usize {
        let mut h = TraceHash::default();
        let need = &demand.need;
        for word in [
            self.seed,
            need.cpu_milli,
            need.mem_bytes,
            u64::from(need.gpus),
            kind_word(demand.kind),
        ] {
            h.word(word);
        }
        u64::try_from(nodes)
            .ok()
            .and_then(|n| h.finish().checked_rem(n))
            .and_then(|at| usize::try_from(at).ok())
            .unwrap_or(0)
    }
}

fn kind_word(kind: Kind) -> u64 {
    match kind {
        Kind::Plain => 0,
        Kind::Build => 1,
        Kind::Benchmark => 2,
        Kind::Session => 3,
    }
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
}

/// A sampled decision: the best fit among the scanned nodes that admit the demand, the window
/// scanned (`span` nodes from `start` in id order, wrapping), and what the filters refused in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sampled {
    pub best: Option<Key>,
    pub start: usize,
    pub span: usize,
    pub refused: Refusals,
}

impl Sampled {
    /// Whether `node` lies in the scanned window, of a cluster of `nodes`.
    pub fn covers(&self, node: NodeId, nodes: usize) -> bool {
        usize::try_from(node.0)
            .ok()
            .and_then(|at| at.checked_add(nodes))
            .and_then(|x| x.checked_sub(self.start))
            .and_then(|x| x.checked_rem(nodes))
            .is_none_or(|offset| offset < self.span)
    }
}

/// Why a node doesn't admit a demand, or that it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Admits,
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
    builds: u32,
    benchmarks: u32,
}

impl Slot {
    fn verdict(&self, demand: &Demand) -> Verdict {
        let need = &demand.need;
        if need.gpus > 0 && self.capacity.gpus == 0 {
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
        if shapes.usable(&left) {
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

#[derive(Debug, Clone)]
pub struct FitIndex {
    /// By node id; ids are dense from 0.
    slots: Vec<Slot>,
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
                builds: 0,
                benchmarks: 0,
            });
        }
        Ok(Self { slots })
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

    /// Whether any node admits `demand` now.
    pub fn admitted_anywhere(&self, demand: &Demand) -> bool {
        self.slots
            .iter()
            .any(|slot| slot.verdict(demand) == Verdict::Admits)
    }

    /// The sampled best fit for `demand` ([`Sample`]). It visits each node at most once, so the node
    /// count bounds it; a demand no node admits scans them all and finds none.
    pub fn sampled_fit(&self, demand: &Demand, sample: &Sample) -> Sampled {
        let n = self.slots.len();
        let start = sample.start(demand, n);
        let want = sample.want(n);
        let (mut best, mut best_score, mut found) = (None::<Key>, 0u128, 0usize);
        let mut refused = Refusals::default();
        // found, offset and the refusals count nodes, so they stay below the node count.
        for offset in 0..n {
            let Some(at) = start.checked_add(offset).and_then(|x| x.checked_rem(n)) else {
                break;
            };
            let (Some(slot), Ok(id)) = (self.slots.get(at), u32::try_from(at)) else {
                break;
            };
            match slot.verdict(demand) {
                Verdict::Admits => {}
                Verdict::NoRoom => continue,
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
            let key = Key::of(NodeId(id), &slot.free);
            let score = match sample.scorer {
                Scorer::BestFit => 0,
                Scorer::MostAllocated => u128::from(slot.allocated_after(&demand.need)),
                Scorer::Align => u128::from(slot.alignment(&demand.need)),
                Scorer::Fragment(tie) => {
                    // Lexicographic in one number: least stranded capacity in the high word, then
                    // the tie in the low word.
                    let stranded = slot.stranded_after(&demand.need, &sample.shapes);
                    let then = match tie {
                        Tie::Align => slot.alignment(&demand.need),
                        Tie::LeastAllocated => {
                            u64::MAX.saturating_sub(slot.allocated_after(&demand.need))
                        }
                        Tie::MostAllocated => slot.allocated_after(&demand.need),
                    };
                    u128::from(u64::MAX.saturating_sub(stranded))
                        .checked_mul(1 << 64)
                        .unwrap_or(0)
                        | u128::from(then)
                }
            };
            // A higher score wins; between equal scores, the least key.
            let wins = best.is_none_or(|b| score > best_score || (score == best_score && key < b));
            if wins {
                best = Some(key);
                best_score = score;
            }
            found = found.saturating_add(1);
            if found >= want {
                return Sampled {
                    best,
                    start,
                    span: offset.saturating_add(1),
                    refused,
                };
            }
        }
        Sampled {
            best,
            start,
            span: n,
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
        *slot = next;
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
        }
    }

    fn nodes(caps: &[Resources]) -> Vec<NodeSpec> {
        caps.iter()
            .enumerate()
            .map(|(i, c)| NodeSpec {
                id: NodeId(u32::try_from(i).unwrap()),
                capacity: *c,
                background_milli: 0,
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
                .sampled_fit(&plain(need), &Sample::whole())
                .best
                .map(|k| k.node)
        };
        assert_eq!(fit(res(1000, 8, 0)), Some(NodeId(1)));
        assert_eq!(fit(res(3000, 8, 0)), Some(NodeId(0)));
        assert_eq!(fit(res(1000, 1, 1)), Some(NodeId(2)));
        assert_eq!(fit(res(1000, 8, 1)), None);
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
        };
        assert_eq!(index.verdict(NodeId(0), &session).unwrap(), Verdict::Hot);
        assert_eq!(index.verdict(NodeId(1), &session).unwrap(), Verdict::Admits);
        let gpu = plain(res(1, 1, 1));
        assert_eq!(index.verdict(NodeId(0), &gpu).unwrap(), Verdict::NoGpu);
        let build = Demand {
            need: res(1, 1, 0),
            kind: Kind::Build,
        };
        let bench = Demand {
            need: res(1, 1, 0),
            kind: Kind::Benchmark,
        };
        index.take(NodeId(1), &build).unwrap();
        assert_eq!(index.verdict(NodeId(1), &bench).unwrap(), Verdict::Shares);
        index.give(NodeId(1), &build).unwrap();
        index.take(NodeId(1), &bench).unwrap();
        assert_eq!(index.verdict(NodeId(1), &build).unwrap(), Verdict::Shares);
        let s = index.sampled_fit(&build, &Sample::whole());
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

    /// T-T7.4.1: a sample that wants one node takes the first from its class's start that admits it.
    #[test]
    fn the_sample_stops_at_its_wanted_admitting_node() {
        let index = FitIndex::new(&nodes(&[
            res(8000, 16, 0),
            res(2000, 16, 0),
            res(4000, 4, 1),
            res(3000, 16, 0),
        ]))
        .unwrap();
        let demand = plain(res(1000, 8, 0));
        let one = Sample {
            seed: 3,
            min_feasible: 1,
            per_mille: 0,
            ..Sample::default()
        };
        let s = index.sampled_fit(&demand, &one);
        let first = (0..4)
            .map(|o| (s.start + o) % 4)
            .map(|at| NodeId(u32::try_from(at).unwrap()))
            .find(|&n| index.verdict(n, &demand).unwrap() == Verdict::Admits)
            .unwrap();
        assert_eq!(s.best.map(|k| k.node), Some(first));
        assert!(s.covers(first, 4));
        let none = index.sampled_fit(&plain(res(1, 100, 0)), &one);
        assert_eq!((none.best, none.span), (None, 4));
    }

    #[test]
    fn want_follows_kube_schedulers_floors() {
        let s = Sample::default();
        assert_eq!(
            (s.want(10), s.want(1_000), s.want(10_000), s.want(0)),
            (10, 100, 500, 0)
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
                let _ = index.take(node, &Demand { need: res(c, m, g), kind: kind(k) });
            }
            for (c, m, g, k) in needs {
                let demand = Demand { need: res(c, m, g), kind: kind(k) };
                proptest::prop_assert_eq!(index.sampled_fit(&demand, &Sample::whole()).best, oracle(&index, &demand));
            }
        }
    }
}
