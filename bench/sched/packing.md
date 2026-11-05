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
  whole cores and GiB, at least one of each (`packing::whole_requests`; kiln declares whole cores and GiB, the
  smallest `{ cpu: "1", memory: 1Gi }`). The one Pareto-minimal request shape is 1 core and 1 GiB.
- Packing: arrivals in order, no completions, through the fast path's sampled choice (here the sample covers
  all 10 nodes), an arrival no node admits passed over, until the load reaches 90%. Load is the dominant share
  placed: the larger of placed CPU over all CPU and placed memory over all memory. This workload binds on
  memory.
- Stranded capacity: on each node where the 1-core, 1-GiB shape doesn't fit what is free, its free CPU and
  free memory, each as a share of the cluster's, summed, in per-mille (`PackReport::stranded_per_mille`).
  - The step defines it as "free capacity no pending workload of the trace's shapes can use". I read
    "pending" as every leg of the recording: its request shapes are the work that could come next, so a
    node's free capacity is usable if any of them fits it. The recording's requests have one Pareto-minimal
    shape, 1 core and 1 GiB, and every other request holds at least that much of both. Free capacity then
    fits some request exactly when it has a core and a GiB.
  - That makes the step's definition Borg's: resources "that cannot be used because another resource on the
    machine is fully allocated" ([1] §3.2), with "fully allocated" meaning less than the smallest request
    left. It matches only because the smallest shape is one shape, 1 core and 1 GiB. A workload with
    several incomparable minimal shapes (a 4-core, 1-GiB request beside a 1-core, 8-GiB one, say) could
    leave free capacity, 2 cores and 4 GiB, that Borg's reading counts as usable, since neither resource is
    spent, but that fits none of its requests.
- Seeds: 16 starting arrivals spread evenly over the recording (seed s starts at arrival s x 4,655 / 16), so
  the seeds pack different stretches of the fourteen days.
- Arms: best fit alone two ways, by free CPU first (`BestFit`) and across CPU and memory (`MostAllocated`,
  Kubernetes' bin packing); Tetris's alignment (`Align`); and least stranded leftover first, then one of three
  ties (`Fragment(Tie)`): Tetris's alignment (the plan's), least allocated (Hex's, `Scorer::HEX`), or most
  allocated, so that what isn't stranded packs. hex-sim.md H11 records the choice.

## The arms

| arm | seeds at least 30% below both best fits (the bar) | least cut vs `BestFit`, vs `MostAllocated` | arrivals passed over | earliest first refusal for room | passed over below half load |
|---|---:|---|---:|---:|---|
| `BestFit` | (baseline) | - | 8,597 | 533 | 0 |
| `MostAllocated` | (baseline) | - | 2,267 | 533 | 0 |
| `Align` (Tetris) | 7/16 | -107.1%, -133.0% | 1,255 | 401 | 12: 5 for room (32 cores, 7 GiB), 7 by the filter |
| least stranded, then alignment (the plan's) | 12/16 | -71.8%, -92.4% | 1,142 | 401 | 12: 5 for room (32 cores, 7 GiB), 7 by the filter |
| **least stranded, then least allocated (Hex's)** | **16/16** | **63.2%, 51.4%** | **152** | **138** | 37: 8 for room (32 cores, 7-16 GiB), 29 by the filter |
| least stranded, then most allocated | 8/16 | -252.9%, -75.1% | 2,914 | 533 | 0 |

Loads are in per-mille. "By the filter" means a benchmark refused because every node with room ran a build, or a
build because every such node ran a benchmark (hex-sim.md H9).

- **The plan's score is not Hex's.** The plan asked for a Tetris-style alignment with a fragmentation term:
  least stranded, then alignment. It strands more than a best fit on 4 seeds (9, 10, 13 and 14) and meets the
  bar on 12 of 16.
- **The most-allocated tie isn't either.** Least stranded, then most allocated, packs what isn't stranded and
  keeps best fit's empty nodes: nothing is refused below half load, and its earliest refusal for room comes at
  53.3% load, as best fit's does. But it meets the bar on only 8 of 16 seeds, so it stays a measured arm.
- **Hex's score holds the bar on every seed, at a cost.** Spreading spends the empty nodes best fit keeps, so
  arrivals are refused while the cluster is mostly idle. 37 below half load across the seeds, against none
  under either best fit:
  - 8 for room, all legs whose measured CPU was capped at a whole 32-thread machine: 32 cores and 16 GiB at
    13.8% load on seed 13, 32 cores and 16 or 9 GiB at 44.7% and 47.9% on seed 12, and five of 32 cores and
    7 GiB at 37.1% on seed 15;
  - 29 by the filter, of 1 to 6 cores and 1 to 49 GiB. The largest, 40-49 GiB, match kiln's `quic-transport`
    benchmark legs (peak RSS about 42 GiB), which found a build on every node with room.

  In all, Hex passes over 152 arrivals on the way to 90%, against best fit's 8,597: best fit's refusals come
  later and stall it.

## The run of record

- Commit: `ad3053c` (cp/t7-sched), release (`cargo bench` profile), from a `git archive` of the commit.
- Host: fedora, `kiln-a094a12d.attlocal.net`, CPUs 16-31 (an untimed unit: hex-sim's packing is virtual,
  so neither load nor the timed lock bears on it).
- Date: 2026-11-05, 22:53:53Z. Unit: hex-sched-g2s4b-20261105T225353Z, which then ran all 7 gates at the
  same commit, exit 0.
- Determinism: every arm's numbers are those the bench printed from the working tree before the commit
  (hex-sched-arms-20261105T225102Z), and Hex's per-seed rows below are those of the first run at `86eb3b4`
  (hex-sched-g2s4-20261105T215956Z): its score didn't change.

Each cell: stranded per-mille (arrivals passed over on the way, load per-mille at the first arrival refused
for room, `-` for none); `*` marks a run that never reached 90% load because every remaining arrival was
refused. The last two columns are Hex's reduction against `BestFit` and against `MostAllocated`.

Per seed, for the two best fits, Tetris's alignment and Hex's score:

| seed | from | best_fit | most_allocated | align | Hex | cut vs best_fit | cut vs most_allocated |
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

- The cost of spreading is above (the arms): on 9 of the 16 seeds the first arrival refused for room came at
  a lower load under Hex's scorer than under `BestFit`, as early as 13.8% on seed 13.
- Time: there are no completions, so this is packing at one load, not a schedule over the fourteen days. A
  leg refused here is passed over for good; in a schedule it waits, and the empty node it needs comes back as
  work completes. Holding room for a whole-machine leg, or preempting for it, is G4's (`CP-T7.5`).
- OS: hex-sim's nodes carry none, so a leg may land on a runner of another OS than the one it ran on.
- The needs are kiln's measurements rounded up as kiln declares, not what the legs declared (kiln's older
  path kept no declaration). Mean cores understate a bursty build's peak, which could flatter packing;
  `bench/sched/kiln_workload.md` says what else would be wrong.
