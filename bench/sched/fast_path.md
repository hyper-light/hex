# The scheduler fast path's decision latency (AC-T7.4.2)

**Bar:** placement decision p99 at 10,000 placements/s on 10,000 simulated nodes is at most 10 ms
(`AC-T7.4.2`; G2 step 5). **Result: MET on the recorded microVM workload (`huawei_workload.md`), for the mix
of kinds and for each kind alone, at `1d72fd5`. Best of 5 by p99: 27.9 µs on the mix, 25.9 µs for OCI
containers alone, 27.9 µs for Firecracker microVMs alone and 19.0 µs for Shards microVMs alone. Every run's
p99 is at most 37.9 µs.**

## What was measured

`crates/hex-bench/benches/sched_fast_path.rs` (its header says how). It runs hex-sim's own fast path
on a release build, wall-clock:
- Hex's sampled choice: `FitIndex::sampled_fit` with `Scorer::HEX`, kube-scheduler's floors of 100 nodes
  or 5%, and the hard filters of `docs/design/hex-sim.md` H9, the kind filter among them;
- the compare-and-set commit to the store;
- the index update.

- **Nodes.** 10,000 of them, repeating the recorded workload's 50: Hex's fleet's host types in their
  proportions (two Linux hosts in ten, one Windows, seven Macs), each running the kinds its OS runs.
- **Launches.** The recorded workload's, with their needs (each with its kind's overhead) and kinds, in
  arrival order from a different place in the day for each run (the seed), repeated as needed.
- **Sets.** The mix as recorded, and each kind alone (`packing::alone`).
- **Arrivals and latency.** Arrivals are open-loop, one every 100 µs (10,000/s). A placement's latency
  runs from its scheduled arrival to its commit, so queueing counts.
- **Load.** The nodes that can run the set's kinds are first filled to 85% of their CPU, untimed. Each
  timed arrival then retires the oldest running launch first, so the load holds.
- **One thread.** The in-order committer is the serial point.

Decisions that find no node ("refused") are included. On the mix, 4,945 to 7,202 launches a run are
refused, over the fill and the timed placements. They are OCI containers and Firecracker microVMs that find
every Linux and Windows node full while the Macs have room. Each kind alone refuses none.

## The run of record

- Host: fedora, `kiln-a094a12d.attlocal.net`, Intel Core i9-14900HX (24 cores, 32 threads), Linux
  6.19.10-300.fc44.
- Placement: CPUs 4-15 (`AllowedCPUs=4-15`, affinity checked: `4-15`), under
  `flock /home/adalundhe/.cache/hex-t7/fedora-timed.lock`.
- Commit: `1d72fd5` (cp/t7-sched), release (`cargo bench` profile), from a `git archive` of the commit.
- Date: 2026-11-06, 09:43:20Z to 09:47:20Z. Units: hex-sched-fastpath-20261106T094305Z-build (build,
  CPUs 16-31) and hex-sched-fastpath-20261106T094305Z-time (timed).
- Load: noisy. Load average 7.21 before and 11.22 after. Eight docker containers were up (others' labs
  and builders: issue556oss-control-plane, three buildx builders, kind-registry, kiln-t10-minio and two
  others). Units hex-integrate-native-20261106T093947Z and hex-sched-g4-gates-20261106T090907Z ran beside
  it. Nothing was done to quiet the host.
- Runs: 5 × 100,000 timed placements per set, best of 5 by p99.

| set | seed | refused | p50 µs | p99 µs | p99.9 µs | max µs |
|---|---:|---:|---:|---:|---:|---:|
| mix | 0 | 4,945 | 15.2 | 32.1 | 44.9 | 367.0 |
| mix | 1 | 6,506 | 14.7 | 37.7 | 211.7 | 5,293.0 |
| mix | 2 | 7,202 | 16.8 | 37.9 | 56.3 | 493.9 |
| mix | 3 | 6,994 | 17.6 | 32.8 | 46.1 | 420.9 |
| mix | 4 | 5,437 | 14.7 | **27.9** | 36.4 | 5,256.2 |
| OCI | 0 | 0 | 12.7 | **25.9** | 113.2 | 5,398.2 |
| OCI | 1 | 0 | 13.2 | 29.9 | 307.5 | 4,774.8 |
| OCI | 2 | 0 | 14.3 | 32.6 | 159.9 | 5,368.7 |
| OCI | 3 | 0 | 14.0 | 32.5 | 298.1 | 5,424.5 |
| OCI | 4 | 0 | 14.2 | 32.7 | 409.4 | 5,467.8 |
| Firecracker | 0 | 0 | 15.8 | 29.1 | 186.2 | 2,576.5 |
| Firecracker | 1 | 0 | 15.4 | 28.6 | 159.2 | 2,941.3 |
| Firecracker | 2 | 0 | 15.6 | 29.5 | 44.3 | 2,884.1 |
| Firecracker | 3 | 0 | 15.9 | **27.9** | 41.9 | 2,790.2 |
| Firecracker | 4 | 0 | 16.0 | 29.4 | 43.8 | 2,876.0 |
| Shards | 0 | 0 | 9.7 | 25.8 | 108.8 | 6,050.2 |
| Shards | 1 | 0 | 9.2 | 23.1 | 57.3 | 5,443.4 |
| Shards | 2 | 0 | 8.3 | 21.7 | 41.8 | 5,457.1 |
| Shards | 3 | 0 | 7.9 | 19.1 | 31.1 | 4,896.5 |
| Shards | 4 | 0 | 8.6 | **19.0** | 30.1 | 5,163.8 |

The maxima are single placements. The host ran at load 7 to 11 beside others' work; p99.9 stayed at or
under 409 µs.

## Earlier runs, not of record

Before the microVM workload, the bench drove generated nodes and needs under best fit. It measured p99 78.0
µs at `e201e11` (2026-11-05) and 81.0 µs at `e6ebcc7` (2026-11-04), best of 5; this file at `e201e11` holds
their tables. At `d457978`, before the kinds, it measured p99 30.6 µs on 10,000 EC2 m5d.metal nodes
(hex-sched-benchtime-20261106T054155Z). None of these compares with the run above: the nodes, the needs and
the scorer all differ.

## What it doesn't show

- No competitor ran on this hardware. The research's published tails are other systems' own
  (`docs/design/scheduler-architecture.md` §The bar: Hydra p99 ≤ 10 s, Shard Manager ~50 s), and
  kube-scheduler against kwok, class for class, is G6's `AC-T7.11.3`.
- One committer thread: deciders run in parallel only in hex-sim's virtual time. The real threaded
  shell, and the store's latency over a network (Amphora), are not in this number.
