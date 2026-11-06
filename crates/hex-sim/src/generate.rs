//! Generated clusters and workloads, from one seed. Each workload's need is drawn inside one node's
//! capacity, so every workload fits somewhere and GPU work is drawn only against a GPU node.

use crate::error::SimError;
use crate::model::{
    Class, Cluster, GangId, Group, GroupId, Kind, NodeId, NodeSpec, Owner, OwnerId, Resources,
    Tenancy, Time, WorkloadId, WorkloadSpec,
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
            class: None,
            arrives,
            runs_for,
            tenancy: Tenancy::default(),
        });
    }
    Ok(Cluster::single_owner(nodes, workloads))
}

/// `cluster` with its workloads spread over `owners` owners of weight 1 to 4 and bands `0..bands`,
/// drawn from `seed`: the model-based test's multi-owner clusters.
pub fn tenants(
    mut cluster: Cluster,
    seed: u64,
    owners: u32,
    bands: u8,
) -> Result<Cluster, SimError> {
    if owners == 0 || bands == 0 {
        return Err(SimError::Config("at least one owner and one band"));
    }
    let mut rng = Rng::new(seed ^ 0x7e4a_4c15);
    cluster.owners = (0..owners)
        .map(|_| Owner {
            weight: u32::try_from(rng.range(1, 4)).unwrap_or(1),
        })
        .collect();
    let top_owner = u64::from(owners.saturating_sub(1));
    let top_band = u64::from(bands.saturating_sub(1));
    for w in &mut cluster.workloads {
        w.tenancy.owner = OwnerId(u32::try_from(rng.range(0, top_owner)).unwrap_or(0));
        w.tenancy.band = u8::try_from(rng.range(0, top_band)).unwrap_or(0);
    }
    Ok(cluster)
}

/// `cluster` with a burst (`T-T7.5.1`): `count` workloads of `owner`'s, all arriving at `at`, each of
/// 1 to `max_cores` cores and 2 GiB a core, running 1 minute to `2 * mean_run` less a minute, and
/// OCI containers, Firecracker microVMs and Shards microVMs in turn ([`Class::nth`]). The
/// workloads are put back in arrival order (the burst after what arrived at `at` before it) and their
/// ids made dense again.
pub fn burst(
    mut cluster: Cluster,
    seed: u64,
    owner: OwnerId,
    at: Time,
    count: u64,
    max_cores: u64,
    mean_run: Time,
) -> Result<Cluster, SimError> {
    let mut rng = Rng::new(seed ^ 0xb025_7b25);
    let minute: Time = 60_000_000_000;
    let longest = mean_run
        .checked_mul(2)
        .and_then(|x| x.checked_sub(minute))
        .ok_or(SimError::TimeOverflow)?;
    let extra = usize::try_from(count).map_err(|_| SimError::Config("burst workloads"))?;
    cluster
        .workloads
        .try_reserve_exact(extra)
        .map_err(|_| SimError::Config("no memory for the burst"))?;
    for i in 0..count {
        let cores = rng.range(1, max_cores.max(1));
        cluster.workloads.push(WorkloadSpec {
            id: WorkloadId(u64::MAX),
            need: Resources {
                cpu_milli: cores.checked_mul(1000).ok_or(SimError::Config("cores"))?,
                mem_bytes: cores
                    .checked_mul(2)
                    .and_then(|g| g.checked_mul(GIB))
                    .ok_or(SimError::Config("memory"))?,
                gpus: 0,
            },
            kind: Kind::Plain,
            class: Some(Class::nth(i)),
            arrives: at,
            runs_for: Some(rng.range(minute, longest)),
            tenancy: Tenancy {
                owner,
                ..Tenancy::default()
            },
        });
    }
    // Stable: the burst stays after what arrived at `at` before it.
    cluster.workloads.sort_by_key(|w| w.arrives);
    for (id, w) in (0u64..).zip(cluster.workloads.iter_mut()) {
        w.id = WorkloadId(id);
    }
    Ok(cluster)
}

