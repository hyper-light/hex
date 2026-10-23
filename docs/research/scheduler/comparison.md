# Scheduler comparison: how each system places, scales, prioritises, handles mixed hardware and contains failures

This is the cross-system comparison over the four scheduler notes, as the runtime notes have
[`../runtime/matrix.md`](../runtime/matrix.md). It adds no new sources. Every cell is drawn from a note,
which holds the primary source, the quotation and its verification, and cites it in the note's own
reference numbering:

- `CM[n]`: [`cluster-managers-at-scale.md`](cluster-managers-at-scale.md), reference `n`.
- `PA[n]`: [`placement-algorithms-and-fairness.md`](placement-algorithms-and-fairness.md).
- `AW[n]`: [`agentic-workloads.md`](agentic-workloads.md).
- `PF[n]`: [`proximity-and-failure-containment.md`](proximity-and-failure-containment.md).

Kinds are the notes' own: `MV` MEASURED-VENDOR, `MI` MEASURED-INDEPENDENT, `CL` CLAIMED, `DE` DESIGN.
`n/p` means the note's sources do not publish it. A cell that says "not covered" means the note did not
research that aspect for that system, which is not the same as the system lacking it.

Hex's own bars, for scale: AC-T7.11.2 asks for 1,000,000 pending workloads placed on 10,000 simulated
nodes in at most 120 s (8,333 placements/s). AC-T7.11.3 asks for at least 10x kube-scheduler's
throughput on the same simulated cluster. AC-T7.11.4 asks for at most 4 KiB of control-plane memory per
workload (`docs/requirements.toml`).

## What the comparison shows

1. **Nobody publishes a p99.9 placement latency.** The tails published are p95 or p99 at best: RAS's p99
   solve of 2.45K s CM[5]; Shard Manager's p99 of about 50 s CM[6]; Hydra's p99 of at most 10 s PA[3];
   Kubernetes' p99 pod-startup SLO of at most 5 s CM[27]. Hex will have to measure its own p99.9 and
   maximum; no competitor gives one to beat.
2. **The fastest placers are distributed and approximate.** Hydra makes over 40,000 decisions/s on
   clusters of over 50,000 nodes PA[3]; Apollo over 20,000 tasks/s on over 20,000 servers PA[2]; Sparrow's
   median queueing delay is under 9 ms PA[1] (all `MV`). The Kubernetes family is an order of magnitude
   slower: up to 1,000 pod creates and binds per second on GKE CM[38], and a 790 pods/s regression floor
   for upstream kube-scheduler CM[30] (`MV`). 8,333 placements/s puts Hex's bar between the two.
3. **The best placements come from optimisers that are too slow for the request path, so every system
   at scale tiers them.** RAS runs a MIP on a 1 h SLO beside an allocator that answers "in seconds"
   CM[5]. Medea runs an ILP only for long-running containers, and is 9.5x worse when the solver takes
   every container PA[7]. Shard Manager speeds its solver with local search CM[6]. Firmament's
   min-cost flow is sub-second until oversubscription pushes it past 400 s PA[6].
4. **Prioritisation is admission plus priority bands almost everywhere; goodput-aware systems are
   research-scale.** Borg admits by quota and has priority bands CM[1], Twine uses entitlements CM[4],
   RAS reservations CM[5], and Kubernetes `PriorityClass` with Kueue for fair-share queues CM[25][20].
   Goodput, finish-time fairness and expected utility report 30-93% lower average job completion time
   (Sia, Pollux), 2.2-3.25x fairer worst cases (Themis) and 2.3-4.0x fewer SLO misses (3Sigma), on 44 to
   256 GPUs or nodes PA[10][12][13][15] (`MV`).
