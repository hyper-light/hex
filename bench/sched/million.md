# 1,000,000 pending workloads placed on 10,000 nodes (AC-T7.11.2)

**Bar:** the time to place 1,000,000 pending workloads on 10,000 simulated nodes is at most 120 s,
measured on fedora (`AC-T7.11.2`, G6 step 4; the method is `docs/design/hex-sim.md` H13).
**Result: MET.** Every run of every row placed all 1,000,000, and the serial specification accepted
every slice's log. With one plane (`slices = 1`), the best of 5 is 8.13 s for pods and 7.55 s for the
mixed row. With eight, it is 0.418 s and 0.394 s. The slowest of all 20 runs took 8.85 s.

## What was measured

`crates/hex-bench/benches/sched_million.rs` (its header says how).
- **The cluster** is H13's: 10,000 nodes of 11 CPU and 55 GiB, with no co-tenant CPU.
  - Row `pods` places 1,000,000 of `scheduler_perf`'s pause pod (100m, 500Mi). They fill the
    1,100,000 slots to 91%.
  - Row `mixed` draws each need uniformly from 1-199m and 1-999 MiB, the same means as the pod. It
    draws kinds as hex-sim's generator does (40% plain, 25% builds, 15% benchmarks, 20% sessions), so
    H9's filters run.
- **Before the clock:** every workload is submitted to the one store, so all 1,000,000 are pending at
  once, and none ends during the run. Workloads are routed to slices by hex-sim's own `sim::Layout`
  (H10).
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
  against the bar (H13's falsifier).

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
  That is far inside H13's arithmetic of 120 µs a placement on one plane. Pending workloads are
  decided back to back, not one at a time against an 85% fill, so it runs well under the 50 µs p50 in
  `fast_path.md`.
- **Eight slices are about 19x faster than one, not 8x.** Each slice samples its own floor: 100 of its
  1,250 nodes against 500 of 10,000 for one plane (`Sample::want`). So each decision scans a fifth as
  many nodes, and the eight run in parallel. The price is the routing and the smaller sample that H10
  decides; its falsifier (work stranded on its slice above 1%) reads 0 here, since nothing was left
  unplaced.
- **The store is in process.** The commit is a compare-and-set behind a mutex, not Amphora over a
  network. When the store is Amphora's, the bench runs again, as H13 says of the kwok comparison.
- **No competitor ran in this record.** kube-scheduler on kwok, class for class, is `AC-T7.11.3` (step
  5, `bench/sched/kube_scheduler_kwok.md`).

Reproduce:

```
cargo bench -p hex-bench --bench sched_million -- 5 1 pods check
cargo bench -p hex-bench --bench sched_million -- 5 8 pods check
cargo bench -p hex-bench --bench sched_million -- 5 1 mixed check
cargo bench -p hex-bench --bench sched_million -- 5 8 mixed check
```