/// `cluster` with a burst drawn from its own workloads (`T-T7.5.1` on a recorded day): `count` workloads of
/// `owner`'s (a new owner of weight 1 if the cluster has fewer), all arriving at `at`, each taking the
/// need, run time and kind of one of the cluster's workloads chosen uniformly, with replacement. The
/// workloads are put back in arrival order and their ids made dense again, as [`burst`] does.
pub fn burst_from(
    mut cluster: Cluster,
    seed: u64,
    owner: OwnerId,
    at: Time,
    count: u64,
) -> Result<Cluster, SimError> {
    let mut rng = Rng::new(seed ^ 0xb025_f0d1);
    let n = u64::try_from(cluster.workloads.len()).map_err(|_| SimError::Config("workloads"))?;
    let last = n
        .checked_sub(1)
        .ok_or(SimError::Config("a burst drawn from no workloads"))?;
    let extra = usize::try_from(count).map_err(|_| SimError::Config("burst workloads"))?;
    let mut drawn = Vec::new();
    drawn
        .try_reserve_exact(extra)
        .map_err(|_| SimError::Config("no memory for the burst"))?;
    for _ in 0..count {
        let i = usize::try_from(rng.range(0, last)).map_err(|_| SimError::Config("workloads"))?;
        let like = cluster
            .workloads
            .get(i)
            .ok_or(SimError::Config("workload index"))?;
        drawn.push(WorkloadSpec {
            id: WorkloadId(u64::MAX),
            arrives: at,
            tenancy: Tenancy {
                owner,
                ..Tenancy::default()
            },
            ..*like
        });
    }
    while usize::try_from(owner.0).is_ok_and(|o| o >= cluster.owners.len()) {
        cluster.owners.push(Owner { weight: 1 });
    }
    cluster
        .workloads
        .try_reserve_exact(extra)
        .map_err(|_| SimError::Config("no memory for the burst"))?;
    cluster.workloads.extend(drawn);
    // Stable: the burst stays after what arrived at `at` before it.
    cluster.workloads.sort_by_key(|w| w.arrives);
    for (id, w) in (0u64..).zip(cluster.workloads.iter_mut()) {
        w.id = WorkloadId(id);
    }
    Ok(cluster)
}

/// `cluster` with `groups` disruption groups of budgets 1 to `max_budget`, each workload in one
/// of them with probability `per_mille / 1000`.
pub fn groups(
    mut cluster: Cluster,
    seed: u64,
    groups: u32,
    max_budget: u32,
    per_mille: u64,
) -> Cluster {
    let mut rng = Rng::new(seed ^ 0x9e0b_d15e);
    cluster.groups = (0..groups)
        .map(|_| Group {
            budget: u32::try_from(rng.range(1, u64::from(max_budget.max(1)))).unwrap_or(1),
        })
        .collect();
    if groups == 0 {
        return cluster;
    }
    for w in &mut cluster.workloads {
        if rng.chance(per_mille) {
            let g = rng.range(0, u64::from(groups.saturating_sub(1)));
            w.tenancy.group = Some(GroupId(u32::try_from(g).unwrap_or(0)));
        }
    }
    cluster
}

/// `cluster` with runs of 2 to `max_size` consecutive workloads made gangs (workloads that only work together),
/// starting one at a workload with probability `per_mille / 1000`: each member takes the first's
/// arrival, owner, band and group, so a gang is submitted at once into one queue.
pub fn gangs(mut cluster: Cluster, seed: u64, per_mille: u64, max_size: u64) -> Cluster {
    let mut rng = Rng::new(seed ^ 0x6a46_9a46);
    let mut next = 0u64;
    let mut at = 0usize;
    while let Some(first) = cluster.workloads.get(at).copied() {
        if !rng.chance(per_mille) {
            at = at.saturating_add(1);
            continue;
        }
        let size = usize::try_from(rng.range(2, max_size.max(2))).unwrap_or(2);
        let gang = Some(GangId(next));
        next = next.saturating_add(1);
        for w in cluster.workloads.iter_mut().skip(at).take(size) {
            w.arrives = first.arrives;
            w.tenancy = Tenancy {
                gang,
                ..first.tenancy
            };
        }
        at = at.saturating_add(size);
    }
    cluster
}

/// One tenant's workloads over a [`Day`]: their sizes, and how much of the cluster's capacity they
/// ask for, in parts per million, in the busy hours and out of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tenant {
    pub weight: u32,
    pub cores: (u64, u64),
    pub gib: (u64, u64),
    pub busy_ppm: u64,
    pub quiet_ppm: u64,
}

/// A day of workloads on identical nodes: each tenant's workloads arrive at its rate, more in the busy
/// hours `busy.0..busy.1`, and run for a minute to `2 * mean_run` less a minute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Day {
    pub nodes: u32,
    pub node: Resources,
    pub tenants: Vec<Tenant>,
    pub length: Time,
    pub busy: (Time, Time),
    pub mean_run: Time,
    /// Past this many workloads the day is refused, not cut short.
    pub max_vms: usize,
}

