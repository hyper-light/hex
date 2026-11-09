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
goodput systems that beat them — 30-93% lower average job completion time (Sia PA[10]; Pollux's own is
37-50% PA[12]), 2.2-3.25x fairer worst cases (Themis PA[13]), 4.0x fewer SLO misses (3Sigma PA[15]),
all `MV` — are research-scale (44-256 GPUs or nodes). Hex takes their objective to the production scale
the cluster managers reach (comparison finding 4).

## The bar

The scheduler's own numbers (`docs/requirements.toml`), with where the research places them:

| Axis | Bar | Where it sits |
|---|---|---|
| placement throughput | 1,000,000 workloads on 10,000 nodes in ≤120 s = 8,333/s (`AC-T7.11.2`) | between the sampled placers (Hydra 40,000/s PA[3], Apollo 20,000/s PA[2]) and the Kubernetes family (GKE ≤1,000/s CM[38], kube-scheduler ~790/s CM[30]) (finding 2) |
| vs kube-scheduler | ≥10x on the same simulated cluster (`AC-T7.11.3`, against kwok) | kube-scheduler's own floor is ~790 pods/s CM[30] (GKE's ~1,000/s CM[38] is its Kueue-backed control plane, not the default scheduler); 10x is ~7,900/s, the same line |
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

**Evidence.** The goodput objective reports 30-93% lower average JCT (Sia; Pollux 37-50%), 4.0x fewer
SLO misses (3Sigma) and 2.2-3.25x fairer worst cases (Themis) than balance- or priority-only baselines
on 44 to 256 GPUs or nodes (finding 4). Best-fit packing alone strands capacity that a best+worst-fit mix recovers
(Borg CM[1]); `AC-T7.4.3` holds Hex to ≥30% less stranded capacity than best-fit at 90% load.

**What would falsify it.** A balance-only or best-fit-only scorer matching the goodput objective on the
recorded workload of OCI containers, Firecracker microVMs and Shards microVMs in hex-sim (`AC-T7.4.3`), or
the goodput model costing more to evaluate than the
10 ms p99 budget allows at 10,000/s (`AC-T7.4.2`) — in which case goodput moves to the optimiser tier
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

**The choice.** The scheduler scores **latency** proximity — the RTT between a workload and the data,
services and peers it talks to — from Vivaldi synthetic coordinates, built on hyper-raft's `hyper-swim`
(`coordinates.rs`, the Dabek et al. SIGCOMM 2004 update rule) rather than a new coordinate system. SWIM
membership and Lifeguard suspicion (`extension.rs`) and the failure detector (`detector.rs`) are the
same substrate (S8). Proximity is a soft score with a confidence weight, never a hard filter, so a
high-error or stale coordinate degrades the ranking rather than excluding a feasible placement.
**Bandwidth is not from the coordinates:** Vivaldi and its kin model latency only, and no source read
predicts bandwidth from coordinates (`proximity-and-failure-containment.md` §1.6, PF[1][2]). Where the
objective's bandwidth term matters, it comes from the topology and measured transfers, not the
coordinate space; that mechanism is unresearched here and is named as a gap (A2 below), not claimed.

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

**Measured (G3, `bench/sched/proximity.md`, `hex-sim.md` H20-H21).** G2's recorded day was run as it
is, 25,295 launches of Hex's three kinds (OCI containers, Firecracker microVMs and Shards microVMs), on
both of H10's node models. The WAN was Azure's inter-region table with every zone pair at Azure's 2 ms
bound, and hyper-swim's coordinates were learned on its own probe schedule. On 118 studies (59 seeds
for each node model, the number the claim "at least 95% of seeds, at 95% confidence" needs),
proximity-scored placement beat proximity-blind best fit on the latency tail for every launch and for
each kind. The p99 completion time was at least 5.1% lower over all launches, and the p50 at least 91%
lower. Goodput did not move: at the day's own load nothing queues, so every launch finishes within the
day under either policy. The falsifier below was not met, so proximity stays a soft score: the plain
predicted RTT, with no widening and no tie-break policy (H21 gives the measured reasons). The claim
covers inter-region and inter-zone structure and the recorded load. Shards' overhead in that run is a
5 MiB stand-in.

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
throughput and tails, finding 1); Hex's bar is the zero in `AC-T7.3.3` and `T-T7.4.1`'s model
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

The cell's sharding, its shard-loss takeover, its memory accounting and its scale benches are decided in
`hex-sim.md`:
- H60: shards as one Plane per contiguous node slice, with workloads routed by a pure function of
  spec and cluster; `slices = 1` is today's plane.
