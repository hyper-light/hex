# The scheduler's architecture: maximal correctness and goodput for bursty agentic work

Hex's scheduler decides, for every workload and every unit of work, the most suitable Shards instance
and node to run it on — by hardware capability, provider usage and rate limits, token and
context-window availability, provider, model, effort, agent and permissions, and network latency and
bandwidth — with urgency that maximises the work completed (goodput), not mere load balance. This
document decides how, from the research and against the track's bars, with each choice's sources,
evidence and what would falsify it (CLAUDE.md §5). It covers the control plane's scheduler (CP-T7.3 to
CP-T7.12); the per-node runtime is `runtime.md` and `runtime-architecture.md`, and the scheduler places
onto its classes through the `hex-api::Runtime` seam.

The evidence:
- `docs/research/scheduler/comparison.md`: the cross-system comparison (9 findings, 5 axes, the gaps),
  over four cited notes. Cells cite `CM[n]` (cluster managers), `PA[n]` (placement and fairness),
  `AW[n]` (agentic and LLM), `PF[n]` (proximity and failure), each resolving in its note to a primary
  source, the quotation and its verification, with the note's kind: `MV` measured-vendor, `MI`
  measured-independent, `CL` claimed, `DE` design.
- `docs/requirements.toml`: the bars this design is held to (`AC-T7.n.k`, `T-T7.n.k`).
- `~/Projects/hyper-raft`, crate `hyper-swim`: the membership, failure detection and network
  coordinates Hex builds on rather than reinventing (`src/coordinates.rs`, `src/extension.rs`,
  `src/detector.rs`).

## The objective

**Maximal correctness first, then goodput.** Correctness is absolute and measured as zero: no workload
lost or started twice (`AC-T7.3.3`, 10,000 crash seeds), no disruption budget broken by preemption
(`AC-T7.5.3`), no flow allowed that no grant names (`AC-T7.7.2`), no session on a machine over 90% CPU
while another has room (`AC-T7.6.3`), no GPU work on a node without one. Among the placements that keep
every invariant, the scheduler maximises goodput — the work actually completed under each workload's
deadline, SLO, provider quota and context window — not the balance of load across machines. A balanced
cluster that misses deadlines or wastes provider quota has failed the objective.

This is a deliberate split from the production cluster managers, which prioritise by admission quota and
priority bands and measure packing, not goodput (Borg CM[1], Twine CM[4], Kubernetes CM[25], `DE`). The
goodput systems that beat them — 30-93% lower average job completion time (Sia, Pollux PA[10][12]),
2.2-3.25x fairer worst cases (Themis PA[13]), 4.0x fewer SLO misses (3Sigma PA[15]), all `MV` — are
research-scale (44-256 GPUs). Hex takes their objective to the production scale the cluster managers
reach (comparison finding 4).

## The bar

The scheduler's own numbers (`docs/requirements.toml`), with where the research places them:

| Axis | Bar | Where it sits |
|---|---|---|
| placement throughput | 1,000,000 workloads on 10,000 nodes in ≤120 s = 8,333/s (`AC-T7.11.2`) | between the sampled placers (Hydra 40,000/s PA[3], Apollo 20,000/s PA[2]) and the Kubernetes family (GKE ≤1,000/s CM[38], kube-scheduler ~790/s CM[30]) (finding 2) |
| vs kube-scheduler | ≥10x on the same simulated cluster (`AC-T7.11.3`, against kwok) | kube-scheduler is ~790-1,000 pods/s CM[30][38]; 10x is ~8,000/s, the same line |
| decision latency | p99 ≤10 ms at 10,000/s on 10,000 nodes (`AC-T7.4.2`) | faster than every published tail (RAS p99 2.45K s CM[5], Shard Manager p99 ~50 s CM[6], Hydra p99 ≤10 s PA[3]); no competitor publishes p99.9 (finding 1), so Hex measures its own |
| packing | stranded capacity at 90% load ≥30% below best-fit alone (`AC-T7.4.3`) | Borg's best+worst-fit mix CM[1]; Tetris ~30% better makespan PA[19] |
| fair share | 4 equal-weight repositories within 2% of 25% under contention (`AC-T7.5.2`) | DRF max-min dominant share PA[18]; Kueue cohorts PA[20] |
| control-plane memory | ≤4 KiB per workload (`AC-T7.11.4`) | an accounting bar; no competitor publishes per-workload control-plane memory |
| correctness | lost-or-double = 0 (`AC-T7.3.3`); disruption breaches = 0 (`AC-T7.5.3`); ungranted flows = 0 (`AC-T7.7.2`) | measured by hex-sim with a nemesis and by property tests |