5. **Mixed hardware is handled by normalising capacity and filtering on facts.** Borg uses an abstract
   compute unit over 7 platforms and 21 shapes CM[3], Twine 17 host profiles CM[4], RAS relative resource
   units over 9 types and 12 subtypes CM[5]. Gavel normalises throughput across GPU types PA[11];
   Kubernetes filters on labels, taints and device plugins CM[10]. For agents, a GPU is a fact of the
   pair (node, runtime class), and Firecracker mainline exposes none AW[21][22][25].
6. **Containment at production scale has five parts.**
   - Spread over failure domains, and cap disruptions (Borg CM[1], PF[37]).
   - Rate-limit reactions to mass loss, since a mass failure looks like a partition (Borg CM[1];
     Kubernetes taints PF[28]).
   - Escalate penalties for repeat offenders, with a forgiveness window (CrashLoopBackOff PF[17]; Envoy
     PF[18]).
   - Find antagonists by correlation, then throttle with a cap (CPI² PF[16]).
   - Budget retries (Google PF[22]), because retries sustained more than half of 22 metastable outages
     PF[20] (`MI`).
7. **Gray failure is caught by peers and requesters, not by heartbeats.** Panorama detected 15 of 15
   reproduced gray failures in under 7 s PF[12]; IASO isolates a slow node within minutes across 39,000
   nodes PF[13]. Lifeguard cuts the false positives that slow members cause 50-100x PF[9] (`MV`).
8. **Agent and LLM schedulers changed the unit from request to program or session.** Autellix reports
   4-15x throughput on small testbeds AW[8], and SMetric keeps a session sticky only while
   its SLO holds AW[12]. Quota-aware scheduling has been evaluated only against mock providers AW[13][14].
9. **Network proximity in placement has two precedents, and neither measured its accuracy.** Consul sorts
   results by estimated RTT PF[7]. Hyperscale's gate blends a quality-weighted RTT upper bound with
   observed latency, and states its own unverified criteria PF[34][35]. hyper-swim's engine is unmeasured
   on a real WAN PF[6].

## 1. Cluster managers

