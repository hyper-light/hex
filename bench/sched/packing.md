# Stranded capacity at 90% load on the recorded kiln workload (AC-T7.4.3)

**Bar:** stranded capacity at 90% load falls by at least 30% against best fit alone, measured by hex-sim with
a recorded kiln workload (`AC-T7.4.3`; G2 step 4). **Result: MET on every one of 16 seeds: Hex's scorer
strands at least 63.2% less than best fit by CPU and at least 51.4% less than best fit across resources
(Kubernetes' `MostAllocated`).**

## What was measured

`crates/hex-bench/benches/sched_packing.rs` (its header says how), and the same packing in the test
`the_recorded_kiln_workload_strands_at_least_30_percent_less_than_best_fit` (crates/hex-sim/tests/sim.rs),
which holds the bar on every seed. hex-sim's virtual packing, not wall-clock: the same trace gives the same
table on any machine.

- Workload: `bench/sched/kiln-ci-20261022-20261105.trace`, fourteen days of kiln's own CI, 4,655 legs on 10
  runners (`bench/sched/kiln_workload.md`), as the requests its legs would declare: measured use rounded up to
  whole cores and GiB, at least one of each (`packing::declared`; kiln declares whole cores and GiB, the
  smallest `{ cpu: "1", memory: 1Gi }`). The one Pareto-minimal request shape is 1 core and 1 GiB.
- Packing: arrivals in order, no completions, through the fast path's sampled choice (here the sample covers
  all 10 nodes), an arrival no node admits passed over, until the load reaches 90%. Load is the dominant share
  placed: the larger of placed CPU over all CPU and placed memory over all memory. This workload binds on
  memory.
- Stranded capacity: on each node where the 1-core, 1-GiB shape doesn't fit what is free, its free CPU and
  free memory, each as a share of the cluster's, summed, in per-mille (`PackReport::stranded_per_mille`).
- Seeds: 16 starting arrivals spread evenly over the recording (seed s starts at arrival s x 4,655 / 16), so
  the seeds pack different stretches of the fourteen days.
- Scorers: `BestFit` (least free CPU first), `MostAllocated` (most allocated across CPU and memory once
  placed, Kubernetes' bin packing), `Align` (Tetris's alignment) and `Fragment`, Hex's (least stranded leftover,
  then least allocated; hex-sim.md H11).

## The run of record

- Commit: `86eb3b4` (cp/t7-sched), release (`cargo bench` profile), from a `git archive` of the commit.
- Host: fedora, `kiln-a094a12d.attlocal.net`, CPUs 16-31 (an untimed unit: hex-sim's packing is virtual,
  so neither load nor the timed lock bears on it).
- Date: 2026-11-05, 21:59:56Z. Unit: hex-sched-g2s4-20261105T215956Z, which then ran all 7 gates at the same
  commit, exit 0.
- Determinism: the table below is byte for byte the one hex-sched-pack-20261105T215732Z printed from the
  working tree before the commit (the same md5 of its rows).

Each cell: stranded per-mille (arrivals passed over on the way, load per-mille at the first arrival refused
for room, `-` for none); `*` marks a run that never reached 90% load because every remaining arrival was
refused. The last two columns are Hex's reduction against `BestFit` and against `MostAllocated`.

| seed | from | best_fit | most_allocated | align | Hex (fragment) | cut vs best_fit | cut vs most_allocated |
|---:|---:|---|---|---|---|---:|---:|
| 0 | 0 | 134 (62, 892) | 382 (196, 875) | 218 (3, 889) | 13 (0, -) | 90.3% | 96.6% |
| 1 | 290 | 293* (4242, 835) | 260 (50, 897) | 85 (7, 827) | 0 (8, -) | 100.0% | 100.0% |
| 2 | 581 | 372 (42, 884) | 293 (15, -) | 238 (23, -) | 9 (7, -) | 97.6% | 96.9% |
| 3 | 872 | 365 (63, -) | 231 (0, -) | 190 (0, -) | 4 (9, -) | 98.9% | 98.3% |
| 4 | 1163 | 285 (22, -) | 221 (7, -) | 210 (33, 890) | 83 (15, 759) | 70.9% | 62.4% |
| 5 | 1454 | 177 (2, -) | 240 (1, 890) | 193 (27, -) | 0 (3, 710) | 100.0% | 100.0% |
| 6 | 1745 | 217 (32, -) | 231 (37, -) | 130 (78, 855) | 27 (7, 875) | 87.6% | 88.3% |
| 7 | 2036 | 96 (0, -) | 245 (95, -) | 174 (6, 831) | 0 (2, -) | 100.0% | 100.0% |
| 8 | 2327 | 175 (25, 828) | 160 (0, -) | 31 (13, -) | 35 (14, 678) | 80.0% | 78.1% |
| 9 | 2618 | 152 (39, -) | 233 (1, -) | 249 (38, -) | 56 (5, -) | 63.2% | 76.0% |
| 10 | 2909 | 268* (1614, 823) | 149 (174, 865) | 98 (49, -) | 1 (18, 832) | 99.6% | 99.3% |
| 11 | 3200 | 455* (1373, 763) | 461* (1368, 795) | 117 (5, 559) | 0 (4, 500) | 100.0% | 100.0% |
| 12 | 3491 | 470 (93, 868) | 470 (93, 868) | 252 (166, 660) | 79 (30, 447) | 83.2% | 83.2% |
| 13 | 3782 | 340* (781, 884) | 185 (14, -) | 431* (779, 772) | 90 (3, 138) | 73.5% | 51.4% |
| 14 | 4073 | 85 (0, -) | 377 (11, -) | 176 (9, -) | 0 (5, 893) | 100.0% | 100.0% |
| 15 | 4364 | 350* (207, 533) | 363* (205, 533) | 128 (19, 401) | 22 (22, 371) | 93.7% | 93.9% |

Least cut over the 16 seeds: 63.2% against `BestFit`, 51.4% against `MostAllocated`. Arrivals passed over on
the way, all seeds: `BestFit` 8,597, `MostAllocated` 2,267, `Align` 1,255, Hex 152.

## What it doesn't show

- The cost of spreading: on 9 of the 16 seeds the first arrival refused for room came at a lower load under
  Hex's scorer than under `BestFit` (as early as 138 per-mille on seed 13), because spreading uses the empty
  nodes best fit keeps for large legs, though Hex refused far fewer arrivals in all.
- Time: there are no completions, so this is packing at one load, not a schedule over the fourteen days.
- OS: hex-sim's nodes carry none, so a leg may land on a runner of another OS than the one it ran on.
- The needs are kiln's measurements rounded to declarations, not what the legs declared (kiln's older path
  kept no declaration); `bench/sched/kiln_workload.md` says what that would get wrong.
