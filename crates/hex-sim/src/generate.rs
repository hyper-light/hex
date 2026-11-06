//! Generated clusters and workloads, from one seed. Each node runs the classes its platform can, as
//! Hex's fleet's do: Linux all three, Windows OCI containers and Shards microVMs, macOS Shards
//! microVMs alone (bench/sched/huawei_workload.md). Each workload's class and need are drawn against
//! one node, a class it runs and a need inside its capacity, so every workload fits somewhere; GPU
//! work is drawn only against a GPU node, and only as an OCI container, the one class a GPU can be
//! given to ([`Class::takes_gpus`]).

use crate::error::SimError;
use crate::model::{
    Class, Classes, Cluster, Kind, NodeId, NodeSpec, Overheads, Owner, OwnerId, Resources, Tenancy,
    Time, WorkloadId, WorkloadSpec,
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
        // Six nodes in ten Linux, two Windows, two macOS: each class on many nodes, and some nodes
        // that run only some.
        let classes = match rng.range(0, 9) {
            0..6 => Classes::ALL,
            6..8 => Classes::NONE.with(Class::Oci).with(Class::Shards),
            _ => Classes::NONE.with(Class::Shards),
        };
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
            classes,
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
        let class = one_of(&mut rng, host.classes);
        let wants_gpu = host.capacity.gpus > 0 && class.takes_gpus() && rng.chance(500);
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
            class,
        });
    }
    Ok(Cluster::single_owner(nodes, workloads))
}

/// G2's two node models (`bench/sched/huawei_workload.md`, "Where each kind may land"; the same hosts
/// as `bench/sched/huawei_trace.py:79-94`). Each node runs the classes its OS runs: Linux all three,
/// Windows OCI containers and Shards microVMs, macOS Shards microVMs alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeModel {
    /// EC2 m5d.metal, the main model: 48,000 millicores, 384 GiB, no GPU, Linux with KVM, so every
    /// class (Agache et al., NSDI 2020 §6: "two Intel Xeon Platinum 8175M processors (for a total of
    /// 48 cores with hyper-threading disabled), 384GB of RAM").
    M5dMetal,
    /// Hex's fleet beside it: its ten hosts from kiln's runner facts (2026-11-05), in G2's order,
    /// node `id` being host `id mod 10`, as `sched_fast_path` repeats them.
    Fleet,
}

/// Hex's fleet: threads, memory bytes, GPUs, and the OS's classes (`huawei_trace.py:83-94`).
const FLEET: [(u64, u64, u32, Os); 10] = [
    (32, 131_986_903_040, 1, Os::Linux),   // adacachy
    (32, 67_128_643_584, 0, Os::Linux),    // fedora, kiln-a094a12d.attlocal.net
    (32, 132_766_306_304, 0, Os::Windows), // jupiter
    (18, 68_719_476_736, 0, Os::Macos),    // adalundhe-macbook-pro
    (14, 38_654_705_664, 0, Os::Macos),    // adas-mac-studio-2.local
    (14, 38_654_705_664, 0, Os::Macos),    // adas-mac-studio-3.local
    (14, 38_654_705_664, 0, Os::Macos),    // adas-mac-studio.local
    (32, 103_079_215_104, 0, Os::Macos),   // adas-mac-studio.local-2
    (15, 51_539_607_552, 0, Os::Macos),    // adas-macbook-pro.local
    (18, 137_438_953_472, 0, Os::Macos),   // e30189ea29182644fe5a4b2e06c7dac2
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Os {
    Linux,
    Windows,
    Macos,
}

impl Os {
    fn classes(self) -> Classes {
        match self {
            Os::Linux => Classes::ALL,
            Os::Windows => Classes::NONE.with(Class::Oci).with(Class::Shards),
            Os::Macos => Classes::NONE.with(Class::Shards),
        }
    }
}

impl NodeModel {
    /// Node `id` of the model.
    pub fn node(self, id: u32) -> NodeSpec {
        let (cpu_milli, mem_bytes, gpus, classes) = match self {
            NodeModel::M5dMetal => (48_000, 384 << 30, 0, Classes::ALL),
            NodeModel::Fleet => {
                // `id mod 10` is always a host; the fallback is unreachable and empty.
                let at = usize::try_from(id.checked_rem(10).unwrap_or(0)).unwrap_or(0);
                let (threads, mem, gpus, os) =
                    FLEET.get(at).copied().unwrap_or((0, 0, 0, Os::Macos));
                (threads.saturating_mul(1000), mem, gpus, os.classes())
            }
        };
        NodeSpec {
            id: NodeId(id),
            capacity: Resources {
                cpu_milli,
                mem_bytes,
                gpus,
            },
            background_milli: 0,
            classes,
        }
    }

    /// `n` nodes of the model, ids from 0.
    pub fn nodes(self, n: u32) -> Vec<NodeSpec> {
        (0..n).map(|id| self.node(id)).collect()
    }

    pub fn name(self) -> &'static str {
        match self {
            NodeModel::M5dMetal => "EC2 m5d.metal",
            NodeModel::Fleet => "Hex's fleet",
        }
    }
}