Every bar but the usable-now change-request criteria is measured by **hex-sim** (decision S8), the
simulated cluster this design builds first.

## Decisions

### S1. The scoring objective is goodput, not balance

**The choice.** A placement's score is the goodput it is expected to add: the work completed under the
workload's own deadline, SLO, provider quota and context window, net of what placing it there costs the
work already there (interference, eviction, fragmentation). The scheduler chooses the feasible placement
of highest marginal goodput, not the one that most evens out load.

**Sources.** Pollux's per-job goodput model and cluster-goodput objective with a fairness exponent
(PA[12], `MV`); Sia's normalised goodput with a restart penalty (PA[10], `MV`); 3Sigma's expected
utility over runtime distributions (PA[15], `MV`); Themis's finish-time fairness (PA[13], `MV`);
Tiresias's least-attained-service (PA[14], `MV`). For agents, effective throughput within TTFT/TPOT SLOs
(DistServe AW[29], Mooncake AW[30]) and program-level cumulative service (Autellix AW[8], `MV`).

**Evidence.** The goodput objective reports 30-93% lower average JCT (Sia, Pollux), 4.0x fewer SLO
misses (3Sigma) and 2.2-3.25x fairer worst cases (Themis) than balance- or priority-only baselines on 44
to 256 GPUs (finding 4). Best-fit packing alone strands capacity that a best+worst-fit mix recovers
(Borg CM[1]); `AC-T7.4.3` holds Hex to ≥30% less stranded capacity than best-fit at 90% load.

**What would falsify it.** A balance-only or best-fit-only scorer matching the goodput objective on the
recorded kiln workload in hex-sim (`AC-T7.4.3`), or the goodput model costing more to evaluate than the
10 ms p99 budget allows at 8,333/s (`AC-T7.4.2`) — in which case goodput moves to the optimiser tier
(S2) and the fast path scores a cheap proxy.

### S2. Placement is tiered: a sampled, late-bound fast path, with an optimiser tier beside it

**The choice.** The request path filters and scores a **sample** of candidate (node, class, Shards
instance) placements, over Borg-style equivalence classes with cached node state, and **late-binds**:
the decision is confirmed against the node's current state at commit, by optimistic concurrency, and
retried on conflict. A second, asynchronous **optimiser tier** runs a constraint solver for the
placements that are worth it — long-lived services, gangs, hard multi-constraint work — on a slower SLO,
and the fast path honours its reservations. No optimiser runs on the request path.

**Sources.** The fastest placers are sampled and late-bound: Sparrow's batch sampling of `d·m` workers
with late binding (median queueing <9 ms PA[1]); Apollo's per-job schedulers with stable matching
(>20,000 tasks/s PA[2]); Hydra's federated local placement (>40,000 decisions/s PA[3]); all `MV`. The
best placements come from optimisers too slow for the request path, so every system at scale tiers them:
RAS's MIP on a 1 h SLO beside a seconds allocator (CM[5]); Medea's ILP only for long-running containers,
9.5x worse if it takes every container (PA[7]); Shard Manager's solver with local search (CM[6]);
Firmament's min-cost flow, sub-second until oversubscription pushes it past 400 s (PA[6]) (finding 3).
Equivalence classes, cached state and relaxed randomisation are Borg's (CM[1], `DE`). Optimistic
concurrency on shared state is Omega's (CM[2]) and Gödel's (PA[4a]).

**Evidence.** 8,333 placements/s (`AC-T7.11.2`) sits between the sampled ceilings (20,000-40,000/s) and
the Kubernetes family's ~1,000/s; the sampled shape reaches it with room, the serial Kubernetes shape
does not (finding 2). Scoring 5-50% of nodes, as kube-scheduler does (CM[11]), keeps the per-decision
cost under the 10 ms p99 budget (`AC-T7.4.2`).

