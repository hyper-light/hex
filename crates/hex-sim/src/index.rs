//! Best fit over a cluster's free capacity: the node with the least free CPU that holds the need,
//! ties broken by least free memory, then fewest free GPUs, then lowest id. One total order
//! ([`Key`]), so the scheduler and its serial specification can't disagree on a tie.
//!
//! Best fit is the baseline the design scores against (Borg's best-fit, `scheduler-architecture.md`
//! S1, CM[1]); the goodput scorer of implementation goal 2 replaces the order, not the index's shape.

use std::collections::BTreeSet;

use crate::error::SimError;
use crate::model::{NodeId, NodeSpec, Resources};
use crate::trace::TraceHash;

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

    pub fn holds(&self, need: &Resources) -> bool {
        need.cpu_milli <= self.cpu_milli
            && need.mem_bytes <= self.mem_bytes
            && need.gpus <= self.gpus
    }
}

/// How the fast path samples (`scheduler-architecture.md` S2): a need's equivalence class scans the
/// nodes in id order from a start its need and `seed` fix, and stops at the `want`-th node that holds
/// it, choosing the best fit among those. `want` is at least `min_feasible` and at least `per_mille` of
/// the nodes, never more than all of them: kube-scheduler's floors, `minFeasibleNodesToFind = 100` and
/// `minFeasibleNodesPercentageToFind = 5` (pkg/scheduler/schedule_one.go:57,62 at v1.37.1), and Borg's
/// relaxed randomization (EuroSys 2015 §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sample {
    pub seed: u64,
    pub min_feasible: u32,
    pub per_mille: u32,
}

impl Default for Sample {
    fn default() -> Self {
        Self {
            seed: 0,
            min_feasible: 100,
            per_mille: 50,
        }
    }
}

impl Sample {
    /// How many feasible nodes a scan over `nodes` stops at.
    pub fn want(&self, nodes: usize) -> usize {
        let share = nodes
            .checked_mul(usize::try_from(self.per_mille).unwrap_or(usize::MAX))
            .map_or(nodes, |x| x.div_ceil(1000));
        let floor = usize::try_from(self.min_feasible).unwrap_or(usize::MAX);
        share.max(floor).max(1).min(nodes)
    }

