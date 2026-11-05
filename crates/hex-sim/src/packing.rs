//! Stranded capacity (`AC-T7.4.3`): what a placement policy leaves free that no workload can use.
//!
//! [`pack`] places a workload's arrivals in order, with no completions, through the fast path's
//! sampled choice ([`FitIndex::sampled_fit`]), passing over one no node admits, until the placed CPU
//! reaches the load asked for (90% for `AC-T7.4.3`). It then measures stranded capacity: the free CPU
//! on each node where no shape of the workload's needs fits what is free. One shape set stands for
//! all of them: a node's free capacity can host some need exactly when it can host a Pareto-minimal
//! one. Borg judges policy by what a workload leaves usable rather than by average utilization (cell
//! compaction, EuroSys 2015 §5.1); this is that question asked of one cell at one load.

use std::collections::BTreeSet;

use crate::error::SimError;
use crate::index::{FitIndex, Sample};
use crate::model::{Cluster, Resources};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PackReport {
    pub total_cpu_milli: u64,
    pub placed_cpu_milli: u64,
    pub free_cpu_milli: u64,
    /// Free CPU on nodes where no workload shape fits what is free.
    pub stranded_cpu_milli: u64,
    pub placements: u64,
    /// Arrivals no node admitted when they came.
    pub passed_over: u64,
    /// Whether the placed CPU reached the load asked for before the arrivals ran out.
    pub reached: bool,
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

pub fn pack(
    cluster: &Cluster,
    sample: &Sample,
    load_per_mille: u64,
) -> Result<PackReport, SimError> {
    let mut index = FitIndex::new(&cluster.nodes)?;
    let mut r = PackReport::default();
    for node in &cluster.nodes {
        r.total_cpu_milli = r
            .total_cpu_milli
            .checked_add(node.capacity.cpu_milli)
            .ok_or(SimError::Resources(node.id))?;
    }
    let target = u128::from(r.total_cpu_milli).saturating_mul(u128::from(load_per_mille));
    for w in &cluster.workloads {
        if u128::from(r.placed_cpu_milli).saturating_mul(1000) >= target {
            r.reached = true;
            break;
        }
        let demand = w.demand();
        let Some(key) = index.sampled_fit(&demand, sample).best else {
            r.passed_over = r.passed_over.saturating_add(1);
            continue;
        };
        index.take(key.node, &demand)?;
        r.placed_cpu_milli = r.placed_cpu_milli.saturating_add(w.need.cpu_milli);
        r.placements = r.placements.saturating_add(1);
    }
    r.reached = r.reached || u128::from(r.placed_cpu_milli).saturating_mul(1000) >= target;
    let shapes = minimal_shapes(cluster);
    for node in &cluster.nodes {
        let free = index.key(node.id)?;
        let free = Resources {
            cpu_milli: free.cpu_milli,
            mem_bytes: free.mem_bytes,
            gpus: free.gpus,
        };
        r.free_cpu_milli = r.free_cpu_milli.saturating_add(free.cpu_milli);
        if !shapes.iter().any(|s| free.holds(s)) {
            r.stranded_cpu_milli = r.stranded_cpu_milli.saturating_add(free.cpu_milli);
        }
    }
    Ok(r)
}

/// Stranded CPU as a share of all CPU, in per-mille.
pub fn stranded_per_mille(r: &PackReport) -> u64 {
    u128::from(r.stranded_cpu_milli)
        .checked_mul(1000)
        .and_then(|x| x.checked_div(u128::from(r.total_cpu_milli)))
        .and_then(|x| u64::try_from(x).ok())
        .unwrap_or(0)
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

    /// AC-T7.4.3's measure on a hand-made case: two nodes of (4 CPU, 4 mem); work of (3,1) and (1,3).
    /// Placing the two (3,1)s on separate nodes leaves (1,3) free on each, which a (1,3) uses; that is
    /// no stranding. The free capacity a shape can't use is counted.
    #[test]
    fn stranded_is_free_cpu_no_shape_fits() {
        let nodes = (0..2)
            .map(|i| NodeSpec {
                id: NodeId(i),
                capacity: res(4000, 4),
                background_milli: 0,
            })
            .collect();
        let w = |id, need| WorkloadSpec {
            id: WorkloadId(id),
            need,
            kind: Kind::Plain,
            arrives: 0,
            runs_for: None,
            tenancy: Tenancy::default(),
        };
        let cluster = Cluster::single_owner(
            nodes,
            vec![w(0, res(3000, 1)), w(1, res(3000, 1)), w(2, res(1000, 3))],
        );
        let shapes = minimal_shapes(&cluster);
        assert_eq!(shapes.len(), 2);
        let r = pack(&cluster, &Sample::whole(), 1000).unwrap();
        assert_eq!((r.placements, r.passed_over), (3, 0));
        // After all three: one node holds (3,1)+(1,3) = full; the other (3,1), free (1,3), which (1,3) fits.
        assert_eq!(r.stranded_cpu_milli, 0);
    }
}