- H61: lease takeover. Renewed once per recorded store write round trip (Amphora's K0 p99.9, 22.27 ms),
  the lease ten of them, etcd's rule; the rebuild from the store is measured within `T-T7.11.1`'s 5 s.
- H62: a counting allocator over the store and the planes (`AC-T7.11.4`).
- H63: the 1,000,000-placement bench and the kwok comparison method, with kube-scheduler at its default
  and raised QPS, on the same node and pod shapes (`AC-T7.11.2`, `AC-T7.11.3`).

Cells and regions are decided there too:
- H64: a federation over independent cells, each today's hex-sim run with its own store and shards;
  workloads routed to a home region, then a cell, by pure functions; one cell is `sim::run` bit for bit.
- H65: the cell-loss drill (`AC-T7.12.2`), measured as every other cell's run unchanged.
- H66: a lost region's work moved only where a surviving region's live free capacity holds it, after
  deducting what is pending there; the rest refused, typed (`T-T7.12.1`).
- H67: the differential suite on one node, one cell and three regions (`AC-T7.12.3`).
- H68: cell selection on 100 simulated cells against Karmada's P99 32 ms.
- H69: all of G6 and G7 over Hex's three classes: each workload an OCI container, a Firecracker or a
  Shards microVM by owner (G2's rule), and each node, cell and region declaring the classes it runs.

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
  a node ejected only where the cell re-places its work, so a correlated fault can't drain a cell.
- **Mass loss** rate-limited, because a mass failure looks like a partition (Borg); retries budgeted,
  because sustained retries drove more than half of 22 metastable outages (PF[20], `MI`).
- **Agent-specific** faults — a stuck tool loop, quota exhaustion that looks like slowness, a partial
  provider outage — are modelled and detected, since no failure-detection study covers them (gap).
- **Dangerous** workloads — one that trips a runtime security signal (a seccomp denial, an unauthorized
  egress attempt, a privilege escalation) — are isolated by runtime class: the workload moves to a
  stronger class (`oci` → a microVM) or is refused, not merely deprioritised. Its network egress is
  already bounded at admission (S3, `AC-T7.7.2`, `T-T7.7.1`); the runtime-signal-driven move is a design
  the research only sketches (`proximity-and-failure-containment.md` §4 lessons; security signals are
  flagged unresearched there, §6), so what counts as a trip and how fast the move is are a gap hex-sim
  and CP-T7.7's networking lab measure, not a cited bound.

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

**What would falsify it.** A bad workload or node not deprioritised and isolated within the bound hex-sim
sets for its fault class (implementation goal 5 states each: crash, gray/fail-slow, antagonist,
agent-specific, dangerous; `AC-T7.10.3`'s disk-stall "before its work fails" is the one already fixed),
a slow member's self-doubt still accusing healthy peers, or a correlated fault ejecting a node whose
work the cell can't re-place — each fixed at its cause.

## Where a bar must be measured, not cited

The comparison's gaps (comparison §6) are the axes where no competitor's number exists to beat, so Hex
measures its own rather than citing one:

- **A1. No p99.9 or maximum placement latency is published** anywhere; the best tails are p95 or p99 on
  the authors' own systems (finding 1). Hex measures its p99.9 and maximum in hex-sim (`AC-T7.4.2` sets
  the p99; the p99.9 and max are recorded beside it).
- **A2. Network proximity's benefit is unmeasured, and bandwidth has no model** (finding 9, S4):
  neither Consul nor Hyperscale reports how much latency-aware placement improves outcomes, and
  hyper-swim's WAN accuracy is unknown; hex-sim measured the latency term's benefit on a modelled WAN
  (G3, `bench/sched/proximity.md`: goodput +0.9-18.6%, p99 5.7-93.7% lower than blind on every
  study), so it stays a soft score (S4); its accuracy on a real WAN remains open. Bandwidth-aware placement is a further gap — Vivaldi models
  latency only (`proximity-and-failure-containment.md` §1.6, PF[1][2]), so the objective's bandwidth
  term must come from topology and measured transfers, which no source read here evaluates; hex-sim
  models it and the design leaves it a soft score until measured.
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
   antagonist throttling, retry budgets, the agent-specific faults, and the dangerous-workload move to a
   stronger runtime class. Done when hex-sim's nemesis states and meets a detection-and-isolation bound
   for each fault class (crash, gray/fail-slow, antagonist, agent-specific, dangerous) and `AC-T7.10.3`
   (disk-stall drain) passes. Built on `cp/t7-sched-g5` (`docs/design/failure-isolation.md`, F1-F8):
   every bound met over 1,000 seeds and the drill drained before work failed. The crash class is
   measured on hyper-swim's own detector, vendored and driven by hex-sim, with memberlist's protocol
   modelled beside it as the competitor; the node agent runs no membership yet.
6. **Scale and shards (CP-T7.11):** done when `AC-T7.11.2` (1,000,000 on 10,000 in ≤120 s), `AC-T7.11.3`
   (≥10x kube-scheduler against kwok), `AC-T7.11.4` (≤4 KiB/workload) and `T-T7.11.1` (shard loss) pass.
7. **Cells and regions (CP-T7.12, S7):** done when the differential suite passes on one node, one cell
   and three simulated regions (`AC-T7.12.3`) and the cell-loss and region-loss drills pass.

## Borrowed from kiln's load balancer

kiln's load balancer (the Load Balancing agent's program, LB00 to LB15; kiln agent
`01a198e7a285772298e00399c5c7d5de`) places kiln's own work, CI legs, commands and agent sessions, on
kiln's runners. It is not Hex's scheduler (Ada, 2026-11-06: "Your scheduler is for *microvms*"), but it
met and measured problems G0-G7 (the implementation goals above) meet too, and Ada asked that Hex take
what it can (2026-11-09: "examine our Load Balancing agent's work. We can learn and borrow much"). This
records which of its ideas Hex takes, for which goal, and what each changes for the three kinds Hex
places: OCI containers, Firecracker microVMs and Shards microVMs. Each is a proposal for the goal it
lands in, held to that goal's number of record; listing it here decides nothing. Hex already shares
kiln's method (pure cores, integers on decision paths, planted mutants, an oracle), so what follows is
mechanism.

**Where it is.** kiln at `a63dd98ef` (origin/main, 2026-11-08); paths are under `crates/kiln-ci/src/`
unless they start with `docs/`. The placement kernel is `lb/` (docs/lb.md); the place core `place/`
(docs/reach.md §2.3-2.5, docs/needs.md); node pace `pace/` and learned run times `runtime.rs`
(docs/needs.md §17); the topology `topo/`; the use-based detector `detect.rs`; protection `protect/`
and `headroom/` (docs/protect.md). The agent's measurements and decisions are in its notes on
adas-macbook-pro-2, `~/.cache/kiln-lb/` (`SWARM-STATUS.md`, `notes/hillclimb.md`,
`notes/federation-research.md`). Room tokens, learned wait weights and the auction reach its place
core only on its integration branch, `lb/adopt` at `a193e0229`, unpushed.

**The rules each is held to** (Ada's): no arbitrary constants, only learned or data-derived values (a
cited published number is still (d), as `failure-isolation-constants.md` counts it); latency and
throughput before memory; no legacy kept, so what a borrowing replaces goes; no switches but test
controls. kiln turns several mechanisms on by a person's setting (`[reach] tokens`, `learned_weights`,
`auction`, Headroom's `false_alarms_per_day`); Hex takes a mechanism unconditionally once its goal
measures it better, or not at all, the comparison a test control. kiln's own comparisons at scale
(tokens against hints, the auction against one at a time, learned weights against two choices,
fleet-wide fair share against its oracle on random workloads) were still owed at its checkpoint of
2026-11-08 (`SWARM-STATUS.md`), so nothing below rests on them.

| | Idea | Lands in |
|---|---|---|
| B1 | The whole class's best in O(1), by bitmaps over buckets, in place of H8's sample | G2, G6 |
| B2 | Parallel deciders deduct at once and widen their pick by the herd they should expect | G2 |
| B3 | The node admits too, against its measured room, and says its room when it changes | G1, G2 |
| B4 | Rank by expected finish on each node's learned pace, folded into the key | G0, G2 |
| B5 | Explain slowness before accusing: grade a node's residual, not its raw slowdown | G0, G5 |
| B6 | Detection per observation, its reference from the node's own calm | G5 |
| B7 | The disk probe's period and share derived from what it protects | G5 |
| B8 | Penalties from measured times, not Envoy's and Kubernetes' | G5 |
| B9 | Detection on use, beside hyper-swim | G1, G5 |
| B10 | Measured-first latency, bandwidth from Hex's own transfers, links' stability | G3, G7 |
| B11 | One store write per batch, published once it lands | G1, G6 |
| B12 | An integer auction for gangs and batches, its ε derived | G2 (S2's optimiser tier), G4 |
| B13 | Fair share under constraints: an exact oracle, and usage carried between planes | G4, G6 |
| B14 | The first queue's head keeps room; later work backfills around it | G4 |
| B15 | A lost shard's slices adopted by rendezvous rank | G6, G7 |
| B16 | Quotas as escrowed rights across cells and regions | G7, A3 |

### B1. The whole class's best in O(1), in place of H8's sample (G2, G6)

**kiln's.** Each signature (an equivalence class of needs) keeps its nodes in buckets by a rank key,
free whole cores (the unit the hardware schedules): a bitset per bucket over node ids, interleaved by
word so a node's move is two bit writes; a summary word per 4,096 nodes; a bitmap of non-empty
buckets, the top two leading-zeros counts away. The key is a multiply and a shift,
`(view × scale) >> 40`; other dimensions are checked exactly at the pick, nodes passed counted; equals
take turns from a cursor; whether a need could ever fit is O(distinct node shapes). A node's record is
32 bytes (four to a 128-byte line), an admitted run's 24, runs are generational slab handles, and a
decision hashes and allocates nothing (`lb/index.rs:1-20, 188, 267-302`; `lb/owner.rs:37-42, 360`;
docs/lb.md:34-69). A bench step, a release and then a pick the node admits, took 10.08 ns at 100 nodes
and 23.02 ns at 10,000 on adacachy2, 10.84 and 18.35 ns on the Mac Studio (`b31682982`, 24
interleaved rounds, `notes/hillclimb.md`).

**Hex's.** H8 scans from a seeded start to the `want`-th admitting node and takes the best fit among
those; `want`'s floors, 100 nodes or 5% (`crates/hex-sim/src/index.rs:138-139`), are kube-scheduler's,
the last (d) on G2's fast path (`hex-sim-constants.md`, on `cp/t7-sched`). With one index per class
(H8's `Demand`) and per set of kinds a node runs, read fewest kinds first (H11's tie), best fit's pick
is the least non-empty bucket at or above the need, one trailing-zeros count over the masked bitmap
(kiln takes the highest); H11's stranding test and allocation tie are checked at each node passed,
the passes counted. The pick is then the whole class's, not a sample's: `Sample`, its floors and the
specification's sample go, and S2's sample becomes this index. A Firecracker microVM's class holds
only the KVM Linux nodes, a Shards microVM's every node that runs Shards (H10), so a kind no node can
hold is known from its classes' node shapes, without a scan. Hex's sampled decide, commit and index
update take p50 9.5-19.0 µs at 10,000 nodes (`bench/sched/fast_path.md`): G2's bench splits that into
its parts before claiming any share of it for the scan, and tunes the index as kiln's hill-climb did,
each idea kept only on an interleaved A/B with its counters (`notes/hillclimb.md`). Falsified by
AC-T7.4.2's p99 rising, or AC-T7.4.3's cut against best fit shrinking, with the walk counted.

### B2. Parallel deciders deduct at once and widen by the expected herd (G2)

**kiln's.** An owner deducts what it sends from its view at once, so its own starts in flight never
herd onto one node (planted `NoDeduct`, caught). Owners that share nodes see the same best and herd on
it, which cursors spread over the fleet (Fibonacci hashing) don't stop. A shared owner widens its pick
to the herd it should expect, the others' measured decision rate times a word's measured delay, plus
one, drawn uniformly from the top buckets down by SplitMix64 (Mitzenmacher, "How Useful Is Old
Information?", TPDS 2000); a sole owner keeps the exact best at no cost (`lb/owner.rs:709-723, 1060`;
docs/lb.md:250-305). Declines per decision at ρ 0.9 fell 7 to 27 times (3.102 to 0.115 at 32 owners),
and to none at ρ 0.5 (`lb/tests.rs:896`).

**Hex's.** H3's deciders decide against one view, and the committer voids a decision whose window
changed (a conflict, counted). With B1 every decider sees the same best node, so conflicts would grow
with deciders. Each decider deducts its decision from its view at once and draws among its class's
best `rate × delay + 1` nodes, the rate the other deciders' measured decisions a second and the delay
its own measured decide-to-commit time, from the run's seeded generator (H1); one decider picks the
exact best. T-T7.4.1's specification states the allowance H4 provides for: any of those best.
Falsified by conflicts per decision not falling at 2-8 deciders, or AC-T7.4.3's stranding rising.

### B3. The node admits too, and says its room when it changes (G1, G2)

**kiln's.** The node that owns the room admits against its own ledger, never past its budget; a
refusal carries its room, so the next pick never repeats the miss and the loop is bounded by the
class's nodes; use the node measures beyond what was placed (`Runner::outside`) is kept off the room
(`lb/runner.rs:1-18, 442`; docs/needs.md:291-296). Every word carries the room, and words to a watcher
coalesce while unsent, the newest replacing the older (backpressure, no timer); a decision reads the
last measure and waits on no read (docs/needs.md:690-721).

**Hex's.** S6's compare-and-set checks a node's capacity against Hex's books, and the node holds what
the books don't: page cache, the host's own processes, Shards' share process, a VMM past its measured
idle overhead (H10). The hexlet admits each start the store's feed shows it against its measured
room, so a guest's memory, which can't be overcommitted, never is; a refusal is a store write carrying
the node's room, which the plane's index takes at once (B1), and the workload is decided again within
its class. The node's state (room outside Hex's books, its pace, B4) reaches the plane when it
changes, read on the OS's events (PSI triggers, power and thermal notifications) and coalesced while
unsent: no state heartbeat, and liveness stays hyper-swim's (`failure-isolation.md` F1). hex-sim's
nemesis moves host memory outside Hex's books to hold it. Falsified by a microVM started past a node's
memory, or a refusal loop longer than its class.

### B4. Rank by expected finish on each node's learned pace (G0, G2)

**kiln's.** A run of work W and parallelism p on a node with f cores free at per-core rate s finishes
in W / (min(p, f) s), plus its I/O at the disk's rate, its fetches at the node's network rate and what
must move to it over the link, less what is warm there, and, for a run a person watches, the link's
latency and expected stall (`lb/owner/terms.rs:387-420`; docs/lb.md:111-241). Speed is learned from
kiln's own history: ln CPU-seconds = kind − machine, least squares over the kinds run on two or more
machines, solved exactly on the machines' Laplacian and shrunk toward the fleet by empirical Bayes, so
a new machine runs no benchmark (`pace/speed.rs:1-46`). Slowness is processor sharing's dilation over
the shares of time lost to CPU, memory, I/O and swap (PSI, or the run queue where there is none), its
weights learned by non-negative least squares from the node's own runs; throttling is speed now
(`pace/slow.rs:1-40`). Run times are Ernest's model per key, each row weighted by its placement's
inverse probability, with a Thompson draw from its posterior in place of an exploration rate
(`runtime.rs:1-36`). Speed and slowness fold into the key's multiplier as a node's word arrives, so a
plain pick stays one lookup; several terms are read from per-rate orders (64 classes an octave) by
Fagin, Lotem and Naor's threshold algorithm, which stops within 1/64 of the soonest, the classes' own
resolution (`lb/owner/terms.rs:125-150, 354`; `lb/order.rs:12-37`); a node not yet measured takes the
measured nodes' mean (empirical Bayes; for a rate, its harmonic mean: `lb/owner/terms.rs:517-540`).
The fits run off the decision path and hand it integers; the kernel's own arithmetic is integer (Q32;
e^-x by `exp_neg_q32`, `lb/owner/terms.rs:611`). Against a brute force on two sets of 20,000 random
fleets the choice was the soonest in 26,984 of 27,340 asks with several terms, never past 1.0155
times it, where the orders' tops alone were 9.7 times it at worst (`lb/finish_tests.rs:305`;
docs/lb.md:469-531).