**What would falsify it.** The sampled fast path missing the 8,333/s or 10 ms p99 bar on 10,000 nodes
(`AC-T7.11.2`, `AC-T7.4.2`), or sampling stranding more than best-fit recovers (`AC-T7.4.3`) — in which
case the sample widens or the optimiser's reservations cover more of the request path.

### S3. Suitability is hard filters on facts, then soft scores; the workload declares what it needs

**The choice.** A workload's spec declares what it needs, and the scheduler reads it and assigns
(the person's rule, 2026-10-23): the resources (CPUs, memory, devices), whether it needs exclusive CPUs
or a quiet host under a load ceiling, its duration, owner, priority, and the host changes it makes; for
agents, its provider, model, effort, agent identity, permissions, token budget and context-window need.
**Hard filters** remove every placement that can't honour a need — a GPU workload off a node whose
(node, runtime class) pair exposes no GPU (Firecracker mainline exposes none, AW[21][22][25]); an
interference-sensitive job off a host already running one that changes it (a benchmark never shares a
host with a build); a workload off a provider over its rate limit or a session whose context window is
spent. **Soft scores** then rank the survivors by suitability: proximity (S4), goodput (S1), fit.

**Sources.** Mixed hardware is handled by normalising capacity and filtering on facts everywhere: Borg's
abstract compute unit over 7 platforms and 21 shapes (CM[3]), Twine's 17 host profiles (CM[4]), RAS's
relative resource units over 9 types and 12 subtypes (CM[5]); Kubernetes filters on labels, taints and
device plugins (CM[10]); Gavel normalises throughput across GPU types (PA[11]) (finding 5). For agents,
quota- and token-aware scheduling: HiveMind's header-driven rate tracking and token budgets (AW[13]),
the Zhihu client's deficit round robin with deadlines (AW[14]), Mooncake's early rejection under
overload (AW[30]); a GPU is a fact of the (node, class) pair (AW[21]). The declare-and-assign contract
is the person's requirement (`hex-scheduler-requirements`), the resolution of the fedora bench clash
where agents messaged each other to avoid co-scheduling a benchmark with a build.

**Evidence.** Hard filters on capability facts are how every production manager keeps GPU and device
work correct (finding 5); the quota-aware agent schedulers cut failure rates from 72-100% to 0-18%
(HiveMind, mock AW[13]) and reach 100% completion at their SLO rate (Zhihu, mock AW[14]). `AC-T7.6.3`
(zero sessions on a machine over 90% CPU when another has room) and `AC-T7.7.2` (zero ungranted flows)
are the filter's correctness, measured by hex-sim and a property test.

**What would falsify it.** A placement in hex-sim that puts GPU work on a non-GPU node, co-schedules a
declared interference-sensitive job with a host-changing one, or admits a workload past a provider's
declared rate limit — any is a filter defect, fixed at its cause (CLAUDE.md §4). Provider-quota
scheduling has been evaluated only against mock providers (AW[13][14][15], finding; gap), so hex-sim
models the quotas and the competitive baseline measures them.

### S4. Network proximity is Vivaldi coordinates carried on hyper-swim

**The choice.** The scheduler scores proximity — latency and bandwidth between a workload and the data,
services and peers it talks to — from Vivaldi synthetic coordinates, built on hyper-raft's `hyper-swim`
(`coordinates.rs`, the Dabek et al. SIGCOMM 2004 update rule) rather than a new coordinate system. SWIM
membership and Lifeguard suspicion (`extension.rs`) and the failure detector (`detector.rs`) are the
same substrate (S8). Proximity is a soft score with a confidence weight, never a hard filter, so a
high-error or stale coordinate degrades the ranking rather than excluding a feasible placement.

**Sources.** Vivaldi predicts RTT at 11% median relative error over 1,740 Internet hosts (PF[1], `MV`);
Azureus runs it in the wild on a million-node network (PF[2], `MV`) despite 83% of pairs violating the
triangle inequality (PF[2], `MI`), which the height vector and filters absorb. Consul sorts by estimated
RTT (PF[7]); Hyperscale's gate blends a quality-weighted RTT upper bound with observed latency and
health buckets (PF[34][35]). hyper-swim already carries the coordinates, SWIM and Lifeguard
(`hex-scheduler-requirements`; PF[3][5][6]).

**Evidence.** Proximity-aware placement has two production precedents (Consul, Hyperscale) and neither
measured how much it improves outcomes, and hyper-swim's accuracy on a real WAN is unknown (finding 9;
gap PF[6][7][34]). So this decision's benefit is **unproven until hex-sim measures it**: the simulator
models inter-node latency and the scheduler's proximity term, and reports the goodput and tail it buys,
against placement that ignores proximity.

