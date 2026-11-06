# The scheduler fast path's decision latency (AC-T7.4.2)

**Bar:** placement decision p99 at 10,000 placements/s on 10,000 simulated nodes is at most 10 ms
(`AC-T7.4.2`; G2 step 5). **Result: MET on the recorded microVM workload (`huawei_workload.md`), on both
node models, for the mix of kinds and for each kind alone, at `5374202`. Best of 5 by p99, on 10,000 EC2
m5d.metal nodes (the main model): 21.4 µs on the mix, 23.4 µs for OCI containers alone, 23.0 µs for
Firecracker microVMs alone and 29.0 µs for Shards microVMs alone. On 10,000 nodes of Hex's fleet's host
types: 30.0, 28.6, 27.4 and 30.3 µs. Every run's p99 is at most 39.1 µs.**

## What was measured

`crates/hex-bench/benches/sched_fast_path.rs` (its header says how). It runs hex-sim's own fast path
on a release build, wall-clock:
- Hex's sampled choice: `FitIndex::sampled_fit` with `Scorer::HEX`, kube-scheduler's floors of 100 nodes
  or 5%, and the hard filters of `docs/design/hex-sim.md` H9, the kind filter among them;
- the compare-and-set commit to the store;
- the index update.

- **Nodes.** 10,000 of them, repeating one node model's hosts: 10,000 m5d.metal hosts, each running every
  kind (`huawei-r5-day30.trace`, the main model); or Hex's fleet's host types in their proportions (two
  Linux hosts in ten, one Windows, seven Macs), each running the kinds its OS runs
  (`huawei-r5-day30.fleet.trace`).
- **Launches.** The recorded workload's, with their needs (each with its kind's overhead) and kinds, in
  arrival order from a different place in the day for each run (the seed), repeated as needed.
- **Sets.** The mix as recorded, and each kind alone (`packing::alone`).
- **Arrivals and latency.** Arrivals are open-loop, one every 100 µs (10,000/s). A placement's latency
  runs from its scheduled arrival to its commit, so queueing counts.
- **Load.** The nodes that can run the set's kinds are first filled to 85% of their CPU, untimed. Each
  timed arrival then retires the oldest running launch first, so the load holds.
- **One thread.** The in-order committer is the serial point.

Decisions that find no node ("refused") are included. On the fleet's mix, 4,945 to 7,202 launches a run are
refused, over the fill and the timed placements. They are OCI containers and Firecracker microVMs that find
every Linux and Windows node full while the Macs have room. Each kind alone, and every set on m5d.metal,
refuses none.

## The run of record

- Host: fedora, `kiln-a094a12d.attlocal.net`, Intel Core i9-14900HX (24 cores, 32 threads), Linux
  6.19.10-300.fc44.
- Placement: CPUs 4-15 (`AllowedCPUs=4-15`, affinity checked: `4-15`), under
  `flock /home/adalundhe/.cache/hex-t7/fedora-timed.lock`.
- Commit: `5374202` (cp/t7-sched), release (`cargo bench` profile), from a `git archive` of the commit.
- Date: 2026-11-06, 10:06:19Z to 10:17:57Z. Units: hex-sched-fastpath-20261106T100603Z-build (build,
  CPUs 16-31) and hex-sched-fastpath-20261106T100603Z-time (timed).
- Load: noisy. Load average 10.40 before and 25.94 after. Eight docker containers were up (others' labs,
  benches and builders: hexbench-oci-2660782, goofy_keldysh, issue556oss-control-plane, three buildx
  builders, kind-registry and kiln-t10-minio). Units hex-g5tail-g16pfab-20261106T094401Z,
  hex-integrate-native-20261106T100543Z and hex-sched-g4-gates-20261106T095115Z ran beside it. Nothing
  was done to quiet the host.
- Runs: 5 × 100,000 timed placements per node model and set, best of 5 by p99.

