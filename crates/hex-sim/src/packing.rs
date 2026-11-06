//! Stranded capacity (`AC-T7.4.3`): what a placement policy leaves free that no workload can use.
//!
//! [`pack`] places a workload's arrivals in order from a given one, with no completions, through the
//! fast path's sampled choice ([`FitIndex::sampled_fit`]), passing over one no node admits, until the
//! load reaches what is asked (90% for `AC-T7.4.3`). The load is the dominant share of what is
//! placed: the larger of placed CPU over all CPU and placed memory over all memory, as Dominant
//! Resource Fairness measures a share, so a workload that runs out of CPU first is loaded by its CPU.
//!
//! It then measures stranded capacity: on each node where no shape of the workload's requests fits
//! what is free, that node's free CPU and free memory, which nothing can use because another
//! resource there is spent. One shape set stands for all of them: a node's free capacity can host
//! some request exactly when it can host a Pareto-minimal one. This is Borg's stranding, resources
//! "that cannot be used because another resource on the machine is fully allocated" (EuroSys 2015
//! §3.2), asked of one cell at one load. A policy that leaves every node part-free strands nothing at
//! a load it never has to pack tightly at, so [`PackReport::first_full_per_mille`] records the load
//! at the first arrival that found no node with room: spreading shows there as an earlier refusal.

use std::collections::BTreeSet;

use crate::error::SimError;
use crate::index::{FitIndex, Refusals, Sample};
use crate::model::{Cluster, Resources};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PackReport {
    pub total_cpu_milli: u64,
    pub total_mem_bytes: u64,
    pub placed_cpu_milli: u64,
    pub placed_mem_bytes: u64,
    pub free_cpu_milli: u64,
    pub free_mem_bytes: u64,
    /// Free CPU and memory on nodes where no workload shape fits what is free.
    pub stranded_cpu_milli: u64,
    pub stranded_mem_bytes: u64,
    pub placements: u64,
    /// Arrivals no node admitted when they came.
    pub passed_over: u64,
    /// Whether the load reached what was asked before the arrivals ran out.
    pub reached: bool,
    /// The load when the first arrival found no node with room (none refused it by a filter alone).
    pub first_full_per_mille: Option<u64>,
    /// Arrivals passed over while the load was below half, for room or by a filter.
    pub early_passed_over: u64,
    /// The first [`EARLY_KEPT`] of them; the rest are counted only.
    pub early: [Option<Refusal>; EARLY_KEPT],
}

/// How many arrivals passed over below half load a [`PackReport`] keeps.
pub const EARLY_KEPT: usize = 8;

/// An arrival no node admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Refusal {
    /// The load when it came, in per-mille.
    pub load_per_mille: u64,
    pub need: Resources,
    /// No node had room for it; else a filter refused it where there was room.
    pub for_room: bool,
}

impl PackReport {
    /// The dominant share placed, in per-mille: the larger of placed CPU over all CPU and placed
    /// memory over all memory.
    pub fn load_per_mille(&self) -> u64 {
        per_mille(self.placed_cpu_milli, self.total_cpu_milli)
            .max(per_mille(self.placed_mem_bytes, self.total_mem_bytes))
    }

    /// Stranded capacity in per-mille: the stranded share of CPU and of memory, summed.
    pub fn stranded_per_mille(&self) -> u64 {
        per_mille(self.stranded_cpu_milli, self.total_cpu_milli)
            .saturating_add(per_mille(self.stranded_mem_bytes, self.total_mem_bytes))
    }
}

/// `part` as a share of `whole` in per-mille; 0 of a whole of 0, and past u64 saturates.
fn per_mille(part: u64, whole: u64) -> u64 {
    u128::from(part)
        .checked_mul(1000)
        .and_then(|x| x.checked_div(u128::from(whole)))
        .map_or(0, |x| u64::try_from(x).unwrap_or(u64::MAX))
}

/// The Pareto-minimal needs: no other need is at most it in every resource. A capacity that holds
/// any need holds one of these. Bounded by the workloads' count; quadratic in the distinct needs.
pub fn minimal_shapes(cluster: &Cluster) -> Vec<Resources> {
    let distinct: BTreeSet<Resources> = cluster.workloads.iter().map(|w| w.need).collect();
    let all: Vec<Resources> = distinct.into_iter().collect();
    all.iter()
        .filter(|&&s| !all.iter().any(|&o| o != s && s.holds(&o)))
        .copied()
        .collect()
}

