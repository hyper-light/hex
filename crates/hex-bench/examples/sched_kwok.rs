//! AC-T7.11.3's cluster, on both sides of the comparison (`docs/design/hex-sim.md` H63,
//! `bench/sched/kube_scheduler_kwok.md`): 10,000 nodes of `scheduler_perf`'s node sized to hold its
//! own 110-pod limit (11 CPU, 55 GiB, 110 pods) and 50,000 of its pause pods (100m CPU, 500Mi).
//!
//!   cargo run --release -p hex-bench --example sched_kwok -- manifests <out-dir> <files>
//!   cargo run --release -p hex-bench --example sched_kwok -- hex <runs> <shards,...>
//!
//! `manifests` writes the cluster for kube-scheduler on kwok's fake nodes (`bench/sched/kwok/run.sh`
//! drives it): `nodes-<i>.yaml` and `pods-<i>.yaml` for `i` in `0..files`, multi-document YAML, so
//! the files can be created side by side. Each node carries kwok's annotation and taint
//! (kwok.x-k8s.io/node=fake, NoSchedule) and the shape as both capacity and allocatable; each pod
//! requests the pause pod's shape and tolerates kwok's taint.
//!
//! Both sides place Hex's three classes (`docs/design/hex-sim.md` H69). Nodes run, by id, six in ten
//! Linux (all three classes), two Windows (OCI containers and Shards microVMs), two macOS (Shards
//! alone); each pod is an OCI container, a Firecracker or a Shards microVM by G2's rule (owners at 75
//! launches each, a third of the pods each), and its memory request carries its class's overhead (OCI
//! 0.42 MiB, Firecracker 5 MiB, Shards 5 MiB, a stand-in). For kube-scheduler a node carries a label
//! `hex.dev/<class>: "true"` for each class it runs (and `kubernetes.io/os` its platform's), and a
//! pod a required node affinity on its class's label (`requiredDuringSchedulingIgnoredDuringExecution`),
//! so kube-scheduler is held to where each class may run, as Hex's index is.
//!
//! `hex` places the same 50,000 on the same 10,000 with hex-sim's own fast path, as
//! `benches/sched_million.rs` does: every pod submitted to the store before the clock starts, the
//! nodes split into `shards` disjoint partitions (node `n` to shard `n % shards`, pod `id % shards`),
//! one thread per shard running the sampled best fit (`FitIndex::sampled_fit`, kube-scheduler's floors
//! of 100 nodes or 5%), the compare-and-set commit (`Store::apply`) and the index update, in submit
//! order. The time runs from the first shard starting to the last one ending. After each run
//! (untimed) every record must be placed, and no node above its capacity or its 110 pods.
// An example (not production, CLAUDE.md): it takes the test-style allowances, as the benches do.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::disallowed_macros,
    clippy::print_stdout,
    clippy::arithmetic_side_effects,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation
)]

use std::fs::File;
use std::io::{BufWriter, Write as _};
use std::path::Path;
use std::sync::Barrier;
use std::time::{Duration, Instant};

use hex_sim::generate;
use hex_sim::index::{FitIndex, Sample};
use hex_sim::model::{
    Class, Cluster, Kind, NodeId, NodeSpec, Resources, Tenancy, WorkloadId, WorkloadSpec,
};
use hex_sim::store::{Outcome, State, Store, Write};

const NODES: u32 = 10_000;
const PODS: u64 = 50_000;
const MAX_PODS: u64 = 110;
const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;
/// `scheduler_perf`'s pause pod: 100m CPU, 500Mi.
const POD: Resources = Resources {
    cpu_milli: 100,
    mem_bytes: 500 * MIB,
    gpus: 0,
};
/// A node that holds its own 110-pod limit of that pod.
const NODE: Resources = Resources {
    cpu_milli: 11_000,
    mem_bytes: 55 * GIB,
    gpus: 0,
};

fn nodes() -> Vec<NodeSpec> {
    (0..NODES)
        .map(|id| NodeSpec {
            id: NodeId(id),
            capacity: NODE,
            background_milli: 0,
            classes: generate::platform(id),
        })
        .collect()
}

fn pods() -> Vec<WorkloadSpec> {
    let work = (0..PODS)
        .map(|id| WorkloadSpec {
            id: WorkloadId(id),
            need: POD,
            kind: Kind::Plain,
            arrives: 0,
            runs_for: None,
            tenancy: Tenancy::default(),
            class: Class::Oci,
        })
        .collect();
    let mut c = Cluster::single_owner(Vec::new(), work);
    generate::owners(&mut c, 7, 75).unwrap();
    generate::by_owner(&mut c, generate::overheads()).unwrap();
    c.workloads
}