**What would falsify it.** Proximity-aware placement not improving goodput or the latency tail over
proximity-blind placement in hex-sim on a modelled WAN — in which case proximity drops to a tie-breaker.
Or hyper-swim's coordinates proving too inaccurate on a real WAN (the open gap), in which case the score
leans on observed latency, as Hyperscale's does.

### S5. Priority bands, quota with fair share, gangs, and preemption that keeps disruption budgets

**The choice.** Admission is quota with cohorts and borrowing; among admitted work, priority bands order
it; fair share (DRF over dominant resources) divides contested capacity between owners; gangs are
admitted whole or not at all; preemption reclaims capacity for higher-priority work but never breaks a
disruption budget. Every pending workload says, in words, why it waits (`AC-T7.4.1`).

**Sources.** Production prioritisation is admission quota plus priority bands plus preemption (Borg
CM[1], Twine entitlements CM[4], RAS reservations CM[5], Kubernetes PriorityClass with Kueue CM[25][20],
finding 4). Fair share is DRF's max-min dominant share (PA[18], `MV`, larger jobs up to 66% faster than
slot fairness) with Kueue's cohorts and borrowing (PA[20]). Gangs are Volcano's PodGroup, admitted whole
(PA[21]). Preemption priced against disruption is Quincy's (PA[5]) and Borg's disruption-rate and
simultaneous-down limits (CM[1], PF[37]).

**Evidence.** `AC-T7.5.2` (4 equal-weight repositories within 2% of 25% under contention) is DRF's
guarantee, measured by hex-sim over a recorded day; `AC-T7.5.3` (zero disruption budgets broken by
preemption) is a property test; `T-T7.5.1` (a 1,000-leg burst from one repository leaves the others'
legs starting within their usual p95) is the fair-share isolation. Matrix jobs admitted whole
(`AC-T7.5.1`) is gang admission.

**What would falsify it.** Any of the fairness, gang or disruption-budget bars failing in hex-sim or its
property test, or a priority inversion in which low-priority work starves a higher band — fixed at its
cause.

### S6. Correctness is an invariant checked against a serial specification

**The choice.** The scheduler's placement is defined by a serial specification — one scheduler, one
cycle — and the real parallel, sharded, late-binding scheduler must produce placements a model-based
test proves equal to the specification's on generated clusters and workloads (`T-T7.4.1`). Desired and
observed state live in Amphora; a placement is committed by optimistic concurrency on the workload's
record, so a lost commit is a conflict retried, never a double start. Across 10,000 crash seeds of the
control plane with the nemesis, workloads lost or started twice is exactly 0 (`AC-T7.3.3`).

**Sources.** Omega's optimistic commit on shared state (CM[2], `DE`); Gödel's one binder with optimistic
concurrency (PA[4a]); the model-based and crash-injection test kinds (CLAUDE.md §6). The split of desired
and observed state with reconcilers on change feeds is the control plane's (CP-T7.3, README).

**Evidence.** No competitor publishes a correctness proof of this kind (the comparison measures
throughput and tails, finding 1); Hex's bar is the zero in `AC-T7.3.3` and `AC-T7.4.1`'s model
equivalence, both measured, not cited.

**What would falsify it.** A crash seed that loses or doubles a workload, or a generated cluster where
the parallel scheduler's placement differs from the serial specification's without a stated,
concurrency-forced reason — either is a defect fixed at its cause (CLAUDE.md §4).

### S7. One code path from a laptop to a cell to a region

**The choice.** The same scheduler runs on one node, on a cell of thousands, and across regions; each
step up in scale adds only the choice it can't avoid (CLAUDE.md §8). The control plane is sharded over
Amphora's groups; a cell is a failure domain with its own control plane; regions are independent control
planes that re-place a lost region's work within the others' capacity. The differential suite passes on
one node, one cell and three simulated regions (`AC-T7.12.3`).

