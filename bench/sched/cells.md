# Cell selection on 100 simulated cells, against Karmada's (G7, H68)

**Bar:** the federation's selection of a cell for a workload, on 100 simulated cells, at p99 at most a
fifth of Karmada's `cluster_schedule` P99 of 32 ms on 100 simulated member clusters, so 6.4 ms
(`docs/design/hex-sim.md` H68, classes H69; Karmada's 2022 report, `cluster-managers-at-scale.md:239-245`,
[41], MV). **Result: MET, over Hex's three classes.** Best p99 of 5 is 5.04 µs, about 6,350x below
Karmada's 32 ms; per class 4.97 µs (OCI), 5.14 µs (Firecracker), 5.03 µs (Shards). Every run selected all
2,000,000, each into a cell of a region that runs its class.

## Of record: Hex's three classes (2026-11-06)

What changed from the run below (H69):
- **Regions** each one platform, in the generator's proportion over the ten: regions 0-5 Linux (all
  three classes), 6-7 Windows (OCI and Shards), 8-9 macOS (Shards). Every node, cell and region
  declares the classes it runs, and `Capacity::admit` passes over a cell that doesn't run the
  workload's class; the bench asserts each selection's region runs its class.
- **Workloads** one class each by G2's rule (owners at 75 launches each, a third each, each class's
  overhead in its memory; Shards' 5 MiB a stand-in): 666,667 OCI, 666,666 Firecracker, 666,667 Shards.
- **Selection** by Hex's packing score (`Scorer::HEX`, H11) against each class's one need, not best fit.
- So cells fill unevenly by design: Firecracker can use 60 cells, OCI 80, Shards all 100. The least-used
  cell got 6,577 workloads, the most-used 60,250, the same in every run.

The run of record: fedora, commit `e311314`, release, built on CPUs 16-31, timed with `taskset -c 4-15`
under the timed lock (taken 2026-11-06 14:18:58Z, released 14:19:29Z), unit
hex-sched-g6-classbench-20261106T141227Z. Load 7.67 when the lock was taken, 6.47 after; 7 containers;
co-tenant hex units: hex-native-s7-gates, hex-sched-g4-gates, hex-sched-g4-gatesm.

| run | p50 µs | p90 µs | p99 µs | p99.9 µs | max µs |
|---|---|---|---|---|---|
| 0 | 2.79 | 3.61 | 5.27 | 8.14 | 33.17 |
| 1 | 2.77 | 3.58 | 5.23 | 8.10 | 37.42 |
| 2 | 2.78 | 4.19 | 7.64 | 10.71 | 82.99 |
| 3 | 2.74 | 3.52 | 5.24 | 7.85 | 46.53 |
| 4 | 2.73 | 3.50 | **5.04** | 7.70 | 38.95 |

Run 4 by class:

| class | selections | p50 µs | p90 µs | p99 µs | p99.9 µs | max µs |
|---|---|---|---|---|---|---|
| OCI | 666,667 | 2.71 | 3.29 | 4.97 | 7.55 | 38.95 |
| Firecracker | 666,666 | 2.82 | 3.92 | 5.14 | 7.74 | 28.75 |
| Shards | 666,667 | 2.70 | 2.96 | 5.03 | 7.82 | 37.64 |

**What moved, against the run below:** best p99 3.77 µs to 5.04 µs, and p50 1.51 µs to 2.73 µs: a
class check per cell visited, and Hex's score ranking each sample against the class shapes. The maxima
fell, 0.75-2.0 ms to 25-83 µs, on a quieter host (load 7 against 40-51). Firecracker's p90 runs highest,
since it has the fewest cells.

## Before classes (2026-11-05, superseded by the above)

**Bar (then):** the federation's selection of a cell for a workload, on 100 simulated cells, at p99 at most a
fifth of Karmada's `cluster_schedule` P99 of 32 ms on 100 simulated member clusters, so 6.4 ms
(`docs/design/hex-sim.md` H68; Karmada's 2022 report, `cluster-managers-at-scale.md:239-245`, [41],
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
  nodes, and 2,000,000 workloads, 20,000 a cell. The node and the pod are H63's: 11 CPU and 55 GiB,
  and `scheduler_perf`'s pause pod, 100m and 500Mi.
- **Timed, each workload alone:** `federation::Capacity::admit`, the federation's own selection (H64,
  H66). It routes the workload to its home region and cell, and on to the first with room. It reads that
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
the cell from `(id / regions) mod cells` (H64), held by
`every_cell_gets_work_when_regions_and_cells_share_a_factor` in `crates/hex-sim/tests/cells.rs`. That
run isn't of record.

## What the comparison is and isn't

Karmada's `cluster_schedule` is a step of its scheduler, measured inside a control plane that handles
API objects for 100 `kind` clusters of `fake-kubelet` nodes, and its authors say the test "is not
designed for stress testing Karmada". Hex's selection is in-process, on hex-sim's cells, with the
capacity check Karmada's estimator makes from a snapshot made exact here. The ratio covers both
differences, as `bench/sched/kube_scheduler_kwok.md`'s does. Karmada was not run on fedora: its
number is its own report's (MV).
