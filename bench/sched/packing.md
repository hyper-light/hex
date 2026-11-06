# Stranded capacity at 90% load on the recorded microVM workload (AC-T7.4.3)

**Bar:** stranded capacity at 90% load falls by at least 30% against best fit alone, measured by hex-sim on
a recorded workload (`AC-T7.4.3`; G2 step 4). The workload of record is a day of Huawei Cloud's serverless
sandbox launches, each one of Hex's three kinds (an OCI container, a Firecracker microVM or a Shards
microVM), on Hex's own fleet (`huawei_workload.md`). **Result: MET on every one of 16 seeds, for the mix of
kinds and for each kind alone, with no completions and with completions (lifetimes as recorded, and with
Huawei's one-minute keep-alive). Hex's scorer strands at least 52.9% less than best fit by CPU and 52.6% less
than best fit across resources (Kubernetes' `MostAllocated`), and outside the mix with no completions it
strands nothing.**

## What was measured

`crates/hex-bench/benches/sched_packing.rs` (its header says how). The same packing runs in the test
`the_recorded_workload_strands_at_least_30_percent_less_than_best_fit` (crates/hex-sim/tests/sim.rs), which
holds the bar on every seed of every set and clock. This is hex-sim's virtual packing, not wall-clock: the
same trace gives the same tables on any machine.

- **Workload.** `bench/sched/huawei-r5-day30.trace` and `huawei-r5-day30.keepalive.trace`: 25,295 launches
  of 339 owners on 50 nodes, Hex's fleet's hosts five times over. Each launch's kind is its owner's, a third
  of the launches each, and each kind's runtime overhead is in its memory (`huawei_workload.md`).
- **Sets.** The mix as recorded, and each kind alone (`packing::alone`): the whole day as OCI containers,
  then as Firecracker microVMs, then as Shards microVMs, each need moved to that kind's overhead. OCI
  containers may use 15 of the nodes (480 cores), Firecracker microVMs 10 (320), Shards microVMs all 50
  (1,105).
- **Packing.** Arrivals in order from the seed's start, wrapping round the day, through the fast path's
  sampled choice (kube-scheduler's floors, 100 nodes or 5%, so here all 50). An arrival no node admits is
  passed over. Packing stops when the load reaches 90%. Load is the dominant share placed over the nodes the
  set's kinds may use. This workload binds on CPU.
- **Stranded capacity.** On each node where no request shape of a kind the node runs fits what is free:
  its free CPU and memory, each as a share of the usable nodes', summed, in per-mille. The shapes are each
  kind's Pareto-minimal requests. Here that is one per kind, 300 millicores and 128 MiB plus the kind's
  overhead. So a node strands what it has left when that is under 300 millicores, or under the smallest
  memory, for every kind it runs. That is Borg's stranding, resources "that cannot be used because another
  resource on the machine is fully allocated" ([1] §3.2), with "fully allocated" meaning less than the
  smallest request of a kind the node runs.
- **Seeds.** 16 starting arrivals spread evenly over the day (seed s starts at arrival s × 25,295 / 16),
  each wrapping round it, so every seed has a whole day of arrivals.
- **Clocks.** Three:
  - no completions;
  - completions with lifetimes as recorded;
  - completions with the keep-alive.

  At the recorded rate, 0.29 launches a second, the day never loads the fleet to 90%. So arrivals come n
  times faster, and each launch keeps its recorded lifetime: the fleet serves n times this region's
  traffic. For each set and trace, n is the slowest speed on the ladder (10, 20, 30, 40, 50, 60, 75, 100,
  150, 200, 300, 500, 1,000, 2,000) at which Hex and both best fits reach 90% on all 16 seeds. That lets
  lifetimes enter as far as the fleet allows.
- **Arms.** Best fit alone, two ways: by free CPU first (`BestFit`) and across CPU and memory
  (`MostAllocated`, Kubernetes' bin packing). Tetris's alignment (`Align`). And least stranded leftover
  first, then one of four ties (`Fragment(Tie)`):
  - Tetris's alignment (the plan's);
  - least allocated;
  - most allocated;
  - the node running the fewest kinds, then most allocated (Hex's, `Scorer::HEX`).

  hex-sim.md H11 records the choice.

## The speeds

The bench walks the ladder for every set and both traces, and prints each speed's row.

| set | lifetimes as recorded | with the keep-alive |
|---|---|---|
| mix | 150x; fewest completions on the way, by seed, 4,963 (Hex), 5,112 and 5,101 (the best fits). At 100x, Hex reaches 90% on 3 seeds and the best fits on none | 50x; 312, 469 and 842. At 40x, the best fits reach 90% on 15 seeds; from 75x up, some seed has none end |
| OCI | 40x; 2,673 | 20x; 301. From 30x up, some seed has none end |
| Firecracker | 30x; 1,110 | 10x; 535. From 20x up, some seed has none end |
| Shards | 150x; 4,963 | 40x; 1,084. From 75x up, some seed has none end |

At 2,000x, the speed this measure first used, the keep-alive made completions vanish: a kept-alive launch
lives at least 60 s, and the fleet fills to 90% in seconds. The pack reports its completions
(`PackReport::completions`), and the test fails if any seed under a completions clock has none.

## Hex against the best fits

Stranded per-mille, the range over the 16 seeds, and the arrivals passed over on the way, summed over the
seeds. Every run of every arm reached 90%.

| set and clock | `BestFit` | `MostAllocated` | Hex | least cut vs `BestFit`, vs `MostAllocated` | passed over: `BestFit`, `MostAllocated`, Hex |
|---|---|---|---|---|---|
| mix, no completions | 651-665 | 637-661 | 0-312 | 52.9%, 52.6% | 6,679, 11,322, 2,556 |
| mix, 150x, as recorded | 441-563 | 280-478 | 0 | 100%, 100% | 1,669, 2,975, 0 |
| mix, 50x, with the keep-alive | 503-635 | 300-574 | 0 | 100%, 100% | 7,001, 8,100, 0 |
| OCI, no completions | 591-608 | 591-608 | 0 | 100%, 100% | 0, 0, 0 |
| OCI, 40x, as recorded | 236-472 | 162-454 | 0 | 100%, 100% | 0, 0, 0 |
| OCI, 20x, with the keep-alive | 144-587 | 113-589 | 0 | 100%, 100% | 0, 0, 0 |
| Firecracker, no completions | 474-602 | 474-602 | 0 | 100%, 100% | 0, 0, 0 |
| Firecracker, 30x, as recorded | 36-528 | 131-387 | 0 | 100%, 100% | 0, 0, 0 |
| Firecracker, 10x, with the keep-alive | 226-492 | 110-469 | 0 | 100%, 100% | 0, 0, 0 |
| Shards, no completions | 626-646 | 626-646 | 0 | 100%, 100% | 0, 0, 0 |
| Shards, 150x, as recorded | 333-566 | 241-511 | 0 | 100%, 100% | 0, 0, 0 |
| Shards, 40x, with the keep-alive | 484-617 | 395-589 | 0 | 100%, 100% | 0, 0, 0 |

Hex refuses nothing below half load in any of them, and no arrival for room: a node always had the room. On
the mix with no completions, Hex's refusals all come from the kind filter. They are OCI containers and
Firecracker microVMs that found every Linux and Windows node full while the Macs, which run only Shards
microVMs, had room. The two best fits refuse those, and more.

Per seed on the mix with no completions, the one set and clock where Hex strands anything. Each cell is
stranded per-mille, with arrivals passed over in brackets.

| seed | from | `BestFit` | `MostAllocated` | the plan's | Hex | cut vs `BestFit` | cut vs `MostAllocated` |
|---:|---:|---|---|---|---|---:|---:|
| 0 | 0 | 659 (245) | 637 (440) | 0 (0) | 0 (0) | 100.0% | 100.0% |
| 1 | 1,580 | 651 (193) | 649 (386) | 0 (1) | 0 (0) | 100.0% | 100.0% |
| 2 | 3,161 | 657 (208) | 651 (523) | 0 (0) | 0 (0) | 100.0% | 100.0% |
| 3 | 4,742 | 659 (289) | 654 (564) | 26 (1) | 0 (0) | 100.0% | 100.0% |
| 4 | 6,323 | 664 (278) | 659 (703) | 54 (4) | 0 (0) | 100.0% | 100.0% |
| 5 | 7,904 | 662 (224) | 656 (514) | 0 (0) | 0 (0) | 100.0% | 100.0% |
| 6 | 9,485 | 660 (225) | 653 (550) | 0 (0) | 0 (0) | 100.0% | 100.0% |
| 7 | 11,066 | 660 (345) | 655 (722) | 203 (14) | 310 (21) | 53.0% | 52.7% |
| 8 | 12,647 | 659 (394) | 657 (648) | 312 (91) | 309 (118) | 53.1% | 53.0% |
| 9 | 14,228 | 659 (471) | 654 (880) | 311 (144) | 310 (165) | 53.0% | 52.6% |
| 10 | 15,809 | 658 (505) | 657 (643) | 310 (175) | 310 (186) | 52.9% | 52.8% |
| 11 | 17,390 | 657 (553) | 655 (933) | 307 (277) | 308 (286) | 53.1% | 53.0% |
| 12 | 18,971 | 664 (629) | 657 (1,002) | 308 (447) | 307 (464) | 53.8% | 53.3% |
| 13 | 20,552 | 661 (754) | 659 (932) | 307 (444) | 306 (460) | 53.7% | 53.6% |
| 14 | 22,133 | 665 (754) | 661 (1,091) | 314 (428) | 312 (446) | 53.1% | 52.8% |
| 15 | 23,714 | 661 (612) | 656 (791) | 307 (371) | 306 (410) | 53.7% | 53.4% |

From seed 7 on, the day's later launches fill the CPU of every node that runs OCI containers and
Firecracker microVMs, and the memory left beside it is stranded under every arm. Hex strands about 310
per-mille there, the best fits about 660.

## The arms

Across the 12 sets and clocks:

| arm | sets and clocks at the bar on 16/16 | least cut, any set and clock | passed over: mix, no completions | passed over: the other 11 | passed over below half load | earliest refusal for room |
|---|---:|---:|---:|---:|---:|---:|
| `BestFit` | (baseline) | - | 6,679 | 8,670 | 419 | - |
| `MostAllocated` | (baseline) | - | 11,322 | 11,075 | 2,257 | - |
| `Align` (Tetris) | 12 | 45.7% | 2,406 | 141 | 0 | 87.4% |
| least stranded, then alignment (the plan's) | 12 | 31.7% | 2,397 | 143 | 0 | 87.9% |
| least stranded, then least allocated | 12 | 50.7% | 3,619 | 123 | 0 | 80.7% |
| least stranded, then most allocated | 12 | 51.9% | 11,195 | 7,429 | 1,925 | - |
| **least stranded, then fewest kinds, then most allocated (Hex's)** | **12** | **52.6%** | **2,556** | **0** | **0** | **-** |

"Earliest refusal for room" is the lowest load, over all sets, clocks and seeds, at which an arrival found no
node with room for it; `-` means none ever did.

- **Every fragment-aware arm meets the bar.** On this workload any arm that looks at stranding first beats
  both best fits by at least 30% on every seed. What sets them apart is what they refuse.
- **The plan's score is not Hex's.** Least stranded, then alignment, the plan's score, passes over a little
  less than Hex on the mix with no completions (2,397 against 2,556). But its cut falls to 31.7% against
  `MostAllocated` on Firecracker alone with lifetimes as recorded. It passes over 143 launches across the
  other sets and clocks, where Hex passes over none, and refuses for room from 87.9% load.
- **The most-allocated tie isn't either.** Least stranded, then most allocated, is Hex's tie without the
  kinds. It strands the least on the mix with no completions (a least cut of 54.1%). But it passes over
  11,195 launches there and 7,429 in the other sets, 1,925 of them below half load. It spreads the Shards
  microVMs over the Linux and Windows nodes, and the Firecracker microVMs that only Linux runs then find no
  room: every early refusal the bench lists for it is Firecracker-shaped, refused while a Mac had room.
- **Hex's score keeps the nodes only some kinds can use for those kinds.** Ranking the node that runs the
  fewest kinds first sends Shards microVMs to the Macs and leaves Linux and Windows for the kinds only they
  can run. Hex's score strands nothing in 11 of the 12 sets and clocks, passes over launches only on the mix
  with no completions, never below half load, and never for room.

## The run of record

- Commit: `1d72fd5` (cp/t7-sched), release (`cargo bench` profile), from a `git archive` of the commit.
- Host: fedora, `kiln-a094a12d.attlocal.net`, CPUs 16-31. The unit is untimed: hex-sim's packing is
  virtual, so neither load nor the timed lock bears on it.
- Date: 2026-11-06. Units: hex-sched-ladder-20261106T093934Z, on the working tree that became `1d72fd5`
  (fmt, clippy, the hex-sim tests in release and debug, and this bench, all exit 0); and
  hex-sched-packrec-20261106T094936Z, this bench at `1d72fd5` itself, which printed the same 1,462 lines of
  tables.

## Side measurement: kiln CI, not of record

Until Ada's correction of 2026-11-06, this measure ran on fourteen days of kiln's own CI
(`kiln_workload.md`): 4,655 legs on 10 runners, needs rounded up to whole cores and GiB, no completions. At
`ad3053c`, least stranded then least allocated met the bar on all 16 seeds (least cuts 63.2% and 51.4%),
and least stranded then most allocated on 8. Every node in that trace runs every kind, so today's Hex tie
cannot tell its nodes apart and is most allocated there: 8 of 16. That record is `packing.md` at
`ad3053c`. It isn't re-run: kiln's CI is the load balancer's workload, not Hex's.

## What it doesn't show

- The kinds are given, not recorded, and Shards' overhead is a stand-in (`huawei_workload.md`, what would
  make it wrong).
- Time: packing to one load, not a schedule over the day. A launch passed over here is refused for good;
  in a schedule it waits for room.
- Devices: this day asks for none. GPU placement by kind, and a benchmark never beside a build, are held
  on generated clusters (hex-sim.md H9).