/// Each class's node label, and its name there.
fn label(class: Class) -> &'static str {
    match class {
        Class::Oci => "oci",
        Class::Firecracker => "firecracker",
        Class::Shards => "shards",
    }
}

const CLASSES: [Class; 3] = [Class::Oci, Class::Firecracker, Class::Shards];

fn manifests(out: &Path, files: u32) {
    assert!(files > 0, "at least one file");
    std::fs::create_dir_all(out).unwrap();
    let open = |what: &str, i: u32| {
        BufWriter::new(File::create(out.join(format!("{what}-{i:03}.yaml"))).unwrap())
    };
    let mut w: Vec<_> = (0..files).map(|i| open("nodes", i)).collect();
    for (i, node) in nodes().iter().enumerate() {
        let f = &mut w[i % files as usize];
        let name = format!("kwok-node-{}", node.id.0);
        let r = format!(
            "\n    cpu: {}m\n    memory: \"{}\"\n    pods: \"{MAX_PODS}\"",
            node.capacity.cpu_milli, node.capacity.mem_bytes
        );
        let classes: String = CLASSES
            .iter()
            .filter(|&&c| node.classes.has(c))
            .map(|&c| format!("\n    hex.dev/{}: \"true\"", label(c)))
            .collect();
        let os = if node.classes.has(Class::Firecracker) {
            "linux"
        } else if node.classes.has(Class::Oci) {
            "windows"
        } else {
            "darwin"
        };
        write!(
            f,
            "---
apiVersion: v1
kind: Node
metadata:
  name: {name}
  annotations:
    kwok.x-k8s.io/node: fake
    node.alpha.kubernetes.io/ttl: \"0\"
  labels:
    kubernetes.io/arch: amd64
    kubernetes.io/hostname: {name}
    kubernetes.io/os: {os}
    kubernetes.io/role: agent
    type: kwok{classes}
spec:
  taints:
  - effect: NoSchedule
    key: kwok.x-k8s.io/node
    value: fake
status:
  allocatable:{r}
  capacity:{r}
  nodeInfo:
    architecture: amd64
    operatingSystem: {os}
  phase: Running
"
        )
        .unwrap();
    }
    w.iter_mut().for_each(|f| f.flush().unwrap());
    let mut w: Vec<_> = (0..files).map(|i| open("pods", i)).collect();
    for (i, pod) in pods().iter().enumerate() {
        let f = &mut w[i % files as usize];
        write!(
            f,
            "---
apiVersion: v1
kind: Pod
metadata:
  name: pause-{id}
  namespace: default
  labels:
    hex.dev/class: {class}
spec:
  affinity:
    nodeAffinity:
      requiredDuringSchedulingIgnoredDuringExecution:
        nodeSelectorTerms:
        - matchExpressions:
          - key: hex.dev/{class}
            operator: In
            values: [\"true\"]
  automountServiceAccountToken: false
  enableServiceLinks: false
  containers:
  - name: pause
    image: registry.k8s.io/pause:3.10
    resources:
      requests:
        cpu: {cpu}m
        memory: \"{mem}\"
  tolerations:
  - effect: NoSchedule
    key: kwok.x-k8s.io/node
    operator: Exists
",
            id = pod.id.0,
            cpu = pod.need.cpu_milli,
            mem = pod.need.mem_bytes,
            class = label(pod.class),
        )
        .unwrap();
    }
    w.iter_mut().for_each(|f| f.flush().unwrap());
}

/// One shard: its nodes (ids dense from 0 within it), its pending pods with the version each submit
/// returned, its slice of the store and its index.
struct Shard {
    nodes: Vec<NodeSpec>,
    pending: Vec<(WorkloadSpec, u64)>,
    store: Store,
    index: FitIndex,
}

fn shards_of(n: usize) -> Vec<Shard> {
    let mut parts: Vec<Vec<NodeSpec>> = vec![Vec::new(); n];
    for node in nodes() {
        let part = &mut parts[node.id.0 as usize % n];
        let local = NodeId(part.len() as u32);
        part.push(NodeSpec { id: local, ..node });
    }
    let mut work: Vec<Vec<WorkloadSpec>> = vec![Vec::new(); n];
    for w in pods() {
        work[w.id.0 as usize % n].push(w);
    }
    parts
        .into_iter()
        .zip(work)
        .map(|(nodes, work)| {
            let mut store = Store::new(&nodes, work.len(), work.len() * 2).unwrap();
            let pending = work
                .into_iter()
                .map(|w| match store.apply(0, Write::Submit(w)).unwrap() {
                    Outcome::Applied { version, .. } => (w, version),
                    other => panic!("submit of {:?}: {other:?}", w.id),
                })
                .collect();
            let index = FitIndex::new(&nodes).unwrap();
            Shard {
                nodes,
                pending,
                store,
                index,
            }
        })
        .collect()
}

struct Placed {
    shard: Shard,
    placed: u64,
    began: Instant,
    ended: Instant,
}

fn place(mut shard: Shard, sample: &Sample, start: &Barrier) -> Placed {
    start.wait();
    let began = Instant::now();
    let mut placed = 0;
    for (spec, version) in &shard.pending {
        let demand = spec.demand();
        let Some(best) = shard.index.sampled_fit(&demand, sample).best else {
            continue;
        };
        let write = Write::Place {
            workload: spec.id,
            node: best.node,
            version: *version,
        };
        match shard.store.apply(0, write).unwrap() {
            Outcome::Applied { .. } => {}
            other => panic!(
                "the store refused the index's choice for {:?}: {other:?}",
                spec.id
            ),
        }
        shard.index.take(best.node, &demand).unwrap();
        placed += 1;
    }
    Placed {
        ended: Instant::now(),
        shard,
        placed,
        began,
    }
}

/// Every record placed on a node that runs its class, and no node above its capacity or its pod
/// limit.
fn check(p: &Placed) -> Result<(), String> {
    let mut used = vec![(Resources::default(), 0u64); p.shard.nodes.len()];
    for r in p.shard.store.records() {
        let State::Placed(node) = r.state else {
            return Err(format!("{:?} is {:?}", r.spec.id, r.state));
        };
        if !p.shard.nodes[node.0 as usize].classes.has(r.spec.class) {
            return Err(format!("{:?} of {:?} on {node:?}", r.spec.id, r.spec.class));
        }
        let u = &mut used[node.0 as usize];
        u.0 = u.0.checked_add(&r.spec.need).ok_or("overflow")?;
        u.1 += 1;
    }
    for (node, (u, pods)) in p.shard.nodes.iter().zip(&used) {
        if !node.capacity.holds(u) || *pods > MAX_PODS {
            return Err(format!("{:?} holds {u:?} in {pods} pods", node.id));
        }
    }
    Ok(())
}

fn hex(runs: u32, shard_counts: &[usize]) {
    let sample = Sample::default();
    let cpus = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    println!(
        "sched_kwok hex: {PODS} pending pods of {POD:?} on {NODES} nodes of {NODE:?}, {cpus} CPUs, {runs} runs each"
    );
    for &n in shard_counts {
        let mut best: Option<Duration> = None;
        for run in 0..runs {
            let parts = shards_of(n);
            let start = Barrier::new(parts.len());
            let done: Vec<Placed> = std::thread::scope(|s| {
                let handles: Vec<_> = parts
                    .into_iter()
                    .map(|part| {
                        let (start, sample) = (&start, &sample);
                        s.spawn(move || place(part, sample, start))
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });
            let began = done.iter().map(|p| p.began).min().unwrap();
            let ended = done.iter().map(|p| p.ended).max().unwrap();
            let wall = ended.saturating_duration_since(began);
            let placed: u64 = done.iter().map(|p| p.placed).sum();
            let mut by_class = [0u64; 3];
            for r in done.iter().flat_map(|p| p.shard.store.records()) {
                if matches!(r.state, State::Placed(_)) {
                    by_class[r.spec.class as usize] += 1;
                }
            }
            let checked: Result<Vec<()>, String> = done.iter().map(check).collect();
            let ok = placed == PODS && checked.is_ok();
            println!(
                "run shards={n} run={run} placed={placed} placed_oci_firecracker_shards={by_class:?} wall_s={:.6} placements_per_s={:.0} check={}",
                wall.as_secs_f64(),
                placed as f64 / wall.as_secs_f64(),
                checked.map_or_else(|e| format!("FAILED {e}"), |_| "ok".into()),
            );
            if ok && best.is_none_or(|b| wall < b) {
                best = Some(wall);
            }
        }
        match best {
            Some(wall) => println!(
                "best shards={n} of {runs}: wall_s={:.6} placements_per_s={:.0}",
                wall.as_secs_f64(),
                PODS as f64 / wall.as_secs_f64()
            ),
            None => println!("best shards={n}: NONE (no run placed every pod within limits)"),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| {
            a.into_string()
                .unwrap_or_else(|a| panic!("an argument isn't Unicode: {a:?}"))
        })
        .collect();
    let usage = "usage: sched_kwok manifests <out-dir> <files> | hex <runs> <shards,...>";
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["manifests", out, files] => manifests(Path::new(out), files.parse().expect(usage)),
        ["hex", runs, shards] => hex(
            runs.parse().expect(usage),
            &shards
                .split(',')
                .map(|s| s.parse().expect(usage))
                .collect::<Vec<_>>(),
        ),
        _ => panic!("{usage}"),
    }
}
