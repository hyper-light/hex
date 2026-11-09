# Proximity-scored placement against proximity-blind on a modelled WAN (S4, A2; G3)

**Question:** does scoring placement by hyper-swim's Vivaldi coordinates improve goodput or the
completion-time tail over placement that ignores proximity (scheduler-architecture.md S4's falsifier),
for the work Hex places, on the day it was recorded?

**Result: the tail, yes, on every study; goodput, no change.** On G2's recorded day, 118 studies
(59 seeds on each node model) of OCI containers, Firecracker microVMs and Shards microVMs:
- **Tail:** proximity cut the p99 completion time against blind on every study, over all launches
  and for each kind. The cut over all launches was at least 5.1% (AllKinds) and 5.8% (Mixed).
- **Median:** the p50 cut was at least 91%.
- **Goodput:** unchanged on every study. At the day's own load nothing queues, so every launch
  finishes within the day under either policy.
The falsifier is not met, so proximity stays a soft score. The claim covers inter-region and
inter-zone structure at the recorded load. It says nothing about goodput under contention, which the
day doesn't have.

**The run of record.**
- Host: fedora, `kiln-a094a12d.attlocal.net`, Intel Core i9-14900HX, Linux 6.19.10-300.fc44; release
  (`cargo bench` profile).
- Commit `272b220` (cp/t7-sched-g3), 2026-11-06, 20:46Z. Unit `hex-sched-g3-record-20261106T204635Z`
  (CPUs 16-31, exit 0). The same unit ran hex-sim's tests, the `no-proximity-term` mutant (caught) and
  all 7 gates (exit 0).
- Load average at the start 12.4 (noisy; it doesn't change virtual-time results). Co-tenants:
  `issue556oss-control-plane`, three kiln buildkit containers, `kind-registry`, `kiln-t10-minio`.
- The same tables came out of adas-mac-studio-2.local (aarch64 macOS) at `fac8372` and `272b220`, digit
  for digit: the study is deterministic across both.
- The full log is on fedora at
  `~/.cache/hex-t7/sched-g3/out/hex-sched-g3-record-20261106T204635Z.bench.log`.

**Shards' overhead is a stand-in.** Each kind's runtime overhead is the recorded workload's, and in it
Shards' is 5 MiB, because no source stated Shards' own when the trace was made. When cp/t7-sched carries
Shards' measured overhead, the run is repeated at that commit.

## What was measured

`crates/hex-bench/benches/sched_proximity.rs` over `hex_sim::proximity` (`docs/design/hex-sim.md` H20, H21).

**The day.** G2's recorded day as it is: Huawei Cloud, Region 5, day 30, 25,295 launches from 339
owners. It runs on each node model, `huawei-r5-day30.trace` (ten EC2 m5d.metal hosts, each running
all three kinds) and `huawei-r5-day30.fleet.trace` (Hex's fleet hosts, 50 rows, some running only some
kinds). Each launch runs at its recorded arrival, with:
- its owner;
- its kind (G2's rule: a kind per owner, in thirds: 8,432 OCI, 8,432 Firecracker, 8,431 Shards);
- its need, with its kind's overhead (OCI 440,402 B, Firecracker 5,242,880 B, Shards 5,242,880 B, a
  stand-in);
- its recorded lifetime as its compute.

**Round trips.** Each launch talks to a home node drawn uniformly. It makes log-normal round trips
through TraceLab's p50 25 and p99 1,438, each waiting the WAN's true RTT from its node to its home,
added to its compute.

**The WAN of record.** Azure's published P50 inter-region round trips, every zone pair at Azure's
stated ~2 ms bound, and no access heights or jitter. Coordinates are hyper-swim's engine on its own
schedule (every member probed once a round, shuffled). They converge by patience: they stop once as
many rounds have passed without a new lowest median error as it took to reach the lowest. Placement
uses the last round's coordinates.

**Policies**, over the fast path's own sample (`FitIndex::sampled_fit`), a launch only on a node that
runs its kind:
- **blind**: sampled best fit.
- **proximity**: the least predicted RTT, a node 0 from itself; best fit breaks ties.
- **oracle**: the least true RTT.

**Measures.**
- Goodput: launches finished by the day's last arrival, per second.
- Completion time: arrival to end, every launch run to its end.
- Both over all launches and for each kind.

**Seeds.** As many as the claim "at least 95% of seeds gain, at 95% confidence" needs:
`⌈ln 0.05 / ln 0.95⌉ = 59` a node model.

**Sensitivity runs, not of record**, over the same seeds:
- skewed homes (half in East US);
- the modelled WAN (zone terms drawn below the bound, access heights of 25-250 µs, probes stretched
  up to 10%).

The tables come from the bench's log by `bench/sched/proximity_tables.py`. Virtual time: the numbers
are a function of the seed and the code, not of the host's load.

## Proximity against blind (the run of record)

### RECORD

| node model | kind | p99 cut on | least p99 cut | mean p99 cut | p50 cut on | least p50 cut | goodput lower on |
|---|---|---|---|---|---|---|---|
| AllKinds | all | 59/59 | +5.051% | +6.55% | 59/59 | +93.8% | 0/59 |
| AllKinds | Oci | 59/59 | +61.180% | +66.15% | 59/59 | +86.0% | 0/59 |
| AllKinds | Firecracker | 59/59 | +2.450% | +9.59% | 59/59 | +99.4% | 0/59 |
| AllKinds | Shards | 59/59 | +0.004% | +3.05% | 59/59 | +76.3% | 0/59 |
| Mixed | all | 59/59 | +5.770% | +7.84% | 59/59 | +91.5% | 0/59 |
| Mixed | Oci | 59/59 | +27.591% | +65.04% | 59/59 | +83.0% | 0/59 |
| Mixed | Firecracker | 59/59 | +2.093% | +8.47% | 59/59 | +96.9% | 0/59 |
| Mixed | Shards | 59/59 | +2.250% | +5.65% | 59/59 | +79.2% | 0/59 |

Final median coordinate error: min 3.1%, median 6.0%, max 29.2% (AllKinds seed 15, 6 rounds); most
rounds 6,350. Proximity is within 0.01% of the oracle's goodput.

### SENSITIVITY skewed homes (not of record)

| node model | kind | p99 cut on | least p99 cut | mean p99 cut | p50 cut on | least p50 cut | goodput lower on |
|---|---|---|---|---|---|---|---|
| AllKinds | all | 59/59 | +2.410% | +4.04% | 59/59 | +79.4% | 0/59 |
| AllKinds | Oci | 59/59 | +57.251% | +57.57% | 59/59 | +51.5% | 0/59 |
| AllKinds | Firecracker | 59/59 | +1.417% | +3.98% | 59/59 | +97.9% | 0/59 |
| AllKinds | Shards | 56/59 | +0.000% | +2.00% | 59/59 | +68.2% | 0/59 |
| Mixed | all | 59/59 | +4.922% | +6.95% | 59/59 | +91.6% | 0/59 |
| Mixed | Oci | 59/59 | +43.776% | +59.85% | 59/59 | +82.0% | 0/59 |
| Mixed | Firecracker | 59/59 | +1.788% | +5.16% | 59/59 | +97.1% | 0/59 |
| Mixed | Shards | 59/59 | +1.646% | +5.46% | 59/59 | +81.1% | 0/59 |

With half the homes in one region, Shards' p99 on AllKinds is equal to blind's on 3 of 59 seeds, and
never worse.

### SENSITIVITY modelled WAN (not of record)

| node model | kind | p99 cut on | least p99 cut | mean p99 cut | p50 cut on | least p50 cut | goodput lower on |
|---|---|---|---|---|---|---|---|
| AllKinds | all | 59/59 | +4.603% | +6.65% | 59/59 | +93.6% | 0/59 |
| AllKinds | Oci | 59/59 | +61.940% | +66.62% | 59/59 | +86.0% | 0/59 |
| AllKinds | Firecracker | 59/59 | +3.025% | +10.17% | 59/59 | +99.4% | 0/59 |
| AllKinds | Shards | 59/59 | +0.091% | +3.72% | 59/59 | +76.2% | 0/59 |
| Mixed | all | 59/59 | +4.839% | +7.73% | 59/59 | +93.1% | 0/59 |
| Mixed | Oci | 59/59 | +60.381% | +65.70% | 59/59 | +85.5% | 0/59 |
| Mixed | Firecracker | 59/59 | +2.109% | +8.61% | 59/59 | +98.0% | 0/59 |
| Mixed | Shards | 59/59 | +2.216% | +5.27% | 59/59 | +79.3% | 0/59 |

Final median coordinate error: min 6.4%, median 9.1%, max 26.7%; most rounds 492.

## Reading it

- **Goodput can't move at this load.** Blind's queue never holds more than one launch, so every
  launch finishes within the day under any policy, and goodput is equal on every study. What
  proximity buys here is time: the median launch finishes in 0.32 s instead of 5.4 s on AllKinds,
  because its round trips stay on its home node.
- **On AllKinds every launch lands on its own home.** Ten hosts with room to spare give a mean RTT of
  0.0 ms, so proximity's p99 there is the lifetimes' own, 777.2 s on every seed, and blind's is higher on every seed.
- **Proximity matches the oracle.** Its goodput is within 0.01% of the oracle's, and on AllKinds its
  tail equals the oracle's.
- **Why the score is the plain prediction, a node 0 from itself, with no tie-break policy and
  last-round coordinates.** These are four measured reasons, each recorded in H21: `K_SIGMA = 2` bought
  no least gain; the tie-break worsened the tail on both node models; Vivaldi's `2·height` for a node
  and itself kept launches off their own homes, the cause of an earlier Shards miss; and choosing the
  best round would read the truth.
- **What the gain rests on.** This is the model, not a measurement on Hex's fleet. Each launch talks
  to one home, its round trips sequential. Bandwidth is not modelled (A2's gap). There are no heights
  or jitter inside a zone. Shards' overhead is a 5 MiB stand-in.

## The record, every seed (all launches; fedora, `272b220`)

Each policy's cell gives goodput (/s), p50 completion (s), p99 completion (s) and mean true RTT to home
(ms). Rounds and final error are the coordinates' convergence under patience, with the last round's
coordinates held.

| node model | seed | rounds | final error | Blind goodput /s, p50 s, p99 s, RTT ms | Proximity goodput /s, p50 s, p99 s, RTT ms | Oracle goodput /s, p50 s, p99 s, RTT ms |
|---|---|---|---|---|---|---|
| AllKinds | 1 | 18 | 6.5% | 0.2930, 5.39, 818.8, 117.4 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 2 | 68 | 6.8% | 0.2930, 5.38, 829.1, 116.5 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 3 | 38 | 5.2% | 0.2930, 5.21, 834.7, 116.6 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 4 | 42 | 6.0% | 0.2930, 5.38, 830.9, 116.4 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 5 | 8 | 6.9% | 0.2930, 5.37, 825.0, 115.9 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 6 | 22 | 7.4% | 0.2930, 5.34, 835.4, 117.0 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 7 | 24 | 4.7% | 0.2930, 5.32, 836.5, 116.0 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 8 | 68 | 4.9% | 0.2930, 5.35, 821.5, 116.4 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 9 | 286 | 3.9% | 0.2930, 5.42, 835.6, 116.0 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 10 | 88 | 4.4% | 0.2930, 5.35, 824.1, 116.7 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 11 | 52 | 3.7% | 0.2930, 5.39, 841.1, 117.0 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 12 | 106 | 3.8% | 0.2930, 5.34, 820.8, 116.3 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 13 | 28 | 7.6% | 0.2930, 5.20, 832.7, 117.0 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 14 | 96 | 6.4% | 0.2930, 5.34, 832.9, 116.1 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 15 | 6 | 29.2% | 0.2930, 5.28, 837.4, 116.7 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 16 | 46 | 6.1% | 0.2930, 5.46, 821.1, 116.8 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 17 | 34 | 3.1% | 0.2930, 5.22, 819.9, 116.1 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 18 | 62 | 3.6% | 0.2930, 5.50, 828.0, 117.1 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 19 | 70 | 4.5% | 0.2930, 5.55, 819.3, 117.6 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 20 | 942 | 3.8% | 0.2930, 5.29, 831.0, 116.2 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 21 | 48 | 6.3% | 0.2930, 5.41, 822.6, 116.6 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 22 | 196 | 5.8% | 0.2930, 5.30, 832.0, 116.2 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 23 | 60 | 7.2% | 0.2930, 5.38, 842.5, 117.0 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 24 | 706 | 3.7% | 0.2930, 5.41, 818.6, 116.7 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 25 | 66 | 7.4% | 0.2930, 5.41, 834.7, 116.3 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 26 | 46 | 7.6% | 0.2930, 5.38, 826.3, 116.5 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 27 | 44 | 3.4% | 0.2930, 5.35, 833.5, 116.1 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 28 | 30 | 6.1% | 0.2930, 5.38, 838.1, 116.8 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 29 | 128 | 4.1% | 0.2930, 5.36, 822.0, 116.9 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 30 | 46 | 4.0% | 0.2930, 5.41, 829.1, 116.7 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 31 | 6350 | 3.2% | 0.2930, 5.29, 819.6, 115.7 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 32 | 8 | 8.6% | 0.2930, 5.63, 837.5, 117.4 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 33 | 42 | 6.6% | 0.2930, 5.32, 839.5, 116.4 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 34 | 256 | 6.0% | 0.2930, 5.36, 836.8, 116.7 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 35 | 70 | 7.0% | 0.2930, 5.55, 836.2, 117.0 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 36 | 138 | 6.7% | 0.2930, 5.47, 836.5, 117.3 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 37 | 46 | 4.5% | 0.2930, 5.47, 837.4, 116.9 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 38 | 34 | 7.9% | 0.2930, 5.32, 837.4, 116.9 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 39 | 76 | 7.7% | 0.2930, 5.36, 830.7, 116.4 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 40 | 1952 | 3.2% | 0.2930, 5.30, 827.2, 116.1 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 41 | 44 | 6.3% | 0.2930, 5.34, 836.9, 116.9 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 42 | 54 | 5.9% | 0.2930, 5.38, 832.1, 117.3 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 43 | 222 | 3.9% | 0.2930, 5.36, 843.4, 117.1 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 44 | 46 | 5.4% | 0.2930, 5.24, 833.6, 115.6 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 45 | 60 | 7.7% | 0.2930, 5.48, 831.5, 116.3 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 46 | 40 | 5.8% | 0.2930, 5.44, 840.8, 116.5 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 47 | 36 | 6.2% | 0.2930, 5.40, 836.3, 116.5 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 48 | 158 | 6.8% | 0.2930, 5.26, 836.9, 115.9 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 49 | 40 | 4.3% | 0.2930, 5.35, 830.5, 117.1 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 50 | 62 | 4.8% | 0.2930, 5.50, 838.9, 117.2 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 51 | 42 | 7.7% | 0.2930, 5.27, 834.1, 116.7 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 52 | 166 | 4.1% | 0.2930, 5.27, 834.9, 117.1 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 53 | 86 | 6.2% | 0.2930, 5.42, 821.4, 117.1 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 54 | 88 | 5.7% | 0.2930, 5.36, 832.5, 117.0 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 55 | 52 | 4.9% | 0.2930, 5.39, 840.5, 117.0 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 56 | 24 | 4.7% | 0.2930, 5.38, 835.8, 117.8 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 57 | 48 | 4.5% | 0.2930, 5.41, 834.8, 117.1 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 58 | 14 | 7.0% | 0.2930, 5.56, 836.1, 116.8 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| AllKinds | 59 | 110 | 4.7% | 0.2930, 5.29, 832.2, 116.2 | 0.2930, 0.32, 777.2, 0.0 | 0.2930, 0.32, 777.2, 0.0 |
| Mixed | 1 | 276 | 5.6% | 0.2930, 5.79, 858.5, 131.4 | 0.2930, 0.40, 777.5, 0.7 | 0.2930, 0.39, 777.5, 0.6 |
| Mixed | 2 | 10 | 6.8% | 0.2930, 5.79, 836.8, 128.2 | 0.2930, 0.42, 777.3, 2.1 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 3 | 6 | 9.4% | 0.2930, 5.71, 832.8, 129.9 | 0.2930, 0.49, 777.3, 10.5 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 4 | 76 | 5.1% | 0.2930, 5.92, 840.0, 129.5 | 0.2930, 0.41, 777.4, 0.7 | 0.2930, 0.40, 777.4, 0.6 |
| Mixed | 5 | 66 | 5.5% | 0.2930, 5.81, 840.1, 129.5 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 6 | 82 | 5.7% | 0.2930, 5.89, 845.0, 130.3 | 0.2930, 0.40, 777.8, 0.7 | 0.2930, 0.39, 777.8, 0.6 |
| Mixed | 7 | 24 | 6.1% | 0.2930, 5.96, 834.8, 129.1 | 0.2930, 0.41, 777.2, 0.7 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 8 | 10 | 6.9% | 0.2930, 5.94, 861.7, 129.8 | 0.2930, 0.41, 777.5, 0.8 | 0.2930, 0.40, 777.5, 0.6 |
| Mixed | 9 | 334 | 5.5% | 0.2930, 5.98, 842.9, 130.3 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 10 | 1188 | 5.8% | 0.2930, 5.95, 847.1, 129.9 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 11 | 122 | 5.1% | 0.2930, 5.98, 844.7, 129.0 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.39, 777.3, 0.6 |
| Mixed | 12 | 12 | 6.9% | 0.2930, 5.86, 836.5, 130.0 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 13 | 134 | 4.9% | 0.2930, 5.81, 835.8, 129.9 | 0.2930, 0.40, 777.2, 0.7 | 0.2930, 0.39, 777.2, 0.6 |
| Mixed | 14 | 10 | 6.4% | 0.2930, 5.88, 848.0, 130.3 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 15 | 266 | 5.3% | 0.2930, 5.81, 851.7, 129.9 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 16 | 10 | 6.6% | 0.2930, 5.90, 838.8, 129.3 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 17 | 68 | 5.3% | 0.2930, 5.85, 854.0, 132.2 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.39, 777.2, 0.6 |
| Mixed | 18 | 20 | 5.9% | 0.2930, 5.79, 839.7, 129.5 | 0.2930, 0.42, 777.3, 0.8 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 19 | 16 | 6.7% | 0.2930, 5.74, 887.4, 129.7 | 0.2930, 0.41, 777.5, 0.8 | 0.2930, 0.40, 777.5, 0.6 |
| Mixed | 20 | 26 | 6.2% | 0.2930, 5.75, 843.1, 129.7 | 0.2930, 0.41, 777.4, 0.8 | 0.2930, 0.40, 777.4, 0.6 |
| Mixed | 21 | 92 | 5.5% | 0.2930, 5.72, 824.9, 128.9 | 0.2930, 0.41, 777.3, 0.7 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 22 | 18 | 6.0% | 0.2930, 5.81, 837.8, 130.1 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 23 | 10 | 6.2% | 0.2930, 5.86, 861.2, 130.5 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.39, 777.3, 0.6 |
| Mixed | 24 | 10 | 6.6% | 0.2930, 6.00, 844.3, 129.7 | 0.2930, 0.41, 777.5, 0.7 | 0.2930, 0.39, 777.5, 0.6 |
| Mixed | 25 | 134 | 6.7% | 0.2930, 5.69, 867.3, 130.2 | 0.2930, 0.41, 777.3, 0.7 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 26 | 62 | 5.7% | 0.2930, 5.92, 832.1, 129.8 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.39, 777.2, 0.6 |
| Mixed | 27 | 28 | 6.1% | 0.2930, 5.91, 830.9, 129.6 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 28 | 52 | 6.2% | 0.2930, 5.78, 832.0, 129.4 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.39, 777.3, 0.6 |
| Mixed | 29 | 26 | 6.3% | 0.2930, 6.01, 859.1, 130.5 | 0.2930, 0.41, 777.3, 0.7 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 30 | 14 | 6.3% | 0.2930, 5.97, 826.6, 130.5 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 31 | 12 | 6.7% | 0.2930, 5.90, 834.9, 129.8 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.39, 777.2, 0.6 |
| Mixed | 32 | 708 | 5.5% | 0.2930, 6.03, 849.4, 130.2 | 0.2930, 0.41, 777.5, 0.8 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 33 | 14 | 6.3% | 0.2930, 5.75, 839.6, 128.6 | 0.2930, 0.40, 777.2, 0.7 | 0.2930, 0.39, 777.2, 0.6 |
| Mixed | 34 | 18 | 6.6% | 0.2930, 5.82, 864.0, 130.3 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 35 | 118 | 5.8% | 0.2930, 5.84, 836.5, 130.1 | 0.2930, 0.41, 777.3, 0.7 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 36 | 34 | 6.5% | 0.2930, 5.89, 836.0, 131.0 | 0.2930, 0.40, 777.2, 0.7 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 37 | 8 | 6.7% | 0.2930, 5.84, 836.1, 129.2 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.39, 777.2, 0.6 |
| Mixed | 38 | 6 | 6.6% | 0.2930, 5.86, 837.4, 129.0 | 0.2930, 0.42, 777.9, 2.2 | 0.2930, 0.40, 777.9, 0.6 |
| Mixed | 39 | 206 | 4.7% | 0.2930, 5.91, 837.8, 129.9 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 40 | 36 | 5.5% | 0.2930, 5.97, 846.5, 129.9 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.39, 777.2, 0.6 |
| Mixed | 41 | 262 | 5.7% | 0.2930, 5.82, 843.4, 130.2 | 0.2930, 0.40, 777.5, 0.7 | 0.2930, 0.40, 777.5, 0.6 |
| Mixed | 42 | 14 | 7.0% | 0.2930, 5.95, 858.1, 130.5 | 0.2930, 0.41, 777.2, 0.7 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 43 | 76 | 5.7% | 0.2930, 5.70, 836.5, 129.6 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.39, 777.2, 0.6 |
| Mixed | 44 | 220 | 5.7% | 0.2930, 5.86, 836.7, 129.1 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 45 | 18 | 6.8% | 0.2930, 6.12, 850.6, 130.9 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 46 | 48 | 5.8% | 0.2930, 5.83, 848.1, 131.1 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 47 | 16 | 6.8% | 0.2930, 5.99, 832.1, 130.6 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 48 | 182 | 5.2% | 0.2930, 5.88, 833.0, 129.6 | 0.2930, 0.42, 777.9, 0.8 | 0.2930, 0.40, 777.9, 0.6 |
| Mixed | 49 | 552 | 5.9% | 0.2930, 5.77, 832.1, 130.1 | 0.2930, 0.41, 777.4, 0.8 | 0.2930, 0.40, 777.4, 0.6 |
| Mixed | 50 | 100 | 5.8% | 0.2930, 6.02, 848.2, 130.0 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 51 | 8 | 6.5% | 0.2930, 5.91, 853.9, 131.5 | 0.2930, 0.40, 777.4, 0.7 | 0.2930, 0.40, 777.4, 0.6 |
| Mixed | 52 | 54 | 5.8% | 0.2930, 5.80, 846.6, 130.5 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.40, 777.3, 0.6 |
| Mixed | 53 | 278 | 4.9% | 0.2930, 6.05, 842.0, 129.9 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 54 | 78 | 5.2% | 0.2930, 5.99, 837.3, 130.5 | 0.2930, 0.42, 777.2, 0.8 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 55 | 12 | 6.5% | 0.2930, 5.90, 852.1, 129.5 | 0.2930, 0.41, 777.5, 0.7 | 0.2930, 0.40, 777.5, 0.6 |
| Mixed | 56 | 18 | 6.4% | 0.2930, 6.03, 843.6, 131.3 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.39, 777.2, 0.6 |
| Mixed | 57 | 206 | 5.4% | 0.2930, 5.97, 843.5, 129.8 | 0.2930, 0.41, 777.2, 0.8 | 0.2930, 0.40, 777.2, 0.6 |
| Mixed | 58 | 10 | 6.8% | 0.2930, 6.06, 839.9, 129.7 | 0.2930, 0.41, 777.4, 0.8 | 0.2930, 0.40, 777.4, 0.6 |
| Mixed | 59 | 14 | 6.7% | 0.2930, 5.84, 841.0, 129.6 | 0.2930, 0.41, 777.3, 0.8 | 0.2930, 0.39, 777.2, 0.6 |