**Sources.** Sharding the control plane by domain is Twine's (largest shard ~170K machines, 1M/region a
target not reached, CM[4]) and RAS's (millions of servers, thousands of reservations, CM[5]). Cells as
failure domains with independent control planes are Omega's split and Borg's cell model (CM[1][2]).
Multi-cluster selection is Karmada's (CM[41], P99 cluster-scheduling 32 ms on 100 simulated clusters,
`MV` on fake nodes). The one-code-path rule and the differential suite are CLAUDE.md §8 and `AC-T7.12.3`.

**Evidence.** The largest measured control-plane domains are ~170K machines per Twine shard and 130,000
nodes for GKE experimental (CM[4][38]); `AC-T7.11.2` (1,000,000 workloads on 10,000 nodes) is within one
sharded cell's reach by these, and `T-T7.11.1` (losing a shard at 1,000,000 workloads, its share taken
over within 5 s) is the shard-failure bound.

**What would falsify it.** The differential suite diverging across one node, one cell and three regions
(`AC-T7.12.3`), or a cell's control-plane loss disturbing another cell's workloads (`AC-T7.12.2`, the
cell-loss drill) — either breaks the failure-domain guarantee.

### S8. Bad workloads and nodes are found by peers and requesters, graded, and isolated fast

**The choice.** Failure is detected not by heartbeats alone but by the views of peers and requesters,
graded into a continuous suspicion, and acted on with escalating, bounded, forgiving penalties:
- **Membership and crash** on hyper-swim: SWIM with Lifeguard's local-health suspicion (`extension.rs`),
  so a slow member doubts itself rather than accusing others.
- **Gray and fail-slow** by correlation: a node whose work is slow for its requesters and peers, not for
  its heartbeat, is suspected (Panorama/IASO-style), its suspicion a φ-accrual level the consumer
  thresholds.
- **Antagonists** by correlation, throttled with a cap (CPI²-style): a workload whose neighbours' CPI
  rises is throttled, not killed, for a bounded window.
- **Repeat offenders** escalated with a forgiveness window (CrashLoopBackOff, Envoy ejection), the
  ejection capped so a correlated fault can't drain a cell.
- **Mass loss** rate-limited, because a mass failure looks like a partition (Borg); retries budgeted,
  because sustained retries drove more than half of 22 metastable outages (PF[20], `MI`).
- **Agent-specific** faults — a stuck tool loop, quota exhaustion that looks like slowness, a partial
  provider outage — are modelled and detected, since no failure-detection study covers them (gap).

**Sources.** Lifeguard cuts slow-member false positives 50-100x (PF[9], `MV`); Panorama detected 15 of
15 gray failures in <7 s and IASO isolates a slow node within minutes across 39,000 nodes (PF[12][13],
`MV`); φ-accrual grades suspicion (PF[25]); CPI² throttles antagonists at Google scale (PF[16], `MV`);
CrashLoopBackOff and Envoy ejection escalate with forgiveness and a cap (PF[17][18]); Google's retry
budget limits growth to 1.1x (PF[22]); Borg rate-limits reactions to mass loss (CM[1]) (findings 6, 7).
The substrate is hyper-swim (`hex-scheduler-requirements`; PF[3][5][6][9]).

**Evidence.** Gray failure is caught by peers and requesters, not heartbeats (finding 7); the agent
failure modes are in none of the studies (gap, comparison §6), so hex-sim injects them with a nemesis
and measures detection and isolation time. `AC-T7.10.3` (a node whose disk stalls is marked and drained
before its work fails) is the fail-slow drill.

**What would falsify it.** A bad workload or node not deprioritised and isolated within its stated bound
in hex-sim's nemesis, a slow member's self-doubt still accusing healthy peers, or a correlated fault
draining a cell past the ejection cap — each fixed at its cause.

## Where a bar must be measured, not cited

The comparison's gaps (comparison §6) are the axes where no competitor's number exists to beat, so Hex
measures its own rather than citing one:

- **A1. No p99.9 or maximum placement latency is published** anywhere; the best tails are p95 or p99 on
  the authors' own systems (finding 1). Hex measures its p99.9 and maximum in hex-sim (`AC-T7.4.2` sets
  the p99; the p99.9 and max are recorded beside it).
