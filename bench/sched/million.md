# 1,000,000 pending workloads placed on 10,000 nodes (AC-T7.11.2)

**Bar:** the time to place 1,000,000 pending workloads on 10,000 simulated nodes is at most 120 s,
measured on fedora (`AC-T7.11.2`, G6 step 4; method H63, classes H69). **Result: MET, over Hex's three
classes.** Every run of every row placed all 1,000,000 (a third each OCI containers, Firecracker and
Shards microVMs, each only on a node that runs it), and the serial specification accepted every
slice's log. Best of 5: pods 17.76 s on one plane, 0.616 s on eight; mixed 12.91 s and 0.612 s. The
slowest of all 20 runs took 19.21 s.

## Of record: Hex's three classes (2026-11-06)

What changed from the run below (H69, the lead's check of 2026-11-06 under Ada's correction):
- **Workloads** each one class by G2's rule: owners at 75 launches each (`generate::owners`), each
  owner's workloads one class, a third of the launches each (`generate::by_owner`), and each class's
  overhead added to its memory: OCI 0.42 MiB, Firecracker 5 MiB, Shards 5 MiB, **a stand-in** until
  Shards states its own. Placed by class every run: OCI 333,303-333,349, Firecracker 333,304-333,350,
  Shards 333,304-333,348, varying by seed with the owner draw.
- **Nodes** declare their classes, by id (`generate::platform`): six in ten Linux (all three), two
  Windows (OCI and Shards), two macOS (Shards). So Firecracker may use 6,000 nodes, OCI 8,000, Shards
  all 10,000.
- **Placement** by Hex's packing score (`Scorer::HEX`, H11: least stranded against each class's
  smallest needs, then the node running the fewest classes), as `sched_fast_path` measures it. The first
  try kept plain best fit and failed: the mixed row on one plane left 4,678 Firecracker microVMs
  unplaced (seed 0, unit hex-sched-g6-classbench-20261106T132322Z), because OCI and Shards work filled
  the Linux nodes, the only ones Firecracker can use. That run isn't of record; its pods rows (13.47 s
  and 0.586 s best) met the bar under best fit.

The run of record: fedora (`kiln-a094a12d.attlocal.net`, i9-14900HX), commit `e311314`
(cp/t7-sched-g6), release, built on CPUs 16-31, each row timed with `taskset -c 4-15` under
`flock /home/adalundhe/.cache/hex-t7/fedora-timed.lock`, unit hex-sched-g6-classbench-20261106T141227Z,
2026-11-06 14:13-14:19Z. Load 13.82 when the first row took the lock, 7.67 after the last; 7-8
containers; co-tenant hex units: hex-ns5route-g21m, hex-sched-g4-gates, hex-sched-g4-gatesm,
hex-sched-g5-main-gates, hex-native-s7-gates.

| row | slices | wall s, seeds 0-4 | best | spec |
|---|---|---|---|---|
| pods | 1 | 19.212, 18.696, 18.015, 17.981, 17.763 | **17.763** | accepted, 5 of 5 |
| pods | 8 | 0.655, 0.616, 0.665, 0.633, 0.679 | **0.616** | accepted, 5 of 5 |
| mixed | 1 | 15.242, 14.991, 12.905, 13.450, 13.428 | **12.905** | accepted, 5 of 5 |
| mixed | 8 | 0.614, 0.627, 0.612, 0.623, 0.648 | **0.612** | accepted, 5 of 5 |

**What moved, against the run below:** pods on one plane 8.13 s to 17.76 s (2.2x), on eight 0.418 s to
0.616 s; mixed 7.55 s to 12.91 s, and 0.394 s to 0.612 s. Hex's score ranks a sample by stranding
against each class's shapes, more work a decision than best fit's single key, and a class narrows the
nodes a sample may take. Every row still meets the bar with at least 6.7x to spare.

## Before classes (2026-11-05, superseded by the above)

**Bar (then):** the time to place 1,000,000 pending workloads on 10,000 simulated nodes is at most 120 s,
measured on fedora (`AC-T7.11.2`, G6 step 4; the method is `docs/design/hex-sim.md` H63).
**Result: MET.** Every run of every row placed all 1,000,000, and the serial specification accepted
every slice's log. With one plane (`slices = 1`), the best of 5 is 8.13 s for pods and 7.55 s for the
mixed row. With eight, it is 0.418 s and 0.394 s. The slowest of all 20 runs took 8.85 s.

## What was measured

`crates/hex-bench/benches/sched_million.rs` (its header says how).
- **The cluster** is H63's: 10,000 nodes of 11 CPU and 55 GiB, with no co-tenant CPU.
  - Row `pods` places 1,000,000 of `scheduler_perf`'s pause pod (100m, 500Mi). They fill the
    1,100,000 slots to 91%.
  - Row `mixed` draws each need uniformly from 1-199m and 1-999 MiB, the same means as the pod. It
    draws kinds as hex-sim's generator does (40% plain, 25% builds, 15% benchmarks, 20% sessions), so
    H9's filters run.
- **Before the clock:** every workload is submitted to the one store, so all 1,000,000 are pending at
  once, and none ends during the run. Workloads are routed to slices by hex-sim's own `sim::Layout`
  (H60).