**Hex's.** S1 scores the goodput a placement adds; H11 packs, and hex-sim's run time doesn't depend on
the node. Expected finish is the cheap proxy S1's falsifier names: a workload's learned work over the
node's learned speed and its slowness now, plus the bytes of its image or snapshot template the node
lacks over the link (B10), less what is warm there: a Firecracker snapshot template or warm pool
(CP-T7.2), a Shards image, an OCI container's layers. A session (H9's role) adds the link's latency and
expected stall; a build or benchmark doesn't, since it runs on while the link is down. Packing breaks
ties within the 1/64, as kiln's design has it (docs/needs.md:246-252: best fit for batch work, most
room for interactive), so H11 still decides among nodes alike in pace, which on m5d.metal is every
node. hex-sim gives its node models speeds and contention (G0), and G2's tests keep kiln's brute-force
oracle and its bound. Falsified on the recorded day, on the fleet's mixed hosts, by goodput or the p99
no better than H11's, or by AC-T7.4.2's p99 passing 10 ms.

### B5. Explain slowness before accusing (G0, G5)

**kiln's.** Pace explains a node's slowness by its hardware, its measured contention, throttling and
swap (B4), and a slowdown is laid on a cause only with evidence it was that cause: the work used that
resource over the span, and a disk idle for part of the span wasn't the bottleneck
(`protect/cause.rs:1-12, 126`; docs/protect.md:71-112).

