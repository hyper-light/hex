# The scheduler fast path's decision latency (AC-T7.4.2)

**Bar:** placement decision p99 at 10,000 placements/s on 10,000 simulated nodes is at most 10 ms
(`AC-T7.4.2`; G2 step 5). **Result: MET, p99 78.0 µs (best of 5) at `e201e11`, every run's p99 between 78
and 94 µs; 81.0 µs at `e6ebcc7` before it.**

## What was measured

`crates/hex-bench/benches/sched_fast_path.rs` (its header says how). It runs hex-sim's own fast path
on a release build, wall-clock:
- the sampled best fit (`FitIndex::sampled_fit`, kube-scheduler's floors of 100 nodes or 5%, with the
  hard filters of `docs/design/hex-sim.md` H9);
- the compare-and-set commit to the store;
- the index update.

Arrivals are open-loop, one every 100 µs (10,000/s), on 10,000 generated nodes (seed per run). A
placement's latency runs from its scheduled arrival to its commit, so queueing counts. The cluster is
first filled to 85% of its CPU, untimed. Each timed arrival then retires the oldest running workload
first, so the load holds. Single-threaded: the in-order committer is the serial point.

Decisions that find no node with room ("refused") are included. They are the slowest kind, a scan of
every node: at 85% placed CPU, some generated needs fit nowhere, 5-39% of timed decisions per run.

## The run of record

Re-measured at the tip after G2 step 4 changed `FitIndex::sampled_fit` (a scorer arm added; the default
`BestFit` path the bench drives is unchanged, and each seed refused as many arrivals as it did before).

- Host: fedora, `kiln-a094a12d.attlocal.net`, Intel Core i9-14900HX (24 cores, 32 threads), Linux
  6.19.10-300.fc44.
- Placement: CPUs 4-15 (`AllowedCPUs=4-15`, affinity checked: `4-15`), under
  `flock /home/adalundhe/.cache/hex-t7/fedora-timed.lock`.
- Commit: `e201e11` (cp/t7-sched), release (`cargo bench` profile).
- Date: 2026-11-05, 22:09:26Z to 22:10:30Z. Units: hex-sched-benchbuild-20261105T220338Z (build, CPUs 16-31)
  and hex-sched-benchtime-20261105T220338Z (timed).
- Load: noisy. Load average 21.62 before and 17.09 after. Six docker containers were up (others' labs and
  builders: issue556oss-control-plane, three buildx builders, kind-registry, kiln-t10-minio), and unit
  hex-sched-gates-20261105T220957Z ran the gates on CPUs 16-31 from 22:09:57Z. Nothing was done to quiet the
  host.
- Runs: 5 x 100,000 timed placements, best of 5 by p99.

| seed | refused | p50 µs | p99 µs | p99.9 µs | max µs |
|---:|---:|---:|---:|---:|---:|
| 0 | 6,971 | 49.1 | 83.8 | 121.1 | 427.8 |
| 1 | 15,198 | 49.9 | 87.4 | 705.1 | 3,632.6 |
| 2 | 38,593 | 45.1 | **78.0** | 123.4 | 3,114.8 |
| 3 | 4,905 | 48.5 | 80.7 | 126.0 | 2,950.6 |
| 4 | 12,643 | 45.8 | 93.8 | 1,228.2 | 3,495.4 |

## The run before it

- Host: fedora, `kiln-a094a12d.attlocal.net`, Intel Core i9-14900HX (24 cores, 32 threads), Linux
  6.19.10-300.fc44.
- Placement: CPUs 4-15 (`AllowedCPUs=4-15`, affinity checked: `4-15`), under
  `flock /home/adalundhe/.cache/hex-t7/fedora-timed.lock`.
- Commit: `e6ebcc7` (cp/t7-sched), release (`cargo bench` profile).
- Date: 2026-11-04, 09:49:48Z to 09:50:52Z. Units: hex-sched-benchbuild-20261104T094930Z (build, CPUs
  16-31) and hex-sched-benchtime-20261104T094930Z (timed).
- Load: noisy, as the bench rules ask. Load average 5.78 before and 13.22 after. About 100 docker
  containers were up (others' labs), and unit hex-integrate-final-20261104T094528Z was active beside
  it. Nothing was done to quiet the host.
- Runs: 5 × 100,000 timed placements, best of 5 by p99.

| seed | refused | p50 µs | p99 µs | p99.9 µs | max µs |
|---:|---:|---:|---:|---:|---:|
| 0 | 6,971 | 50.8 | 81.2 | 119.3 | 164.7 |
| 1 | 15,198 | 54.2 | 92.1 | 799.6 | 3,908.1 |
| 2 | 38,593 | 42.2 | 85.9 | 549.7 | 3,357.5 |
| 3 | 4,905 | 50.9 | 84.9 | 123.8 | 2,969.7 |
| 4 | 12,643 | 39.6 | **81.0** | 119.5 | 2,828.6 |

## What it doesn't show

- No competitor ran on this hardware. The research's published tails are other systems' own
  (`docs/design/scheduler-architecture.md` §The bar: Hydra p99 ≤ 10 s, Shard Manager ~50 s), and
  kube-scheduler against kwok, class for class, is G6's `AC-T7.11.3`.
- One committer thread: deciders run in parallel only in hex-sim's virtual time. The real threaded
  shell, and the store's latency over a network (Amphora), are not in this number.
