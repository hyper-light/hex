# Cell selection on 100 simulated cells, against Karmada's (G7, H18)

**Bar:** the federation's selection of a cell for a workload, on 100 simulated cells, at p99 at most a
fifth of Karmada's `cluster_schedule` P99 of 32 ms on 100 simulated member clusters, so 6.4 ms
(`docs/design/hex-sim.md` H18; Karmada's 2022 report, `cluster-managers-at-scale.md:239-245`, [41],
MV). **Result: MET.** Best p99 of 5 is 3.77 µs, about 8,500x below Karmada's 32 ms. Every run selected
all 2,000,000, and every cell got exactly 20,000.

| run | p50 µs | p90 µs | p99 µs | p99.9 µs | max µs | per cell |
|---|---|---|---|---|---|---|
| 0 | 1.52 | 2.74 | 3.88 | 7.12 | 796.75 | 20,000 |
| 1 | 1.51 | 2.70 | **3.77** | 7.26 | 749.88 | 20,000 |
| 2 | 1.57 | 2.85 | 3.91 | 7.35 | 2,024.01 | 20,000 |
| 3 | 1.53 | 2.78 | 3.90 | 7.29 | 778.57 | 20,000 |
| 4 | 1.54 | 2.77 | 3.88 | 7.58 | 1,633.62 | 20,000 |

Karmada, as reported: P50 12 ms, P90 15 ms, P99 32 ms.

## What was measured

`crates/hex-bench/benches/sched_cells.rs` (its header says how).
- **The topology** is Karmada's report's: 100 cells (10 regions of 10) of 5,000 nodes each, 500,000
  nodes, and 2,000,000 workloads, 20,000 a cell. The node and the pod are H13's: 11 CPU and 55 GiB,
  and `scheduler_perf`'s pause pod, 100m and 500Mi.
- **Timed, each workload alone:** `federation::Capacity::admit`, the federation's own selection (H14,
  H16). It routes the workload to its home region and cell, and on to the first with room. It reads that
  cell's live free nodes by the cells' sampled best fit (kube-scheduler's floors of 100 nodes or 5%), and
  deducts the node it takes. Wall-clock, `Instant` around each call, release build.
- The run of record is the one with the best p99. Every run is above.

## The run of record

- Host: fedora, `kiln-a094a12d.attlocal.net`, Intel Core i9-14900HX (24 cores, 32 threads).
- Commit: `84485f5` (cp/t7-sched-g6), built on CPUs 16-31; timed with `taskset -c 4-15` under
  `flock /home/adalundhe/.cache/hex-t7/fedora-timed.lock` (taken 2026-11-05 10:42:47Z, released
  10:43:07Z).
- Unit: hex-sched-g6-cells-20261105T102857Z.
- Load: 41.43 / 39.53 / 40.25 before; 51.14 / 42.10 / 40.02 when the lock was taken; 49.53 at the end.
  24 containers running before, 23 after. Co-tenant hex units: hex-g5tail-abf,
  hex-sched-kiln-check, hex-shards-bench.
- The host ran far above CLAUDE.md §7's load average of 2 (noisy, as Ada directs for the swarm). The
  maxima, 0.75-2.0 ms, are that load's preemptions; p99.9 stays under 8 µs.

## An earlier run, and the defect it found

At 531f75a (unit hex-sched-g6-cells-20261105T092251Z, same host and method) the best p99 was 6.45 µs,
but `cells_min=0 cells_max=200000`. The home region was `id mod regions` and the home cell `id mod
cells`, the same residue on 10 regions of 10, so only the 10 diagonal cells got work. 84485f5 starts
the cell from `(id / regions) mod cells` (H14), held by
`every_cell_gets_work_when_regions_and_cells_share_a_factor` in `crates/hex-sim/tests/cells.rs`. That
run isn't of record.

## What the comparison is and isn't

Karmada's `cluster_schedule` is a step of its scheduler, measured inside a control plane that handles
API objects for 100 `kind` clusters of `fake-kubelet` nodes, and its authors say the test "is not
designed for stress testing Karmada". Hex's selection is in-process, on hex-sim's cells, with the
capacity check Karmada's estimator makes from a snapshot made exact here. The ratio covers both
differences, as `bench/sched/kube_scheduler_kwok.md`'s does. Karmada was not run on fedora: its
number is its own report's (MV).