**Hex's.** `failure-isolation.md` F2 grades a node by its median slowdown, "time over what its class
expects", against its peers' medians, with a picked 2× bar. On a fleet of mixed hosts (H10's) a node
slower by hardware, by measured contention or by throttling has a known slowdown, which B4's key
multiplier already answers, in proportion and at once. F2 grades the residual instead: each workload's
time over what B4 predicts for its kind on that node at the contention measured as it started, so only
unexplained slowness counts toward `Suspect` and `Drain`. A Firecracker guest's vCPUs are host threads,
so the host's PSI explains them; a Shards guest on macOS has the run queue. The same cause rule takes
F4 past CPI: a co-tenant is an antagonist suspect for a resource only if it used that resource over the
span (each microVM's cgroup's PSI). hex-sim's gray nemesis draws node heterogeneity. Falsified by more
healthy nodes suspected, or a 2.5× node caught later, than F2's medians give on the same seeds.

### B6. Detection per observation, its reference from the node's calm (G5)

**kiln's.** A CUSUM on the logarithm of a target's latency steps on each sample; its reference is
halfway from the target's calm, learned only from excursions that ended without harm, to harm (Page,
Biometrika 1954); a sample at harm alarms at once and needs no record; a shift down starts a new regime
(`headroom/detect.rs:1-27`; `protect/stall.rs:1-23`). Its threshold is calibrated against a person's
false-alarm budget.