/// A generated day of workloads ([`Day`]): every tenant an owner, and the workloads OCI containers,
/// Firecracker microVMs and Shards microVMs in turn by id ([`Class::nth`]), a third of each.
pub fn day(seed: u64, day: &Day) -> Result<Cluster, SimError> {
    if day.nodes == 0 || day.tenants.is_empty() {
        return Err(SimError::Config("a day has nodes and tenants"));
    }
    let nodes: Vec<NodeSpec> = (0..day.nodes)
        .map(|id| NodeSpec {
            id: NodeId(id),
            capacity: day.node,
            background_milli: 0,
        })
        .collect();
    let total_cpu = day
        .node
        .cpu_milli
        .checked_mul(u64::from(day.nodes))
        .ok_or(SimError::Config("cluster CPU"))?;
    let total_mem = day
        .node
        .mem_bytes
        .checked_mul(u64::from(day.nodes))
        .ok_or(SimError::Config("cluster memory"))?;
    let mut rng = Rng::new(seed);
    let mut vms: Vec<(Time, u32, Resources, Time)> = Vec::new();
    for (owner, tenant) in (0u32..).zip(&day.tenants) {
        let mut t: Time = 0;
        while t < day.length {
            if vms.len() >= day.max_vms {
                return Err(SimError::Full {
                    what: "the day's workloads",
                    bound: day.max_vms,
                });
            }
            let cores = rng.range(tenant.cores.0, tenant.cores.1);
            let need = Resources {
                cpu_milli: cores.checked_mul(1000).ok_or(SimError::Config("cores"))?,
                mem_bytes: rng
                    .range(tenant.gib.0, tenant.gib.1)
                    .checked_mul(GIB)
                    .ok_or(SimError::Config("memory"))?,
                gpus: 0,
            };
            // The workload's dominant share of the cluster, in parts per million.
            let ppm = |n: u64, of: u64| {
                n.checked_mul(1_000_000)
                    .and_then(|x| x.checked_div(of))
                    .unwrap_or(0)
            };
            let leg_ppm = ppm(need.cpu_milli, total_cpu).max(ppm(need.mem_bytes, total_mem));
            let rate = if (day.busy.0..day.busy.1).contains(&t) {
                tenant.busy_ppm
            } else {
                tenant.quiet_ppm
            };
            // Mean gap = run time * workload share / the share asked for, so the workloads in flight ask for
            // `rate` of the cluster.
            let mean_gap = day
                .mean_run
                .checked_mul(leg_ppm)
                .and_then(|x| x.checked_div(rate.max(1)))
                .ok_or(SimError::TimeOverflow)?
                .max(1);
            t = t
                .checked_add(rng.range(1, mean_gap.checked_mul(2).ok_or(SimError::TimeOverflow)?))
                .ok_or(SimError::TimeOverflow)?;
            let minute: Time = 60_000_000_000;
            let runs_for = rng.range(
                minute,
                day.mean_run
                    .checked_mul(2)
                    .and_then(|x| x.checked_sub(minute))
                    .ok_or(SimError::TimeOverflow)?,
            );
            if t < day.length {
                vms.push((t, owner, need, runs_for));
            }
        }
    }
    vms.sort_unstable_by_key(|&(t, owner, _, _)| (t, owner));
    let mut workloads = Vec::new();
    workloads
        .try_reserve_exact(vms.len())
        .map_err(|_| SimError::Config("no memory for the workloads"))?;
    for (id, (arrives, owner, need, runs_for)) in (0u64..).zip(vms) {
        workloads.push(WorkloadSpec {
            id: WorkloadId(id),
            need,
            kind: Kind::Plain,
            class: Some(Class::nth(id)),
            arrives,
            runs_for: Some(runs_for),
            tenancy: Tenancy {
                owner: OwnerId(owner),
                ..Tenancy::default()
            },
        });
    }
    Ok(Cluster {
        nodes,
        workloads,
        owners: day
            .tenants
            .iter()
            .map(|r| Owner { weight: r.weight })
            .collect(),
        groups: Vec::new(),
    })
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
        for kind in [Kind::Plain, Kind::Plain, Kind::Benchmark, Kind::Session] {
            assert!(a.workloads.iter().any(|w| w.kind == kind), "{kind:?}");
        }
        assert!(a.workloads.windows(2).all(|p| p[0].arrives <= p[1].arrives));
        assert!(a.workloads.iter().any(|w| w.need.gpus > 0));
    }
}