| System | Places | Scales (as measured) | Prioritises | Mixed hardware | Contains failures | Measured results |
|---|---|---|---|---|---|---|
| **Kubernetes** (kube-scheduler, v1.37) | One leader-elected scheduler, serial cycle, concurrent binding; Filter then Score plugins, scoring only 5-50% of nodes CM[11][13][10] DE | Supported envelope 5,000 nodes, 150,000 pods, 110 pods/node CM[29][26] DE; OpenAI ran 7,500 nodes CM[36] MV | int32 `PriorityClass` with preemption; ResourceQuota; fair-share queues only through Kueue or Volcano CM[25][20] DE | Labels, taints, affinity; GPUs as integer extended resources through device plugins CM[10] AW[21] DE | Preemption keeps victims' grace period; PodDisruptionBudgets "best effort"; no cross-node preemption CM[25]; taints with a 300 s toleration and rate-limited taint addition PF[28]; CrashLoopBackOff 10 s to 300 s PF[17] DE | 790 pods/s CI floor at 5,000 fake nodes CM[30] MV; 100-150 pods/s expected in the 5,000-node load test CM[22] MV; stateless pod startup p99 at most 5 s, only at churn of 20/s or less CM[27] DE |
| **GKE mega-clusters** | Managed Kubernetes on a Spanner-backed store; Kueue for queueing, gangs and preemption CM[37][38] | 65,000 nodes supported CM[37] CL; 130,000 nodes "in experimental mode" CM[38] MV | 3 priority classes plus Kueue quotas and preemption CM[38] | 250,000+ accelerators CM[37] CL; method n/p | 39,000 pods preempted in 93 s CM[38] MV | Up to 1,000 pod creates/s and binds/s; 130,000 pods up in 3 min 40 s; inference p99 pod startup about 10 s CM[38] MV |
| **Borg** (2015) | Separate scheduler on cached cell state; feasibility then scoring; equivalence classes and relaxed randomisation CM[1] DE | Median cell about 10k machines; over 10,000 tasks/min arrivals in several cells CM[1] MV | Priority bands (no preemption within prod); quota at admission; resource reclamation CM[1] DE | Heterogeneous machines, hard and soft constraints, fine-grained requests, cell compaction as the metric CM[1] DE | Failure-domain spreading; disruption-rate and simultaneous-down limits; rate-limited rescheduling from unreachable machines; no repeated task::machine pairings that crash; a machine is marked down after missed polls CM[1] PF[37] DE | Online pass under 0.5 s; whole cell "a few hundred seconds" against over 3 days without caching; median task start about 25 s CM[1] MV |
| **Borg 2019 trace** | As Borg, plus a batch queueing scheduler, Autopilot and alloc sets CM[3] DE | 8 cells, 96.4k machines CM[3] MV | 5 tiers; lower tiers evicted for production CM[3] DE | 7 hardware platforms, 21 shapes, an abstract compute unit CM[3] DE | Evictions SLO-capped: under 0.2% of production collections have any evicted CM[3] MV | 3,360 jobs/h per cell (3.5x 2011); median scheduling delay lower, tail longer; top 1% of jobs use 99.2% of CPU CM[3] MV |
| **Omega** (2013) | Shared cell state, parallel schedulers, optimistic commit CM[2] DE | Up to 32 batch schedulers; 2.5-9.5x load absorbed (simulation) CM[2] MV | Per-scheduler policy, bounded by priority-range limits CM[2] DE | Not covered (its split is by workload: batch over 80% of jobs, service 55-80% of resources CM[2]) | Service placement resists independent and coordinated failures over "tens of failure domains" CM[2] DE | Conflict fraction passes 1 once a service decision takes about 10 s CM[2] MV (simulated) |
| **Twine** (Meta) | Region-wide control plane sharded by entitlement; allocator plus application-level schedulers CM[4] DE | Largest shard about 170K machines; 1M machines per region a target, not reached CM[4] MV | Entitlement quotas; no preemption described; rate-limited destructive operations CM[4] DE | One small server type and 17 host profiles CM[4] DE | TaskControl lets the application approve, delay or reorder disruptive operations, with a maintenance deadline; shards no bigger than a cluster; failure tests CM[4] DE | About 1,000 job allocations/s at peak; cache hit rate over 99%; no shard lost its second replica during a rolling update plus an MSB failure CM[4] MV |
| **RAS** (Meta) | Asynchronous MIP assigns servers to reservations; a fast allocator places containers within them CM[5] DE | Millions of servers, thousands of reservations CM[5] MV | Reservations with guaranteed capacity; an emergency path CM[5] DE | Relative resource units over 9 types and 12 subtypes CM[5] DE | Buffers: about 94% of servers guaranteed to workloads, 2% for random failures, 4% for correlated ones; replacement servers within one minute CM[5] MV | Solve SLO 1 h: mean 1.8K s, p95 2.2K s, p99 2.45K s CM[5] MV |
| **Shard Manager** (Meta) | Application-level scheduler on Twine; constraint solver with local search CM[6] DE | About 100M shards on over 1M servers CM[6] MV | Hard constraints, then soft goals batched by priority CM[6] DE | Capacity and load models; shard load varies 20x CM[6] DE | Works with TaskControl, because planned events are about 1,000x as frequent as unplanned failures CM[6] DE | Solve P90 about 10 s, P99 about 50 s CM[6] MV |
| **Multi-cluster** (Karmada, MultiKueue) | Cluster selection over member clusters CM[41][38] DE | 100 simulated clusters of 5,000 nodes CM[41] MV (fake nodes) | n/p | n/p | KubeFed archived in 2023 CM[40] | Karmada cluster scheduling P50 12 ms, P99 32 ms; distribution P99 1,298 ms CM[41] MV |

## 2. Placement algorithms and fairness