/// Packs `cluster`'s arrivals from the `from`th on (none past the last) to `load_per_mille`, then
/// measures what is stranded.
pub fn pack(
    cluster: &Cluster,
    sample: &Sample,
    load_per_mille: u64,
    from: usize,
) -> Result<PackReport, SimError> {
    let mut index = FitIndex::new(&cluster.nodes)?;
    let mut r = PackReport::default();
    for node in &cluster.nodes {
        r.total_cpu_milli = r
            .total_cpu_milli
            .checked_add(node.capacity.cpu_milli)
            .ok_or(SimError::Resources(node.id))?;
        r.total_mem_bytes = r
            .total_mem_bytes
            .checked_add(node.capacity.mem_bytes)
            .ok_or(SimError::Resources(node.id))?;
    }
    for w in cluster.workloads.get(from..).unwrap_or_default() {
        if r.load_per_mille() >= load_per_mille {
            r.reached = true;
            break;
        }
        let demand = w.demand();
        let sampled = index.sampled_fit(&demand, sample);
        let Some(key) = sampled.best else {
            let refusal = Refusal {
                load_per_mille: r.load_per_mille(),
                need: w.need,
                for_room: sampled.refused == Refusals::default(),
            };
            r.passed_over = r.passed_over.saturating_add(1);
            if r.first_full_per_mille.is_none() && refusal.for_room {
                r.first_full_per_mille = Some(refusal.load_per_mille);
            }
            if refusal.load_per_mille < 500 {
                if let Some(slot) = r.early.iter_mut().find(|e| e.is_none()) {
                    *slot = Some(refusal);
                }
                r.early_passed_over = r.early_passed_over.saturating_add(1);
            }
            continue;
        };
        index.take(key.node, &demand)?;
        r.placed_cpu_milli = r.placed_cpu_milli.saturating_add(w.need.cpu_milli);
        r.placed_mem_bytes = r.placed_mem_bytes.saturating_add(w.need.mem_bytes);
        r.placements = r.placements.saturating_add(1);
    }
    r.reached = r.reached || r.load_per_mille() >= load_per_mille;
    let shapes = minimal_shapes(cluster);
    for node in &cluster.nodes {
        let free = index.key(node.id)?;
        let free = Resources {
            cpu_milli: free.cpu_milli,
            mem_bytes: free.mem_bytes,
            gpus: free.gpus,
        };
        r.free_cpu_milli = r.free_cpu_milli.saturating_add(free.cpu_milli);
        r.free_mem_bytes = r.free_mem_bytes.saturating_add(free.mem_bytes);
        if !shapes.iter().any(|s| free.holds(s)) {
            r.stranded_cpu_milli = r.stranded_cpu_milli.saturating_add(free.cpu_milli);
            r.stranded_mem_bytes = r.stranded_mem_bytes.saturating_add(free.mem_bytes);
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Kind, NodeId, NodeSpec, Tenancy, WorkloadId, WorkloadSpec};

    fn res(cpu: u64, mem: u64) -> Resources {
        Resources {
            cpu_milli: cpu,
            mem_bytes: mem,
            gpus: 0,
        }
    }

    fn cluster(nodes: u32, node: Resources, needs: &[Resources]) -> Cluster {
        Cluster::single_owner(
            (0..nodes)
                .map(|i| NodeSpec {
                    id: NodeId(i),
                    capacity: node,
                    background_milli: 0,
                })
                .collect(),
            needs
                .iter()
                .zip(0u64..)
                .map(|(&need, id)| WorkloadSpec {
                    id: WorkloadId(id),
                    need,
                    kind: Kind::Plain,
                    arrives: id,
                    runs_for: None,
                    tenancy: Tenancy::default(),
                })
                .collect(),
        )
    }

    /// AC-T7.4.3's measure on a hand-made case: two nodes of (4 CPU, 4 mem); work of (3,1) and (1,3).
    /// Placing the two (3,1)s on separate nodes leaves (1,3) free on each, which a (1,3) uses; that is
    /// no stranding. The free capacity a shape can't use is counted.
    #[test]
    fn stranded_is_free_capacity_no_shape_fits() {
        let c = cluster(2, res(4000, 4), &[res(3000, 1), res(3000, 1), res(1000, 3)]);
        assert_eq!(minimal_shapes(&c).len(), 2);
        let r = pack(&c, &Sample::whole(), 1000, 0).unwrap();
        assert_eq!((r.placements, r.passed_over), (3, 0));
        // After all three: one node holds (3,1)+(1,3) = full; the other (3,1), free (1,3), which (1,3) fits.
        assert_eq!((r.stranded_cpu_milli, r.stranded_mem_bytes), (0, 0));
    }

    /// Do a memory-bound workload; expect its load to be its memory's share, the CPU left on a node
    /// whose memory is spent counted as stranded, and the first refusal's load recorded.
    #[test]
    fn a_memory_bound_load_strands_the_cpu_beside_spent_memory() {
        let c = cluster(1, res(8000, 4), &[res(1000, 3), res(1000, 2)]);
        let r = pack(&c, &Sample::whole(), 1000, 0).unwrap();
        assert_eq!((r.placements, r.passed_over), (1, 1));
        assert_eq!(r.load_per_mille(), 750);
        assert_eq!(r.first_full_per_mille, Some(750));
        // Free (7000, 1): the (1000, 2) shape doesn't fit, so all of it is stranded.
        assert_eq!((r.stranded_cpu_milli, r.stranded_mem_bytes), (7000, 1));
        assert_eq!(r.stranded_per_mille(), 875 + 250);
    }

    /// Do an arrival no node has room for while the cluster is mostly idle; expect it kept as an early
    /// refusal with its need and load, and arrivals past the kept count counted only.
    #[test]
    fn refusals_below_half_load_are_kept_with_their_needs() {
        let mut needs = vec![res(1000, 1)];
        needs.extend(std::iter::repeat_n(res(9000, 1), EARLY_KEPT + 2));
        let c = cluster(1, res(8000, 8), &needs);
        let r = pack(&c, &Sample::whole(), 1000, 0).unwrap();
        assert_eq!(r.early_passed_over, u64::try_from(EARLY_KEPT).unwrap() + 2);
        assert_eq!(
            r.early[0],
            Some(Refusal {
                load_per_mille: 125,
                need: res(9000, 1),
                for_room: true
            })
        );
        assert!(r.early.iter().all(Option::is_some));
    }

    /// Do a pack from a later arrival; expect the arrivals before it left out.
    #[test]
    fn a_pack_starts_at_its_arrival() {
        let c = cluster(1, res(8000, 8), &[res(8000, 8), res(1000, 1)]);
        let r = pack(&c, &Sample::whole(), 1000, 1).unwrap();
        assert_eq!((r.placements, r.placed_cpu_milli), (1, 1000));
        let past = pack(&c, &Sample::whole(), 1000, 9).unwrap();
        assert_eq!(past.placements, 0);
    }
}