**Hex's.** F2's window (1 s), minimum count (5) and drain run (3 windows) are (d), and a CUSUM needs
none of them: it steps on each requester's observation of a node's residual (B5), its reference halfway,
in log, from the node's calm to the fault class's 2.5× (the bounds table's definition), and a workload
of that node missing its declared deadline because of it is harm, acted on at once. The threshold is
not kiln's false-alarm budget, which Ada's rule excludes, but Hex's cost rule as F3 prices it: act
once the posterior that the node changed makes waiting cost more than acting (the Bayesian form of the
same test: Shiryaev, "On optimum methods in quickest detection problems", Theory Probab. Appl. 8(1),
1963), a false action priced at the measured re-placement of each kind, waiting at the work blocked.
The same form can replace F4's 2σ, 3-in-5-minutes and 0.35, which stay (d) today. Falsified by F2's or
F4's bounds missed over the nemesis's 1,000 seeds, or more false drains than today's rule.

### B7. The disk probe's period and share derived (G5)

**kiln's.** A probe's interval is the time left to act (the tolerance less the longest a cut took to
show) shared over the samples a shift takes to alarm, plus one; and probes take no more of the target
than would move its latency by the detector's own reference: under M/M/1, δ ≤ (1 − ρ)(1 − e^−k), the
idle share 1 − ρ being the fastest answer over the calm one, so probing can't make the trouble it looks
for (`headroom/detect.rs:270-285, 647-662`; docs/protect.md:118-123). kiln learned why on 2026-10-30:
a 221 s lag had every runner's disk probed back to back, and production's server about 270 times a
second (`headroom/detect.rs:653-654`).

**Hex's.** F3's probe period is its caller's (the drill's 100 ms, (d)), and the drill drained its
healthy disk in warm-up (`0d5fbed2`; this branch's `c4f157dc` ends one of its false drains, not the
other). The period becomes the work's own deadline less the measured re-placement time, over two (one
write outliving the drain age decides, one more to see it start), never taking more of the disk than
kiln's δ; and a write slow while the device sat idle for part of the span (its busy time, from the
OS's counters) is no disk stall but F2's (B5). Falsified by the dm-delay drill missing its deadline at
the derived period, or the probe's own load moving the disk's healthy latency past its reference.