/// Each class's runtime overhead in bytes of memory, as G2 measured or cited them
/// (`bench/sched/huawei_workload.md`, "Overheads"): OCI 440,402 (0.42 MiB, Hex's idle container,
/// `bench/competitive/RESULT.md:95`); Firecracker 5 MiB (its specification's bound on the VMM, and
/// Agache et al., NSDI 2020); Shards 5 MiB, A STAND-IN: no source states Shards' own yet.
pub fn overheads() -> Overheads {
    let mut o = Overheads::default();
    o.set(Class::Oci, 440_402);
    o.set(Class::Firecracker, 5 << 20);
    o.set(Class::Shards, 5 << 20);
    o
}

/// Gives each workload an owner, uniformly from `ceil(workloads / per_owner)` owners of weight 1,
/// from `seed`. G2's recorded day has 75 launches an owner (25,295 of 339 owners,
/// `bench/sched/huawei_workload.md`, "Each request's kind").
pub fn owners(cluster: &mut Cluster, seed: u64, per_owner: u64) -> Result<(), SimError> {
    let n = u64::try_from(cluster.workloads.len())
        .map_err(|_| SimError::Config("too many workloads"))?;
    let count = n
        .checked_add(per_owner.saturating_sub(1))
        .and_then(|x| x.checked_div(per_owner))
        .ok_or(SimError::Config("an owner has at least one launch"))?
        .max(1);
    let last = count.checked_sub(1).ok_or(SimError::Config("owners"))?;
    let mut rng = Rng::new(seed ^ 0x0e1e_7e45);
    for w in &mut cluster.workloads {
        w.tenancy.owner =
            OwnerId(u32::try_from(rng.range(0, last)).map_err(|_| SimError::Config("owners"))?);
    }
    cluster.owners =
        vec![Owner::default(); usize::try_from(count).map_err(|_| SimError::Config("owners"))?];
    Ok(())
}