- **Timed:** one thread per slice runs hex-sim's fast path over that slice's pending workloads, in
  submit order:
  - the sampled best fit, `FitIndex::sampled_fit`, which stops at kube-scheduler's floors of 100 nodes
    or 5% of the slice;
  - the commit by compare-and-set to the one store, `Store::apply`, on the version its submit returned;
  - the index update.

  The time runs from the first slice's first decision to the last slice's last commit, wall-clock, on
  a release build.
- **Untimed, after each run (`check`):** each slice's part of the store's log is replayed, in the
  store's order, into a store of that slice's nodes alone. `spec::check_log` then judges it (H4 per
  slice): each placement must be the slice's sampled best fit at its point, no queue may be jumped, and
  nothing may be left pending that a node holds. A placement across a slice's edge is refused.
  `spec_placements` counts the placements the specification accepted.

The threads, the mutex around the store and the per-slice replay are the bench's own shell. They wrap
hex-sim's `Layout`, `FitIndex`, `Store` and `spec::check_log`. This is not `sim::run`'s event loop,
which runs in virtual time and is what `tests/shards.rs` checks. The bench doesn't call
`spec::check_slices`, because that function takes its shard count from `Run.planes`, and a
`ControlPlane` can't be built outside hex-sim. Its replay gives the same per-slice judgement through
`check_log`.

## The run of record

- **Host:** fedora, `kiln-a094a12d.attlocal.net`, Intel Core i9-14900HX (32 threads), Linux
  6.19.10-300.fc44.
- **Placement:** CPUs 4-15 (`AllowedCPUs=4-15`, affinity checked: `4-15`; 12 CPUs, so the 8 slices'
  threads each had a CPU), under `flock /home/adalundhe/.cache/hex-t7/fedora-timed.lock`, in unit
  `hex-sched-s4-time-20261105T042221Z` (MemoryHigh 6G, MemoryMax 8G, started through `hex-slot`).
- **Commit:** `2651541` (cp/t7-sched-g6), release (`cargo bench` profile). Clippy `-D warnings` on
  the bench exited 0 in the same unit.
- **Date:** 2026-11-05. The unit started at 04:23:10Z, the timed rows ran from 04:28:57Z to 04:32:05Z.
- **Load:** noisy, as the bench rules ask. Load average was 42.75 before the unit and 27.79 at the
  first row (6.45 at the last), with 29 docker containers up: 23 kiln CI burst-pool jobs, two buildx
  builders, a kind control plane and registry, and minio. Three other hex units were active beside it:
  `hex-g6s3-memory`, `hex-g6s5-series` and `hex-shards-gates`. Nothing was done to quiet the host.
- **Runs:** 5 seeds per row, best of 5 by wall time. Every run is listed, so the spread can be judged
  against the bar (H63's falsifier).

| row | slices | seed 0 s | seed 1 s | seed 2 s | seed 3 s | seed 4 s | best s | best placements/s | spec |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---|
| pods | 1 | 8.131 | 8.600 | 8.820 | 8.852 | 8.640 | **8.131** | 122,985 | all accepted |
| pods | 8 | 0.482 | 0.461 | 0.448 | 0.445 | 0.418 | **0.418** | 2,393,253 | all accepted |
| mixed | 1 | 8.015 | 7.631 | 8.012 | 8.077 | 7.546 | **7.546** | 132,512 | all accepted |
| mixed | 8 | 0.400 | 0.400 | 0.394 | 0.429 | 0.394 | **0.394** | 2,540,965 | all accepted |

In every run: placed = 1,000,000, unplaced = 0, records placed in the store = 1,000,000, and
spec_placements = 1,000,000. The `check` replay took 8.3-9.8 s untimed at one slice and 0.4-0.6 s at
eight. Setup (generation, routing, submits) took 0.2 s, untimed.

## What the numbers say, and what they don't

- **One plane meets the bar by about 14x.** At 8.85 s worst, the pods row runs at 8.1-8.9 µs a placement.
  That is far inside H63's arithmetic of 120 µs a placement on one plane. Pending workloads are
  decided back to back, not one at a time against an 85% fill, so it runs well under the 50 µs p50 in
  `fast_path.md`.
- **Eight slices are about 19x faster than one, not 8x.** Each slice samples its own floor: 100 of its
  1,250 nodes against 500 of 10,000 for one plane (`Sample::want`). So each decision scans a fifth as
  many nodes, and the eight run in parallel. The price is the routing and the smaller sample that H60
  decides; its falsifier (work stranded on its slice above 1%) reads 0 here, since nothing was left
  unplaced.
- **The store is in process.** The commit is a compare-and-set behind a mutex, not Amphora over a
  network. When the store is Amphora's, the bench runs again, as H63 says of the kwok comparison.
- **No competitor ran in this record.** kube-scheduler on kwok, class for class, is `AC-T7.11.3` (step
  5, `bench/sched/kube_scheduler_kwok.md`).

Reproduce:

```
cargo bench -p hex-bench --bench sched_million -- 5 1 pods check
cargo bench -p hex-bench --bench sched_million -- 5 8 pods check
cargo bench -p hex-bench --bench sched_million -- 5 1 mixed check
cargo bench -p hex-bench --bench sched_million -- 5 8 mixed check
```