### B8. Penalties from measured times (G5)

**kiln's.** A reaction's budget is the deadline less calm, spent in four measured parts: sensing,
deciding, acting and draining what was already issued (Little: L/λ) (`protect/budget.rs:1-6`;
docs/protect.md:57-69). A budget cut by trouble rises again only after it held calm for the slowest
target's detection delay plus the longest a cut took to show, and budgets move by bisection and
doubling (Bentley and Yao 1976) (docs/protect.md:127-130, 296).

**Hex's.** F6's ejection (30 s to 300 s: Envoy's), eviction rates (0.1 and 0.01 nodes a second; 55%,
3 and 50: Kubernetes') and crash-loop backoff (10 s, 300 s, 600 s) are (d), each with an "(a)" way out
(`failure-isolation-constants.md`); kiln's rules give those ways out their form. An ejection lasts its
detector's own detection delay plus the measured time from a drain to its effect, times the node's
consecutive ejections (Envoy's form, its base now measured), and the node returns only once it has
held healthy that long. Work moves off failed nodes at the cell's measured re-placement throughput, so
what it queues drains within the reaction budget. A crash-looping workload's first delay is its own
measured start (an OCI start, a Firecracker restore, a Shards boot), doubling from there as
CrashLoopBackOff's does. Falsified by F6's correlated-fault runs stranding work, or by nodes ejected
and returned more often than under Envoy's numbers on the same seeds.

### B9. Detection on use, beside hyper-swim (G1, G5)

**kiln's.** A peer is suspected only when something is owed on its link: the link dropped (the
transport's verdict), the transport's timeout on unacknowledged data, an answer overdue, or a peer's
report of its own failed traffic (Panorama); a link owing nothing is never probed, and a wrong
suspicion costs a redundant claim, never a second start (Chandra and Toueg) (`detect.rs:1-29, 84-145`).

**Hex's.** A hexlet owes the plane an acknowledgement of each start the feed shows it. One outstanding
longer than any healthy acknowledgement that node has given (F3's rule for `Suspect`, not kiln's RFC
6298 timer) makes the node `Suspect` at once, so no more work goes to it, before hyper-swim condemns it
(F1: 1.62 s at p99 at need, 12.1 s at a probe a second); its work moves only on condemnation, so
nothing starts twice. hyper-swim stays: a long-running microVM owes nothing for hours, and only
membership sees its node die. Falsified by nodes suspected on use whose acknowledgements were only
queued, counted in hex-sim's crash class with work arriving.

### B10. Measured-first latency, bandwidth from Hex's own transfers, links' stability (G3, G7)

**kiln's.** A pair's latency is, in order of what it rests on, its own filtered round trip, then its
relayed legs plus the relay's measured time, then coordinates with their announced error
(`topo/estimate.rs:1-16, 72-97`). A pair's rate is Hockney's `a + n/r`, least squares on its own
transfers; a pair never seen is bounded by its ends' access rates (the smaller of the sender's most
seen up and the receiver's most seen down), an upper bound, since about half of Internet paths are
bottlenecked inside the network (Akella et al., IMC 2003), tightened by a measured pair between the
same two sites; sites are single-linkage clusters in coordinate space cut where a gap passes the
estimates' own error (Zahn 1971) (`topo/rate.rs:1-32, 265`; `topo/sites.rs:1-10`). A link's drops are
Poisson over its time up (none seen: the rule of three), and a run of length T expects to lose (1 −
e^(−λT)) times the mean outage (`topo/stability.rs:1-18`). All from traffic that already flows, in
integers that agree on x86-64 and arm64; a round-trip estimate costs 6.5 ns and a never-seen pair's rate
5.5 ns (docs/reach.md:212-273).

**Hex's.** G3 places by hyper-swim's predicted RTT, a node 0 from itself (H21, on `cp/t7-sched-g3`);
measured-first generalises that fix: every pair Hex has its own round trips for takes them, and
coordinates serve only pairs never measured. A2's bandwidth term gets a model: an image's layers from
Reliquary, a Firecracker snapshot template or a volume from Slates moved to a node cost B4 their bytes
over the pair's learned rate, edge-bounded where never seen; a session pays the link's expected stall,
a build doesn't. Not taken: He, Dovrolis and Ammar's window and thresholds (10, 0.4, 0.3) and Ledlie's
25th percentile of four, which kiln uses: published numbers, (d) by Hex's audit; a level shift is
B6's change test on the fit's residuals. hyper-swim's engine stays vendored unchanged (H20), so kiln's
coordinate refinements are findings for its owner. kiln's failure domains, learned from links that drop
together (`topo/domains.rs:1-18`), serve G7 beside the declared cells once their test's level (a
person's 5% in kiln) is priced by the cost rule. Falsified by measured-first and the transfer term not
improving goodput or the tail over H21's RTT-only proximity on its study.

### B11. One store write per batch, published once it lands (G1, G6)

**kiln's.** A node commits once for each batch of inputs it handles, the rows as they are now, and
publishes nothing a batch gates until it lands; a refused commit holds what it gates and is tried again
with the next; a row written twice is written once (docs/reach.md:169-211; `b9c027b19`, `3d098ad7b`).
One at a time, placement cost two forced commits a decision, 4.5 ms median each (p99 8.6 ms, fedora-2);
a burst of 1,000 asks committed in 204 ms in all (the LB lead's record, kiln conversation
`agent:01a198e7a285772298e00399c5c7d5de`, 2026-11-08).

**Hex's.** H3's committer writes each placement to the store alone. At Amphora's recorded durable write
(`hex-sim.md` H61, on `cp/t7-sched-g6`: p99.9 22.27 ms) that holds a plane to about 45 placements a
second; AC-T7.11.2's 8,333 a second needs about 186 placements a write on one plane, 23 on each of
H60's eight. The committer writes each batch it holds as one store write, every placement in it still
checked by its record's version and its node's capacity (as `Store::apply_gang` writes a gang,
`scheduler-fairness.md` F3, on `cp/t7-sched-g4`); the feed being the store's, hexlets see none of it
until it lands. A refused write is made again with its conflicting decisions decided again (H3), and
H2's nemesis crashes at each batch write, both sides. Falsified by H63's million placements missing
120 s at Amphora's recorded write time, or a crash seed losing or doubling a workload inside a batch.

### B12. An integer auction for gangs and batches (G2's optimiser tier, G4)

**kiln's.** Runs placed together go to the least total cost by Bertsekas's forward auction, costs
scaled by min(n, m) + 1 and ε = 1, so ε < 1/n and < 1/m and the assignment is optimal with ε derived,
not chosen; a node's slots are one similarity class, so bidders don't raise identical slots one ε at a
time; feasibility is found first by augmenting paths, so the auction ends; a gang's places are all of
them or none (`lb/auction.rs:1-24, 92-107`; `lb/owner.rs:2424-2437`). Checked against every assignment
on 400 cases, a price war planted and caught (docs/lb.md:502-503).

**Hex's.** `scheduler-fairness.md` F3 places a gang member by member in order, each taking its room
before the next is sought (greedy), then writes all or none; S2's optimiser tier is unbuilt. The
auction gives a gang's microVMs their nodes at the least total cost (B4's expected finish, or H11's
stranding as a cost) in one pass, then F3's single write; and it is the optimiser tier's first solver,
for a batch of arrivals of one class, as H63's million pending of one shape are. Falsified on F3's
contended day by gangs starting no sooner, or stranding no less, than in order, or by the auction's
time at the recorded gang sizes (2 to 12) passing the decision budget.

### B13. Fair share under constraints: an exact oracle, and usage carried between planes (G4, G6)

**kiln's.** Its fair-share tests measure against Choosy's constrained max-min fair shares, exact in
integers by Fujishige's lexicographically optimal base of the polymatroid whose rank of a set of
tenants is the capacity of the nodes any of them may use (`place/fair_tests.rs:1-20, 457-469`). With
no center, each word an owner sends anyway carries a tenant's usage elsewhere since its last word there
(DSFQ's delay, Wang and Merchant, FAST 2007; PS-DSF's per-server count), and the node gives freed room
to the waiting tenant that got least across the fleet, Choosy's rule (`place/msg.rs:494-520`;
`place/runner.rs:1224-1235`); held within one run of the oracle, its planted mistakes caught.

