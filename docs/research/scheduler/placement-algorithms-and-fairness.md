# Placement algorithms, optimality and fairness: Sparrow to Kueue

The evidence base for the objective function and the algorithm of Hex's placement (CLAUDE.md §5): what fast,
optimal, heterogeneity-aware, deadline-aware and fair schedulers optimise, how fast they decide, at what scale,
and what they cost. Method and number kinds follow [the research method](../README.md#method):
`MEASURED-INDEPENDENT` (a third party measured it), `MEASURED-VENDOR` (the authors measured their own system),
`CLAIMED` (no stated method), `DESIGN` (a fact from the paper's or project's design, not a speed). Percentiles
are quoted exactly as reported; "not published / not found" marks a gap, never an inference. Bracketed numbers
`[n]` are the numbered references at the end; every source was opened and read on 2026-10-22 (the one
exception, the Gödel paper itself, could not be opened and is marked wherever it matters). Papers were read as
full text (downloaded PDFs converted to text); project documentation was read through a page-fetch tool that
summarises, so quotes from it are those the tool returned. Quotations keep the sources' wording; for plain
ASCII a multiplication sign is written `x` and a range dash is written `-`, and `...` marks an omission.

**A caveat that applies to every number below.** All performance figures here were produced by the authors of
the system measured, on their own workloads and hardware. Head-to-head numbers against a baseline are
comparisons the proposing authors ran, usually against their own reimplementation of the baseline. The only
third-party-style measurements are noted as such (Firmament's reimplementation of Quincy [6], Sia's timing of
Pollux and Gavel [10]), and even those were made by competitors' authors. None is a neutral benchmark and none
was measured on Hex's hardware; under CLAUDE.md §7 they are leads for Hex's own competitive baseline, not facts
to design against.

## Summary table

Kind codes: `MV` MEASURED-VENDOR, `MI` MEASURED-INDEPENDENT (see caveat), `CL` CLAIMED, `DE` DESIGN. `n/p` is
"not published / not found".

| System | Algorithm | Objective | Decision latency | Throughput and scale | Quality against baselines |
|---|---|---|---|---|---|
| Sparrow (SOSP 2013) | Batch sampling of `d*m` random workers, late binding, worker-side priority and fair queues (DE) [1] | Job response time (last task) for sub-second tasks; no bin packing (DE) [1] | Median queueing delay under 9 ms; median 7 ms (14 ms with data constraints) (MV) [1] | 110 EC2 VMs, 20k jobs in 200 s; simulated to 10,000 machines; decisions per second n/p; Spark's central scheduler about 1,500 tasks/s (MV) [1] | Within 12% of an ideal scheduler; 4-8x lower median than random, over 10x at p95 (MV) [1] |
| Apollo (OSDI 2014) | Per-job schedulers, per-server wait-time matrices, stable matching on estimated completion time, deferred correction (DE) [2] | Minimise estimated task completion time; opportunistic tasks for utilisation (DE) [2] | 95% of regular tasks queue under 1 s (MV) [2] | Over 20,000 tasks/s in one cluster of over 20,000 servers; 140,000 concurrent tasks (MV) [2] | About 80% of recurring jobs faster, up to 3x; within 4.5% of an oracle on one job; stable matching within 5% of optimal, greedy 57% slower (simulation) (MV) [2] |
| Hydra (NSDI 2019) | Federated YARN sub-clusters place locally; a Global Policy Generator sets shares from aggregates (DE) [3] | Match Apollo's scale and utilisation with strict sharing (DE) [3] | Allocation median 2-3 s, p99 typically at most 10 s (MV) [3] | Over 40,000 decisions/s in the busiest clusters; clusters over 50,000 nodes; one RM at most 1k/s (5k ideal) (MV) [3] | 92% node-local, 97% rack-local; +1-2% CPU utilisation; median job runtime -4% (MV) [3] |
| Gödel (SoCC 2023) | Dispatcher, scheduler shards, single binder; optimistic concurrency; Unit/Pod levels (DE) [4a] | Unified online and offline scheduling, co-location (DE/CL) [4a] | n/p | Claimed 2,000+ pods/s per shard, 5,000+ with shards, 20,000 nodes (CL) [4a, 4b]; repo benchmark about 1,000 pods/s against kube-scheduler about 300 on 30,000 simulated nodes (MV) [4c] | Average CPU utilisation 30% to 60% with Katalyst (CL) [4b]; paper itself unread |
| Quincy (SOSP 2009) | Min-cost flow over a task-computer graph, re-solved from scratch (DE) [5] | Global cost of data movement, wait and preemption under fairness bounds (DE) [5] | 7.64 ms mean, 57.59 ms max at 243 computers (MV) [5]; median 64 s, p99 83 s at 12,500 machines (MI, by Firmament's authors) [6] | 243 computers; about 1 s per solve simulated at 2,500 (MV) [5] | Up to 3.9x less data moved, up to 40% more throughput (MV) [5] |
| Firmament (OSDI 2016) | Quincy's min-cost max-flow with relaxation and incremental cost scaling run together (DE) [6] | Same optimum as Quincy for a given policy (DE) [6] | Hundreds of ms, above 1 s only at p90 (12,500 machines, 90% utilisation) (MV) [6] | 12,500 machines, 150,000 tasks (simulation); short-task limit 5 ms at 100 machines, 375 ms at 1,000 (MV) [6] | 20x lower placement latency than Quincy at equal quality; p99 response 6.2x better than Sparrow (40 machines) (MV) [6] |
| Medea (EuroSys 2018) | ILP (CPLEX) for long-running containers, task scheduler for short ones (DE) [7] | Applications placed, constraint violations, fragmentation (DE) [7] | ILP mean 850 ms at 5,000 simulated machines (MV) [7] | 400-node cluster; 9.5x worse latency if the solver handles all containers (MV) [7] | Median runtime -32% against Kubernetes' algorithm, 2.1x against YARN; near-zero violations at 90% load (MV) [7] |
| TetriSched (EuroSys 2016) | MILP each 4 s cycle with plan-ahead window, 10% gap, warm start (DE) [8] | SLO attainment and best-effort latency (DE) [8] | Per-cycle values only in a figure (n/p in text) [8] | 256 real nodes; 10,000 simulated with similar cycle latency (MV) [8] | Up to 3.5x more SLOs, best-effort latency -80% against Rayon/Capacity (MV) [8] |
| POP (SOSP 2021) | Random partition of granular optimisation problems into `k` sub-problems (DE) [9] | Same as the underlying policy (DE) [9] | Full LP 8 min at 1,000 jobs; POP 22.7x faster at 2,048 jobs (MV) [9] | 1,536 GPUs, 2,048 jobs (MV) [9] | Within 1.5% of optimal; 1.9x better than Gandiva's heuristic (MV) [9] |
| Sia (SOSP 2023) | ILP over (job, configuration) with restart penalty, GLPK_MI (DE) [10] | Sum of normalised goodput with fairness exponent (DE) [10] | 96 ms median, 426 ms p95 at 64 GPUs (MV); Pollux 2.2 s / 4.8 s (MI) [10] | About 1 s up to 2,048 GPUs (MV) [10] | 30-93% lower average JCT on 44-64 GPUs (MV) [10] |
| Gavel (OSDI 2020) | Policies as optimisation over allocation matrices, round-based mechanism (DE) [11] | Any policy in effective throughput: fairness, makespan, cost (DE) [11] | Policy solve under 10 min at 2,048 jobs (hierarchical); 6-min rounds (MV) [11] | 48 GPUs physical, 108 simulated (MV) [11] | Average JCT 1.5x better (physical), 3.5x (simulated); makespan 2.5x (MV) [11] |
| Pollux (OSDI 2021) | Goodput model per job, population-based search over allocations, power-mean fairness (DE) [12] | Cluster goodput with fairness exponent `p` (DE) [12] | 1 s per 60-s interval on 1 vCPU (MV) [12] | 64 GPUs, 160 jobs (MV) [12] | 37-50% shorter average JCT than tuned baselines, 72-73% than untuned (MV) [12] |
| Themis (NSDI 2020) | Lease-based partial-allocation auction on finish-time fairness (DE) [13] | Minimise the maximum finish-time-fairness ratio (DE) [13] | Arbiter 354 ms median, 1,398 ms p95 (MV) [13] | 64 GPUs real, 256 simulated (MV) [13] | 2.2-3.25x smaller maximum ratio; 5-250% more efficient (MV) [13] |
| Tiresias (NSDI 2019) | Discretised two-dimensional least-attained-service or Gittins index (DE) [14] | Average JCT, utilisation, no starvation (DE) [14] | n/p (preemption cost reported) | 60 GPUs, 480 jobs (MV) [14] | Average JCT up to 5.5x better than YARN-CS (testbed), 2.4x (simulation) (MV) [14] |
| 3Sigma (EuroSys 2018) | Runtime-distribution expected utility with MILP and mis-estimate handling (DE) [15] | Minimise SLO misses, maximise goodput (DE) [15] | Predictor at most 14 ms; MILP bounded by timeout (MV) [15] | 256 real nodes; 12,583 simulated (MV) [15] | SLO misses 4.0x lower than a point-estimate scheduler, 2.3x lower than priority (MV) [15] |
| Decima (SIGCOMM 2019) | Reinforcement-learned graph-neural-network policy (DE) [16] | Average JCT or makespan (DE) [16] | Under 15 ms per decision (MV) [16] | 25 nodes real; 30,000 executors simulated (MV) [16] | At least 21% lower average JCT than tuned heuristics; up to 2x at high load (MV) [16] |
| Morpheus (OSDI 2016) | Inferred SLOs, recurring reservations, LowCost packing, dynamic re-provisioning (DE) [17] | Deadline attainment with utilisation (DE) [17] | Reservation submission within 10 s (2,700 nodes) (MV) [17] | About 100k concurrent containers on 2,700 nodes (MV) [17] | 5-13x fewer worst-case SLO violations; footprint -14% to -28% (MV) [17] |
| DRF (NSDI 2011) | Lowest dominant share first (progressive filling) (DE) [18] | Max-min dominant share with four fairness properties (theorem) [18] | `O(log n)` per decision with a heap (DE) [18] | 48-node Mesos; 2,000-node trace simulated (MV) [18] | Larger jobs up to 66% faster than slot fairness (MV) [18] |
| Tetris (SIGCOMM 2014) | Dot-product alignment plus SRTF score plus fairness knob (DE) [19] | Makespan and JCT with tunable fairness (DE) [19] | Heartbeat 0.06-0.07 ms at 15K-45K pending tasks (MV) [19] | 250 machines; Facebook trace simulation (MV) [19] | About 30% better makespan and JCT (deployment), up to 40% (simulation) (MV) [19] |
| Kueue | Quota admission with cohorts, borrowing, preemption, DRF-variant fair sharing (DE) [20a-g] | Quota and fairness; admission, not placement (DE) [20a] | n/p | Design target over 1k ClusterQueues, 10k workloads (DE) [20g]; CI test 1,000 queues, 50,000 workloads (MV) [20h] | n/p |
| Volcano | Session of actions (enqueue, allocate, preempt, reclaim, backfill) and plugins; gang by PodGroup (DE) [21a, 21d] | Batch and gang scheduling with queue shares (DE) [21a, 21c] | Batch sessions delay placement, per the project's own agent-scheduler record (DE) [21e] | 10,000-pod scenarios on a Kind cluster; per-second figure n/p [21g] | n/p |

## Fast placement

### Sparrow (SOSP 2013)

Source: [1] (Ousterhout, Wendell, Zaharia, Stoica; UC Berkeley; SOSP 2013; the SOSP version, read in full).
Every number below is `MEASURED-VENDOR` (the authors' own deployment or simulation) unless marked `DESIGN`
or `ANALYSIS` (a closed-form result under the paper's stated simplifying assumptions).

**Algorithm** (`DESIGN`, [1] §3-§4). Stateless, many schedulers, no shared state. For a job of `m` tasks a
scheduler probes `d·m` random workers (`d = 2` throughout; a probe is a lightweight RPC), and places the
`m` tasks on the `m` least-loaded of them ("batch sampling"). *Late binding* removes the dependence on queue
length as a predictor of wait: a probed worker places a reservation at the back of its queue and only asks
the scheduler for a task when the reservation reaches the front; the scheduler hands tasks to the first `m`
workers to ask and no-ops the other `(d-1)m`. Proactive cancellation of the leftover reservations cuts median
response time by 6% at 95% load for 2% more RPCs ([1] §3.6). Policy is enforced at the worker with one queue
per priority (strict priority) or per user (weighted fair queuing); per-job constraints just restrict the
candidate set, per-task constraints fall back to per-task sampling with late binding ([1] §4).

**Objective** (`DESIGN`, [1] §2, §3.1). Minimise job response time, the time to the *last* task, by
minimising queueing delay for short parallel tasks. It is explicitly not packing: Sparrow "does not
perform bin packing, and does not support gang scheduling", workers run a "fixed number of slots", and
Sparrow "does not launch new processes for each task; instead, Sparrow assumes that a long-running executor
process is already running on each worker machine" ([1] §2, §3.1).

**Why not the power of two choices per task** (`ANALYSIS`, [1] §5). With per-task sampling the probability
that all `m` tasks of a job land on idle machines is `(1 - ρ^d)^m` (load `ρ`), which falls exponentially with
job width: for a 100-task job it "drops to < 2% at 20% load". Batch sampling places all tasks on idle
machines in expectation whenever `d >= 1/(1-ρ)`, an expression that "does not depend on the number of tasks in a
job". With 4 cores per machine and 100-task jobs the paper reports near-perfect performance, 99.9% of jobs with
zero wait time, at up to 79% load ([1] §5).

**Simulation of the techniques** (`MEASURED-VENDOR`, [1] §3, Fig. 3): 10,000 4-core machines, 1 ms RTT,
100-task jobs of equal-length tasks drawn from an exponential with mean 100 ms, at 80% load. Relative to an
omniscient greedy scheduler: per-task sampling "over 2.6x"; batch sampling 1.92x (0.73x per-task sampling);
batch sampling plus late binding 0.55x of batch sampling, "within 5% (4ms) of an omniscient scheduler".

**Decision latency, throughput and scale** (`MEASURED-VENDOR`, [1] §7). The deployment is 110 EC2 VMs: 100
workers and 10 schedulers, running TPC-H queries through Shark at an average 80% load for about 15 minutes
(over 20,000 jobs / 6.2k queries in the analysed 200 s window).
- Response times "within 12% of an ideal scheduler" (median); "median queueing delay of less than 9ms";
  scheduling delay median 7 ms for unconstrained and 14 ms for data-constrained stages ([1] §1, §7.1, §7.3).
- Against random placement, Sparrow cut median query response time 4-8x and 95th percentile "by over 10x";
  against per-task sampling, 0.8x the response time on average and "almost a factor of two" lower at the 95th
  percentile; late binding alone cut the median by 14% on average ([1] §7.1).
- Scheduler failover in under 120 ms: 100 ms to detect plus under 5 ms to reconnect plus under 15 ms to
  relaunch ([1] §7.4).
- A centralised baseline, Spark's own standalone scheduler on the same 100-node cluster, "cannot keep up"
  with task durations under 1,355 ms (at 80% load, 8 slots per machine) and had a best-case throughput of
  "approximately 1500 tasks per second" after profiling. The authors state that "a cluster containing ten
  thousand 16-core machines and running 100ms tasks may require over 1 million scheduling decisions per
  second", and that tens of thousands of machines with sub-second tasks would need "1000x higher scheduling
  throughput" than Spark's scheduler ([1] §1, §7.6).
- Scale beyond 110 machines is simulation only: "the only way to conclusively evaluate Sparrow's performance at
  scale will be to deploy it on a large cluster" ([1] §7.11). The paper does not report a decisions-per-second
  figure for Sparrow itself (not published / not found in the text).
- Probe ratio: oversampling by 10% (`d = 1.1`) reduced median response time "by more than 2.5x" against
  random sampling at 90% load; more oversampling eventually hurts through messaging ([1] §7.9).

**Fairness and isolation** (`MEASURED-VENDOR`, [1] §7.7-§7.8). Per-worker weighted fair queuing gives
cluster-wide shares "in expectation"; on a 400-slot cluster two users received their max-min fair shares with
"only small fluctuations". Priority isolation: a high-priority user's response time was within 40% of ideal
beside a low-priority user overloading the cluster, and the 40% penalty exists because Sparrow does not
preempt, so a high-priority task may wait "as long as the longest running low-priority task".

**Costs and limits** ([1] §3.4, §4.1, §6.3, §7.10, §8).
- Late binding costs idle worker time of `(d·RTT)/(t + d·RTT)`: "at most a 2% efficiency loss" at 1 ms RTT,
  100 ms tasks and `d = 2`. It does not pay when RTT is comparable to task length.
- Queue length is a poor wait-time predictor and duration estimates are "notoriously difficult" (a reason for
  late binding rather than duration-aware probing).
- Extreme heterogeneity hurts: "Sparrow works well unless a large fraction of tasks are long and the long
  tasks are many orders of magnitude longer than the short tasks"; short tasks see more significant
  performance degradation on 4-core machines than on 16-core machines ([1] §7.10).
- No inter-job constraints, no gang scheduling, no preemption, no worker-failure handling (envisioned as a
  soft-state store), and "does not provide any safeguards against rogue schedulers": a misbehaving
  scheduler can raise its probe ratio at others' expense ([1] §6.3, §8).

### Apollo (OSDI 2014)

Source: [2] (Boutin, Ekanayake, Lin, Shi, Zhou; Qian, Wu, Zhou; Microsoft; OSDI 2014; read in full). Every
number below is `MEASURED-VENDOR` (Microsoft's production SCOPE clusters, each "over 20,000 commodity
servers", or the authors' own simulator) unless marked `DESIGN`.

**Algorithm** (`DESIGN`, [2] §3). Many schedulers, one per job (the "Job Manager"), decide independently but
read shared cluster load. Every server (the "Process Node") keeps a local queue of the tasks assigned to it
and publishes a *wait-time matrix*: a cell per (CPU, memory) quota giving the expected wait before a task of
that size would start, computed by simulating the local queue. A Resource Monitor aggregates the matrices; it
is "never on the performance critical path" (Apollo keeps scheduling at degraded quality if it is down). To
place a task a scheduler estimates its completion time `E_succ = I + W + R` (initialisation/file fetch, queue
wait from the matrix, runtime from I/O plus CPU), then weights it for failure: `C = P_succ*E_succ +
K_fail*(1-P_succ)*E_succ` with an empirically set penalty `K_fail` ([2] §3.3). Tasks of similar priority are
placed in batches by a *stable-matching* variant: each task proposes the server with the earliest estimated
completion among a bounded candidate set (servers holding its input, same-rack servers, and two random
lightly-loaded servers); a server keeps the proposal with the greatest completion-time saving; losers
re-propose, at most a capped number of iterations, and only one task per server per batch ([2] §3.3).
Conflicts between schedulers are corrected after dispatch rather than prevented: duplicate scheduling when the
wait estimate proves much worse, random jitter on estimates, a confidence discount on stale matrices, and
straggler detection ([2] §3.4). Capacity is a *token* per job (a token is the right to run one regular task up
to a fixed CPU and memory); jobs may additionally dispatch *opportunistic* tasks, at lower priority and
preemptible, up to an allowance proportional to their tokens, picked at random by the server, and upgraded to
regular when a token frees ([2] §2.1, §3.5).

**Objective** (`DESIGN`). Minimise each task's estimated completion time, which folds locality, wait and
failure risk into one number, while keeping utilisation high through opportunistic work. Fairness is token
quota plus *probabilistic* fairness for opportunistic tasks: "we focus on avoiding the worst case of a few jobs
consuming all the available capacity of the cluster and target average fairness" ([2] §3.5).

**Decision latency, throughput and scale** (`MEASURED-VENDOR`, [2] §1, §5.1).
- "Apollo schedules over 20,000 tasks per second in a production cluster with over 20,000 machines"; the
  hourly-peak scheduling rate was above 10,000 per second constantly and reached up to 20,000 per second in a
  single cluster over six months; scheduling-request peaks exceeded 100,000 per second ([2] §1, §2.3, §5.1).
- "95% of regular tasks experiencing a queuing delay of under 1 second" while CPU utilisation was "consistently
  high (over 80%) and balanced"; at weekday peaks 750 concurrent jobs and 140,000 concurrent regular tasks ran
  at over 90% CPU, with a 3% gap between the 20th and 80th percentile servers ([2] §1, §5.1).
- Matching cost: "O(n log(n))" with a capped iteration count (it "usually converges in less than 3 iterations"),
  for batches usually under 1,000 tasks ([2] §5.5).

**Quality against baselines** (`MEASURED-VENDOR`).
- Production, before and after replacing the previous (baseline) scheduler: about 80% of recurring jobs
  improved, up to 3x ([2] §5.2, Fig. 8). On one business-critical hourly job, average latency improved "around
  22%" over the baseline and was "within 4.5% of the oracle scheduler" (a simulated scheduler with zero wait,
  zero scheduling latency, zero failures and exact runtimes) ([2] §5.2, Fig. 9).
- Stable matching vs alternatives, simulated, 100 single-slot servers: "within 5% of the optimal matching"
  (exhaustive search), while greedy one-task-at-a-time placement "was 57% slower than the optimal matching"
  ([2] §5.5, Fig. 12).
- Estimates: "over 95% tasks have a wait time estimation error of less than 1 second"; CPU time within 25% of
  actual for 75% of tasks; enabling estimation visibly balanced queue times across servers ([2] §5.3).
- Corrections are rare: duplicate scheduling triggered on 0.12-0.17% of tasks with 81-83% success, under 0.5%
  of task creations overall ([2] §5.4, Table 2).
- 72% of tasks were placed on servers that read their input remotely (56% same rack, 16% across racks) to avoid
  queue wait, i.e. a locality-only policy would have concentrated load on hot-data servers ([2] §5.1, Table 1).

**Costs and limits** ([2] §2.3, §3.5, §4).
- Needs runtime and resource estimates per task; it gets them from the query optimiser and from earlier tasks
  in the same stage, which works because "more than 98% of tasks are part of stages with DOP of more than 100".
  The authors list early-out operators and complex user code as cases where estimation fails.
- Opportunistic fairness is statistical only: a "fully decentralized mechanism that maintains a strong fairness
  guarantee in a dynamic environment remains a challenging topic for future work".
- Operations lessons: a first centralised global scheduler "became a performance bottleneck and its scheduling
  quality degraded as the numbers of machines in the cluster and concurrent jobs continued to grow"; a
  failure-prediction model kept recovered servers out of service after a power event; lower-priority
  opportunistic I/O leaked into regular tasks because priority was not enforced through the storage stack
  ([2] §4).
- On Sparrow-style reservations the authors say such a mechanism "would introduce excessive initialization
  costs on multiple servers in our workload" ([2] §6).

### Hydra (NSDI 2019)

Source: [3] (Curino, Krishnan, Karanasos, Rao, Fumarola, Huang, Chaliparambil, Suresh and others; Microsoft;
NSDI 2019; read in full). Every number below is `MEASURED-VENDOR` (Microsoft's production fleet, or the
authors' simulator where stated) unless marked `DESIGN`.

**Algorithm** (`DESIGN`, [3] §3-§4). Hydra splits the two problems a cluster scheduler solves and scales
each separately.
- *Placement* (which node runs a container) is local: the cluster is divided into *sub-clusters*, each an
  independent Apache Hadoop YARN deployment with one centralised Resource Manager (RM) and "a few thousand
  machines"; each RM places containers using delay scheduling (node, then rack, then anywhere) and node
  labels. Routers pick a job's "home" sub-cluster (spreading Application Masters for fault tolerance), and an
  AM-RM Proxy at every worker splits a job's container requests across the RMs, preferring the RM that owns the
  requested node and falling back to less-loaded RMs after a configurable time-out ([3] §3.2, §4.1).
- *Share-determination* (how much each tenant gets) is global but needs only an aggregate: a Global Policy
  Generator (GPG) receives each RM's usage and demand "every few seconds", decides each sub-cluster's policy
  and pushes it down. "The GPG is not on the critical path of container allocation decisions"; RMs keep
  allocating under the last policy if it is unreachable ([3] §3.2, §4.2). The policy in production is *gang
  admission control* (a job with declared parallelism `k` waits until `k` containers fit in its queue's
  quota); *global instantaneous fairness* (GIF) is a candidate that reassigns cluster-wide resources by
  work-preserving fair sharing, aggregated at queue granularity ([3] §4.2).
- Policies can be pushed "across tens of thousands of nodes within seconds" without redeploying ([3]
  abstract, §6.2). Guaranteed and opportunistic containers are borrowed from Apollo and the authors' earlier
  YARN work ([3] §4.1).

**Objective** (`DESIGN`). Match the scale and utilisation of the legacy Apollo system while adding strict
sharing invariants for >10k users and >5k hierarchical queues (5-12 levels, average 10 and maximum 1000
priority levels), arbitrary frameworks, and fast policy change ([3] §2, Tables 2-3).

**Decision latency, throughput and scale** (`MEASURED-VENDOR`).
- Fleet: more than 250,000 servers, more than 50,000 servers per cluster, with up to 5% machine
  unavailability; workload of more than 500,000 jobs and billions of tasks daily and more than 40,000
  scheduling decisions per second (SDPS) ([3] §2, Tables 1 and 3).
- Cluster C2 needs "around 10k" SDPS and cluster C4 "sustained rates of 30k to 40k SDPS" across 15-20
  sub-clusters of 2k-3k nodes each ([3] §6.1, §7.1, Fig. 7).
- The single-RM limit that forces federation: "the RM cannot support more than 1k scheduling decisions per
  second (SDPS), while recent, extensive efforts to improve RM's scalability have raised this bar to 5k SDPS
  under ideal conditions". "The deciding factor for this limit is the scheduling rate and not the number of
  nodes" ([3] §6.1).
- Allocation latency in one busy cluster over 30 days, measured at the client for guaranteed containers:
  "median allocation latency is 2-3 seconds. The 99th percentile is typically no more than 10 seconds"
  ([3] §7.1, Fig. 8). The legacy Apollo had no central scheduling delay "but it could incur high queuing delays
  when tasks were dispatched to each node".
- GIF's own runtime: "<300ms runtime when simulating our largest clusters" ([3] §4.2).
- The authors' comparison point: Borg's published rate of "10k decisions per min (or 166 decisions per
  second)" against Hydra's "routinely reaches 40k decisions per second in our busiest clusters" ([3] §8;
  they attribute Borg's lower rate to co-evolution with its workload, not an intrinsic limit).

**Quality against baselines** (`MEASURED-VENDOR`).
- Placement: "92% node-local allocations and 97% rack-local allocations" on average over a month; per cluster
  70-99% on the preferred node and 75-99.8% within the rack ([3] §4.1.1, §7.1, Fig. 4).
- Utilisation: no regression across 8 months of migration, "a slight increase in average CPU utilization of
  about 1-2%" and a narrower gap between high and low machine percentiles, i.e. more even load ([3] §7.2).
- Job runtime: over 18k recurring job templates (1.25M job instances) the median job grew 18% while median
  runtime "decreasing by 4%" ([3] §7.4, Fig. 12).
- Fairness and utilisation of share-determination policies, simulated on "a few hundreds of different cluster
  setups, each with a few tens of sub-clusters": GIF gave "much higher utilization and comparable fairness
  error" against per-sub-cluster static or load-based quota, and "closely tracks" an idealised single
  centralised scheduler. Fairness error is the mean absolute error against a centralised oracle that may
  relax locality; "0.1% corresponds to just a few hundred unfairly allocated containers across our 50k-node
  clusters" ([3] §4.2, Fig. 5). Running an independent RM per sub-cluster "would lead to unacceptable resource
  fragmentation" ([3] §8).

**Costs and limits.**
- Rich placement constraints: node labels and the "more sophisticated placement constraints described in
  Medea" are supported, but "the more advanced constraints are not yet employed by most of our production
  users" ([3] §4.1).
- GAC leans on opportunistic execution for utilisation, "making it harder to maintain fairness across users"
  ([3] §4.2 footnote); GIF is only in test clusters.
- "Predicting all system 'dynamics' and the effects of multiple interacting algorithmic policies was
  exceedingly hard", which is why the authors invested in a faithful simulator and fast policy push
  ([3] §6.2).
- Sub-cluster size was found "by trial and error" ([3] §6.1). The hypothesis that locality "matters less with
  modern networks" was confirmed during migration ([3] §6.1).

### Gödel (SoCC 2023)

**Evidence status: the paper's full text was not read.** The ACM Digital Library returned HTTP 403 for both the
landing page and the PDF, ResearchGate returned 403, Semantic Scholar returned 429, and the Internet Archive is
blocked from this environment. What follows is therefore built from: the bibliographic record [4]
(Crossref: Wu Xiang, Yakun Li, Yuquan Ren, Fan Jiang, and others, ByteDance; SoCC '23, Santa Cruz, published
2023-10-30), a post written by ByteDance for the CNCF (April 2024) [4a], a KubeFM podcast transcript with a
ByteDance guest [4b], and the open-source repository's benchmark and feature documents [4c, 4d]. These are the
vendor's own statements about the system. Where they report a figure the paper's abstract also reports (as shown
in a search-engine snippet that I did not open at its source: "up to 5,000 pods per second", "overall
utilization above 60%", "tens of thousands of machines"), I say so, and treat it as `CLAIMED` until the paper
is read.

**Algorithm** (`DESIGN`, from the vendor post [4a] and repository documents [4d]). Gödel can substitute for the
default Kubernetes scheduler and splits scheduling into three roles. A single *Dispatcher* queues applications
(FIFO sort; a DRF/fair-share sort exists but is "still pending production use"), assigns applications to
scheduler shards (load-balance by default), and partitions the nodes among shards. *Scheduler* shards run the
expensive work, filtering and scoring nodes (the post names three phases: node organising, unit scheduling,
unit preempting), and decide placements and preemptions but do not execute them. A single *Binder* does conflict
detection, executes preemption, and binds: conflicting applications are "immediately rejected and rescheduled".
The post calls the design optimistic concurrency, with each shard checking its own node partition first and then
others ([4a]). A two-level abstraction schedules a job-level *unit* (a gang or batch) and then pods
within it; PodGroup gang scheduling treats "all pods under a job (PodGroup) as a unified entity" with a
`minMember` and `scheduleTimeoutSeconds`, so a job never holds partial reservations ([4d]). The post also
credits earlier optimisations to the stock Kubernetes scheduler: incremental snapshots, caching results for
homogeneous scheduling units, and preemption pruning ([4a]).

**Objective** (`DESIGN`/`CLAIMED`). One scheduler and resource pool for online services and offline batch,
streaming and training jobs, to remove fragmentation between separate pools and to give elasticity between
priorities; ByteDance says average CPU utilisation went "from 30% to 60%" with Gödel plus its node-level
system Katalyst, which feeds Gödel node (including NUMA-level) resource information for co-location ([4a],
[4b]).

**Throughput and scale.**
- `CLAIMED` (vendor post and podcast, no stated method): the stock Kubernetes scheduler reached "around 10
  pods per second in a cluster of 5000 nodes"; after ByteDance's optimisations of it, "a remarkable 30-fold
  increase" to "300 pods per second" at 10,000 nodes; Gödel "2000+ pods/s in a single shard and 5000+ pods/s
  across multiple shards"; the largest production cluster has "over 20,000 nodes and more than 1,000,000 pods"
  ([4a], [4b]). The two baselines used different node counts, so they are not a like-for-like comparison.
- `MEASURED-VENDOR` (repository benchmark with a stated method, [4c]): 3 Gödel scheduler shards plus one
  dispatcher and binder, against kube-scheduler (QPS and burst 10,000), on a simulated 30,000-node cluster
  (KWOK) with 800,000 pods created directly at 2,000 per second, one Xeon Platinum 8260 machine class,
  Kubernetes 1.29.14. As the fetch tool summarised the document: kube-scheduler reached about 300 pods/s
  end-to-end on average with a peak of 430 pods/s, and Gödel about 1,000 pods/s on average with a peak above
  1,300 pods/s. The document notes Gödel roughly kept pace with the creation rate, which suggests the control
  plane was the limit, and reports no latency figures. The same document says production supports up to 1
  million pods on at most 20K nodes with creation rates up to 1,000 pods/s.
- Decision latency (p50, p99, p99.9) is not published in any source I could open (not published / not found).

**Costs and limits.** The Dispatcher and Binder are each a single instance, and the Binder is where every
placement's conflict check happens (cross-node and single-node conflict modes, [4a]), so it is a
serialisation point by construction; the benchmark's own reading is that the control plane, not the
scheduler, was the limit at 2,000 pods per second ([4c]). Scoring is Kubernetes-plugin based, so quality is
whatever the plugins compute. Of the repository's feature documents I read two (gang scheduling and
concurrent scheduling); neither discusses fairness, and the gang-scheduling guide does not say what happens
when `scheduleTimeoutSeconds` expires ([4d]).

## Optimal and optimisation-based placement

### Quincy (SOSP 2009)

Source: [5] (Isard et al., Microsoft Research, SOSP 2009; read in full). Every number below is
`MEASURED-VENDOR` (the authors on their own 243-computer research cluster, or their own simulation) unless
marked `DESIGN`.

**Algorithm** (`DESIGN`, [5] §4 and Appendix). Quincy maps fair scheduling to min-cost flow on a graph. Every
ready worker task is a source of one unit of flow; a unit that reaches the sink through a computer node is a
placement, and a unit that reaches it through the job's "unscheduled" node `U_j` leaves the task waiting
(or preempts it, if running). Each computer's outgoing edge has capacity 1. Rack and cluster aggregator
nodes keep the edge count bounded (each task has at most ten preferred computers and ten preferred racks, at
a 10% data threshold). The graph has `2J + N + L + M + 2` nodes for `J` jobs, `N` tasks, `L` racks and `M`
computers. A standard solver (Goldberg's scaling min-cost-flow implementation, supplied by IG Systems) returns the
globally minimum-cost flow; the scheduler re-solves "whenever a salient event occurs ... and on a regular timer event" and then
starts or kills tasks to match ([5] §4.1-4.2). The cost model is three terms in common units: bytes moved
across the rack switch and the core switch, a wait-time penalty that grows with time queued, and a
preemption penalty that grows with the work already invested in a running task; three weights (`ω`, `ξ`,
`ψ`) tune it ([5] §4.2). Fairness is a pair of capacities on the `U_j` to sink edge, which bound the number
of computers a job may hold between `E_j` and `F_j`; four regimes (fair or unfair, with or without
preemption) differ only in those edge capacities ([5] §4.3). The authors sketch a proof that every job
eventually makes progress (starvation-freedom) under weak assumptions ([5] §4.2).

**Objective** (`DESIGN`). Minimise a global cost of data movement, waiting and wasted work, subject to a
fairness bound. Fairness is defined informally as: a job that takes `t` seconds with exclusive access should
take no more than `Jt` seconds when `J` jobs share the cluster, enforced by admission control of at most
`K` concurrent jobs ([5] §1, §2.4).

**Decision latency and scale** (`MEASURED-VENDOR`, [5] §6.5). On the 243-computer cluster "the average cost
to solve our scheduling graph was 7.64 ms and the maximum cost was 57.59 ms". A simulated graph for "a much
larger cluster of 2500 computers running 100 concurrent jobs" took "a little over a second per solution".
Quincy re-solved from scratch each time; the authors expected incremental solving to be much faster. At a
larger scale a reimplementation by other authors measured median 64 s and p99 83 s per solver run at 12,500
machines (see the Firmament section; [6] §2.2; independent of Quincy's authors, but made by the authors of
a competing system, replaying the Google trace in simulation).

**Quality against baselines** (`MEASURED-VENDOR`, [5] abstract, §6). The baselines were the authors' own
queue-based schedulers (greedy, simple fairness, fairness with preemption, sticky-slot hysteresis).
- "The volume of data transferred across the cluster is reduced by up to a factor of 3.9 in our
  experiments, leading to a throughput increase of up to 40%" ([5] abstract).
- With fairness and preemption, "policies that enforce fairness and include preemption perform best for this
  workload"; they "can reduce Unfairness almost by a factor of two" and Quincy beats the queue-based
  scheduler "under all our metrics" for that policy ([5] §6.2). Policies without preemption saw "worst-case
  slowdowns of around a factor of 100 for some small jobs".
- On a constrained (1/6 bandwidth) network, tuned Quincy (QFPX) improved the system-normalised performance of
  network-bound jobs by 32% over untuned Quincy and 87% over the queue-based greedy-fairness-preemption
  scheduler; cross-cluster traffic was 8% of total data moved (Quincy tuned) against 30% (queue-based)
  ([5] §6.3, Table 4).
- On the unconstrained network, overall throughput (makespan) was "remarkably similar across policies", and
  "only the very poor placement decisions from GFP [the queue-based fairness-with-preemption policy]
  substantially hurt throughput" ([5] §6.2). The gain from optimisation was in locality and fairness, not
  raw throughput, when the network was not the bottleneck.

**Costs and limits, in the authors' words** ([5] §8). Costs must be independent: "It is difficult to see
how to encode correlated constraints directly in a network flow model like Quincy's". Capacities are
"naturally one-dimensional", and "multi-dimensional capacities cannot be easily represented by Quincy's flow
network". The evaluation ran one task per computer on a homogeneous cluster, fairness is expressed only as
a count of computers (no network or other resource shares), and the scheduler is given no run-time
estimates.

### Firmament (OSDI 2016)

Source: [6] (Gog, Schwarzkopf, Gleave, Watson, Hand; OSDI 2016; read in full). Every number below is
`MEASURED-VENDOR`: the authors, in simulation on the public 12,500-machine Google trace and on their own
40-machine testbed, unless marked `DESIGN`.

**Algorithm** (`DESIGN`, [6] §3-§6). Firmament generalises Quincy: tasks, machines and policy-defined
aggregators form a flow network, and a min-cost max-flow (MCMF) solver continuously reschedules the whole
workload, so a task may be preempted or migrated when that lowers the global cost. A policy API changes the
graph (the paper evaluates three: load spreading, Quincy's locality policy, and a network-bandwidth-aware
policy). The speed comes from four things:
- *Two solvers run at once and the first to finish wins.* Relaxation is fastest in the common case and
  cost scaling is the robust fallback; the authors run both rather than predict which to use, because "the
  heuristic would depend on both scheduling policy and cluster utilization ... making it brittle and
  complex" ([6] §6.1).
- *Incremental re-solving* from the previous flow (incremental cost scaling), plus "price refine" to hand a
  relaxation result to cost scaling ([6] §5.2, §6.2).
- *Problem-specific heuristics* (arc prioritisation, efficient task removal) ([6] §5.3).
- *Cheap graph update and placement extraction*: two breadth-first passes to update the graph and one pass
  to extract placements ([6] §6.3).
The solver is "about 8,000 lines of C++", and the cluster manager plus simulator about 24,000 ([6] §6).

**Objective** (`DESIGN`, [6] §1, §3.3). The same optimum as Quincy for a given policy: "guarantees overall
optimal task placements for a given scheduling policy", with the policy supplying the costs (data locality,
wait time, preemption cost and network bandwidth are the ones the paper evaluates; it says the policy API
"may incorporate e.g., multi-dimensional resources, fairness, and priority preemption", citing the first
author's PhD thesis, and evaluates none of those three).

**Decision latency and scale** (`MEASURED-VENDOR`).
- Quincy's cost-scaling solver, reimplemented, "up to a median of 64s and a 99th percentile of 83s for the
  full Google cluster (12,500 machines)" per run, at about 150,000 live tasks ([6] §2.2, Fig. 3).
- Relaxation "completes in under 200ms" on average at 12,500 machines at about 50% slot utilisation with the
  Quincy policy, "two orders of magnitude" faster than cost scaling ([6] §4.2).
- At 90% slot utilisation on the same trace, "Quincy takes between 25 and 60 seconds to place tasks,
  Firmament typically places tasks in hundreds of milliseconds and only exceeds a sub-second placement
  latency in the 90th percentile", a "more than a 20x" improvement at identical placement quality ([6] §7.2,
  Fig. 14).
- Near saturation relaxation degrades sharply: at about 93% utilisation its runtime exceeds cost scaling's
  and grows "to over 400s in the oversubscribed case" ([6] §4.3, Fig. 8); with a contended load-spreading
  policy it loses to cost scaling once about 3,000 tasks of one job arrive together ([6] §4.3, Fig. 9).
  At 97% average utilisation relaxation alone takes "hundreds of seconds" per run and cost scaling alone
  about 30 s, while Firmament's incremental cost scaling took 10-15 s, "about 2x faster than using cost
  scaling only" ([6] §7.3).
- Short-task limit, with jobs of 10 tasks at a constant 80% load: near-ideal job response time "down to task
  durations as low as 5ms (100 machines) or 375ms (1,000 machines)"; "at 10,000 machines, Firmament keeps up
  with task durations >=5s" ([6] §7.4). The authors compare this with the 2013 centralised Spark scheduler,
  whose breaking point was "on 100 machines at a 1.35 second task duration" (quoting Sparrow).
- Mixed workload: with the Google trace sped up 300x Firmament "keeps up" and places 75% of the tasks with
  sub-second latency; at 200x the median batch task takes 2.1 s and the 90th and 99th percentile tasks take
  18 s and 92 s ([6] §7.4).

**Quality against baselines** (`MEASURED-VENDOR`, [6] §7.5, Fig. 19, 40 machines, 10 Gbps, network-aware
policy). On short batch analytics tasks (3.5-5 s alone, 4-8 GB inputs) with background network load,
"Firmament's 99th percentile response time is 3.4x better than the SwarmKit and Kubernetes ones, and 6.2x
better than Sparrow's". The abstract's "6x" is this tail figure against Sparrow. The comparison schedulers
(Sparrow, SwarmKit, Kubernetes, Mesos) ignore network bandwidth or place randomly; the win is the objective
(avoid overcommitting a link), not the optimiser alone. On a 2%-locality-threshold Quincy policy Firmament
raised input data locality from 56% to 71% "which saves 4 TB of network traffic per simulated hour" while
staying sub-second, where Quincy's solver took over 40 s ([6] §7.2, Fig. 15).

**Costs and limits** ([6]).
- Early termination does not work: "Both cost scaling and relaxation misplace thousands of tasks when
  terminated early", so the authors reject approximate MCMF. TetriSched's MILP, by contrast, "degrades
  gracefully when terminated early" ([6] §5.1, §8).
- The simulation used slot-based assignment (to compare with Quincy), no task constraints, and estimated
  job types and input sizes because the trace lacks them ([6] §7.1). Firmament "supports multi-dimensional
  feasibility checking" but the evaluation used slots; of its network-aware policy the authors write that
  "a production policy would be more complex and extend it with a priority notion and additional resource
  dimensions (e.g., CPU/RAM)" ([6] §3.3, §7.1).
- One scheduler only: "its MCMF optimization does not trivially admit multiple independent schedulers"
  ([6] §8).
- Incremental relaxation is slower than from-scratch in some graphs, and the algorithm choice depends on
  utilisation ([6] §5.2).

### Medea (EuroSys 2018)

Source: [7] (Garefalakis, Karanasos, Pietzuch, Suresh, Rao; Imperial College London and Microsoft; EuroSys 2018;
read in full). Numbers are `MEASURED-VENDOR` (a 400-node pre-production cluster, or the authors' simulator)
unless marked `DESIGN`.

**Algorithm** (`DESIGN`, [7] §3-§5). Medea splits the workload by how long a container lives. A *task-based
scheduler* (YARN's Capacity Scheduler) places short-lived containers with low latency. A dedicated *LRA
scheduler* places *long-running applications* (LRAs; containers that live "hours to months") with an
optimisation, invoked at a configurable interval (10 s in the evaluation) over all LRA requests submitted
since the last run. LRAs express constraints with one generic form, `{subject_tag, {c_tag, c_min, c_max},
node_group}`: every container with `subject_tag` must sit in a node set in which the number of containers
tagged `c_tag` is between `c_min` and `c_max` (so affinity is `c_min = 1`, anti-affinity `c_max = 0`, and
cardinality limits in between); constraints are soft by default, with weights, and can refer to containers of
other or future applications by tag. The placement is an **integer linear program** solved with CPLEX. Its
objective is a weighted sum of three terms, each normalised to the range 0 to 1: the share of LRAs fully
placed (weight `w1`), the constraint violation (to be minimised, `w2`), and the number of nodes left with too
little free space to be useful (to be minimised, `w3`). It is subject to each container being placed at most
once, node capacities, and all-or-none placement of an LRA's containers; the evaluation used `w1 = 1`,
`w2 = 0.5`, `w3 = 0.25` ([7] Fig. 5, §7.1). The LRA scheduler only *chooses*; the task-based
scheduler performs the allocation, so there are no conflicting writers, and a conflict is handled by
resubmitting the LRA. Two heuristics (tag popularity, node candidates) trade quality for latency ([7] §5.3).

**Objective** (`DESIGN`, [7] §2.4, §5.2). Maximise the number of deployed LRAs, minimise (the extent of)
constraint violations and resource fragmentation, with operator-set weights; load balance and machines used are
optional terms.

**Decision latency and scale** (`MEASURED-VENDOR`, [7] §7.5, Fig. 11). Simulated clusters from 50 to 5,000
machines with LRAs consuming 20% of resources: the ILP "has the highest latency, even with 5000 machines, the
average latency is 850 ms, which is low compared to the typical execution times of LRAs (hours, days or
months)". The two-scheduler design avoids the cost of putting everything through the solver: a variant that used
the solver for short and long containers "results in an increased scheduling latency (e.g., 9.5x worse latency
for 20% LRA utilization)". Short-task latency on a Google trace sped up 200x, with an extra 10% LRA load, was
"similar to those of YARN". Real-cluster evidence is 400 machines in 10 racks.

**Quality against baselines** (`MEASURED-VENDOR`, [7] §1, §7.2-§7.4). The baselines were YARN (no constraints),
and `J-Kube`, the authors' implementation of Kubernetes' one-request-at-a-time scoring algorithm (plus `J-Kube++`
with cardinality constraints added).
- Application performance on 400 nodes (45 TensorFlow and 50 HBase instances, plus GridMix batch load on 50% of
  memory): Medea "reduces median runtime of HBase and TensorFlow workloads by up to 32% compared to our
  implementation of Kubernetes' scheduling algorithm (J-Kube) and by 2.1x compared to YARN"; the 99th percentile
  of J-Kube++ was up to 54% higher ([7] §1, §7.2).
- Resilience: with anti-affinity across service units, median and maximum container unavailability improved by
  16% and 24% against J-Kube over 15 days of unavailability traces ([7] §7.3).
- Constraint violations (simulated 500 nodes): the ILP gave "almost no constraint violations, even for 90%
  utilization"; the heuristics 10-20%; J-Kube more. With inter-application constraints involving 10 LRAs the ILP
  stayed under 10% violations and J-Kube above 20%. Considering several LRAs per run mattered: with one LRA per
  run the ILP also showed violations ([7] §7.4, Fig. 9).
- Short-task jobs ran as fast as under plain YARN ([7] Fig. 7d).

**Costs and limits.** Meant for containers that live long enough to tolerate optimisation latency; weights are
set by the operator; the constraint language cannot say "spread all spark containers across 3 racks" directly; constraints on
short-task jobs are not supported; automatic inference of constraints is left to future work ([7] §2.5, §4.3,
§5.4). Hydra reports that its production users mostly do not use the advanced constraints Medea provides
([3] §4.1).

### MIP / ILP approaches and solve times

Sources: TetriSched [8] (Tumanov, Zhu, Park, Kozuch, Harchol-Balter, Ganger; CMU and Intel Labs; EuroSys 2016),
POP [9] (Narayanan, Kazhamiaka, Abuzaid, Kraft, Agrawal, Kandula, Boyd, Zaharia; Stanford and Microsoft Research;
SOSP 2021) and Sia [10] (Jayaram Subramanya, Arfeen, Lin, Qiao, Jia, Ganger; CMU, Cornell, Petuum; SOSP 2023),
each read in full (TetriSched's companion technical report, which it cites for scale tests, was not read). All numbers are
`MEASURED-VENDOR` (the authors' systems) unless marked `DESIGN`. Solve times already reported by other systems in
this note are collected in the table at the end of this section.

**TetriSched** (`DESIGN`, [8] §3-§5). Each scheduling cycle (4 s in every experiment), all pending jobs' requests,
written in a space-time request language (STRL), are compiled into one mixed-integer linear program (MILP)
solved with CPLEX; the plan covers a *plan-ahead window* of future start times, and is re-made each cycle so
runtime mis-estimates are absorbed. *Equivalence sets* (machines interchangeable for a job) make the model
depend on the number of sets, not machines; a time limit returns a solution "within 10% of the optimal"; the
previous cycle's solution seeds the next solve; expressions are culled by deadline and by zero value ([8] §3.2.2,
§7.3). Reported size: "hundreds of thousands of decision variables on a 1000 node cluster with hundreds of jobs".
- *Quality*: on a 256-node real cluster, with workloads derived from Facebook, Yahoo and Microsoft traces, it
  "satisfies up to 3.5x more SLOs while simultaneously lowering average best effort job latency by as much as
  80%" than the Rayon/Capacity scheduler in YARN ([8] §1). Ablations show each feature matters: dropping global
  batching (a greedy one-job-at-a-time variant) costs up to 36% SLO attainment at 50% over-estimated runtimes;
  dropping soft constraints costs 2-3x SLO attainment; plan-ahead helps up to about 100 s ([8] §7.2).
- *Latency and scale*: the solver dominates cycle time for the global policy; the authors report it scaled "to a
  1000-node simulated cluster" (citing a companion report I did not read) and that at 10,000 simulated nodes
  "TetriSched exhibits a similar cycle latency distribution with insignificant degradation in scheduling
  quality"; they add that "even greater scale and complexity may require exploring solver heuristics to address
  the quality-scale tradeoff" ([8] §7.3). The per-cycle latency values are only in Fig. 12 and are not quoted.
- *Costs*: bin-packing "is known to be NP-Hard"; needs reservation-supplied runtimes and deadlines; no preemption
  (at 50% under-estimation TetriSched had 3x higher mean best-effort latency than Rayon/CS); inter-job
  dependencies among pending jobs are inexpressible ([8] §3, §7.2, §7.3).

**POP: partitioning an optimisation instead of replacing it** (`DESIGN`, [9] §1-§4, §7). For "granular"
allocation problems (many clients, each wanting a small share of the resources, resources interchangeable) POP
splits clients and resources randomly into `k` sub-problems with the same formulation, solves them in parallel,
and merges the allocations. Reported: solving the full program can take "8 minutes for a cluster with 1000
jobs using SCS"; across six objectives POP gave "runtime improvements of up to 100x ... while staying within 1.5% of
optimal, and even up to 20x faster and 1.9x higher quality than heuristics" ([9] §1). On Gavel's max-min
fairness policy with space sharing at 2048 jobs on a 1536-GPU cluster, POP changed mean effective throughput by
under 1% with a "22.7x improvement in runtime", while Gandiva's heuristic gave "1.9x worse allocation quality"
([9] §7.1.1). In end-to-end Gavel simulations at 96 GPUs the paper reports "only up to a 5% drop in average JCT"
(a quality loss; not pictured) and, for the space-sharing policy, an 11x speed-up with `k = 8` at similar
average JCT ([9] §7.1.1). Limits stated: it needs granularity; skewed workloads and
"large" clients need client splitting; some problems "are not granular" or need non-trivial partitioning
([9] §1, §4.4, §7).

**Sia: an ILP over (job, configuration) pairs** (`DESIGN`, [10] §3.3-§3.4, §4). Each round (60 s) Sia builds a
goodput matrix `G` (rows jobs, columns "configurations" such as `(nodes, GPUs, type)`, entries each job's
estimated goodput, normalised per job), and solves a binary ILP: choose at most one configuration per job so the
sum of normalised goodputs (with a restart penalty `r = (T - N*S)/(T + S)`, and a fairness exponent `p`) is
maximal, subject to GPUs per type. It is solved with GLPK_MI through CVXPY. Policy runtime on the 64-GPU
heterogeneous setting was "a median and 95th percentile runtime of 96ms and 426ms"; Pollux 2.2 s and 4.8 s; Gavel
13 ms and 28 ms; at up to 2,048 GPUs Sia stayed at "a single-second runtime for policy optimization" and Pollux's
genetic search was about "100x slower" ([10] §5.6). On 44- to 64-GPU clusters with three GPU types Sia reduced
average JCT by 30-93% and p99 JCT and makespan by 28-95% against Pollux, Gavel and Shockwave ([10] abstract);
on the 44-GPU testbed, 35% and 50% lower average JCT than Pollux and Gavel ([10] §1). Going from 60 s to 300 s
rounds raised average JCT by 12%, and 30 s rounds caused more re-allocations and a worse JCT ([10] §5.7).

**Solve times in the sources of this note** (kinds as in the individual sections; sizes are as reported).

| System | Formulation / solver | Size and solve or cycle time | Source |
|---|---|---|---|
| Quincy | min-cost flow (Goldberg cost scaling) | 7.64 ms mean, 57.59 ms max at 243 computers; "a little over a second" simulated at 2,500 computers and 100 jobs | [5] §6.5 |
| Firmament | MCMF, relaxation plus incremental cost scaling | relaxation under 200 ms mean at 12,500 machines and about 50% slot utilisation (Quincy's cost scaling at that scale: median 64 s, p99 83 s) | [6] §2.2, §4.2 |
| Medea | ILP (CPLEX), long-running containers only | 850 ms mean at 5,000 simulated machines, LRAs at 20% of resources | [7] §7.5 |
| TetriSched | MILP (CPLEX), 4 s cycle, 10% gap time limit | hundreds of thousands of variables; per-cycle values only in a figure | [8] §3.2.2, §7.3 |
| 3Sigma | MILP, 1-2 s cycle, timeout and warm start | 12,583 simulated nodes; per-cycle values only in a figure | [15] §4.3, §6.5 |
| Themis | auction solved with Gurobi, 10 min lease | 354 ms median, 1,398 ms p95 at 64 GPUs | [13] §6.2.3 |
| Morpheus | LP per periodic job ("a state-of-the-art solver (e.g., Gurobi, CPlex)") | "within few seconds for all periodic jobs", under 100K variables | [17] §5 |
| Gavel | LP/MILP sequence per policy (cvxpy) | hierarchical policy under 10 min at 2,048 active jobs without space sharing; under 10 min at 512 jobs with it | [11] §7.4 |
| POP | partitioned LP | full formulation 8 min at 1,000 jobs; POP 22.7x faster at 2,048 jobs and 1,536 GPUs | [9] §1, §7.1.1 |
| Sia | ILP (GLPK_MI) | 96 ms median, 426 ms p95 at 64 GPUs; about 1 s at 2,048 GPUs | [10] §5.6 |

## Heterogeneity- and goodput-aware scheduling

### Gavel (OSDI 2020)

Source: [11] (Narayanan, Santhanam, Kazhamiaka, Phanishayee, Zaharia; Stanford and Microsoft Research;
OSDI 2020; read in full). Every number below is `MEASURED-VENDOR` (the authors' physical cluster or their
simulator, which they say agrees with the physical runs within 8%) unless marked `DESIGN`.

**Algorithm** (`DESIGN`, [11] §3-§5). Gavel splits scheduling into a *policy* and a *mechanism*.
- *Policy.* Every policy is written as an optimisation problem over an allocation matrix `X`, whose entry
  `X_mj` is the fraction of wall-clock time job `m` should spend on accelerator type `j`, with constraints
  that no job gets more than 100% of time and no accelerator type is oversubscribed. Throughput terms are
  replaced by *effective throughput*, `throughput(m, X) = sum_j T_mj * X_mj`, the time-weighted average over
  the resources in the allocation, which makes the same policy heterogeneity-aware. Space sharing
  (colocation of at most two jobs) and placement sensitivity (consolidated or not) add rows or worker types.
  Most policies are one linear program; cost policies are linear-fractional programs reduced to a sequence
  of LPs; hierarchical policies use water-filling, an LP plus a MILP that finds bottlenecked jobs, repeated
  until all jobs are bottlenecked ([11] §4.2-§4.3). The policies the paper expresses are max-min fairness
  (least attained service), weighted fairness, makespan, finish-time fairness (Themis' metric), FIFO,
  shortest job first, minimum cost, minimum cost under SLOs, and multi-level fairness/FIFO ([11] Table 1).
- *Mechanism.* A round-based scheduler (6-minute rounds in the evaluation) gives each job a priority equal to
  the element-wise ratio of its target allocation `X` to the fraction of time it has actually received, and
  greedily fills the workers with the highest-priority job (or job pair) per round, placing larger jobs first
  on the same server to limit fragmentation. Solving the assignment exactly with distributed jobs and space
  sharing "makes the problem similar to a multiple-choice knapsack problem [52], which is NP-hard", so each round
  is greedy and errors are corrected in later rounds ([11] §5). Jobs checkpoint and are preempted at round boundaries; "lease renewals" avoid
  preempting a job that would run again on the same worker ([11] §6).
- A throughput estimator (profiling plus matrix completion, "similar" to Quasar) supplies the throughput
  matrix `T` when a job's performance on some accelerator or pairing is not known ([11] §3.3, §6).

**Objective** (`DESIGN`). Whatever the operator picks: fairness, job completion time (JCT), makespan, cost,
SLOs, or a hierarchy of them, all in terms of effective throughput. The abstract and §1 position this as the
fix for existing GPU schedulers (Themis, Tiresias, Gandiva), which "do not consider this heterogeneous
performance behavior" ([11] §1).

**Decision latency and scale** (`MEASURED-VENDOR`, [11] §7.4-§7.5, Fig. 14). The policy is a solve over all
active jobs, not a per-job placement: "even with 2048 active jobs, the hierarchical policy without space
sharing can be run in < 10 minutes. With space sharing, the policy can be run with 512 jobs in < 10
minutes. The single-level LAS policy is much cheaper". The authors "believe latencies of < 30 minutes for
large clusters are still preferable" to a non-preemptive scheduler, and note allocations need not be
recomputed every round, because jobs can be given heterogeneity-agnostic allocations in the interim. The
implementation is "approximately 9000 lines of Python" using cvxpy ([11] §6). Experiments ran on a physical
cluster of 8 V100, 16 P100 and 24 K80 GPUs, and in simulation on 36 GPUs of each of the three types; jobs run
hours to days ([11] §2.1, §7.1). No millisecond-scale or per-job placement latency is reported.

**Quality against baselines** (`MEASURED-VENDOR`, [11] §7.2-§7.3).
- Physical cluster: heterogeneity-aware fairness cut average JCT from 5.1 to 3.4 hours (1.5x), and the
  heterogeneity-aware makespan policy cut makespan from 21.3 to 17.7 hours against Gandiva's ad-hoc space
  sharing (1.2x), over 25 steady-state jobs and a 100-job static trace respectively ([11] Table 3).
- Simulation: heterogeneity-aware policies "reduce average JCT by 3.5x for the continuous-single trace, and by
  2.2x for the continuous-multiple trace" and sustain higher load than heterogeneity-agnostic LAS. Makespan was
  2.5x lower than a FIFO baseline and 1.4x lower than Gandiva-style space sharing, and the cost policy cut cost
  about 1.4x against the throughput-maximising policy ([11] §7.3); the abstract summarises these as makespan
  1.4x and average JCT 3.5x ([11] abstract, §1).
- It matches AlloX, a policy built to minimise average JCT, at moderate load, and supports higher load than
  AlloX, which "can give short jobs preferential treatment ... leading to long jobs experiencing starvation"
  ([11] §7.3). Principled space sharing gave "2.2x better average JCT for both traces at high load" than
  Gandiva-style random packing.
- Finish-time fairness: average JCT 3x lower and average FTF 2.8x better than the agnostic version; FIFO 2.7x
  (3.8x with space sharing) at high load; with 20% high-priority jobs, high-priority JCT 1.5x and
  low-priority 2.7x better ([11] §7.3).
- Cost: the cost policy cut cost "by a factor of roughly 1.4x" but "approximately 35% of jobs violate their
  SLO"; adding SLOs removed all violations for a cost reduction of 1.2x ([11] §7.3).
- Multi-level fairness: a heterogeneity-agnostic static partition has "total effective throughput ... about
  17% lower" ([11] §7.3).
- Round and preemption cost: with 6-minute rounds preemption overhead is "< 3%" without lease renewals (0.22%
  to 2.91% across six models) and 0% to 0.47% with them ([11] §7.2, Table 4); a 6-minute round is "a good
  tradeoff" because checkpoint load and save take under 5 seconds ([11] §7.5).

**Properties** (`DESIGN`, [11] §4.4). Homogeneous clusters reduce to the baseline policies; every policy has
sharing incentive; max-min policies with water-filling are Pareto efficient. The authors cite a result that
no fair-sharing policy can satisfy Pareto efficiency, sharing incentive and strategy-proofness together with
interchangeable resources, and Gavel's policies are not strategy-proof: users who misreport throughputs can
gain.

**Costs and limits.** Training jobs that checkpoint at iteration boundaries and run for hours to days
([11] §2.1); only fairness above and fairness or FIFO below are supported in hierarchies ([11] §4.3);
the mechanism is round-greedy rather than exact; a policy solve takes minutes at hundreds to thousands of
active jobs and grows with the job count (the text gives only the data points above, [11] §7.4); throughput
estimates are needed for every (job, resource type).

### Pollux (OSDI 2021)

Source: [12] (Qiao et al.; Petuum and Carnegie Mellon; OSDI 2021; read in full). Every number below is
`MEASURED-VENDOR` (the authors' 64-GPU testbed, or their trace-driven simulator built from measurements on
it) unless marked `DESIGN`.

**Algorithm** (`DESIGN`, [12] §3-§4). Two cooperating levels with one shared objective, goodput.
- *Goodput* of a job at a configuration is `THROUGHPUT x EFFICIENCY`: examples per second times the useful
  training progress per example (statistical efficiency, predicted from the gradient noise scale as
  `(phi + M0) / (phi + M)` for batch size `M`). Goodput is "the portion of the throughput that is useful for
  training progress" ([12] §3, Eqns. 4 and 6).
- *PolluxAgent*, one per job, fits a throughput model (`T_iter` from gradient compute plus synchronisation,
  with different parameters for co-located and cross-node placement) and the efficiency model online, and
  re-tunes the job's own batch size, gradient accumulation and learning rate for the GPUs it currently holds
  ([12] §3.2, §4.1).
- *PolluxSched* periodically searches an allocation matrix `A` (GPUs per node per job) that maximises
  `FITNESS_p(A) = ( (1/J) sum_j SPEEDUP_j(A_j)^p )^(1/p)`, a generalised power mean of each job's speedup,
  where `SPEEDUP_j` is the job's best goodput under `A_j` divided by its best goodput under an exclusive `1/J`
  share. `p` is a "fairness knob": `p = 1` maximises average speedup (favours jobs that scale), and
  `p -> -infinity` approaches the minimum speedup (equal speedups, ignoring efficiency); the default is
  `p = -1`. Re-allocation is charged: a job that was moved pays a penalty `(T_j - R_j*delta)/(T_j + delta)`
  for its historical rate of moves, because checkpoint-restart cost "between 15 and 120 seconds". At most one
  distributed job may use a node (interference avoidance). The search is a population-based perturb-and-
  combine algorithm ([12] §4.2). Initial throughput priors assume perfect scaling until measured, and a job
  can be given at most twice the most GPUs it has had before.

**Objective** (`DESIGN`). Cluster-wide goodput, subject to a tunable fairness measure (the power mean).
Pollux contrasts this with throughput-only schedulers (Optimus, Gavel) and fixed-allocation schedulers
(Tiresias, Gandiva), which are "agnostic to the statistical efficiency of DL training" ([12] §2.3).

**Decision latency and scale** (`MEASURED-VENDOR`, [12] §5.2). Testbed: 16 AWS g4dn.12xlarge nodes, 64 T4
GPUs, Kubernetes 1.18.2; 160 jobs sampled from the busiest 8-hour window of a Microsoft cluster trace.
"During each 60s scheduling interval, PolluxSched spent an average of 1 second on 1 vCPU computing the cluster
allocations"; each agent's model fit took 0.2 s on average and the per-job batch-size optimisation 0.4 ms. The
scheduling interval can be up to 2 minutes before average JCT degrades ([12] §5.3.2). Scale beyond 64 GPUs and
160 jobs is not reported (not published / not found); the simulator re-runs the same 160-job traces.

**Quality against baselines** (`MEASURED-VENDOR`, [12] §5.2, Table 2, testbed).
- Against Tiresias and Optimus+Oracle given hand-tuned job configurations (a setting the authors say "strongly
  favors the baseline schedulers"): average JCT 0.76 h against 1.2 h (Tiresias) and 1.5 h (Optimus), 99th
  percentile JCT 11 h against 15 h and 15 h, makespan 16 h against 24 h and 20 h. In the authors' terms:
  against Tiresias 37% shorter average JCT, 27% shorter tail, 33% shorter makespan; against Optimus+Oracle
  50%, 27% and 20%.
- Against the same schedulers with job configurations as the trace users set them: 72% and 73% shorter
  average JCT, 50% and 56% shorter tail, 43% and 48% shorter makespan (Optimus+Oracle and Tiresias).
- Fairness (finish-time fairness `rho`, Themis' metric): 99% of jobs had `rho < 2` at `p = -1`, and the
  authors report finish-time fairness improved "by 1.5-5.4x" over the baseline DL schedulers (the text does
  not say which baseline gets which factor) ([12] §1, §5, §5.3.1). Fairness is not free: `p = -1` improved average JCT
  but degraded tail JCT against `p = 1` in this workload, and `p = -10` (more fair) hurt average JCT, tail JCT
  and makespan ([12] §5.2, Table 2).
- Costs: each job was re-allocated about once every 7 minutes, "resulting in an average 8% run-time overhead
  due to checkpoint-restarts" ([12] §5.2). Without interference avoidance, average JCT is 1.4x longer when
  interference slows co-located distributed jobs by 50% ([12] §5.3.2). A goodput-driven cloud auto-scaler
  trained ImageNet "25% cheaper cost, with only a 6% longer completion time" than a throughput-driven one
  ([12] §5.4.1, simulation).

**Costs and limits.** Needs a cooperating agent inside every job and training-specific models of progress;
the throughput model is for synchronous data-parallel training and "does not consider accelerator
heterogeneity" (footnote 1: the authors suggest Gavel's metric could extend it) ([12] §3.2, §2.3); every move
costs a checkpoint-restart of 15-120 s, so the objective must price churn. The evidence is a 64-GPU,
160-job benchmark; there is no statement about thousands of nodes or of sub-minute scheduling.

### Themis (NSDI 2020)

Source: [13] (Mahajan, Balasubramanian, Singhvi, Venkataraman, Akella, Phanishayee, Chawla; University of
Wisconsin-Madison and Microsoft Research; NSDI 2020; read in full). Numbers are `MEASURED-VENDOR` (the authors'
64-GPU testbed or simulator) unless marked `DESIGN`.

**Why fairness has to change for long tasks** (`DESIGN`/`MEASURED-VENDOR`, [13] §2-§3). In the Microsoft trace
the authors analysed (85 hyper-parameter-search apps), "the median app takes 11.5 GPU days and the median task
takes 3.75 GPU hours"; DRF "provide[s] instantaneous resource fairness. Whenever resources become available,
they are allocated to the task from an app with the least current share", which works for short analytics tasks
because "frequent task completions serve as opportunities to redistribute resources", but with long gang-scheduled
tasks it "could lead to newly arriving jobs waiting inordinately long for resources", violating sharing
incentive. Placement sensitivity adds
a second failure: with VGG16 at 103.6 images/s on 4 GPUs of one server against 80.4 images/s across two servers
(Inception-v3 is insensitive), equal aggregate shares yield unequal performance, so DRF and least-attained-service
also violate Pareto efficiency and envy-freeness (Theorem 3.1, [13] §3.1).

**Algorithm** (`DESIGN`, [13] §3-§4). The metric is *finish-time fairness* `rho = T_shared / T_independent`,
the ratio of an app's finish time in the shared cluster to its finish time on its own `1/N` slice; sharing
incentive holds when `rho <= 1`, and the goal is to minimise the maximum `rho`. The mechanism is a round-by-round
*partial-allocation auction* run by a central Arbiter: when a GPU lease expires, the Arbiter makes the free GPUs
visible to the fraction `(1-f)` of apps with the worst `rho` (`f = 0.8` chosen), each app's Agent returns a bid
table of its estimated `rho` for GPU subsets (placement-aware), and winners are computed by maximising the
product of the apps' `1/rho` with a hidden payment that makes truth-telling the best strategy. GPUs left over by
the hidden payments go at random to non-participants, which keeps the scheme work-conserving. Leases are 10
minutes. The architecture is "two-level semi-optimistic": many apps see the offer, only one gets each GPU, so
allocations are conflict-free ([13] §4.2). Proven properties: strategy-proofness, Pareto efficiency and
envy-freeness; sharing incentive is "maximised" rather than guaranteed ([13] Theorem 3.3), and its `rho <= 1`
guarantee "assumes the presence of an admission control mechanism" ([13] §3.2 footnote).

**Decision latency and scale** (`MEASURED-VENDOR`, [13] §6.2.3). Per-round costs on the testbed: each Agent
computes bids in "29 (334) milliseconds ... at the median (95-%)"; the Arbiter's auction uses Gurobi and takes
"354 (1398) milliseconds at the median (95-%ile)", rising when both the number of GPUs offered and of bidders
are high; applying a new allocation takes "35 (50) seconds" at the median (95th percentile), "0.2% (2%) of the
app duration", plus 5-10 s to checkpoint. Scale: 64 K80 GPUs on 20 Azure machines in the testbed, and a
simulated heterogeneous 256-GPU cluster ([13] §6.1). Nothing larger is reported.

**Quality against baselines** (`MEASURED-VENDOR`, [13] §6.2). Against Gandiva, Tiresias, Optimus, SLAQ, SRTF and
SRSF on a Microsoft-trace-derived workload, "Themis gives 2.2X to 3.25X better (smaller) maximum ρ values
compared to all baselines", and is the only scheme keeping sharing incentive when contention is raised to 2x
and 4x. The abstract: "can improve fairness by more than 2.25X and is ~5% to 250% more cluster efficient in
comparison to state-of-the-art schedulers". On the second workload Themis beat Gandiva by about 4.8% and SLAQ
by about 250% in aggregate GPU time. With a random error of
up to 20% in bid valuations, the maximum `rho` moved by only 10.76% ([13] §6.3). In the simulator a strategic
liar did no better than telling the truth up to a 34% misreport, past which it lost resources ([13] §6.3).

**Costs and limits.** Apps must supply estimates of their own finish time under each GPU subset (an
"AGENT" per app plus a profiler); the model assumes synchronous SGD with checkpointing at lease boundaries;
the Arbiter's allocation is computed with an off-the-shelf solver (Gurobi), in under 1.4 s at the 95th
percentile for rounds of this size; and the paper explicitly chooses long-term fairness over short-term efficiency: "a trade-off between finish-time fairness in
the long-term and placement efficiency in the short-term" ([13] §6, §6.4).

### Tiresias (NSDI 2019)

Source: [14] (Gu, Chowdhury, Shin, Zhu, Jeon, Qian, Liu, Guo; University of Michigan, Microsoft, ByteDance,
UNIST, Alibaba; NSDI 2019; read in full). Numbers are `MEASURED-VENDOR` (the authors' 60-GPU testbed or
trace-driven simulator) unless marked `DESIGN`.

**Algorithm** (`DESIGN`, [14] §3). Tiresias schedules distributed deep-learning jobs whose durations are
unknown and which need all their GPUs at once (all-or-nothing).
- *Priority* is the job's two-dimensional **attained service**, `GPUs x time run so far`. With no duration
  information the priority is least-attained-service (LAS: less service, higher priority). If the operator
  supplies the historical distribution of job durations, priority is the **Gittins index**, the probability a
  job completes within the next service quantum divided by its expected remaining need, which the paper notes is
  optimal for average JCT on one server when the distribution is known ([14] §1, §3.2).
- *Discretisation*: continuous priorities mean continual preemption and "degenerates 2DAS to fair sharing by
  time-division multiplexing", so jobs sit in `K` priority queues (multi-level feedback queue), demoted when
  their attained service crosses a threshold; the evaluation uses `K = 2` and one threshold (3,200 GPU-seconds in
  the testbed), and found `K = 3` or `4` improved average JCT by only 1% ([14] §3.2.3, §5.4.2). Because jobs are
  all-or-nothing, a high-priority job without enough GPUs is skipped so smaller jobs can run.
- *Starvation* is avoided by promoting a job to the top queue when its waiting time exceeds `PromoteKnob` times
  its execution time ([14] §3.2.3).
- *Placement* consolidates a job only if a profiler (an RDMA-intercepting library) measures its tensor-size
  skew above a `PackLimit`; otherwise it spreads to reduce fragmentation. The authors built an ILP for optimal
  placement and found it "extremely slow to solve ... for a large-scale cluster with many DDL jobs" and no
  better, so they use the heuristic ([14] §3.3).

**Objective** (`DESIGN`, [14] §3.1). Minimise average job completion time, keep GPU utilisation high, and
avoid starvation; no user input about duration or resources.

**Decision latency and scale.** Tiresias re-evaluates on arrival, completion and resource events. No
per-decision latency or decisions-per-second figure is reported (not published / not found); the cost it
reports is preemption. Preempting a TensorFlow DDL job means checkpointing and reloading its model (figures 4
and 5 of the paper give the per-model times; I did not extract a number from them). Scale: a 60-GPU testbed
(15 servers of 4 P100) with 480 jobs scaled down from a Microsoft trace, and trace-driven simulation of the
production cluster ([14] §5.1).

**Quality against baselines** (`MEASURED-VENDOR`).
- Testbed, against Apache YARN's capacity scheduler as used in production at Microsoft (FIFO, no preemption):
  "improves the average JCT by up to 5.5x", median JCT "27x" better, makespan 1.21x lower (27,400 s against
  33,270 s), and average queueing delay down from 8,146 s to 963 s with the median 13 s against 7,464 s;
  comparable to SRTF with complete prior knowledge ([14] abstract, §5.2). Short-and-small jobs gained most
  (27.6x); long-and-large jobs were unchanged ([14] §5.2.1).
- Simulation of the Microsoft trace: average JCT 2.4x better than YARN-CS, 1.5x better than a best-effort
  variant, 2x better than Gandiva's time-sharing; median JCT 30.85x better than YARN-CS; 95th percentile only
  1.25x better than YARN-CS ([14] §5.3, Table 5).
- Placement: up to 1.67x faster training than random placement for skew-sensitive jobs ([14] §5.2.3).
- Overhead: in the testbed "Tiresias-L spent 13724 seconds performing 221 preemptions" and Tiresias-G 297
  preemptions with 17,425 s; SRTF 316 preemptions with 18,057 s ([14] §5.2.4).

**Costs and limits.** Requires checkpoint-and-resume preemption of whole jobs; the Gittins variant needs a
duration distribution and more preemptions; "optimally picking [K and thresholds] is an open problem"; the
formal conditions under which Discretized 2DAS is near-optimal are left to future work ("formal analyses are
still needed to precisely present its applicable boundaries"). Its stated goals are average JCT, utilisation
and starvation-freedom (through promotion); it states no fairness guarantee, and its 95th-percentile gain
over YARN-CS in simulation (1.25x) is far smaller than its mean (2.4x) and median (30.85x) gains ([14] §3.1,
§3.2.3, §6, Table 5).

## Urgency, deadlines and learned scheduling

### 3Sigma (EuroSys 2018)

Source: [15] (Park, Tumanov, Jiang, Kozuch, Ganger; Carnegie Mellon, UC Berkeley, Intel Labs; EuroSys 2018;
read in full). Numbers are `MEASURED-VENDOR` (the authors' real 256-node cluster, or their simulator) unless
marked `DESIGN`.

**Algorithm** (`DESIGN`, [15] §3-§4). 3Sigma replaces the scheduling component of Hadoop YARN. For each job a
predictor (`3σPredict`) returns the empirical *distribution* of runtimes of similar past jobs (a histogram of at
most 80 bins; candidate groups by user, job name, resources and so on; the group whose past point estimates
had the lowest error is used) rather than one number. For each job the scheduler (`3σSched`) enumerates
placement options `(resource equivalence set, start time)` over a plan-ahead window, scores each by *expected
utility* `E[U] = integral U(start+t) PDF(t) dt` (a utility curve is a constant until the deadline then zero for
an SLO job, and linearly decaying for a latency-sensitive best-effort job), models each running job's future
resource draw as `demand x (1 - CDF(t))`, and solves a mixed-integer linear program (MILP) that picks which jobs
start now and which are deferred, subject to capacity and a constraint that at most one option is chosen per job; preemption of running
jobs is an extra option with a cost. It re-plans every cycle (1-2 s) and handles errors: when a job outlives its
whole history the estimate is incremented exponentially, and when every historical runtime exceeds the time to
the deadline the utility decays instead of dropping to zero, enabled only when the distribution suggests an
over-estimate ([15] §4.2-§4.3).

**Objective** (`DESIGN`, [15] §5). Primarily minimise the share of SLO (deadline) jobs that miss their deadline,
then maximise goodput ("the total work completed in machine-hours") and minimise best-effort latency.

**Why distributions** (`MEASURED-VENDOR`, [15] §1-§2). Analysing a Google trace, a hedge-fund trace and a Los
Alamos trace with a state-of-the-art predictor, "77-92% [were] within a factor of two of the actual runtime",
but "8-23% are not within that range, and some are off by an order of magnitude or more"; runtimes are
heavy-tailed. A worked toy case shows that two jobs with the same 5-minute mean runtime call for scheduling the
SLO job first or the best-effort job first depending on the spread of the distribution ([15] §2.3).

**Decision latency and scale** (`MEASURED-VENDOR`, [15] §6.5). Real cluster of 257 machines (1 master, 256
slaves in 8 racks, quad-core, 16 GB), plus a simulated 256-node cluster and a simulated 12,583-node cluster for
scalability. Workloads are about 1,500 jobs over 5 hours (even mix of SLO and best-effort jobs, offered load 1.4,
deadline slack 20-80%), derived from the Google trace's statistics. `3σPredict` latency was "negligible
(maximum=14ms)"; the per-cycle time and solver time for the 12,583-node run are only in a figure; the solver is
bounded by a timeout parameter, seeded with the previous cycle's solution, and the plan-ahead window bounds the
MILP size ([15] §4.3.6, §6.5). The MILP size depends on the number of equivalence sets, not machines.

**Quality against baselines** (`MEASURED-VENDOR`, [15] §6.1-§6.4). Compared with `PointRealEst` (a
state-of-the-art point-estimate scheduler in the style of TetriSched/Rayon/Morpheus, given a real predictor),
`PointPerfEst` (a hypothetical perfect point-estimate scheduler) and `Prio` (runtime-unaware strict priority, in
the style of Borg):
- "PointRealEst exhibits much higher SLO miss rates (18%, or 4.0X higher than 3Sigma), and lower goodput (5.4%
  lower than 3Sigma)"; "Prio misses 12% of SLO job deadlines (2.3x more than 3Sigma)" and is much worse for
  best-effort goodput; 3Sigma is close to PointPerfEst, missing "a few more SLO job deadlines" and finishing
  fewer best-effort jobs because it preempts more of them to leave room ([15] §6.1, Fig. 6).
- Even when about 70% of estimates were within 10% (no artificial shift), distribution estimates gave "2X fewer
  SLO misses compared to the point estimates" ([15] §6.3). Performance converged to PointPerfEst with about 25
  historical samples per feature ([15] §6.4).
- With the HedgeFund and Mustang workloads 3Sigma "slightly outperforms PointPerfEst", because the perfect
  estimator still lacks knowledge of future arrivals ([15] §6.1).

**Costs and limits.** Needs a utility function per job class from an administrator or expert; assumes recurring
or similar jobs to build a history; the MILP is "NP-hard" and needs a bounded solver; "3Sigma requires more CPU
time to make decisions than not using runtime estimates (e.g., Prio)"; under load it sacrifices best-effort
goodput to protect SLO jobs, and the 12,583-node run is simulation only ([15] §4.3.6, §6.4-§6.5).

### Decima (SIGCOMM 2019)

Source: [16] (Mao, Schwarzkopf, Venkatakrishnan, Meng, Alizadeh; MIT CSAIL and Tsinghua; SIGCOMM 2019; the arXiv
v4 text, read in full except the appendix tables). Numbers are `MEASURED-VENDOR` (the authors' 25-node Spark
cluster or simulator) unless marked `DESIGN`.

**Algorithm** (`DESIGN`, [16] §4-§5). Decima is a learned scheduler for Spark-style data-processing jobs that are
DAGs of stages. A reinforcement-learning agent (policy gradient) observes the cluster, encodes every job's DAG
with a graph neural network (per-node, per-job and global embeddings, 12,736 parameters, 50 KB), and at each
scheduling event outputs two things: which runnable stage to run next and how many executors (a per-job
parallelism limit) to give that job. It is trained offline in a simulator with the reward set to the operator's
objective (for average job completion time, a penalty per job per unit time in the system). Three techniques
make that trainable: the graph embedding (scalable state), decomposing an action into a stage choice plus a
parallelism limit (a small action space), and, for continuous job arrivals, a curriculum of growing episode
lengths with a "memoryless" termination, and an input-dependent baseline that replays the same arrival sequence
across episodes to cancel the variance that random arrivals add to the reward ([16] §5.3). The paper stresses
that the policy it learns changes with the objective and environment: for average JCT it shares executors to
finish small jobs quickly, and for a makespan objective "Decima no longer works to finish jobs early" ([16]
§7.4).

**Objective** (`DESIGN`). Whatever scalar the reward encodes, evaluated on average JCT and on makespan; "the
current design of Decima does not have fairness an objective" ([16] §9).

**Decision latency and scale** (`MEASURED-VENDOR`, [16] §6.1, §7.4, Fig. 15). "Mapping the cluster state to a
scheduling decision takes less than 15ms"; "the average scheduling delay for Decima is less than 15ms, while the
interval between scheduling events is typically in the scale of seconds", about 50x smaller than that interval,
and in under 5% of cases the interval is shorter than the delay. One training iteration took about 5 s in the
main experiments (about 1.5 s per iteration in the appendix's setup on a Xeon with a P100 GPU); total training
time to a good policy is shown only as a learning curve (Fig. 15a), so I do not quote a wall-clock figure. Real
experiments ran on 25 worker VMs (2 executors each) with a TPC-H job mix; the larger setting, an Alibaba
production trace of about 20,000 jobs with up to 30,000 executors, is simulation only ([16] §7.2-§7.3).

**Quality against baselines** (`MEASURED-VENDOR`, [16] abstract, §7). Baselines included Spark FIFO, a shortest-
job-first critical-path heuristic, simple fair, naive and tuned weighted fair, Tetris, and Graphene adapted.
- Real 25-node cluster, 20-job TPC-H batches: Decima "improves the average JCT by 21% over the closest heuristic
  ('opt. weighted fair')", where that heuristic's weighting exponent was swept offline per experiment.
- Continuous arrivals at about 85% load, 1,000 TPC-H jobs: only the tuned weighted-fair baseline keeps up, and
  Decima's average JCT is 29% lower; in the busiest hours it ran jobs about 2x faster; the abstract states "at
  least 21% ... up to 2x improvement during periods of high cluster load".
- Alibaba-trace simulation with four executor memory classes: 32% lower average JCT than the best competing
  scheme (Graphene*), 43% on the TPC-H variant with memory requests; against Tetris its fragmentation is "within
  4%-13%" but its average JCT is 52% lower ([16] §7.3).
- Quality is not proved optimal: the authors compare with an exhaustive search over job orders in a simplified
  setting only (Appendix H).

**Costs and limits, in the authors' words** ([16] §7.4, §8, Appendices). The policy is workload-specific: a model
trained on an "anti-skewed" arrival rate (75 s inter-arrival) "generalizes poorly and underperforms the
optimized weighted fair policy", and one trained only on batch arrivals starves large jobs under load; robustness
needs diverse training workloads or online learning (open). Decima "currently never preempts running tasks".
It needs a faithful simulator with task-duration profiles. Each removed component (graph embedding, parallelism
control, variance reduction) makes it worse than the tuned heuristic at high load, i.e. the result depends on
the whole recipe. No fairness, deadline or SLO objective is evaluated (the authors suggest a deadline-miss
penalty in the reward as future work).

### Morpheus (OSDI 2016)

Source: [17] (Jyothi, Curino, Menache, Narayanamurthy, Tumanov, Yaniv, Mavlyutov, Goiri, Krishnan, Kulkarni, Rao;
Microsoft and academic partners; OSDI 2016; read in full). Numbers are `MEASURED-VENDOR` (Microsoft production
traces replayed in simulation, and live runs on 275-, 2,700- and 4,000-node clusters) unless marked `DESIGN`.

**What the operators said** (`MEASURED-VENDOR`, [17] §2.1). "Predictability trumps fairness": users are "120x
more likely to complain about the performance (un)predictability (25% of all job/resource-management
escalations) than about fairness (< 0.2%), despite the fact that our system does not enforce fairness strictly".
Production jobs are over 75% of the workload, over 60% of jobs in the larger clusters recur, 75% of jobs are
over-provisioned even at peak (20% of them by over 10x), and over 90% of escalations concern production jobs.

**Algorithm** (`DESIGN`, [17] §3-§7). For periodic production jobs Morpheus (1) infers a deadline (the SLO) from
the job's data-dependency "provenance graph" and a *resource skyline* (a time-varying demand vector) from
telemetry of past runs by a linear program that explicitly penalises both under- and over-provisioning
(under 100K variables and constraints; "we obtain a solution within few seconds for all periodic jobs in our
clusters"); (2) admits a *recurring reservation* for that skyline and packs reservations with `LowCost`, an
online greedy algorithm that places containers stage by stage in reverse chronological order to minimise the
maximum cluster load over time (it forbids rescheduling jobs already in the system, keeps allocations
contiguous, and is `O(log(Pmax/Pmin))`-competitive for the maximum allocation under the paper's simplifying
assumptions); and (3) *dynamically re-provisions* a running instance when its demand exceeds its skyline,
granting extra resources for one-minute windows capped at a factor of 2 of recent allocation ([17] §5-§7). The
authors note the offline packing problem is NP-hard and "can be casted as a Mixed Integer Linear Program" but
prefer a lighter online algorithm because reservations are updated often.

**Objective** (`DESIGN`). Meet inferred completion-time SLOs for production jobs while keeping utilisation, and
reduce the cluster footprint reserved for them; ad-hoc jobs get the residual capacity.

**Decision latency and scale** (`MEASURED-VENDOR`, [17] §9.3). A sustained 8-hour live run on a 2,700-node cluster
scheduled "almost 100k concurrent containers" through the YARN ResourceManager with hundreds of reservation
submissions an hour, up to 900 concurrent reservations in the plan (270 active), and "reservation submission
latencies within 10sec". The authors had to rework the RM's queue locking to reach this: the original design
"limited Morpheus' scalability to levels far below our production needs" ([17] §8). Trace-based simulation used a
50k-node cluster's month-long trace; a limited production run was on a 4,000-node cluster.

**Quality against baselines** (`MEASURED-VENDOR`, [17] abstract, §9). "Morpheus can lower the number of deadline
violations by 5x to 13x, while retaining cluster-utilization, and lowering cluster footprint by 14% to 28%",
measured against users' own static provisioning on the enterprise trace; "worst-case SLO misses" are the
violations a periodic job would suffer if no opportunistic fair-share capacity were available. A breakdown shows
SLO extraction plus packing lowers needed cluster size by 6%, the skyline model by 16%, and both together by 19%
([17] §9.2). The SLO extractor produced a target for over 70% of millions of periodic-job instances. Missing a
deadline is nearly as harmful as failing for downstream jobs: the probability that a dependent job fails is
"> 4x" higher when its predecessor missed its SLO than when it met it ([17] §9.1).

**Costs and limits.** It depends on recurrence ("over 60% of the jobs in our larger/busier clusters are
recurrent") and history; cold start falls back to fair queueing or manual SLOs; inferring minimum parallelism is
"an open future direction"; the guarantee is a reservation, so unreserved ad-hoc work is best-effort ([17] §5,
§8).

## Fairness and admission

### Dominant Resource Fairness (NSDI 2011)

Source: [18] (Ghodsi, Zaharia, Hindman, Konwinski, Shenker, Stoica; UC Berkeley; NSDI 2011; read in full).
Properties and proofs are `DESIGN`/theorem; the experiments are `MEASURED-VENDOR` (the authors' Mesos
implementation and simulation).

**Algorithm** (`DESIGN`, [18] §4). A task has a demand vector over resource types. A user's *dominant share*
is the largest fraction of any one resource type it holds, and DRF "seeks to maximize the minimum dominant
share across all users". Online it is a loop: pick the user with the lowest dominant share who has a task
ready, launch that task if its demand vector fits, repeat; "the above algorithm can be implemented using a
binary heap that stores each user's dominant share. Each scheduling decision then takes O(log n) time for n
users" ([18] §4.2). Weighted DRF takes a weight vector per user ([18] §4.3). In the continuous idealisation it
is "progressive filling": raise all dominant shares at the same rate, each user's allocation proportional to
its demand vector, freeze users when a resource saturates ([18] Appendix A.1). In a discrete cluster of
machines, allocating task by task to the lowest-dominant-share user bounds the gap to the continuous
allocation by one maximum-size task (Theorem 7, [18] §6.2).

**Objective and guarantees** (theorem, [18] §3, §6, Appendix A). DRF satisfies four properties that the paper
argues any multi-resource policy should:
- *Sharing incentive*: each user does at least as well as with an equal static `1/n` partition;
- *Strategy-proofness*: no user gains by misreporting its demands;
- *Envy-freeness*: no user prefers another's allocation;
- *Pareto efficiency*: no user's allocation can rise without lowering another's;
plus single-resource fairness, bottleneck fairness and (for strictly positive demands) population
monotonicity. It gives up *resource monotonicity*: "no allocation policy that satisfies the sharing incentive
and Pareto efficiency properties can also satisfy resource monotonicity" (Theorem 6). The competitors fail
properties: Asset Fairness (equalise the sum of shares) violates sharing incentive, bottleneck fairness and
resource monotonicity, with a constructed case where one user receives less than half of both resources;
Competitive Equilibrium from Equal Incomes, "the preferred fair division mechanism in microeconomic theory",
is not strategy-proof and violates population monotonicity ([18] §5-§6). The paper motivates strategy-proofness
with operator anecdotes: a user at a Yahoo! datacenter launched jobs as long reduce phases to dodge contended
map slots, and users at a search company "would sprinkle their code with infinite loops to artificially
inflate utilization levels" ([18] §3).

**Decision latency and scale.** Only the `O(log n)` per-decision bound above; no wall-clock decision latency or
throughput is reported (not published / not found). The workload evidence is a 48-node Mesos cluster on EC2 and
a trace simulation of a 2,000-node Facebook cluster, one week of October 2010, scaled to 400 nodes of 12 slots,
16 cores and 32 GB so that fairness binds ([18] §7). The motivation figures are `MEASURED-VENDOR` from one
month of Facebook's 2,000-node Hadoop cluster: tasks varied widely in CPU and memory demand, and "most of the
tasks either underutilize or overutilize some of their slot resources" ([18] §2).

**Quality against baselines** (`MEASURED-VENDOR`, [18] §7). Against slot-based fair sharing (3 to 6 slots per
node) and CPU-only fair sharing on the 48-node cluster, "with slot-based scheduling, both the throughput and
job response times are worse than with DRF, regardless of the number of slots"; CPU-only fairness completes
far fewer large jobs because memory is overcommitted on some machines ([18] §7.2; the exact per-bar counts are
only in the figures). In the Facebook-trace simulation DRF "provides higher utilization" and cut the
completion time of the larger jobs "by as much as 66%", while small jobs saw no gain ("-3%") because their
completion is dominated by one task ([18] §7.3, Fig. 14).

**Costs and limits, in the authors' words** ([18] §9.1, Appendix A). The proofs assume users with unbounded
work and one demand vector per user. On discrete machines fragmentation remains: "one interesting problem is to
minimize resource fragmentation without compromising fairness. This problem is similar to bin-packing".
Fairness with placement constraints is open ("defining fairness when tasks have placement constraints, such as
machine preferences"). DRF equalises shares of declared demand at an instant; the paper evaluates no
deadline or priority policy. It offers Weighted DRF for importance, and notes weighted max-min fairness "can
support a variety of other resource allocation policies, including priority, reservation, and deadline based
allocation" by citation to prior work ([18] §1, §4.3). The authors also record that multi-resource requirements
appear "difficult to incorporate" into Quincy's min-cost-flow formulation ([18] §8).

### Tetris (SIGCOMM 2014)

Source: [19] (Grandl, Ananthanarayanan, Kandula, Rao, Akella; Microsoft, UW-Madison, UC Berkeley; SIGCOMM 2014; the
copy hosted on an HKUST course page, read in full). Numbers are `MEASURED-VENDOR` (the authors' 250-machine YARN
deployment, or their simulator replaying Facebook traces) unless marked `DESIGN`.

**Algorithm** (`DESIGN`, [19] §3-§4). Tetris is an online multi-resource packing heuristic added to the YARN
resource manager. Each task has a peak demand vector over CPU, memory, disk and network. When resources free on a
machine: (1) keep only tasks whose peak demand fits; (2) score each by *alignment*, the dot product of the task's
demand and the machine's available-resource vector, both normalised by the machine's capacity (cosine
similarity); (3) add a job-level term to prefer jobs with little remaining work, a multi-resource SRTF
score `a + e*p` with `e` set to the ratio of the average alignment to the average remaining-work score; (4) pick the
best. Disk and network demands depend on where the task's input lies, so the score uses only local resources
with a "remote penalty (of, say, 10%)" and checks that remote machines have bandwidth. Estimates come from
earlier runs of recurring jobs and from the first tasks of the same phase (over-estimating the very first tasks);
a per-node resource tracker corrects for estimation error and non-scheduled load (ingestion, evacuation), and
allocations are enforced with token buckets. *Fairness is a knob*: sort jobs by distance from their fair share
(slot, capacity or DRF) and pick the best task only among the first `ceil((1-f)*|J|)` jobs; `f = 0` is pure
efficiency, `f -> 1` is strict fairness. A *barrier knob* `b` favours the last tasks before a stage barrier.

**Objective** (`DESIGN`). Minimise makespan (equivalently maximise packing efficiency) and average job
completion time, with fairness as a tunable constraint, on CPU, memory, disk and network together.

**Complexity** (theorem, cited by the paper, [19] §1, §3.1). Multi-dimensional bin packing "is known to be
APX-Hard"; for the online case "no polynomial time approximation scheme is possible with competitive ratio of the
number of dimensions unless NP=ZPP"; with `d` resources the paper says a slot-based scheduler's packing
efficiency can be 1/(2d) of optimal (its footnote 2). A one-dimensional "largest ball that fits" heuristic uses no more than `(11/9)OPT+1` bins.
The authors state that "scalability was a key reason behind our choice to avoid more complex solutions based on
flow-networks and integer linear programming".

**Decision latency and scale** (`MEASURED-VENDOR`, [19] §5.2.2, Table 7). Heartbeat processing at the resource
manager, 250 servers: node-manager heartbeat ".06ms (.07ms)" with 15K (45K) pending tasks under Tetris against
".06ms (.12ms)" under the stock scheduler; application-master heartbeat .15 ms against .2 ms. The authors "estimate
that an RM using Tetris scales similarly to the default Yarn". The deployment is 250 machines (32 cores, 96 GB, 8
disks, 10 Gbps each), over 100 jobs, five repetitions of multi-hour runs; the larger-scale evidence is a
trace-driven simulator mimicking a "several thousand server" Facebook cluster.

**Quality against baselines** (`MEASURED-VENDOR`).
- Deployment against the Capacity scheduler (slots) and DRF: median job completion time improved by 30%;
  makespan reduced by 29% (stdev 2.94%) against the Capacity scheduler and 27.5% (3.50%) against DRF ([19]
  §5.2, Fig. 4).
- Simulation against the fair scheduler and DRF: average job completion time "40% and 35%", makespan "33% and
  24%"; average gains are 81% of a simple, loose upper bound (an aggregated, fragmentation-free scheduler, which
  the authors say is "not a true upper bound") ([19] §5.3.1). About two-thirds of the gain came from avoiding
  over-allocation of disk and network, one-third from reduced fragmentation: considering only CPU and memory cut
  the average gain from 40% to 15% ([19] §5.3.1).
- Alignment score: cosine similarity beat L2-norm-difference, L2-norm-ratio, FFD-product and FFD-sum (average
  completion-time gain 40% against 30%, 33%, 25% and 26%) ([19] Table 8).
- Fairness cost: with `f = 0.25`, fewer than 6% of jobs slow down, each by less than 10%; `f = 0` can slow down up
  to 15% of jobs; even `f -> 1` (strict fairness) improved makespan by 10% and average job duration by 23%
  ([19] §5.3.2). In a worked example, a packing schedule cut average completion time 50% and makespan 33% below
  DRF's, and every job finished earlier ([19] §2.1).

**Costs and limits.** Needs demand estimates (history or the first tasks of a phase); large tasks may starve
("A more principled solution that reserves machine resources for starved tasks is left to future work", [19]
§3.5); fairness is a trade-off knob, not a guarantee; the alignment score uses only local resources; the
evaluation is batch analytics DAGs with barriers.

### Kueue

Sources: the project's own documentation and design records, all opened for this note: overview [20a], ClusterQueue
concept [20b], Cohort concept [20c], Preemption concept [20d], Fair Sharing concept page [20e], Admission Fair
Sharing page [20f], the Fair Sharing design record KEP-1714 [20g], and the CI scheduler-performance configuration
[20h]. They are `DESIGN` (what the system does) unless marked otherwise. The fetch tool summarised pages, so
quoted phrases are those the tool returned as quotes; field names and defaults were checked against more than
one page where they appear on more than one.

**What it is** (`DESIGN`, [20a]). "Kueue is a kubernetes-native system that manages quotas and how jobs consume
them": it decides "when a job should wait, when a job should be admitted to start (as in pods can be created) and
when a job should be preempted". It is admission control, not placement: "Kueue does not replace any existing
Kubernetes components", and pod-to-node placement stays with kube-scheduler, cluster-autoscaler and the
controller manager. It supports resource flavors with fungibility, cohorts, preemption "with a variety of
policies between different tenants", priority queueing, a "timeout-based implementation of All-or-nothing
scheduling", multi-cluster offload (MultiKueue) and topology-aware scheduling.

**Quota, cohorts and borrowing** ([20b], [20c]). A `ClusterQueue` governs a pool of resources per flavor with
`nominalQuota`, an optional `borrowingLimit` ("the maximum amount of quota that this ClusterQueue is allowed to
borrow") and an optional `lendingLimit` (what it lets others borrow when idle). ClusterQueues in the same
cohort borrow each other's unused quota; to borrow a resource a queue "must define nominal quota for the desired
Resource and Flavor," even if zero. Cohorts can form a tree (`parentName`) with their own quota, and "These
Borrowing and Lending Limits can be specified for Cohorts, as well as for ClusterQueues". Queueing is
`StrictFIFO` (an older workload that does not fit blocks newer ones) or `BestEffortFIFO` (the default; it does
not), both ordered by priority then creation time.

**Preemption** ([20b], [20d]). Three cases: a higher-priority workload preempting within its own queue
(`withinClusterQueue`: `Never`, `LowerPriority`, `LowerOrNewerEqualPriority`); a queue reclaiming quota that other
queues in the cohort borrowed (`reclaimWithinCohort`: `Never`, `LowerPriority`, `Any`); and borrowing with
preemption (`borrowWithinCohort`, capped by a priority threshold). Candidates are ordered: workloads from
borrowing queues, then lowest priority, then most recently admitted; Kueue then greedily removes preemptions the
incoming workload does not need. The defaults are `Never`, so nothing is preempted unless configured.

**Fair sharing** ([20e], [20f], [20g]). KEP-1714 states "The value function is a variation of DRF" and cites the DRF
paper. The share value of a ClusterQueue or cohort is, per resource, the usage above nominal quota divided by the
total nominal quota (or lending limits), taken as the maximum across resources "divided by the weight, if
defined" (weight defaults to 1). Admission prefers the queue or cohort with the lowest share; preemption prefers
victims from queues with the highest share, under the strategies `LessThanOrEqualToFinalShare` and
`LessThanInitialShare`. The KEP's goals (as the fetch tool paraphrased them) include similar shares for comparable
sub-organisations, proportional weights, resistance to gaming, and preferring fairness over utilisation in
pathological cases; its non-goals include using historical data and "Maximize utilization at the cost of Fair
Sharing". A separate *Admission Fair Sharing* mode orders
workloads from LocalQueues by decayed historical usage (`usageHalfLifeTime`, per-resource weights, an entry
penalty to stop flooding) and is beta ([20f]). The concept page's own proof that preemptions cannot cycle covers
the flat case; "A proof that a chain of preemptions won't cause a cycle is needed for the Hierarchical case"
([20e]).

**Scale and latency.** Design target (`DESIGN`, [20g]): "should be able to handle >1k ClusterQueues, >100 Cohorts and
>10k workloads"; the authors note a naive algorithm is "O(n^2) on the number of workloads" and that fair
sharing with hierarchical cohorts "significantly increases the complexity of scheduling in Kueue". Measured
(`MEASURED-VENDOR`, the project's own periodic CI job, [20h]): the large-scale scheduler performance test
simulates 10 cohorts x 100 ClusterQueues = 1,000 ClusterQueues and 50,000 one-pod workloads (small, medium and
large, with simulated runtimes of 150, 350 and 700 ms and priorities 50, 100, 200). Its regression thresholds
come from 19 successful runs: mean wall-clock 1,179,411 ms (standard deviation 13,280 ms, limit 1,416,000 ms),
mean time to admission 700,379 ms (small), 256,413 ms (medium) and 105,604 ms (large), and mean ClusterQueue
usage 38.64%. These time-to-admission values are dominated by waiting for quota in a saturated test, not by
decision cost, and the test does not report admissions per second or per-decision latency (not published / not
found); the project docs I opened make no throughput claim.

**Costs and limits.** Kueue decides *whether and when* a workload may start against quota; it does not choose a
node. Its fairness is a point-in-time share with weights (plus optional decayed history), not a guarantee of
completion time. The KEP lists the risks: "Fair Sharing may increase the number of preemptions in the system" and
"may decrease utilization of the unused resources" ([20g]).

### Volcano

Sources: the Volcano project's documentation and design records, opened for this note: scheduler introduction
[21a], plugin list [21b], Queue [21c], PodGroup [21d], the "agent scheduler" design record [21e], the DRF design
record [21f], and the performance-tuning guide [21g]. `DESIGN` unless marked otherwise.

**What it is** (`DESIGN`, [21a]). A Kubernetes batch scheduler built from *actions* and *plugins*. Each scheduling
cycle opens a *session*: the scheduler caches submitted jobs, then runs the configured actions in order and
closes the session. The actions are `enqueue` (admit a job when its minimum resources fit), `allocate` (choose
nodes), `preempt` (higher-priority tasks in the same queue evict lower-priority ones), `reclaim` (take back
resources lent to other queues by queue weight) and `backfill`. Plugins supply the policies, configured in tiers
in a ConfigMap; the sample configuration lists gang, priority, conformance, DRF, predicates, proportion,
nodeorder and binpack. "Volcano itself will not check the rationality of the order" of actions.

**Gang scheduling and queues** ([21c], [21d]). A `PodGroup` carries `minMember` ("the minimum number of pods or
tasks running under the PodGroup"), `minResources` and a `queue`; if the cluster cannot meet them, none of the group's pods is scheduled and the
group stays `Pending`; it moves to `Inqueue` when admitted and to `Running` when at least `minMember` run. A `Queue` has `weight` (default 1; the soft share used by the proportion plugin),
`capability` (hard cap), `guarantee` (reserved, others cannot use it), `deserved` (the amount above which others
may reclaim; needs the capacity plugin), `reclaimable` (default true), `priority` and `parent` for hierarchy.
`deserved` shares follow weight: weight divided by total weight times total cluster resources; a queue may
borrow idle resources and must return them when the owner needs them.

**Fairness plugin** ([21f], [21b]). The DRF design record is the kube-batch-era description: the share is "the
highest value of ratio of the (allocated resource/Total Resource)", the job with the lowest share goes first,
under gang scheduling jobs that have not met `minAvailable` rank above jobs that have, and "the preemptor can only
preempt other tasks only if the share of the preemptor is less than the share of the preemptee" after
recomputing both allocations. I could not confirm from the pages I read that the current plugin implements
exactly this; the plugin page says only that DRF favours tasks with fewer resources.

**Agent workloads** (`DESIGN`, [21e]). Volcano's own design record proposes "An independent Agent scheduler" to
"identify and make fast scheduling for Agent workloads". As the fetch tool summarised the record, agent
workloads are latency-sensitive and create tasks frequently, whereas the Volcano scheduler processes workloads
in batches at fixed session intervals, so a pod may not be placed immediately. The design: multiple workers
pulling pods from a shared queue (active, backoff and unschedulable pools), a binder that uses "optimistic
concurrency control to resolve scheduling conflict" and returns several candidate nodes with binding versions
per result, requeueing a pod with high priority if all candidates conflict, and a sharding controller dividing
nodes into shards (`none`, `soft`, `hard`). It states no numeric targets and its snapshot and configuration
sections are marked TBD.

**Throughput and scale** (`MEASURED-VENDOR` for what exists). The official tuning guide ([21g]) uses a Kind
cluster on an 8-core Xeon Gold 6230 with 15 GB RAM, Volcano v1.11.0 and 10,000 pods per scenario; the node count
is not stated. It documents two operational findings: with the default 10 s webhook timeout, of 526,767 pod
creation requests only 712 (0.13%) succeeded and 98.7% failed with "context deadline exceeded" (raising the
timeout to 30 s brought the 10,000-pod scenarios to nearly 100% success), and disabling webhooks cut the
10,000 x 1 scenario from "about 250 seconds" to "about 180 seconds", a throughput gain of "nearly 30%". The
guide says Volcano "significantly outperforms other schedulers" with gang scheduling but gives no figures, and
publishes no decisions-per-second figure at scale (not published / not found in the pages I opened). A
"1000 pods per second" figure appears in cloud-vendor documentation surfaced by search; I did not open it, so I
do not rely on it.

**Costs and limits.** Session-based batch cycles trade latency for batching (the project's own agent-scheduler
record says so); the action order is operator-configured and unchecked; gang semantics are expressed as
`minMember` and `minResources` on the PodGroup, so partial progress is not modelled; fairness documentation for
the current DRF plugin is thin.

## Lessons for Hex

Hex must place extremely bursty agent workloads with maximal correctness, on the most suitable node, with urgency
that maximises goodput rather than balance, fast enough for millions of workloads. Each lesson below separates
*evidence* (cited), *inference* (my reasoning from the evidence, not a result) and *what would falsify it*
(CLAUDE.md §5). None of the evidence is about microVM or sandbox placement: every system studied places data-
analytics tasks, deep-learning training jobs or long-running containers (see Open questions).

### What "optimal" can and cannot mean

| Claim | Status | Source |
|---|---|---|
| Min-cost flow returns the global optimum for a given *separable* cost model, and Firmament's solver design made it fast enough at 12,500 machines | Proven for the model; measured speed | [5] §4, [6] §7.2 |
| The flow model cannot express correlated constraints or multi-dimensional capacities | Stated by Quincy's authors | [5] §8 |
| Multi-dimensional bin packing is APX-hard; the online problem has no polynomial approximation scheme whose ratio is the number of dimensions unless NP=ZPP | Theorem, cited by Tetris | [19] §1, §3.1 |
| DRF is strategy-proof, envy-free, Pareto-efficient and gives sharing incentive; no policy can add resource monotonicity | Theorem | [18] §6 |
| Batch sampling with `d >= 1/(1-load)` places all tasks of a job on idle machines in expectation, independent of job width (single-core model) | Analysis under stated assumptions | [1] §5 |
| The Gittins index minimises average JCT on one server when the duration distribution is known | Cited known result | [14] §1 |
| `LowCost` is `O(log(Pmax/Pmin))`-competitive for maximum allocation under simplifying assumptions | Theorem | [17] §6 |
| Per-round exact assignment with distributed jobs and space sharing is NP-hard (multiple-choice knapsack) | Stated | [11] §5 |
| Stable matching was within 5% of the exhaustive optimum and greedy 57% slower, in a single-slot simulation | Measured (simulation) | [2] §5.5 |
| ILP placement had near-zero constraint violations at 90% load where heuristics had 10-20% | Measured (simulation) | [7] §7.4 |

*Inference.* "Maximal correctness" for Hex has two parts that need different evidence. The *hard* part (capacity in
every enforced dimension, isolation class, hardware, anti-affinity that is declared mandatory, gang
all-or-nothing) can be made exact: it is a property of each decision and can be checked by construction and by an
oracle. The *soft* part (which feasible node is best, in what order) is NP-hard in general, so what can be
claimed is a measured gap to an exact solver on instances small enough to solve, as Apollo did against an
exhaustive matcher [2] and Medea against the ILP [7], plus a stated bound where a theorem exists.

### Proposed objective and algorithm shape (a hypothesis for docs/design/, not a result)

1. *Per-decision score*: for workload `w` on candidate node `n`, the expected utility `E[U_w(T)]` of the estimated
   completion time `T = init + wait + run` (Apollo's decomposition, with a failure-risk weighting, [2] §3.3),
   where `U_w` is a deadline step for deadline work and a decaying line for latency-sensitive work, and `T` is a
   distribution, not a point (3Sigma, [15] §3.1, §4.1). Initialisation cost is a first-class term (warm image or
   snapshot on the node versus cold), because Apollo found that for some tasks the files to fetch are bigger than
   the input, so locality should follow where those files are cached ([2] §2.3).
2. *Packing term*: subtract an alignment/fragmentation penalty over every dimension the node enforces (Tetris'
   normalised dot product, [19] §3.2; Medea's fragmentation term, [7] §5.2).
3. *Churn term*: charge re-placement with a penalty that grows with a job's history of moves, as in
   `(T - N*S)/(T + S)` ([12] §4.2, [10] §3.4).
4. *Cluster objective*: maximise the sum of expected utilities of completed work (goodput) subject to hard
   constraints and a fairness floor, with a single explicit trade-off knob (Pollux's power-mean exponent `p`, [12];
   Tetris' `f`, [19]; Sia's `p`, [10]).
5. *Algorithm shape*: (a) a fast path of sampled candidates (locality and warm-cache set plus a few random
   lightly-loaded nodes, [2] §3.3; or `d*m` probes with late binding for wide jobs, [1]) with batch matching of
   same-priority arrivals (stable matching, [2]); (b) optimistic commit at the node or shard with typed
   conflict handling ([2] §3.4, [4a]); (c) a slower global re-optimisation of long-lived, constrained or gang
   placements with an incremental flow or ILP and a concurrent fallback solver ([6] §6.1, [7], [8], [9]); (d)
   fair shares and quotas set off the critical path from aggregated state ([3] §3.2, [11], [20a-g]).

### Lesson 1. Make the objective expected useful work against a deadline, not balance

*Evidence.* The systems that measured end-to-end gains optimised something more specific than balance:
completion-time estimates that fold in locality, queue wait and failure risk (Apollo, [2] §3.3, where a
data-local server can lose to a remote idle one: 72% of Apollo's tasks ran on servers that read their input
remotely to avoid waiting, [2] §5.1); expected deadline utility (3Sigma cut SLO misses to a quarter of a
point-estimate scheduler's and raised goodput, [15] §6.1); goodput that discounts throughput by useful progress
(Pollux, [12] §3); and effective throughput normalised across heterogeneous resources (Gavel, [11] §3.1). Pure
load spreading is used in Firmament only to stress the solver, not recommended as a policy ([6] §3.3). Tetris found two-thirds of its gain
came from not over-allocating unmanaged resources (disk, network) and one-third from reducing fragmentation
([19] §5.3.1). Morpheus found predictability, not fairness, drove user escalations (120x) ([17] §2.1).

*Inference.* For agent work, "useful work" must be defined per class (deadline-bound, interactive, background) and
measured as completed work within its deadline per unit of resource; Hex does not yet have that definition and the
evidence does not supply one.

*Falsified if* Hex's measured agent workloads show no deadline or latency structure (all work equally urgent), in
which case a simpler utilisation-plus-fairness objective suffices; or if the marginal gain of an expected-utility
score over a load-aware score is under noise on Hex's own differential suite.

### Lesson 2. Do not put an optimiser on the per-request path; tier by decision rate and value

*Evidence.* A single centralised RM tops out at "1k scheduling decisions per second" (5k under ideal conditions),
so Hydra shards placement into sub-clusters of 2-3 thousand nodes and moves share-determination to an aggregate
that is "not on the critical path" while reaching over 40,000 decisions per second ([3] §3.2, §6.1, §7.1). Apollo
reaches 20,000 tasks per second with per-job schedulers and node-published wait estimates ([2] §5.1); Gödel
claims 2,000+ pods per second per shard and its repository benchmark shows about 1,000 against about 300 for
kube-scheduler ([4a], [4c]). The optimisers pay for their quality in latency: Quincy took median 64 s and p99 83 s
at 12,500 machines ([6] §2.2), Firmament cut that to hundreds of ms, but its authors report it "does not
trivially admit multiple independent schedulers" ([6] §8) and its fast solver has pathological cases: relaxation
became slower than cost scaling at about 93% utilisation and reached over 400 s when oversubscribed, and it
also lost to cost scaling when a job of about 3,000 tasks arrived at once ([6] §4.3). Sending short-running
containers through the same solver as long-running ones raised long-running-application scheduling latency 9.5x
in Medea at 20% LRA utilisation ([7] §7.5). Early termination of the exact solver does not work: "Both cost
scaling and relaxation misplace thousands of tasks when terminated early" ([6] §5.1), so the remedy is
incremental re-solving, a warm start from the previous cycle ([8] §3.2.2) and a concurrent fallback algorithm
([6] §6.1), or partitioning the problem ([9]).

*Inference.* Hex's bursts (fan-outs of many sandboxes at once) are exactly Firmament's hard case, so the burst path
needs a bounded-time algorithm (sampling plus batch matching) and the optimiser needs a fallback and a deadline.

*Falsified if* a warm-started, equivalence-class flow or ILP on Hex's hardware sustains the required decision rate
at p99 under a stated latency bound through a 10,000-way burst at 90% utilisation, or if the aggregate of
independent shard decisions loses more than a stated fraction of goodput to conflicts.

### Lesson 3. Use weighted dominant-share fairness for admission, and add time-integrated fairness for long-lived work, behind one explicit knob

*Evidence.* DRF gives four provable properties including strategy-proofness, which matters when users declare
their own demands ([18] §3, §6); Kueue's fair sharing is "a variation of DRF" with weights and borrowing ([20g]);
Volcano's DRF and queue weights are similar ([21b], [21c]). But DRF equalises instantaneous shares, which Themis
shows "can arbitrarily violate sharing incentive" when tasks are long and gang-scheduled, because frequent
completions are what redistribute resources ([13] §2-§3); Themis, Tiresias and Kueue's Admission Fair Sharing
all add time-integrated state (finish-time fairness, attained service, decayed history) ([13], [14], [20f]).
Fairness does not have to cost goodput: Tetris with strict fairness (`f -> 1`) still improved makespan by 10% and
JCT by 23%, and `f = 0.25` slowed under 6% of jobs by under 10% ([19] §5.3.2); Pollux's `p = -1` gave 99% of jobs
a finish-time ratio below 2 while cutting JCT 37-50% ([12] §5.2, §5.3.1). The trade is real, though: a more
fair `p` or `f` cost tail JCT in Pollux and makespan in Tetris ([12] Table 2, [19] Fig. 8).

*Inference.* Hex needs (i) per-tenant weighted multi-dimensional quotas with borrow and reclaim and typed refusal
at admission, (ii) a lease or attained-service term for sandboxes that live long enough that instantaneous
shares stop redistributing, and (iii) the fairness-versus-goodput trade exposed as a measured parameter rather
than hard-coded.

*Falsified if* Hex's sandbox lifetimes are short relative to the reallocation interval (then instantaneous DRF is
adequate and the time-integrated term adds nothing), or if tenants cannot gain by misreporting demands (then
strategy-proofness is not worth its cost).

### Lesson 4. Plan for wrong runtime estimates: use distributions, or no estimate at all

*Evidence.* With a state-of-the-art predictor "8-23%" of runtimes were off by a factor of two or more and some by
an order of magnitude; a scheduler using the full distribution made "2X fewer SLO misses" than one using point
estimates even when about 70% of estimates were accurate ([15] §2.1, §6.3). Tiresias' least-attained-service
needs no duration and performed "comparably" to SRTF with complete knowledge ([14] §5.2). Sparrow's late binding
replaces a queue-length forecast with a pull at start time, and notes duration prediction is "notoriously
difficult" ([1] §3.4-§3.5). Morpheus' history-based SLOs apply to recurring jobs (over 60% of jobs in its larger
clusters) and it falls back to fair queueing or manually supplied SLOs for the rest ([17] §2.1, §8).

*Inference.* Agent tasks are likely heavy-tailed and often unrecurring, so the default policy should not require a
declared duration: late binding plus a least-attained-service-style priority within a class, with distributions
learned per (tenant, agent type) where history exists.

*Falsified if* Hex's workloads are dominated by recurring templates with low runtime variance (Morpheus-like), in
which case point estimates or reservations are enough, or if attained-service priority measurably starves
long-lived interactive sessions in Hex's differential suite.

### Lesson 5. Charge churn inside the objective and make re-optimisation incremental

*Evidence.* Every preemption or re-placement in the DL systems costs real work: 15-120 s per checkpoint-restart and
about 8% run-time overhead at one move per 7 minutes (Pollux, [12] §4.2, §5.2); 25-250 s restarts and a 12% worse
average JCT when rounds are lengthened from 60 s to 300 s, but more re-allocations at 30 s (Sia, [10] §3.4, §5.7);
under 3% preemption overhead at 6-minute rounds even without lease renewals (Gavel, [11] §7.2); 221 preemptions costing 13,724 s
(Tiresias, [14] §5.2.4), which is why it uses two priority queues rather than continuous priorities ([14] §3.2.3).
The optimisers keep churn low by warm-starting from the previous solution ([8] §3.2.2, [15] §4.3.6) and by
incremental solving ([6] §5.2).

*Inference.* The churn price for Hex is the cost of moving a sandbox (snapshot, restore, cold start), which the
runtime notes measure; the objective needs that number and hysteresis so small score changes do not move work.

*Falsified if* Hex's sandboxes are effectively free to restart (sub-100 ms), in which case a churn term only adds
complexity.

### Lesson 6. Constraint-first, score-second, and validate heuristics against an exact oracle

*Evidence.* Apollo's stable-matching placement was within 5% of the exhaustive optimum while greedy was 57%
slower ([2] §5.5); Medea's ILP had near-zero violations where heuristics had 10-20% and a one-request-at-a-time
Kubernetes-style scorer had more, and considering several requests together mattered ([7] §7.4); Tetris' cosine
similarity beat four other alignment scores by 7-15 points of JCT gain ([19] Table 8); Hydra's production users
mostly do not use advanced constraints ([3] §4.1). Quincy's graph cannot carry correlated or multi-dimensional
constraints ([5] §8), and Decima compared against exhaustive search only in a simplified setting ([16] App. H).

*Inference.* Treat capacity and isolation as hard filters that no score may override, make scores a vector with a
deterministic tie-break, and run every placement heuristic differentially against an exact flow or ILP on small
instances in CI (CLAUDE.md §6 model-based and oracle tests).

*Falsified if* Hex's heuristic exceeds a stated gap to the exact solver on the instances it meets. The response is
to change the heuristic, or to route that class of request to the solver, not to raise the budget (CLAUDE.md §4).

### Lesson 7. Separate admission from placement, and refuse with a typed reason

*Evidence.* Kueue decides "when a job should wait, ... be admitted ... and ... be preempted" and leaves node
choice to kube-scheduler ([20a]); Volcano's PodGroup refuses to schedule any pod of a gang unless `minMember` and
`minResources` fit ([21d]); Quincy bounds concurrency with admission control ([5] §2.4); Morpheus admits
periodic jobs only against reservations ([17]); Themis states its guarantee "assumes the presence of an admission
control mechanism" ([13] §3.2). Gödel's gang guide leaves the timeout outcome unspecified ([4d]).

*Inference.* The number of states a request can end in (admitted, queued with a reason and a bound, refused with
a reason) is part of the contract; the evidence supports having an explicit admission step in front of placement.

*Falsified if* a placement-only design with a bounded queue and typed overflow refusals meets every requirement
with fewer states, which the differential suite can test.

### What not to copy

Inferences from the evidence above, not results.

- Sparrow-style probe reservations when starting an instance is expensive: Apollo reports such reservations "would
  introduce excessive initialization costs on multiple servers" ([2] §6). A reservation that is only a queue entry
  is cheap; one that warms a VM is not.
- Single-instance binders and dispatchers on the hot path (Gödel [4a]) and a single flow solver ([6] §8) as the
  scaling story for millions of workloads.
- Fairness claims that rest on instantaneous shares alone when work is long-lived ([13]).
- Any number above as a design constant: each is a lead for Hex's own measurement on named hardware (CLAUDE.md §5,
  §7).

## Open questions / gaps

Things I could not verify, and questions the evidence does not answer. Each is a candidate for Hex's own
measurement or for a further research note.

1. **The Gödel paper was not read.** The ACM Digital Library returned HTTP 403 for the landing page and the PDF,
   ResearchGate returned 403, Semantic Scholar 429, the Internet Archive is blocked from this environment, and the
   Internet Archive's scholar search needs an interactive check. The Gödel section rests on the Crossref record,
   a ByteDance-authored CNCF post, a podcast transcript and the open-source repository's documents. The paper's
   architecture details, its measured scheduling latency and its evaluation against the Kubernetes scheduler are
   therefore not established here.
2. **No source studies sandbox or microVM placement.** Every system above places analytics tasks, DL training
   jobs, or long-running containers. How agent bursts (thousands of sandboxes requested within seconds, heavy-
   tailed lifetimes, mostly idle) behave under sampling, flow, or ILP placement is unmeasured in this evidence.
   A sibling note, `agentic-workloads.md` in this directory, characterises agent workloads; it was still a draft
   when this note was finalised and I did not verify its claims, so none is used here. Hex's own workload
   characterisation and the runtime notes are needed before any constant is chosen.
3. **No neutral benchmark exists.** Every figure is the authors' own, with baselines often reimplemented by them
   (the cluster-managers note covers Borg, Omega and Kubernetes separately). Decision-latency percentiles beyond
   a median and p95 are rarely published as numbers (no source reports a p99.9; maxima appear in Quincy's text
   and in the whiskers of some figures), and Sparrow's own decisions-per-second, Kueue's admissions-per-second and
   Volcano's decisions-per-second at scale are not published in the sources I read.
4. **What is "goodput" for an agent?** The evidence defines it for DL training (throughput times statistical
   efficiency, [12]), as completed machine-hours ([15]), and as normalised goodput over configurations ([10]).
   For agent sandboxes the unit of useful work (a completed tool call, a finished episode, tokens served) and its
   deadline structure have to be defined and justified before an expected-utility objective can be built.
5. **Strategy-proofness for agent-declared demands.** DRF and Themis price gaming ([18], [13]), but no source
   measures whether untrusted agents misreport resource needs, nor what that costs; Hex's tenants are the
   likely adversary.
6. **Bound on the cost of stale or sampled state at very large scale.** Sparrow's analysis assumes zero network
   delay and an infinite number of servers ([1] §5); Apollo reports staleness handled by confidence discounts
   and deferred correction ([2] §3.4); the largest measured clusters are about 50,000 nodes ([3]) and 12,583
   simulated ([15]). Nothing here tests millions of concurrent workloads in one cluster.
7. **Not read (outside my assignment or unavailable):** Borg, Omega, Kubernetes' scheduler and Twine (the cluster-
   managers note); hybrid schedulers Hawk, Eagle and Mercury; Tarcil; Paragon and Quasar; Carbyne; Graphene;
   Shockwave and AlloX; Azure's Protean allocator; the TetriSched companion technical report; Mitzenmacher's
   survey of the power of two choices (Sparrow's theory is taken from Sparrow [1]); the Kueue Configuration API
   reference for exact field semantics; the Volcano proportion and capacity plugin documents (the plugin page I
   opened did not cover them).
8. **Preemption and migration costs for Hex's runtimes.** The churn lessons use DL checkpoint-restart numbers
   (15-250 s). The cost of re-placing a process box, an OCI container or a microVM comes from Hex's runtime notes
   and bench records, not from these papers.
9. **Fairness over time for sandboxes that are never "finished".** Themis, Tiresias and Gavel assume jobs that
   end; long-lived agent sessions that idle and resume are not modelled in any source here.
10. **Hierarchical fairness correctness.** Kueue's own documentation says a proof that preemption chains cannot cycle
    "is needed for the Hierarchical case" ([20e]); Gavel supports only fairness above and fairness or FIFO below
    in hierarchies ([11] §4.3). A hierarchy of tenants, projects and agents is likely in Hex.

## References

All accessed 2026-10-22. Papers marked "PDF read" were downloaded and read as text; documentation pages were read
through a page-fetch tool that summarises, and are marked "doc". URLs are the ones opened.

1. Kay Ousterhout, Patrick Wendell, Matei Zaharia, Ion Stoica (UC Berkeley). [Sparrow: Distributed, Low Latency
   Scheduling](https://www.andrew.cmu.edu/user/gaurij/18-847F-Readings/sparrow.pdf). SOSP 2013 (3-6 Nov 2013);
   the SOSP version, hosted as a course reading; DOI http://dx.doi.org/10.1145/2517349.2522716. PDF read.
2. Eric Boutin, Jaliya Ekanayake, Wei Lin, Bing Shi, Jingren Zhou (Microsoft); Zhengping Qian, Ming Wu, Lidong Zhou
   (Microsoft Research). [Apollo: Scalable and Coordinated Scheduling for Cloud-Scale
   Computing](https://www.usenix.org/system/files/conference/osdi14/osdi14-paper-boutin_0.pdf). OSDI 2014 (6-8
   Oct 2014). PDF read.
3. Carlo Curino, Subru Krishnan, Konstantinos Karanasos, Sriram Rao and others (Microsoft). [Hydra: a federated
   resource manager for data-center scale analytics](https://www.usenix.org/system/files/nsdi19-curino.pdf). NSDI
   2019 (26-28 Feb 2019). PDF read.
4. Wu Xiang, Yakun Li, Yuquan Ren, Fan Jiang and others (ByteDance). [Gödel: Unified Large-Scale Resource
   Management and Scheduling at ByteDance](https://api.crossref.org/works/10.1145/3620678.3624663) (Crossref
   bibliographic record only; DOI https://doi.org/10.1145/3620678.3624663). SoCC '23, Santa Cruz, published 30-31
   Oct 2023. Paper not read.
   - 4a. ByteDance (CNCF member post). [Gödel Scheduler open-sourced: a unified scheduler for online and offline
     workloads](https://www.cncf.io/blog/2024/04/02/godel-scheduler-open-sourced-a-unified-scheduler-for-online-and-offline-workloads).
     2 Apr 2024. Doc.
   - 4b. KubeFM. [5,000 pods/second and 60% utilization with Gödel and Katalyst](https://kube.fm/5000-pods-second-yue-yin).
     Podcast transcript with a ByteDance guest; date not stated in the text read. Doc.
   - 4c. kubewharf/godel-scheduler. [docs/performance/best-practice.md](https://raw.githubusercontent.com/kubewharf/godel-scheduler/main/docs/performance/best-practice.md).
     Undated repository document, main branch as of 2026-10-22. Doc.
   - 4d. kubewharf/godel-scheduler. [docs/features/gang-scheduling.md](https://raw.githubusercontent.com/kubewharf/godel-scheduler/main/docs/features/gang-scheduling.md)
     and [docs/features/concurrent-scheduling.md](https://raw.githubusercontent.com/kubewharf/godel-scheduler/main/docs/features/concurrent-scheduling.md).
     Undated repository documents. Doc.
5. Michael Isard, Vijayan Prabhakaran, Jon Currey, Udi Wieder, Kunal Talwar, Andrew Goldberg (Microsoft Research).
   [Quincy: Fair Scheduling for Distributed Computing
   Clusters](https://www.sigops.org/s/conferences/sosp/2009/papers/isard-sosp09.pdf). SOSP 2009 (Oct 2009). PDF read.
6. Ionel Gog, Malte Schwarzkopf, Adam Gleave, Robert N. M. Watson, Steven Hand. [Firmament: Fast, Centralized
   Cluster Scheduling at Scale](https://www.usenix.org/system/files/conference/osdi16/osdi16-gog.pdf). OSDI 2016
   (2-4 Nov 2016). PDF read.
7. Panagiotis Garefalakis, Konstantinos Karanasos, Peter Pietzuch, Arun Suresh, Sriram Rao. [Medea: Scheduling of
   Long Running Applications in Shared Production Clusters](https://lsds.doc.ic.ac.uk/sites/default/files/medea-eurosys18.pdf).
   EuroSys 2018 (23-26 Apr 2018). PDF read.
8. Alexey Tumanov, Timothy Zhu, Jun Woo Park, Michael A. Kozuch, Mor Harchol-Balter, Gregory R. Ganger. [TetriSched:
   global rescheduling with adaptive plan-ahead in dynamic heterogeneous
   clusters](https://www.pdl.cmu.edu/ftp/CloudComputing/tetrisched-tumanov-eurosys16.pdf). EuroSys 2016 (18-21 Apr
   2016). PDF read.
9. Deepak Narayanan, Fiodar Kazhamiaka, Firas Abuzaid, Peter Kraft, Akshay Agrawal, Srikanth Kandula, Stephen Boyd,
   Matei Zaharia. [Solving Large-Scale Granular Resource Allocation Problems Efficiently with
   POP](https://arxiv.org/pdf/2110.11927). SOSP 2021; arXiv v1, 22 Oct 2021. PDF read.
10. Suhas Jayaram Subramanya, Daiyaan Arfeen, Shouxu Lin, Aurick Qiao, Zhihao Jia, Gregory R. Ganger. [Sia:
    Heterogeneity-aware, goodput-optimized ML-cluster
    scheduling](https://www.pdl.cmu.edu/PDL-FTP/BigLearning/sia_sosp23-final.pdf). SOSP 2023 (23-26 Oct 2023). PDF read.
11. Deepak Narayanan, Keshav Santhanam, Fiodar Kazhamiaka, Amar Phanishayee, Matei Zaharia. [Heterogeneity-Aware
    Cluster Scheduling Policies for Deep Learning
    Workloads](https://www.usenix.org/system/files/osdi20-narayanan_deepak.pdf). OSDI 2020 (4-6 Nov 2020). PDF read.
12. Aurick Qiao, Sang Keun Choe, Suhas Jayaram Subramanya, Willie Neiswanger, Qirong Ho, Hao Zhang, Gregory R.
    Ganger, Eric P. Xing. [Pollux: Co-adaptive Cluster Scheduling for Goodput-Optimized Deep
    Learning](https://www.usenix.org/system/files/osdi21-qiao.pdf). OSDI 2021 (14-16 Jul 2021). PDF read.
13. Kshiteej Mahajan, Arjun Balasubramanian, Arjun Singhvi, Shivaram Venkataraman, Aditya Akella, Amar Phanishayee,
    Shuchi Chawla. [Themis: Fair and Efficient GPU Cluster
    Scheduling](https://www.usenix.org/system/files/nsdi20-paper-mahajan.pdf). NSDI 2020 (25-27 Feb 2020). PDF read.
14. Juncheng Gu, Mosharaf Chowdhury, Kang G. Shin, Yibo Zhu, Myeongjae Jeon, Junjie Qian, Hongqiang Liu, Chuanxiong
    Guo. [Tiresias: A GPU Cluster Manager for Distributed Deep Learning](https://www.usenix.org/system/files/nsdi19-gu.pdf).
    NSDI 2019 (26-28 Feb 2019). PDF read.
15. Jun Woo Park, Alexey Tumanov, Angela Jiang, Michael A. Kozuch, Gregory R. Ganger. [3Sigma: Distribution-based
    cluster scheduling for runtime uncertainty](https://www.pdl.cmu.edu/PDL-FTP/CloudComputing/eurosys18-final49.pdf).
    EuroSys 2018 (23-26 Apr 2018). PDF read.
16. Hongzi Mao, Malte Schwarzkopf, Shaileshh Bojja Venkatakrishnan, Zili Meng, Mohammad Alizadeh. [Learning
    Scheduling Algorithms for Data Processing Clusters](https://arxiv.org/pdf/1810.01963). SIGCOMM 2019 (19-23 Aug
    2019); arXiv v4, 21 Aug 2019. PDF read.
17. Sangeetha Abdu Jyothi, Carlo Curino, Ishai Menache, Shravan Matthur Narayanamurthy, Alexey Tumanov, Jonathan
    Yaniv, Ruslan Mavlyutov, Iñigo Goiri, Subru Krishnan, Janardhan Kulkarni, Sriram Rao. [Morpheus: Towards
    Automated SLOs for Enterprise
    Clusters](https://www.usenix.org/system/files/conference/osdi16/osdi16-jyothi.pdf). OSDI 2016 (2-4 Nov 2016). PDF read.
18. Ali Ghodsi, Matei Zaharia, Benjamin Hindman, Andy Konwinski, Scott Shenker, Ion Stoica. [Dominant Resource
    Fairness: Fair Allocation of Multiple Resource
    Types](https://www.usenix.org/legacy/event/nsdi11/tech/full_papers/Ghodsi.pdf). NSDI 2011 (USENIX; date not
    printed in the PDF text). PDF read.
19. Robert Grandl, Ganesh Ananthanarayanan, Srikanth Kandula, Sriram Rao, Aditya Akella. [Multi-Resource Packing for
    Cluster Schedulers](https://home.cse.ust.hk/~weiwa/teaching/Fall15-COMP6611B/reading_list/Tetris.pdf).
    SIGCOMM 2014 (17-22 Aug 2014); a course-hosted copy of the SIGCOMM paper (DOI 10.1145/2619239.2626334). PDF read.
20. Kueue (Kubernetes SIG Scheduling). Documentation and design records; undated living documents as of 2026-10-22. Doc.
    - 20a. [Overview](https://kueue.sigs.k8s.io/docs/overview/)
    - 20b. [ClusterQueue](https://kueue.sigs.k8s.io/docs/concepts/cluster_queue/)
    - 20c. [Cohort](https://kueue.sigs.k8s.io/docs/concepts/cohort/)
    - 20d. [Preemption](https://kueue.sigs.k8s.io/docs/concepts/preemption/)
    - 20e. [Fair Sharing](https://kueue.sigs.k8s.io/docs/concepts/fair_sharing/)
    - 20f. [Admission Fair Sharing](https://kueue.sigs.k8s.io/docs/concepts/admission_fair_sharing/)
    - 20g. [KEP-1714 Fair Sharing](https://raw.githubusercontent.com/kubernetes-sigs/kueue/main/keps/1714-fair-sharing/README.md)
    - 20h. Scheduler performance test, large-scale configuration: [generator.yaml](https://raw.githubusercontent.com/kubernetes-sigs/kueue/main/test/performance/scheduler/configs/large-scale/generator.yaml)
      and [rangespec.yaml](https://raw.githubusercontent.com/kubernetes-sigs/kueue/main/test/performance/scheduler/configs/large-scale/rangespec.yaml)
      (README: https://raw.githubusercontent.com/kubernetes-sigs/kueue/main/test/performance/scheduler/README.md)
21. Volcano (CNCF project). Documentation and design records; undated living documents as of 2026-10-22. Doc.
    - 21a. [Scheduler introduction](https://volcano.sh/en/docs/schduler_introduction/)
    - 21b. [Plugins](https://volcano.sh/en/docs/plugins/)
    - 21c. [Queue](https://volcano.sh/en/docs/queue/)
    - 21d. [PodGroup](https://volcano.sh/en/docs/podgroup/)
    - 21e. [Agent scheduler design record](https://raw.githubusercontent.com/volcano-sh/volcano/master/docs/design/agent-scheduler.md)
    - 21f. [DRF design record](https://raw.githubusercontent.com/volcano-sh/volcano/master/docs/design/drf.md) (describes the kube-batch implementation)
    - 21g. [How to Tune Volcano Performance in Large-Scale Scenarios](https://volcano.sh/en/docs/user-guide/how_to_tune_volcano_performance)