| node model | set | seed | refused | p50 µs | p99 µs | p99.9 µs | max µs |
|---|---|---:|---:|---:|---:|---:|---:|
| m5d.metal | mix | 0 | 0 | 11.4 | 24.5 | 40.3 | 853.6 |
| m5d.metal | mix | 1 | 0 | 11.6 | 22.5 | 33.2 | 250.1 |
| m5d.metal | mix | 2 | 0 | 11.1 | 24.7 | 40.0 | 536.1 |
| m5d.metal | mix | 3 | 0 | 11.3 | **21.4** | 31.1 | 117.6 |
| m5d.metal | mix | 4 | 0 | 10.9 | 23.0 | 31.7 | 210.5 |
| m5d.metal | OCI | 0 | 0 | 10.1 | **23.4** | 34.7 | 199.7 |
| m5d.metal | OCI | 1 | 0 | 10.1 | 25.5 | 40.0 | 364.8 |
| m5d.metal | OCI | 2 | 0 | 9.9 | 28.2 | 46.6 | 521.9 |
| m5d.metal | OCI | 3 | 0 | 9.8 | 24.3 | 43.3 | 491.5 |
| m5d.metal | OCI | 4 | 0 | 9.5 | 23.5 | 36.7 | 235.1 |
| m5d.metal | Firecracker | 0 | 0 | 10.9 | **23.0** | 31.4 | 232.6 |
| m5d.metal | Firecracker | 1 | 0 | 10.6 | 23.5 | 33.9 | 479.5 |
| m5d.metal | Firecracker | 2 | 0 | 9.5 | 24.9 | 53.0 | 756.6 |
| m5d.metal | Firecracker | 3 | 0 | 12.7 | 34.2 | 106.7 | 1,993.9 |
| m5d.metal | Firecracker | 4 | 0 | 11.9 | 28.5 | 40.9 | 273.0 |
| m5d.metal | Shards | 0 | 0 | 10.6 | 30.0 | 42.1 | 810.9 |
| m5d.metal | Shards | 1 | 0 | 10.1 | 29.1 | 38.0 | 261.7 |
| m5d.metal | Shards | 2 | 0 | 10.7 | **29.0** | 44.9 | 1,128.1 |
| m5d.metal | Shards | 3 | 0 | 10.9 | 31.0 | 41.9 | 222.8 |
| m5d.metal | Shards | 4 | 0 | 11.0 | 29.8 | 39.4 | 211.1 |
| fleet | mix | 0 | 4,945 | 18.2 | 33.8 | 45.2 | 228.3 |
| fleet | mix | 1 | 6,506 | 17.3 | **30.0** | 44.1 | 5,765.8 |
| fleet | mix | 2 | 7,202 | 19.0 | 35.1 | 46.0 | 503.7 |
| fleet | mix | 3 | 6,994 | 17.9 | 32.3 | 49.3 | 468.2 |
| fleet | mix | 4 | 5,437 | 17.3 | 31.8 | 55.7 | 5,245.3 |
| fleet | OCI | 0 | 0 | 14.2 | 29.5 | 174.4 | 6,099.6 |
| fleet | OCI | 1 | 0 | 14.1 | **28.6** | 204.2 | 5,609.5 |
| fleet | OCI | 2 | 0 | 14.3 | 29.0 | 212.0 | 5,854.7 |
| fleet | OCI | 3 | 0 | 14.9 | 33.5 | 77.5 | 5,286.7 |
| fleet | OCI | 4 | 0 | 13.8 | 29.0 | 205.4 | 5,761.5 |
| fleet | Firecracker | 0 | 0 | 14.7 | **27.4** | 42.4 | 2,680.5 |
| fleet | Firecracker | 1 | 0 | 14.5 | 30.1 | 55.0 | 2,560.1 |
| fleet | Firecracker | 2 | 0 | 16.7 | 39.1 | 115.1 | 2,957.7 |
| fleet | Firecracker | 3 | 0 | 16.9 | 36.5 | 63.4 | 3,202.2 |
| fleet | Firecracker | 4 | 0 | 17.2 | 35.1 | 64.9 | 2,667.4 |
| fleet | Shards | 0 | 0 | 11.9 | 33.2 | 350.6 | 5,885.1 |
| fleet | Shards | 1 | 0 | 11.4 | 33.2 | 92.7 | 6,202.3 |
| fleet | Shards | 2 | 0 | 10.3 | 31.2 | 81.8 | 6,328.3 |
| fleet | Shards | 3 | 0 | 10.4 | **30.3** | 48.0 | 5,435.1 |
| fleet | Shards | 4 | 0 | 10.2 | 32.0 | 103.6 | 5,355.0 |

The maxima are single placements. The host ran at load 10 to 26 beside others' work. p99.9 stayed at or
under 107 µs on m5d.metal and 351 µs on the fleet.

## Earlier runs, not of record

- At `1d72fd5`, on the fleet alone, the same bench measured p99 27.9 µs (mix), 25.9 (OCI), 27.9
  (Firecracker) and 19.0 (Shards), best of 5, at load 7 to 11 (hex-sched-fastpath-20261106T094305Z-time;
  this file at `04b0ef1` holds its table).
- At `d457978`, before the kinds, it measured p99 30.6 µs on 10,000 EC2 m5d.metal nodes
  (hex-sched-benchtime-20261106T054155Z).
- Before the microVM workload, it drove generated nodes and needs under best fit: p99 78.0 µs at `e201e11`
  (2026-11-05) and 81.0 µs at `e6ebcc7` (2026-11-04), best of 5. This file at `e201e11` holds their tables.

None of these compares with the run above class for class: the nodes, the needs, the load or the scorer
differ.

## What it doesn't show

- No competitor ran on this hardware. The research's published tails are other systems' own
  (`docs/design/scheduler-architecture.md` §The bar: Hydra p99 ≤ 10 s, Shard Manager ~50 s), and
  kube-scheduler against kwok, class for class, is G6's `AC-T7.11.3`.
- One committer thread: deciders run in parallel only in hex-sim's virtual time. The real threaded
  shell, and the store's latency over a network (Amphora), are not in this number.