/// G2's kind rule (`bench/sched/huawei_trace.py`'s `classes_by_owner`): all of an owner's workloads
/// are one class, and owners are split so each class gets a third of the launches: the owner with the
/// most launches first (then the lower id), each to the class with the fewest launches so far, ties
/// OCI, then Firecracker, then Shards. Each workload's memory then gains its class's overhead. Returns
/// the launches each class got, OCI, Firecracker, Shards.
pub fn by_owner(cluster: &mut Cluster, overheads: Overheads) -> Result<[u64; 3], SimError> {
    let mut launches = vec![0u64; cluster.owners.len()];
    for w in &cluster.workloads {
        let at = usize::try_from(w.tenancy.owner.0).map_err(|_| SimError::Config("owner"))?;
        let n = launches.get_mut(at).ok_or(SimError::Config(
            "a workload's owner is one of the cluster's",
        ))?;
        *n = n.checked_add(1).ok_or(SimError::Config("launches"))?;
    }
    let mut order: Vec<usize> = (0..launches.len()).collect();
    order.sort_by_key(|&o| (std::cmp::Reverse(launches.get(o).copied().unwrap_or(0)), o));
    const CLASSES: [Class; 3] = [Class::Oci, Class::Firecracker, Class::Shards];
    let mut got = [0u64; 3];
    let mut class_of = vec![Class::Oci; launches.len()];
    for o in order {
        let k = (0..3)
            .min_by_key(|&k| (got.get(k).copied().unwrap_or(u64::MAX), k))
            .ok_or(SimError::Config("classes"))?;
        let n = launches.get(o).copied().unwrap_or(0);
        let g = got.get_mut(k).ok_or(SimError::Config("classes"))?;
        *g = g.checked_add(n).ok_or(SimError::Config("launches"))?;
        if let (Some(slot), Some(&class)) = (class_of.get_mut(o), CLASSES.get(k)) {
            *slot = class;
        }
    }
    for w in &mut cluster.workloads {
        let at = usize::try_from(w.tenancy.owner.0).map_err(|_| SimError::Config("owner"))?;
        w.class = *class_of.get(at).ok_or(SimError::Config("owner"))?;
        w.need.mem_bytes =
            w.need
                .mem_bytes
                .checked_add(overheads.of(w.class))
                .ok_or(SimError::Config(
                    "a workload's memory with its class's overhead",
                ))?;
    }
    cluster.overheads = overheads;
    Ok(got)
}

/// One of the classes `classes` holds, each alike (OCI when it holds none).
fn one_of(rng: &mut Rng, classes: Classes) -> Class {
    let pick = rng.range(0, u64::from(classes.count().saturating_sub(1)));
    [Class::Oci, Class::Firecracker, Class::Shards]
        .into_iter()
        .filter(|&c| classes.has(c))
        .nth(usize::try_from(pick).unwrap_or(0))
        .unwrap_or_default()
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

    /// T-T7.4.1's generator: one seed, one cluster; every workload fits an empty node, arrivals in
    /// order; each class drawn, some nodes running only some, and GPU work only as OCI containers.
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
        for class in [Class::Oci, Class::Firecracker, Class::Shards] {
            assert!(a.workloads.iter().any(|w| w.class == class), "{class:?}");
        }
        assert!(a.nodes.iter().any(|n| n.classes != Classes::ALL));
        assert!(
            a.workloads
                .iter()
                .all(|w| w.need.gpus == 0 || w.class == Class::Oci)
        );
    }

    /// G2's kind rule (`huawei_trace.py`'s `classes_by_owner`): do owners of 5, 3, 3, 2 and 1
    /// launches; expect OCI 5, Firecracker 5 (owners 1 and 3), Shards 4 (owners 2 and 4), each
    /// workload's memory raised by its class's overhead.
    #[test]
    fn owners_go_whole_to_the_class_with_fewest_launches() {
        let mut c = cluster(3, &shape()).unwrap();
        c.workloads.truncate(14);
        let counts = [5u32, 3, 3, 2, 1];
        let mut at = 0;
        for (o, &n) in counts.iter().enumerate() {
            for _ in 0..n {
                c.workloads[at].tenancy.owner = OwnerId(u32::try_from(o).unwrap());
                at += 1;
            }
        }
        c.owners = vec![Owner::default(); 5];
        let before: Vec<u64> = c.workloads.iter().map(|w| w.need.mem_bytes).collect();
        let got = by_owner(&mut c, overheads()).unwrap();
        assert_eq!(got, [5, 5, 4]);
        let want = [
            Class::Oci,
            Class::Firecracker,
            Class::Shards,
            Class::Firecracker,
            Class::Shards,
        ];
        for (w, mem) in c.workloads.iter().zip(before) {
            let class = want[usize::try_from(w.tenancy.owner.0).unwrap()];
            assert_eq!(w.class, class);
            assert_eq!(w.need.mem_bytes, mem + overheads().of(class));
        }
        assert_eq!(c.overheads, overheads());
    }
}