- **A2. Network proximity's benefit is unmeasured** (finding 9, S4): neither Consul nor Hyperscale
  reports how much latency-aware placement improves outcomes, and hyper-swim's WAN accuracy is unknown.
  hex-sim measures it; until then proximity is a soft score that can only help the ranking (S4).
- **A3. No scheduler is evaluated against real provider quotas at agent scale** (finding; AW[13][14][15]
  are all mock). hex-sim models provider quotas and the competitive baseline measures Hex against them.
- **A4. Agent-specific failure modes** — stuck tool loops, quota exhaustion that reads as slowness,
  partial provider outages — are in no failure-detection study (gap). hex-sim's nemesis injects them.

## hex-sim: the measurement vehicle, built first

Almost every bar above is "measured by hex-sim" (`docs/requirements.toml`). hex-sim is a simulated
cluster of fake nodes, as kwok is for Kubernetes (`AC-T7.11.3` compares against kwok), with:
- **Deterministic simulation** of placement, time, inter-node latency (for S4) and provider quotas (for
  A3), so a run is reproducible and a regression is a changed number, not noise.
- **A nemesis** that injects crashes at every control-plane write (`AC-T7.3.3`), gray and fail-slow
  faults, mass loss, and the agent-specific faults (A4), so S6 and S8 are measured, not asserted.
- **Accounting** of control-plane memory per workload (`AC-T7.11.4`) and placement throughput and tail
  (`AC-T7.4.2`, `AC-T7.11.2`).
- **The serial specification** as the oracle for the model-based equivalence test (`T-T7.4.1`, S6).

hex-sim runs one node, one cell and three simulated regions on one code path (`AC-T7.12.3`, S7), so the
differential suite is the same at every scale.

## Implementation goals

They go on the goal stack after this document, hex-sim first, each done only when hex-sim (or the named
bench) shows its number of record:

0. **hex-sim:** the simulated cluster, the nemesis, the accounting and the serial specification. Done
   when it runs 10,000 fake nodes deterministically, injects a crash at every control-plane write, and
   the model-based equivalence test (`T-T7.4.1`) passes on generated clusters.
1. **The control plane (CP-T7.3):** desired and observed state in Amphora, reconcilers on change feeds,
   optimistic-concurrency commit. Done when `AC-T7.3.3` (lost-or-double = 0 over 10,000 crash seeds) and
   `AC-T7.3.4` (a restart leaves running workloads running) pass in hex-sim.
2. **The fast path (CP-T7.4, S1-S3):** sampled filter-and-score over equivalence classes, late binding,
   goodput scoring, the declare-and-assign contract. Done when `AC-T7.4.2` (p99 ≤10 ms at 10,000/s on
   10,000 nodes) and `AC-T7.4.3` (stranded ≥30% below best-fit) pass in hex-sim.
3. **Proximity (S4):** the hyper-swim Vivaldi score. Done when hex-sim shows proximity-aware placement's
   goodput and latency-tail gain over proximity-blind on a modelled WAN (A2), or the falsifier drops it
   to a tie-breaker.
4. **Priority, fair share, gangs, preemption (CP-T7.5, S5):** done when `AC-T7.5.2` (4 repos within 2%
   of 25%), `AC-T7.5.3` (disruption breaches = 0) and `T-T7.5.1` pass.
5. **Failure detection and isolation (S8):** Lifeguard membership, gray/fail-slow by correlation,
   antagonist throttling, retry budgets, the agent-specific faults. Done when hex-sim's nemesis shows
   each bad workload or node deprioritised and isolated within its bound, and `AC-T7.10.3` (disk-stall
   drain) passes.
6. **Scale and shards (CP-T7.11):** done when `AC-T7.11.2` (1,000,000 on 10,000 in ≤120 s), `AC-T7.11.3`
   (≥10x kube-scheduler against kwok), `AC-T7.11.4` (≤4 KiB/workload) and `T-T7.11.1` (shard loss) pass.
7. **Cells and regions (CP-T7.12, S7):** done when the differential suite passes on one node, one cell
   and three simulated regions (`AC-T7.12.3`) and the cell-loss and region-loss drills pass.
