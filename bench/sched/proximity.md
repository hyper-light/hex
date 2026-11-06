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

**Pending: the run of record.** The numbers below are from adas-mac-studio-2.local (virtual time, at
commit `fac8372`) while fedora was unreachable. The run of record is fedora's at the final commit; when
it lands, this file takes its numbers, host and commit, and its per-seed table.

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

## Proximity against blind (adas-mac-studio-2.local, `fac8372`; pending the run of record)

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