**Hex's.** AC-T7.5.2's 25% each is the fair share only where every owner may use every node. Under
H9's kind filters on the fleet's hosts (Firecracker only on Linux, macOS only Shards, a kind per owner
by H10's rule) an owner's share is its constrained max-min share, which the decomposition gives
exactly: G4's fairness test takes it as its oracle on the recorded day. With H60's slices each plane
ranks owners by weighted DRF (`scheduler-fairness.md` F1) over shares no plane sees whole; each
plane's per-owner usage, carried as a delta on writes it makes anyway, gives the others the cell's
shares without a shared counter on the decision path, to within DSFQ's bound on what isn't yet
carried. Falsified by held shares missing the constrained shares by more than AC-T7.5.2's 2%, on one
plane or on eight.

### B14. The first queue's head keeps room; later work backfills around it (G4)

**kiln's.** A start passes the first in line only in room the first can't use (EASY backfilling's
guarantee, Mu'alem and Feitelson, TPDS 2001); with expected ends, the first keeps room on the node
where room frees soonest, each run there ending by its p90, else its median, else its timeout, and once
past that by its start plus twice what it has run; a gang keeps room on all its nodes from the latest
(`lb/runner.rs:1-11`; docs/needs.md:253-263; planted `NoKeep`, the first starving, caught).

**Hex's.** `scheduler-fairness.md` F2 stops all placement at the first queue whose head fits nowhere;
its falsifier names "a reservation for the first queue's head (Borg-style) with backfill elsewhere"
should strict order cost more goodput than fairness is worth, GPU work the case to watch. kiln's rule is
that reservation, built and tested, its time from B4's learned run lengths: a large Shards guest or a
GPU OCI container at the lowest share keeps its room while small starts fill around it. Falsified by
idle capacity while a head waits not falling, or a head waiting past its reservation plus what ran over
before it.

### B15. A lost shard's slices adopted by rendezvous rank (G6, G7)

**kiln's.** The holder of a gone owner's news ranks the live nodes by rendezvous hashing on the run's
handle (Thaler and Ravishankar, ToN 1998): every node works the order out alone, and only the gone
owner's runs move; safety is per-run Paxos under integer ballots, never agreement on who is alive
(`place/handoff/mod.rs:1-35, 106-141`; `b39559d83`). A bounded-load variant (Mirrokni, Thorup and
Zadimoghaddam, SODA 2018) keeps one adopter from taking too much (`notes/federation-research.md` Q3).

**Hex's.** H61's live shards take a lapsed slice by compare-and-set on its lease, all of them racing.
By rendezvous rank over the live shards on the slice's id, each knows alone whether it is the slice's
adopter, so one compare-and-set is tried, not one per shard, and a lost shard's slices spread over the
survivors, each slice to its own top, keeping each survivor's rebuild near one slice's, inside
T-T7.11.1's 5 s. The lease and its epoch stay the fence. H66 can order the surviving regions for a lost
one's work by the same rank, its capacity check unchanged. Falsified by takeover at 1,000,000 not
falling, or two shards writing one slice.

### B16. Quotas as escrowed rights across cells and regions (G7, A3)

**kiln's.** A tenant's budget is integer rights a period (resource × time), split among the owners
once a period and kept before anyone sees the split; a run reserves its worst case, its ask times its
timeout, before it's placed, and settles to what it used; an owner short asks one peer at a time,
which lowers its own holding and keeps that before it sends (demarcation), each transfer a grow-only
total merged by maximum (bounded counter, Balegas et al., SRDS 2015); spend under a cap can't avoid
coordination (Bailis et al., PVLDB 2014), so it comes once a period, ahead of every spend. Zero
overspend under any partition, crash or word said twice; its model plants `GiveBeforeLower`,
`SettleTwice`, `HoardWhenShort` and `PublishBeforeKeep`, each caught (`place/budget.rs:1-38, 220-243`;
docs/needs.md:411-484).

**Hex's.** S5's quotas, and A3's provider quotas (the rate limits and token budgets of the providers
agent sessions call), cross cells and regions that share no store (H64, on `cp/t7-sched-g6`). Each cell
holds escrowed rights of a tenant's or a provider account's period; a workload reserves its worst case
before it's placed (its request times its declared duration, S3; for a session, its declared token
budget); a cell short of rights asks a peer. A lost region's unspent rights are under-use until the
period ends: kiln left reclaiming them without the holder's records open
(`notes/federation-research.md`, its unproven item 4), and H66's epoch fence, which refuses the lost
region's word, may be what makes reclaiming safe; hex-sim's model must show it before H66 moves work
that draws on them. Falsified by any period overspent under partitions and region loss, or a held
workload given no rights while its peers hold them free.

### Not taken

- **Room tokens** (`lb/tokens.rs`): with one owner a node's token is its whole room (docs/lb.md:361),
  and each Hex node has one deciding plane (H60's slices, H64's cells).
- **Waiting at two nodes, and learned wait weights** (Ready, Go and Withdraw; Lattanzi et al.'s
  Algorithm 1, `lb/weights.rs`): kiln's runs with no room wait at nodes; Hex's wait in their plane's
  queue, under H8's class skips.
- **Settings that switch mechanisms on, and a person's defaults**: `on_battery` (BOINC's default), the
  guard's false-alarm budget, the failure domains' 5%, predicted order's γ. Ada's rules allow none.
- **kiln's bar of ten times every proxy's pick**: not Hex's (AC-T7.11.3 is against kube-scheduler), and
  kiln showed it can't be met against a stateless hash (docs/lb.md:588-591).
- **Detection on use in place of hyper-swim**: B9 adds it beside.
