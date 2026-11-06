# Stranded capacity at 90% load on the recorded microVM workload (AC-T7.4.3)

> **Superseded in part (2026-11-06, 21:50Z).** The tables below are the run at `680116a`: 16 seeds spread
> evenly, the speed ladder, kube-scheduler's sample floors. Since then the branch has 59 seeds drawn
> uniformly, each set's exact slowest speed and refusals counted by kind (`b1e88bd`), and the sample's
> floors are being derived (below). The record reruns on the release-build traces; until then these
> tables are context. At `b1e88bd` the test holds the bar on all 59 seeds at the exact speeds: m5d.metal
> 35x as recorded and 14x with the keep-alive; the fleet's mix 126x and 42x, OCI 35x and 14x,
> Firecracker 22x and 8x, Shards 122x and 37x.

**Bar:** "stranded capacity at 90% load, against best fit alone falls by at least 30% compared with best fit
alone, measured by hex-sim with a recorded workload of OCI containers, Firecracker microVMs and Shards
microVMs" (`AC-T7.4.3`, T7's plan, version 4; G2 step 4). The workload of record is a day of Huawei Cloud's serverless
sandbox launches, each one of Hex's three kinds (an OCI container, a Firecracker microVM or a Shards
microVM), on two node models (`huawei_workload.md`): EC2 m5d.metal hosts, each running every kind (the main
one), and Hex's own fleet's host types beside it, each running the kinds its OS runs. **Result: MET on every
one of 16 seeds on both node models, for the mix of kinds and for each kind alone, with no completions and
with completions (lifetimes as recorded, and with Huawei's one-minute keep-alive). On m5d.metal, Hex's scorer
strands nothing on any seed. On the fleet it strands at least 53.0% less than best fit by CPU and 52.7% less
than best fit across resources (Kubernetes' `MostAllocated`), and outside the mix with no completions it
strands nothing.**

## What was measured

`crates/hex-bench/benches/sched_packing.rs` (its header says how). The same packing runs in the test
`the_recorded_workload_strands_at_least_30_percent_less_than_best_fit` (crates/hex-sim/tests/sim.rs), which
holds the bar on every seed of every node model, set and clock. This is hex-sim's virtual packing, not
wall-clock: the same trace gives the same tables on any machine.

- **Workload.** 25,295 launches of 339 owners. Each launch's kind is its owner's, a third of the launches
  each, and each kind's runtime overhead is in its memory (`huawei_workload.md`). Shards' overhead is
  measured, by guest size (`shards_overhead.md`).
- **Node models.**
  - `huawei-r5-day30.trace`, the main one: 10 EC2 m5d.metal hosts (48 cores, 384 GiB), each running all
    three kinds, so every request may use every node.
  - `huawei-r5-day30.fleet.trace`, beside it: Hex's fleet's hosts five times over (50 nodes). OCI
    containers may use 15 of them (480 cores), Firecracker microVMs 10 (320), Shards microVMs all 50
    (1,105).
  - Each has a `.keepalive.trace` twin with the keep-alive added.
- **Sets.** The mix as recorded, and each kind alone (`packing::alone`): the whole day as OCI containers,
  then as Firecracker microVMs, then as Shards microVMs, each need moved to that kind's overhead.
- **Packing.** Arrivals in order from the seed's start, wrapping round the day, through the fast path's
  sampled choice (kube-scheduler's floors, 100 nodes or 5%, so here every node). An arrival no node admits
  is passed over. Packing stops when the load reaches 90%. Load is the dominant share placed over the nodes
  the set's kinds may use. This workload binds on CPU.
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

  At the recorded rate, 0.29 launches a second, the day never loads the nodes to 90%. So arrivals come n
  times faster, and each launch keeps its recorded lifetime: the nodes serve n times this region's traffic.
  For each node model, set and trace, n is the slowest speed on the ladder (10, 20, 30, 40, 50, 60, 75,
  100, 150, 200, 300, 500, 1,000, 2,000) at which Hex and both best fits reach 90% on all 16 seeds. That
  lets lifetimes enter as far as the nodes allow.
- **Arms.** Best fit alone, two ways: by free CPU first (`BestFit`) and across CPU and memory
  (`MostAllocated`, Kubernetes' bin packing). Tetris's alignment (`Align`). And least stranded leftover
  first, then one of four ties (`Fragment(Tie)`):
  - Tetris's alignment (the plan's);
  - least allocated;
  - most allocated;
  - the node running the fewest kinds, then most allocated (Hex's, `Scorer::HEX`).

  hex-sim.md H11 records the choice.

## The speeds

The bench walks the ladder for every node model, set and trace, and prints each speed's row.

| node model, set | lifetimes as recorded | with the keep-alive |
|---|---|---|
| m5d.metal, every set | 40x; fewest completions on the way, by seed, 2,673. At 30x, no arm reaches 90% on any seed | 20x; 301. From 30x up, some seed has none end |
| fleet, mix | 150x; 4,963 (Hex), 5,112 and 5,099 (the best fits). At 100x, Hex reaches 90% on 3 seeds and the best fits on none | 50x; 312, 469 and 842. At 40x, the best fits reach 90% on 15 seeds; from 75x up, some seed has none end |
| fleet, OCI | 40x; 2,673 | 20x; 301. From 30x up, some seed has none end |
| fleet, Firecracker | 30x; 1,110 | 10x; 535. From 20x up, some seed has none end |
| fleet, Shards | 150x; 4,963 | 40x; 1,084. From 75x up, some seed has none end |

At 2,000x, the speed this measure first used, the keep-alive made completions vanish: a kept-alive launch
lives at least 60 s, and the nodes fill to 90% in seconds. The pack reports its completions
(`PackReport::completions`), and the test fails if any seed under a completions clock has none.

No set runs faster than 500x. At 500x every seed of every set reaches 90%, on both node models and on the
fleet as it is (below), because each seed wraps round the day. The earlier model's shortfall at 500x (on
the fleet five times over, the mix on 10 of 16 seeds and Shards on 11) came from seeds that started late in
the day and ran out of arrivals before 90%; wrapping gives each the whole day. Each set runs slower than
500x instead, at the slowest speed that still reaches 90%, from 10x to 150x. The table above says how far
the next speed down falls short.

The bar doesn't hang on these speeds. Of the 179 ladder rows at which Hex and both best fits reach 90% on
every seed, Hex meets the bar on every seed of 177. In the other two (m5d.metal at 60x, lifetimes as
recorded: the mix and Firecracker alone), one seed falls short only because `MostAllocated` strands
nothing there either, so no cut is possible. Hex strands nothing on it too. The ladder's last
column counts those seeds.

## Hex against the best fits

Stranded per-mille, the range over the 16 seeds, and the arrivals passed over on the way, summed over the
seeds. Every run of every arm reached 90%.

On m5d.metal, the main node model:

| set and clock | `BestFit` | `MostAllocated` | Hex | least cut vs `BestFit`, vs `MostAllocated` | passed over: `BestFit`, `MostAllocated`, Hex |
|---|---|---|---|---|---|
| mix, no completions | 708-804 | 708-804 | 0 | 100%, 100% | 0, 0, 0 |
| mix, 40x, as recorded | 262-700 | 87-702 | 0 | 100%, 100% | 0, 0, 0 |
| mix, 20x, with the keep-alive | 261-793 | 264-792 | 0 | 100%, 100% | 0, 0, 0 |
| OCI, no completions | 708-804 | 708-804 | 0 | 100%, 100% | 0, 0, 0 |
| OCI, 40x, as recorded | 261-701 | 174-615 | 0 | 100%, 100% | 0, 0, 0 |
| OCI, 20x, with the keep-alive | 266-793 | 262-794 | 0 | 100%, 100% | 0, 0, 0 |
| Firecracker, no completions | 708-804 | 708-804 | 0 | 100%, 100% | 0, 0, 0 |
| Firecracker, 40x, as recorded | 261-700 | 87-700 | 0 | 100%, 100% | 0, 0, 0 |
| Firecracker, 20x, with the keep-alive | 266-793 | 177-792 | 0 | 100%, 100% | 0, 0, 0 |
| Shards, no completions | 708-804 | 708-804 | 0 | 100%, 100% | 0, 0, 0 |
| Shards, 40x, as recorded | 261-700 | 88-700 | 0 | 100%, 100% | 0, 0, 0 |
| Shards, 20x, with the keep-alive | 267-793 | 262-792 | 0 | 100%, 100% | 0, 0, 0 |

Every m5d.metal node runs every kind, so nothing is refused for its kind, and Hex's fewest-kinds tie can't
tell the nodes apart: Hex is least stranded, then most allocated, here.

On Hex's fleet:

| set and clock | `BestFit` | `MostAllocated` | Hex | least cut vs `BestFit`, vs `MostAllocated` | passed over: `BestFit`, `MostAllocated`, Hex |
|---|---|---|---|---|---|
| mix, no completions | 651-665 | 637-661 | 0-312 | 53.0%, 52.7% | 6,679, 11,322, 2,556 |
| mix, 150x, as recorded | 429-611 | 282-544 | 0 | 100%, 100% | 1,917, 2,502, 0 |
| mix, 50x, with the keep-alive | 473-636 | 357-587 | 0 | 100%, 100% | 7,305, 8,043, 0 |
| OCI, no completions | 591-608 | 591-608 | 0 | 100%, 100% | 0, 0, 0 |
| OCI, 40x, as recorded | 236-472 | 162-454 | 0 | 100%, 100% | 0, 0, 0 |
| OCI, 20x, with the keep-alive | 144-587 | 113-589 | 0 | 100%, 100% | 0, 0, 0 |
| Firecracker, no completions | 474-602 | 474-602 | 0 | 100%, 100% | 0, 0, 0 |
| Firecracker, 30x, as recorded | 36-528 | 131-387 | 0 | 100%, 100% | 0, 0, 0 |
| Firecracker, 10x, with the keep-alive | 226-492 | 110-469 | 0 | 100%, 100% | 0, 0, 0 |
| Shards, no completions | 626-647 | 626-647 | 0 | 100%, 100% | 0, 0, 0 |
| Shards, 150x, as recorded | 383-573 | 286-510 | 0 | 100%, 100% | 0, 0, 0 |
| Shards, 40x, with the keep-alive | 470-611 | 366-599 | 0 | 100%, 100% | 0, 0, 0 |

On either model, Hex refuses nothing below half load and no arrival for room: a node always had the room.
On the fleet's mix with no completions, Hex's refusals all come from the kind filter. They are OCI
containers and Firecracker microVMs that found every Linux and Windows node full while the Macs, which run
only Shards microVMs, had room. The two best fits refuse those, and more.

Per seed on the fleet's mix with no completions, the one set and clock where Hex strands anything. Each
cell is stranded per-mille, with arrivals passed over in brackets.

| seed | from | `BestFit` | `MostAllocated` | the plan's | Hex | cut vs `BestFit` | cut vs `MostAllocated` |
|---:|---:|---|---|---|---|---:|---:|
| 0 | 0 | 660 (245) | 637 (440) | 0 (0) | 0 (0) | 100.0% | 100.0% |
| 1 | 1,580 | 651 (193) | 649 (386) | 0 (1) | 0 (0) | 100.0% | 100.0% |
| 2 | 3,161 | 658 (208) | 651 (523) | 0 (0) | 0 (0) | 100.0% | 100.0% |
| 3 | 4,742 | 659 (289) | 655 (564) | 26 (1) | 0 (0) | 100.0% | 100.0% |
| 4 | 6,323 | 664 (278) | 660 (703) | 54 (4) | 0 (0) | 100.0% | 100.0% |
| 5 | 7,904 | 662 (224) | 656 (514) | 0 (0) | 0 (0) | 100.0% | 100.0% |
| 6 | 9,485 | 660 (225) | 653 (550) | 0 (0) | 0 (0) | 100.0% | 100.0% |
| 7 | 11,066 | 660 (345) | 655 (722) | 203 (14) | 310 (21) | 53.0% | 52.7% |
| 8 | 12,647 | 659 (394) | 658 (648) | 312 (91) | 309 (118) | 53.1% | 53.0% |
| 9 | 14,228 | 659 (471) | 655 (880) | 311 (144) | 310 (165) | 53.0% | 52.7% |
| 10 | 15,809 | 659 (505) | 657 (643) | 310 (175) | 310 (186) | 53.0% | 52.8% |
| 11 | 17,390 | 658 (553) | 656 (933) | 307 (277) | 308 (286) | 53.2% | 53.0% |
| 12 | 18,971 | 664 (629) | 657 (1,002) | 308 (447) | 307 (464) | 53.8% | 53.3% |
| 13 | 20,552 | 661 (754) | 659 (932) | 307 (444) | 306 (460) | 53.7% | 53.6% |
| 14 | 22,133 | 665 (754) | 661 (1,091) | 314 (428) | 312 (446) | 53.1% | 52.8% |
| 15 | 23,714 | 661 (612) | 656 (791) | 307 (371) | 306 (410) | 53.7% | 53.4% |

From seed 7 on, the day's later launches fill the CPU of every node that runs OCI containers and
Firecracker microVMs, and the memory left beside it is stranded under every arm. Hex strands about 310
per-mille there, the best fits about 660.

## The arms

Across each node model's 12 sets and clocks.

On m5d.metal:

| arm | sets and clocks at the bar on 16/16 | least cut, any set and clock | passed over | passed over below half load | earliest refusal for room |
|---|---:|---:|---:|---:|---:|
| `BestFit` | (baseline) | - | 0 | 0 | - |
| `MostAllocated` | (baseline) | - | 0 | 0 | - |
| `Align` (Tetris) | 12 | 66.4% | 42 | 0 | 88.0% |
| least stranded, then alignment (the plan's) | 12 | 66.4% | 42 | 0 | 88.0% |
| least stranded, then least allocated | 12 | 67.0% | 29 | 0 | 88.3% |
| least stranded, then most allocated | 12 | 100% | 0 | 0 | - |
| **least stranded, then fewest kinds, then most allocated (Hex's)** | **12** | **100%** | **0** | **0** | **-** |

On the fleet:

| arm | sets and clocks at the bar on 16/16 | least cut, any set and clock | passed over: mix, no completions | passed over: the other 11 | passed over below half load | earliest refusal for room |
|---|---:|---:|---:|---:|---:|---:|
| `BestFit` | (baseline) | - | 6,679 | 9,222 | 421 | - |
| `MostAllocated` | (baseline) | - | 11,322 | 10,545 | 2,244 | - |
| `Align` (Tetris) | 12 | 45.7% | 2,406 | 138 | 0 | 88.1% |
| least stranded, then alignment (the plan's) | 12 | 31.7% | 2,397 | 142 | 0 | 87.8% |
| least stranded, then least allocated | 12 | 50.7% | 3,683 | 109 | 0 | 82.0% |
| least stranded, then most allocated | 12 | 53.0% | 11,195 | 7,433 | 1,910 | - |
| **least stranded, then fewest kinds, then most allocated (Hex's)** | **12** | **52.7%** | **2,556** | **0** | **0** | **-** |

"Earliest refusal for room" is the lowest load, over all sets, clocks and seeds, at which an arrival found no
node with room for it; `-` means none ever did.

- **Every fragment-aware arm meets the bar.** On this workload, any arm that looks at stranding first beats
  both best fits by at least 30% on every seed of both node models. What sets them apart is how much they
  cut and what they refuse.
- **The plan's score is not Hex's.** Least stranded, then alignment, the plan's score, cuts less: as little
  as 66.4% on m5d.metal, and 31.7% on the fleet (against `MostAllocated`, Firecracker alone with lifetimes
  as recorded). It passes over 42 launches on m5d.metal and 142 across the fleet's other sets and clocks,
  where Hex passes over none, and refuses for room from 87.8% load. On the fleet's mix with no completions
  it passes over a little less than Hex (2,397 against 2,556).
- **The most-allocated tie is Hex's on m5d.metal, and only there.** On m5d.metal every node runs every
  kind, so Hex's tie reduces to most allocated, and the two arms are one: nothing stranded, nothing passed
  over. On the fleet the most-allocated tie strands the least on the mix with no completions (a least cut
  of 53.5%, against Hex's 52.7%), but it passes over 11,195 launches there and 7,433 in the other sets,
  1,910 of them below half load. It spreads the Shards microVMs over the Linux and Windows nodes, and the Firecracker microVMs that
  only Linux runs then find no room: every early refusal the bench lists for it is Firecracker-shaped,
  refused while a Mac had room.
- **Hex's score keeps the nodes only some kinds can use for those kinds.** Ranking first the node that
  runs the fewest kinds sends Shards microVMs to the Macs and leaves Linux and Windows for the kinds only
  they can run. Across both models, Hex's score strands nothing in 23 of the 24 sets and clocks. It passes
  over launches only on the fleet's mix with no completions, never below half load, and never for room.

## Context: the fleet as it is

The lead asked for Hex's fleet as it is beside the two node models: its 10 hosts, one of each. That is
the fleet trace's first copy; `huawei_trace.py --nodes fleet:1` writes the same nodes. There, OCI
containers may use 3 hosts (96 cores), Firecracker microVMs 2 (64 cores), and Shards microVMs all 10 (221
cores). It isn't of record: with Firecracker's whole sub-cluster two hosts, a 90%-load packing bar turns on
one node's state (the lead, 2026-11-06). The bench measures it the same way, at its own clocks.

| set and clock | `BestFit` | `MostAllocated` | Hex | seeds where Hex meets the bar | passed over: `BestFit`, `MostAllocated`, Hex |
|---|---|---|---|---|---|
| mix, no completions | 499-658 | 493-654 | 0-316 | 16/16 (least cuts 38.9%, 38.5%) | 1,423, 2,317, 505 |
| mix, 20x, as recorded | 118-511 | 107-343 | 0 | 16/16 | 898, 1,475, 7 |
| mix, 10x, with the keep-alive | 192-536 | 100-481 | 0 | 16/16 | 8,668, 9,248, 272 |
| OCI, no completions | 115-446 | 115-446 | 0 | 16/16 | 0, 0, 0 |
| OCI, 10x, as recorded | 0-400 | 0-404 | 0 | 8/16; on the other 8 the best fits strand nothing either | 0, 0, 0 |
| OCI, 10x, with the keep-alive | 115-446 | 115-446 | 0 | 16/16, but no launch ends on the way | 0, 0, 0 |
| Firecracker, no completions | 157-687 | 157-687 | 0 | 16/16 | 0, 0, 1 |
| Firecracker, 10x, as recorded | 0-175 | 0-175 | 0 | 4/16; on the other 12 the best fits strand nothing either | 0, 0, 0 |
| Firecracker, 10x, with the keep-alive | 157-687 | 157-687 | 0 | 16/16, but no launch ends on the way | 0, 0, 1 |
| Shards, no completions | 484-624 | 484-624 | 0 | 16/16 | 0, 0, 0 |
| Shards, 20x, as recorded | 167-475 | 79-278 | 0 | 16/16 | 0, 0, 0 |
| Shards, 10x, with the keep-alive | 115-615 | 148-607 | 0 | 16/16 | 0, 0, 0 |

What differs from the two node models:
- **Seeds with nothing to cut.** OCI alone and Firecracker alone, with lifetimes as recorded, leave 8 and 12
  seeds on which the best fits strand nothing either. The bar can't be met there: there is nothing to cut.
- **No keep-alive completions.** OCI alone and Firecracker alone, with the keep-alive, fill their 2 or 3
  hosts before any kept-alive launch ends, even at 10x, the ladder's slowest speed. Those two clocks
  measure the same as no completions.
- **One refusal.** On Firecracker alone, with no completions and with the keep-alive, Hex passes over
  one launch on the way; the best fits pass over none.

Along the ladder, two cases miss the bar with something to cut, and both show why this model isn't of
record:
1. **Firecracker alone at 500x, as recorded**, the speed and set of one of the earlier model's two 15/16s
   (the other, the mix at 500x, now meets the bar on all 16 seeds). 10 seeds meet the bar, and 5 have
   nothing to cut. On seed 10, Hex strands 493 per-mille against the best fits'
   161. Firecracker runs only on the two Linux hosts, and the pack stops one launch past 90%, here at 99.4%
   of their CPU, with 400 millicores left between them. Hex left its last 100 millicores on adacachy,
   stranding the 91 GiB of memory beside them; the best fits left theirs on fedora, stranding 30 GiB. One
   node's leftover decides the seed.
2. **The mix at 20x with the keep-alive**, a speed the measure doesn't use. Seeds 8, 10 and 14 miss against
   `MostAllocated`: Hex strands 305, 316 and 183 per-mille, `MostAllocated` 372, 426 and 157. Hex fills the
   10 hosts before any kept-alive launch ends (seeds 8 and 10 have none). `MostAllocated` passes over 122 to
   207 launches on the way, reaches 90% later, and by then launches it placed have begun to end. Each host
   is a tenth of the measure.

Its 10,000-node latency is the fleet's above: the same host types in the same proportions make the same
10,000 nodes (`fast_path.md`).

## The run of record

- Commit: `680116a` (cp/t7-sched), release (`cargo bench` profile), from a `git archive` of the commit.
- Host: fedora, `kiln-a094a12d.attlocal.net`, CPUs 16-31. The unit is untimed: hex-sim's packing is
  virtual, so neither load nor the timed lock bears on it.
- Date: 2026-11-06. Units: hex-sched-measured-20261106T134534Z, on the working tree that became
  `680116a` (clippy, the hex-sim tests in release and this bench, all exit 0; rustfmt then reflowed two of
  the parser's lines, formatting alone); and PACKREC_UNIT, this bench at `680116a` itself, which printed
  the same tables.
- The traces were regenerated on fedora with Shards' measured overhead (hex-sched-traces-20261106T134334Z,
  `shards_overhead.md`). With the 5 MiB stand-in before it, at `5374202`, the bar held on every seed too
  (hex-sched-models2-20261106T100201Z). Of what moved, the fleet's mix with no completions gives the least
  cuts, 52.9% and 52.6% then, 53.0% and 52.7% now. OCI alone and Firecracker alone are unchanged.

## Side measurement: kiln CI, not of record

Until Ada's correction of 2026-11-06, this measure ran on fourteen days of kiln's own CI
(`kiln_workload.md`): 4,655 legs on 10 runners, needs rounded up to whole cores and GiB. Those results ran
without completions: `pack` had none until `d457978`, so no leg ever ended and gave its room back, and the
legs' lifetimes never entered them. At `ad3053c`, least stranded then least allocated met the bar on all 16
seeds (least cuts 63.2% and 51.4%), and least stranded then most allocated on 8. Every node in that trace
runs every kind, so today's Hex tie cannot tell its nodes apart and is most allocated there: 8 of 16. That
record is `packing.md` at `ad3053c`. It isn't re-run: kiln's CI is the load balancer's workload, not Hex's.

## What it doesn't show

- The kinds are given, not recorded, and Shards' overhead is measured on idle guests
  (`huawei_workload.md`, what would make it wrong; `shards_overhead.md`).
- Time: packing to one load, not a schedule over the day. A launch passed over here is refused for good;
  in a schedule it waits for room.
- Devices: this day asks for none. GPU placement by kind, and a benchmark never beside a build, are held
  on generated clusters (hex-sim.md H9).

## The sample's floors, derived (in progress)

Ada's rule (2026-11-06) makes kube-scheduler's floors, 100 nodes or 5%, a pick to replace. The lead
(17:14Z): derive them from the fewest sampled nodes that hold the bar on every seed. `sched_packing
floors MODEL:COPIES` finds, on copies of a node model's hosts, the fewest admitting nodes a decision can
sample with Hex still meeting the bar on every one of the 59 seeds, set and clock, at that sample and
every larger one. Each sample runs at its own slowest speeds, as the record finds them. Deterministic;
measured on adas-mac-studio-3 at `0bb1521`'s code:

| cluster | nodes | fewest sampled | least cut there | below it |
|---|---:|---:|---:|---|
| m5d.metal x10 (the record's) | 10 | 6 | 63.0% | at 5, a best fit strands nothing (OCI, seed 19) and Firecracker is under 30% (seed 4) |
| fleet x5 (the record's) | 50 | 9 | 36.5% | at 8, a best fit strands nothing (Firecracker at 22x, seed 33); the cut stays near 52% down to 14 |

On jupiter, before each sample re-found its speeds (whole-scan speeds throughout): m5d.metal x20 needs
7 and x50 needs 16, each missing below with the mix under 30% with no completions, where speed doesn't
enter. m5d.metal x100 and the fleet x10 were still running at the pause (kiln runs r32 on
adalundhe-macbook-pro and r33 on adas-mac-studio-3; logs `~/.cache/hex-t7/sched/out/floors-mbp.log` and
`floors-s3b.log` there).