| System | Places | Scales (as measured) | Prioritises | Mixed hardware | Contains failures | Measured results |
|---|---|---|---|---|---|---|
| **Sparrow** | Batch sampling of `d·m` random workers, late binding PA[1] DE | 110 EC2 VMs; simulated to 10,000 machines PA[1] MV | Worker-side priority and fair queues PA[1] DE | "Extreme heterogeneity hurts" when long tasks dominate PA[1] DE | None: no worker-failure handling, no safeguards against rogue schedulers PA[1] DE | Median queueing under 9 ms; within 12% of an ideal scheduler PA[1] MV |
| **Apollo** | Per-job schedulers on per-server wait-time matrices; stable matching on estimated completion PA[2] DE | Over 20,000 tasks/s on over 20,000 servers; 140,000 concurrent tasks PA[2] MV | Tokens per job; opportunistic tasks for spare capacity PA[2] DE | Not covered | Completion estimate weighted by failure risk; duplicate scheduling and straggler detection after dispatch PA[2] DE | 95% of regular tasks queue under 1 s; duplicates on 0.12-0.17% of tasks, 81-83% of them successful PA[2] MV |
| **Hydra** | Federated YARN sub-clusters place locally; a global policy sets shares PA[3] DE | Over 40,000 decisions/s; clusters over 50,000 nodes PA[3] MV | Strict sharing through the global policy PA[3] DE | Not covered | Not covered | Allocation median 2-3 s, p99 typically at most 10 s; 92% node-local PA[3] MV |
| **Gödel** | Dispatcher, scheduler shards, one binder; optimistic concurrency PA[4a] DE | 2,000+ pods/s per shard claimed PA[4a][4b] CL; about 1,000 pods/s against kube-scheduler's 300 at 30,000 simulated nodes PA[4c] MV | Unified online and offline jobs PA[4a] DE | Not covered | Not covered | CPU utilisation 30% to 60% with Katalyst PA[4b] CL |
| **Quincy** | Min-cost flow over a task-computer graph PA[5] DE | 243 computers PA[5] MV | Fairness bounds in the cost PA[5] DE | Not covered | Preemption priced in the cost PA[5] DE | Up to 3.9x less data moved; p99 83 s at 12,500 machines PA[5][6] MV, MI |
| **Firmament** | Quincy's flow with incremental cost scaling and relaxation PA[6] DE | 12,500 machines, 150,000 tasks (simulation) PA[6] MV | As its policy PA[6] DE | Not covered | Not covered | Hundreds of ms, over 1 s only at p90; over 400 s when oversubscribed PA[6] MV |
| **Medea** | ILP for long-running containers, task scheduler for short ones PA[7] DE | 400 nodes; 5,000 simulated PA[7] MV | Constraint violations and fragmentation in the objective PA[7] DE | Not covered | Near-zero constraint violations at 90% load PA[7] MV | ILP mean 850 ms; median runtime 32% lower than Kubernetes' algorithm PA[7] MV |
| **TetriSched** | MILP per 4 s cycle with a plan-ahead window PA[8] DE | 256 real nodes; 10,000 simulated PA[8] MV | SLO jobs, then best effort PA[8] DE | Not covered | Not covered | Up to 3.5x more SLOs met; best-effort latency 80% lower PA[8] MV |
| **POP** | Splits an optimisation into `k` random sub-problems PA[9] DE | 1,536 GPUs, 2,048 jobs PA[9] MV | As the underlying policy PA[9] DE | Not covered | Not covered | Within 1.5% of optimal; 22.7x faster solve PA[9] MV |
| **Sia** | ILP over (job, configuration) with a restart penalty PA[10] DE | About 1 s up to 2,048 GPUs PA[10] MV | Normalised goodput with a fairness exponent PA[10] DE | Three GPU types PA[10] DE | Not covered | 30-93% lower average JCT on 44-64 GPUs; 96 ms median, 426 ms p95 decisions PA[10] MV |
| **Gavel** | Policies as optimisations over allocation matrices, in rounds PA[11] DE | 48 GPUs physical, 108 simulated PA[11] MV | Any policy in effective throughput PA[11] DE | Throughput normalised across GPU types PA[11] DE | Not covered | Average JCT 1.5x better (physical), 3.5x (simulated) PA[11] MV |
| **Pollux** | Per-job goodput model, population search over allocations PA[12] DE | 64 GPUs, 160 jobs PA[12] MV | Cluster goodput with a power-mean fairness exponent PA[12] DE | Not modelled (a footnote suggests Gavel's metric) PA[12] DE | Not covered | 37-50% shorter average JCT than tuned baselines PA[12] MV |
| **Themis** | Lease-based auction on finish-time fairness PA[13] DE | 64 GPUs real, 256 simulated PA[13] MV | Minimises the worst finish-time-fairness ratio PA[13] DE | Simulated heterogeneous 256-GPU cluster PA[13] MV | Not covered | 2.2-3.25x smaller worst ratio PA[13] MV |
| **Tiresias** | Two-dimensional least-attained-service or Gittins index PA[14] DE | 60 GPUs, 480 jobs PA[14] MV | Attained service, no starvation PA[14] DE | Not covered | Not covered | Average JCT up to 5.5x better than YARN-CS PA[14] MV |
| **3Sigma** | Expected utility over runtime distributions, MILP, with mis-estimate handling PA[15] DE | 256 real nodes, 12,583 simulated PA[15] MV | SLO misses and goodput PA[15] DE | Not covered | Not covered | 4.0x fewer SLO misses than a point-estimate scheduler PA[15] MV |
| **Decima** | Reinforcement-learned graph neural network PA[16] DE | 25 real nodes; 30,000 executors simulated PA[16] MV | Average JCT or makespan PA[16] DE | Not covered | Not covered | At least 21% lower average JCT; under 15 ms per decision PA[16] MV |
| **Morpheus** | Inferred SLOs, recurring reservations, packing, dynamic re-provisioning PA[17] DE | About 100k containers on 2,700 nodes PA[17] MV | Deadline attainment PA[17] DE | Not covered | Not covered | 5-13x fewer worst-case SLO violations PA[17] MV |
| **DRF** | Lowest dominant share first PA[18] DE | 48-node Mesos; 2,000-node trace simulated PA[18] MV | Max-min dominant share PA[18] DE | Multi-resource by construction PA[18] DE | Not covered | Larger jobs up to 66% faster than slot fairness PA[18] MV |
| **Tetris** | Dot-product alignment plus SRTF plus a fairness knob PA[19] DE | 250 machines PA[19] MV | Tunable fairness PA[19] DE | Multi-resource packing PA[19] DE | Not covered | About 30% better makespan and JCT PA[19] MV |
| **Kueue** | Quota admission with cohorts, borrowing and preemption; admission, not placement PA[20a] DE | CI test of 1,000 queues, 50,000 workloads PA[20h] MV | DRF-variant fair sharing PA[20a] DE | Resource flavors with fungibility PA[20a] DE | Not covered | n/p |
| **Volcano** | Sessions of actions and plugins; gangs by PodGroup PA[21a][21d] DE | 10,000-pod scenarios on a Kind cluster PA[21g] MV | Queue shares PA[21a] DE | Not covered | At the default 10 s webhook timeout 98.7% of pod creations failed in its own tuning guide PA[21g] MV | 10,000 x 1 scenario about 180 s with webhooks off PA[21g] MV |

## 3. LLM serving, agent schedulers and gateways

| System | Places | Scales (as measured) | Prioritises | Mixed hardware | Contains failures | Measured results |
|---|---|---|---|---|---|---|
| **vLLM** | KV cache in blocks; continuous batching AW[36] DE | Single engine AW[36] MV | n/p | n/p | n/p | 2-4x throughput at equal latency AW[36] MV |
| **DistServe** | Prefill and decode on different GPUs AW[29] DE | Several LLMs AW[29] MV | Goodput within TTFT and TPOT SLOs AW[29] DE | n/p | n/p | 7.4x more requests or 12.6x tighter SLO AW[29] MV |
| **Splitwise** | Phases on separate machines AW[31] DE | Azure traces AW[31] MV | Throughput, cost, power AW[31] DE | Mixes GPU generations by phase AW[31] DE | n/p | Up to 1.4x throughput at 20% lower cost AW[31] MV |
| **Mooncake** | KV-centric conductor, disaggregated KV cache AW[30] DE | Kimi production service AW[30] MV | Effective throughput within SLOs AW[30] DE | n/p | Early rejection under overload AW[30] DE | 75% more requests on real workloads AW[30] MV |
| **Preble** | Prefix-aware routing with explore and exploit AW[33] DE | Five workloads AW[33] MV | Latency with load balance AW[33] DE | n/p | n/p | 1.5-14.5x average, 2-10x p99 latency AW[33] MV |
| **Llumnix** | Live KV migration across instances AW[10] DE | 16 A10 GPUs; 64 instances with sleeps AW[10] MV | Priorities, one virtual-usage policy AW[10] DE | n/p | Migration relieves preemption, about 20-30 ms downtime AW[10] MV | P99 first-token latency up to 15x better; 36% cost saving AW[10] MV |
| **Parrot** | Application dataflow (Semantic Variables), co-scheduled AW[9] DE | A100 and A6000 GPUs AW[9] MV | Per-request objective from the app's goal AW[9] DE | n/p | n/p | Up to 11.7x (MetaGPT) AW[9] MV |
| **Autellix** | Program-level, non-clairvoyant PLAS/ATLAS; locality-aware routing AW[8] DE | 8x A100-80GB AW[8] MV | Cumulative service, anti-starvation AW[8] DE | n/p | n/p | 4-15x throughput at the same latency AW[8] MV |
| **Continuum** | KV time-to-live across tool calls AW[40] DE | Up to GLM-4.5 355B AW[40] MV | Program FCFS AW[40] DE | n/p | TTL expires when a tool call overruns AW[40] DE | Delay 1.12-3.66x lower AW[40] MV |
| **SAGA** | Workflow graphs, session-affinity batching with work stealing AW[11] DE | 64 A100-80GB AW[11] MV | Agent Fair Share AW[11] DE | n/p | n/p | 1.64x faster task completion; about 30% lower peak throughput AW[11] MV |
| **SMetric** | Session-centric: first turn balanced, follow-ups sticky while the SLO holds AW[12] DE | Replay of two Bailian agent traces AW[12] MV | SLO first, then cache hits AW[12] DE | n/p | Migrates a session when its SLO would break AW[12] DE | 9-15% more peak throughput AW[12] MV |
| **HiveMind** (client side) | Transparent proxy: admission, header-driven rate tracking, AIMD, circuit breaking AW[13] DE | 5-50 mock agents AW[13] MV (mock) | Priorities and token budgets AW[13] DE | n/a | Transparent retry the most critical primitive AW[13] MV | Failures from 72-100% to 0-18% AW[13] MV (mock) |
| **Zhihu client scheduler** | Allocation (DRR), ordering, overload control AW[14] DE | Mock provider AW[14] MV (mock) | Classes by deficit round robin, with deadlines AW[14] DE | n/a | Defer-or-reject cost ladder AW[14] DE | 100% completion at 4.2 ± 1.6 SLO-meeting requests/s AW[14] MV (mock) |
| **LiteLLM, OpenRouter, Portkey** | Weighted shuffle; price-weighted providers; weighted balancing AW[44][45][46] DE | n/p | Hard filters plus soft preferences AW[45] DE | n/a | Cooldown after a 429 (default 5 s); outage exclusion over 30 s; fallback on listed codes AW[44][45][47] DE | n/p |
| **Copilot auto mode** | Re-routes a session's model when throttled AW[2] DE | 13M sessions a week AW[2] MV | n/p | n/a | Switches in about 6.4% of sessions, mostly reactive AW[2] MV | Cache hit falls to 8% after a switch AW[2] MV |

## 4. Proximity, failure detection and containment

| Mechanism | What it decides | Scales (as measured) | How it ranks | Mixed hardware and networks | Containment | Measured results |
|---|---|---|---|---|---|---|
| **Vivaldi** | Synthetic coordinates predict RTT PF[1] DE | 1,740 Internet hosts (simulation over measured latencies) PF[1] MV | Distance in the coordinate space PF[1] DE | Height vectors model access links PF[1] DE | Resists high-error nodes, not malicious ones PF[1] DE | 11% median relative error PF[1] MV |
| **Azureus coordinates** | The same, in the wild PF[2] DE | Million-node network; about 10k test clients PF[2] MV | Six filters and decay techniques PF[2] DE | 83% of pairs violate the triangle inequality PF[2] MI | Violator exclusion PF[2] DE | 43% better accuracy; 4 orders of magnitude more stable PF[2] MV |
| **Serf and Consul** | RTT estimates for sorting and failover PF[4][7] DE | n/p | `?near=` sorting; prepared-query failover by RTT PF[7] DE | Separate LAN and WAN pools PF[7] DE | n/p | n/p (defaults "not tuned to any particular type of cluster" PF[4]) |
| **Hyperscale gate** | Cross-datacenter job routing PF[34] DE | n/p | Health buckets first; then latency x load x health x SLO factors PF[34] DE | Quality-weighted RTT upper bound blended with observed latency PF[35] DE | Hard excludes with reasons; per-job cooldown of a failed datacenter PF[34] DE | Its under-10 s loss criterion "unverified"; death classified within 30 s in test PF[34] |
| **hyper-swim** | Vivaldi (2-D plus height) ranks indirect-probe relays PF[3][5] DE | Membership-bounded PF[3] DE | Nearest relay to the target PF[5] DE | Measured only on loopback and container links PF[6] | Verdicts are hints to the owner's membership PF[5] DE | n/p |
| **SWIM** | Membership and failure detection PF[8] DE | Constant per-member load PF[8] DE | Suspicion before declaration PF[8] DE | n/a | Deterministic detection bound PF[8] DE | n/p in the note |
| **Lifeguard** | Local health on SWIM PF[9] DE | Test cluster in which each member is 1% PF[9] MV | Local health multiplier PF[9] DE | n/a | Slow members doubt themselves PF[9] DE | 50-100x fewer false positives; p99.9 detection up to 1.5-1.8 s later PF[9] MV |
| **φ-accrual** | Continuous suspicion level PF[25] DE | n/p | Per-consumer threshold PF[25] DE | n/a | Graded verdicts PF[25] DE | Φ = 1, 2, 3 for about 10%, 1%, 0.1% mistakes PF[25] DE |
| **hyper-raft pair detectors** | NFD-E per node pair PF[26] DE | Bounded by placement PF[26] DE | Measured lateness, Cantelli bound PF[26] DE | Disk-stall-aware heartbeats PF[26] DE | Overloaded hosts never extended PF[27] DE | Crash detected within `E(D) + α + η` PF[26] DE |
| **Panorama, IASO, Perseus, OmegaGen** | Gray and fail-slow detection PF[12][13][14][15] DE | 39,000 nodes (IASO); 248K drives (Perseus) PF[13][14] MV | Requester and peer views PF[12][13] DE | Per-device and per-node outliers PF[14] DE | Isolation of the slow part PF[13][14] DE | 15 of 15 under 7 s; within minutes; p99.99 down 48%; 20 of 22 at 4.2 s median PF[12][13][14][15] MV |
| **CPI²** | Antagonist throttling PF[16] DE | All Google shared compute clusters PF[16] MV | Victim CPI over 2σ, 3 times in 5 minutes PF[16] DE | Learned per job PF[16] DE | Cap at 0.01 or 0.1 CPU-s/s for 5 minutes PF[16] DE | n/p in the note |
| **Kubernetes kubelet and Envoy** | Restart and ejection PF[17][18] DE | n/p | Escalating backoff and ejection time PF[17][18] DE | n/a | Ejection capped by `max_ejection_percent`; forgiveness after 10 minutes PF[17][18] DE | n/p |
| **Shuffle sharding** | Tenant-to-worker assignment PF[19] DE | Route 53 PF[19] DE | n/a | n/a | One tenant reaches 1/28 at 8 workers, shards of 2 PF[19] DE | 7x better than plain sharding PF[19] DE |
| **Google SRE overload rules** | Client throttling, criticality, retry budgets PF[22][31] DE | n/p | Four criticalities PF[22] DE | n/a | 3 tries per request; 10% retry ratio per client PF[22] DE | Growth limited to 1.1x PF[22] DE |

## 5. The five axes, side by side

- **Places.** Three shapes recur. Sampled and late-bound (Sparrow, Apollo's matching, Hydra) is fastest.
  Cached-state scoring with equivalence classes (Borg, kube-scheduler) is the general case. An optimiser
  on a slower tier (RAS, Medea, Shard Manager) gives the best placements. The agent schedulers add the
  program or session as the unit, and cache or KV locality as a priced term (Autellix, SMetric) AW[8][12].
  Hyperscale adds health-first buckets and latency as evidence with confidence PF[34][35].
- **Scales.** Measured ceilings: 40,000 decisions/s (Hydra), 20,000 tasks/s (Apollo), 1,000 pods/s
  (GKE), about 1,000 job allocations/s (Twine). The largest control-plane domains are about 170K machines
  per Twine shard and 130,000 nodes for GKE (experimental) PA[3][2] CM[38][4]. Every one of these is the
  operator's or authors' own measurement.
- **Prioritises.** Production systems use priority bands, quota at admission and preemption (Borg,
  Kubernetes, Twine, RAS). Research systems use goodput, finish-time fairness, expected utility and
  attained service (Pollux, Themis, 3Sigma, Tiresias). For agents the evidence adds deadline classes,
  early rejection and anti-starvation AW[8][14][30].
- **Handles mixed hardware.** Normalised capacity units (Borg's compute unit, Twine's host profiles,
  RAS's relative resource units) and throughput normalised per accelerator type (Gavel, Sia). Hard
  filters on capability facts handle GPUs and devices (Kubernetes device plugins; the (node, class) GPU
  fact for agents) CM[3][4][5] PA[10][11] AW[21].
- **Contains failures.** Spread and budget disruptions (Borg); give applications a veto with a deadline
  (TaskControl); hold capacity buffers for random and correlated loss (RAS); grade health and correlate
  outliers (φ, Panorama, IASO, CPI²); escalate penalties with forgiveness and an ejection cap (Kubernetes,
  Envoy); bound blast radius (shuffle sharding); budget retries (Google) CM[1][4][5] PF[25][12][13][16][17][18][19][22].

## 6. Gaps across the comparison

- **No p99.9 and almost no maxima** anywhere; nearly every figure is `MV`, on the authors' own systems.
- **No independent head-to-head** of these schedulers on one workload and one cluster. The closest are
  the authors' comparisons against their own reimplementations, and Firmament's re-run of Quincy PA[6].
- **No scheduler evaluated against real provider quotas at agent scale** AW[13][14][15].
- **Network proximity in placement is unmeasured**: neither Consul nor Hyperscale reports how much
  latency-aware placement improves outcomes, and hyper-swim's accuracy on a WAN is unknown PF[7][34][6].
- **Agent-specific failure modes** (stuck tool loops, quota exhaustion that looks like slowness, partial
  provider outages) are in none of the failure-detection studies PF (section 6).

These gaps are what hex-sim and the competitive baseline (goals 3 and 5) must measure. The runtime matrix
covers the per-node side ([`../runtime/matrix.md`](../runtime/matrix.md)).
