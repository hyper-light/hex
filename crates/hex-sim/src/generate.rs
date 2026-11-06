//! Generated clusters and workloads, from one seed. Each workload's need is drawn inside one node's
//! capacity, so every workload fits somewhere and GPU work is drawn only against a GPU node.

use crate::error::SimError;
use crate::model::{
    Class, Classes, Cluster, Kind, NodeId, NodeSpec, Resources, Tenancy, Time, WorkloadId,
    WorkloadSpec,
};
use crate::rng::Rng;

const GIB: u64 = 1 << 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub nodes: u32,
    pub workloads: u64,
    /// Mean gap between arrivals; gaps are uniform on `0..=2 * mean`.
    pub mean_gap: Time,
    /// Run time is uniform on `1..=2 * mean`; `None` runs until the run ends.
    pub mean_run: Option<Time>,
    /// Per mille of nodes with GPUs.
    pub gpu_nodes_per_mille: u64,
    /// The largest share of its node a workload's need is drawn up to, per mille.
    pub max_share_per_mille: u64,
}

pub fn cluster(seed: u64, shape: &Shape) -> Result<Cluster, SimError> {
    if shape.nodes == 0 {
        return Err(SimError::Config("a cluster has at least one node"));
    }
    let mut rng = Rng::new(seed);
    // At most 64 cores and 8 GiB a core: every product below is far inside u64.
    let mut nodes = Vec::new();
    for id in 0..shape.nodes {
        let cores = 2u64
            .checked_pow(u32::try_from(rng.range(2, 6)).map_err(|_| SimError::Config("cores"))?)
            .ok_or(SimError::Config("cores"))?;
        let mem_gib = cores
            .checked_mul(rng.range(2, 8))
            .ok_or(SimError::Config("memory"))?;
        let gpus = if rng.chance(shape.gpu_nodes_per_mille) {
            u32::try_from(rng.range(1, 8)).map_err(|_| SimError::Config("gpus"))?
        } else {
            0
        };
        let cpu_milli = cores.checked_mul(1000).ok_or(SimError::Config("cores"))?;
        nodes.push(NodeSpec {
            id: NodeId(id),
            capacity: Resources {
                cpu_milli,
                mem_bytes: mem_gib.checked_mul(GIB).ok_or(SimError::Config("memory"))?,
                gpus,
            },
            // Uniform up to the node's CPU: about one node in ten starts above 90% from its
            // co-tenants alone, and placements push others past it.
            background_milli: rng.range(0, cpu_milli),
            classes: Classes::ALL,
        });
    }
    let mut workloads = Vec::new();
    workloads
        .try_reserve_exact(
            usize::try_from(shape.workloads).map_err(|_| SimError::Config("too many workloads"))?,
        )
        .map_err(|_| SimError::Config("no memory for the workloads"))?;
    let mut arrives: Time = 0;
    let last = u64::from(shape.nodes)
        .checked_sub(1)
        .ok_or(SimError::Config("a cluster has at least one node"))?;
    for id in 0..shape.workloads {
        let at = rng.range(0, last);
        let host = nodes
            .get(usize::try_from(at).map_err(|_| SimError::Config("node index"))?)
            .ok_or(SimError::Config("node index"))?;
        let share = |rng: &mut Rng, of: u64| -> u64 {
            let most = of
                .checked_mul(shape.max_share_per_mille)
                .map_or(of, |x| x / 1000)
                .max(1);
            rng.range(1, most.min(of.max(1)))
        };
        let wants_gpu = host.capacity.gpus > 0 && rng.chance(500);
        let need = Resources {
            cpu_milli: share(&mut rng, host.capacity.cpu_milli),
            mem_bytes: share(&mut rng, host.capacity.mem_bytes),
            gpus: if wants_gpu {
                u32::try_from(rng.range(1, u64::from(host.capacity.gpus))).unwrap_or(1)
            } else {
                0
            },
        };
        let gap = rng.range(
            0,
            shape
                .mean_gap
                .checked_mul(2)
                .ok_or(SimError::TimeOverflow)?,
        );
        arrives = arrives.checked_add(gap).ok_or(SimError::TimeOverflow)?;
        let runs_for = match shape.mean_run {
            Some(mean) => Some(rng.range(1, mean.checked_mul(2).ok_or(SimError::TimeOverflow)?)),
            None => None,
        };
        let kind = match rng.range(0, 999) {
            0..400 => Kind::Plain,
            400..650 => Kind::Build,
            650..800 => Kind::Benchmark,
            // A session drawn against a host above 90% might fit no node that admits it; keep the
            // promise that every workload fits somewhere.
            _ if hot(host) => Kind::Plain,
            _ => Kind::Session,
        };
        workloads.push(WorkloadSpec {
            id: WorkloadId(id),
            need,
            kind,
            arrives,
            runs_for,
            tenancy: Tenancy::default(),
            class: Class::Oci,
        });
    }
    Ok(Cluster::single_owner(nodes, workloads))
}

/// Above 90% CPU from its background alone: `index::HOT_PER_MILLE`, as busy * 1000 > capacity * 900.
fn hot(node: &NodeSpec) -> bool {
    let lhs = u128::from(node.background_milli).checked_mul(1000);
    let rhs =
        u128::from(node.capacity.cpu_milli).checked_mul(u128::from(crate::index::HOT_PER_MILLE));
    matches!((lhs, rhs), (Some(l), Some(r)) if l > r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape() -> Shape {
        Shape {
            nodes: 50,
            workloads: 500,
            mean_gap: 1_000,
            mean_run: Some(50_000),
            gpu_nodes_per_mille: 200,
            max_share_per_mille: 500,
        }
    }

    /// T-T7.4.1's generator: one seed, one cluster; every workload fits an empty node, arrivals in order.
    #[test]
    fn a_seed_names_one_cluster_of_workloads_that_fit() {
        let a = cluster(3, &shape()).unwrap();
        assert_eq!(a, cluster(3, &shape()).unwrap());
        assert_ne!(a, cluster(4, &shape()).unwrap());
        let empty = crate::index::FitIndex::new(&a.nodes).unwrap();
        for w in &a.workloads {
            assert!(empty.admitted_anywhere(&w.demand()), "{w:?}");
        }
        for kind in [Kind::Plain, Kind::Build, Kind::Benchmark, Kind::Session] {
            assert!(a.workloads.iter().any(|w| w.kind == kind), "{kind:?}");
        }
        assert!(a.workloads.windows(2).all(|p| p[0].arrives <= p[1].arrives));
        assert!(a.workloads.iter().any(|w| w.need.gpus > 0));
    }
}