    /// Where `need`'s class starts its scan over `nodes` (0 for none).
    pub fn start(&self, need: &Resources, nodes: usize) -> usize {
        let mut h = TraceHash::default();
        for word in [
            self.seed,
            need.cpu_milli,
            need.mem_bytes,
            u64::from(need.gpus),
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

/// A sampled decision: the best fit among the scanned nodes that hold the need, and the window
/// scanned, `span` nodes from `start` in id order (wrapping).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sampled {
    pub best: Option<Key>,
    pub start: usize,
    pub span: usize,
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

#[derive(Debug, Clone)]
pub struct FitIndex {
    /// Free capacity by node id; ids are dense from 0.
    free: Vec<Resources>,
    order: BTreeSet<Key>,
}

impl FitIndex {
    /// Nodes' ids must be dense from 0, in order.
    pub fn new(nodes: &[NodeSpec]) -> Result<Self, SimError> {
        let mut free = Vec::with_capacity(nodes.len());
        let mut order = BTreeSet::new();
        for (at, node) in nodes.iter().enumerate() {
            if usize::try_from(node.id.0).ok() != Some(at) {
                return Err(SimError::Config("node ids must be dense from 0, in order"));
            }
            free.push(node.capacity);
            order.insert(Key::of(node.id, &node.capacity));
        }
        Ok(Self { free, order })
    }

    pub fn len(&self) -> usize {
        self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.free.is_empty()
    }

    pub fn key(&self, node: NodeId) -> Result<Key, SimError> {
        Ok(Key::of(node, self.free_of(node)?))
    }

    fn free_of(&self, node: NodeId) -> Result<&Resources, SimError> {
        usize::try_from(node.0)
            .ok()
            .and_then(|at| self.free.get(at))
            .ok_or(SimError::UnknownNode(node))
    }

    /// The best-fit node for `need`, or `None` when no node holds it. The scan starts at the first
    /// node with enough CPU and ends at the first that holds the rest; it visits each node at most
    /// once, so the node count bounds it.
    pub fn best_fit(&self, need: &Resources) -> Option<Key> {
        let from = Key {
            cpu_milli: need.cpu_milli,
            mem_bytes: 0,
            gpus: 0,
            node: NodeId(0),
        };
        self.order
            .range(from..)
            .find(|key| key.holds(need))
            .copied()
    }

    /// The sampled best fit for `need` ([`Sample`]). It visits each node at most once, so the node
    /// count bounds it; a need no node holds scans them all and finds none.
    pub fn sampled_fit(&self, need: &Resources, sample: &Sample) -> Sampled {
        let n = self.free.len();
        let start = sample.start(need, n);
        let want = sample.want(n);
        let (mut best, mut found) = (None::<Key>, 0usize);
        for offset in 0..n {
            let Some(at) = start.checked_add(offset).and_then(|x| x.checked_rem(n)) else {
                break;
            };
            let (Some(free), Ok(id)) = (self.free.get(at), u32::try_from(at)) else {
                break;
            };
            if !free.holds(need) {
                continue;
            }
            let key = Key::of(NodeId(id), free);
            best = Some(best.map_or(key, |b| b.min(key)));
            // found and offset count nodes, so they stay below the node count.
            found = found.saturating_add(1);
            if found >= want {
                return Sampled {
                    best,
                    start,
                    span: offset.saturating_add(1),
                };
            }
        }
        Sampled {
            best,
            start,
            span: n,
        }
    }

    /// Takes `need` from `node`'s free capacity; its new key.
    pub fn take(&mut self, node: NodeId, need: &Resources) -> Result<Key, SimError> {
        self.change(node, |free| free.checked_sub(need))
    }

    /// Gives `need` back to `node`'s free capacity; its new key.
    pub fn give(&mut self, node: NodeId, need: &Resources) -> Result<Key, SimError> {
        self.change(node, |free| free.checked_add(need))
    }

    fn change(
        &mut self,
        node: NodeId,
        f: impl FnOnce(&Resources) -> Option<Resources>,
    ) -> Result<Key, SimError> {
        let at = usize::try_from(node.0).map_err(|_| SimError::UnknownNode(node))?;
        let slot = self.free.get_mut(at).ok_or(SimError::UnknownNode(node))?;
        let next = f(slot).ok_or(SimError::Resources(node))?;
        self.order.remove(&Key::of(node, slot));
        *slot = next;
        let key = Key::of(node, &next);
        self.order.insert(key);
        Ok(key)
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

    fn nodes(caps: &[Resources]) -> Vec<NodeSpec> {
        caps.iter()
            .enumerate()
            .map(|(i, c)| NodeSpec {
                id: NodeId(u32::try_from(i).unwrap()),
                capacity: *c,
            })
            .collect()
    }

    /// Brute force: the least key among the nodes that hold the need.
    fn oracle(index: &FitIndex, need: &Resources) -> Option<Key> {
        (0..index.len())
            .map(|i| index.key(NodeId(u32::try_from(i).unwrap())).unwrap())
            .filter(|k| k.holds(need))
            .min()
    }

    /// T-T7.4.1: place on a few nodes, expect the least-free node that holds the need, GPU work only on
    /// a GPU node.
    #[test]
    fn best_fit_is_the_least_free_node_that_holds_the_need() {
        let index = FitIndex::new(&nodes(&[
            res(8000, 16, 0),
            res(2000, 16, 0),
            res(4000, 4, 1),
        ]))
        .unwrap();
        assert_eq!(
            index.best_fit(&res(1000, 8, 0)).map(|k| k.node),
            Some(NodeId(1))
        );
        assert_eq!(
            index.best_fit(&res(3000, 8, 0)).map(|k| k.node),
            Some(NodeId(0))
        );
        assert_eq!(
            index.best_fit(&res(1000, 1, 1)).map(|k| k.node),
            Some(NodeId(2))
        );
        assert_eq!(index.best_fit(&res(1000, 8, 1)), None);
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
            index.take(NodeId(0), &res(2000, 1, 0)),
            Err(SimError::Resources(NodeId(0)))
        );
        assert_eq!(index.key(NodeId(0)).unwrap().cpu_milli, 1000);
        assert_eq!(
            index.take(NodeId(3), &res(1, 1, 0)),
            Err(SimError::UnknownNode(NodeId(3)))
        );
    }

    /// T-T7.4.1: with a sample that wants every node, the sampled fit is best fit; with one that wants
    /// one node, it is the first node from the class's start that holds the need.
    #[test]
    fn the_sample_stops_at_its_wanted_feasible_node() {
        let index = FitIndex::new(&nodes(&[
            res(8000, 16, 0),
            res(2000, 16, 0),
            res(4000, 4, 1),
            res(3000, 16, 0),
        ]))
        .unwrap();
        let need = res(1000, 8, 0);
        let all = Sample {
            seed: 3,
            min_feasible: 100,
            per_mille: 0,
        };
        assert_eq!(index.sampled_fit(&need, &all).best, index.best_fit(&need));
        let one = Sample {
            seed: 3,
            min_feasible: 1,
            per_mille: 0,
        };
        let s = index.sampled_fit(&need, &one);
        let first = (0..4)
            .map(|o| (s.start + o) % 4)
            .find(|&at| {
                index
                    .key(NodeId(u32::try_from(at).unwrap()))
                    .unwrap()
                    .holds(&need)
            })
            .unwrap();
        assert_eq!(
            s.best.map(|k| k.node),
            Some(NodeId(u32::try_from(first).unwrap()))
        );
        assert!(s.covers(NodeId(u32::try_from(first).unwrap()), 4));
        let none = index.sampled_fit(&res(1, 100, 0), &one);
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
        /// T-T7.4.1 (oracle test): after any sequence of takes and gives, best fit equals the
        /// brute-force least key.
        #[test]
        fn best_fit_matches_brute_force(
            caps in proptest::collection::vec((0u64..8, 0u64..8, 0u32..3), 1..12),
            ops in proptest::collection::vec((0u32..12, 0u64..4, 0u64..4, 0u32..2, proptest::bool::ANY), 0..40),
            needs in proptest::collection::vec((0u64..8, 0u64..8, 0u32..3), 1..12),
        ) {
            let caps: Vec<Resources> = caps.into_iter().map(|(c, m, g)| res(c, m, g)).collect();
            let mut index = FitIndex::new(&nodes(&caps)).unwrap();
            for (node, c, m, g, give) in ops {
                let node = NodeId(node % u32::try_from(caps.len()).unwrap());
                let delta = res(c, m, g);
                let _ = if give { index.give(node, &delta) } else { index.take(node, &delta) };
            }
            for (c, m, g) in needs {
                let need = res(c, m, g);
                proptest::prop_assert_eq!(index.best_fit(&need), oracle(&index, &need));
            }
        }
    }
}
