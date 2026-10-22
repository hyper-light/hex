# Cluster managers at scale: Kubernetes, Borg, Omega, Twine, RAS and Shard Manager

The evidence base for Hex's scheduler (CLAUDE.md §5): how the largest cluster managers place work, what
scale they reached, how they prioritise, isolate and handle heterogeneous hardware, and where they break.
Method and number kinds follow [the research method](../README.md#method): `MEASURED-INDEPENDENT` (a third
party measured it), `MEASURED-VENDOR` (the operator or project measured itself), `CLAIMED` (no stated
method), `DESIGN` (a fact from code or documents, not a speed). Percentiles are quoted exactly as reported;
"not published / not found" marks a gap, never an inference. Every cited document was opened and read on 2026-10-22
(see [References](#references)); anything derived by arithmetic on cited numbers is labelled as derived.

Two findings qualify the brief. First, **gang scheduling is no longer absent from the
Kubernetes core**: as of v1.37 a `Workload`/`PodGroup` API and a `GangScheduling` plugin ship as Beta, off by
default ([24], [18]). Second, **no source reports a task-placement rate near Hex's targets**: the best published
sustained figure for a Kubernetes control plane is 1,000 pod creations and bindings per second ([38]), and
Borg reports arrival rates "above 10 000 tasks per minute" in several cells ([1] §3.4) (see
[Lessons](#lessons-for-hex) for the arithmetic).

## Summary table

Kinds: MV = `MEASURED-VENDOR`, MI = `MEASURED-INDEPENDENT`, CL = `CLAIMED`, DE = `DESIGN`. "n/p" = not
published in the sources read. Bracketed numbers are [References](#references).

| System | Architecture | Scale reached | Decision throughput / latency | Prioritisation | Heterogeneity | Bottlenecks reported |
|---|---|---|---|---|---|---|
| **Kubernetes** (OSS, v1.37) | API server + etcd; one leader-elected kube-scheduler, serial scheduling cycle, concurrent binding; plugin framework [11][13][16] DE | Supported envelope 5,000 nodes, 150,000 pods, 300,000 containers, 110 pods/node [29][26] DE; OpenAI ran 7,500-node clusters [36] MV | CI regression floor 790 pods/s (5,000 fake nodes, 50,000 pods) [30] MV; 100-150 pods/s expected in 5,000-node load test [22] MV; 50.5 pods/s in a competitor's test [39] MI (not neutral); SLO: stateless pod startup p99 <= 5 s, only at churn <= 20/s [27] DE | int32 PriorityClass with preemption; ResourceQuota; no fair-share queues (Kueue, Volcano) [25][20] DE | Filter/Score plugins over labels, taints, affinity; scores only 5-50% of nodes [10][11][13] DE | Serial loop, 50 QPS client default, O(pods x nodes), LIST memory, watch fan-out, etcd request/size limits [13][15][19][34][36][32] |
| **GKE mega-clusters** | Managed Kubernetes; etcd replaced by a Spanner-backed store; Kueue for queueing, gang semantics and preemption [37][38] | 65,000 nodes supported, 250,000+ accelerators [37] CL; 130,000 nodes "in experimental mode" [38] MV | Up to 1,000 pod creates/s and binds/s; 130,000 pods up in 3 min 40 s; 39,000 pods preempted in 93 s; inference p99 pod startup about 10 s [38] MV | 3 priority classes plus Kueue quotas and preemption [38] | n/p | Storage (13,000 QPS of lease updates at 130,000 nodes), read amplification [38] |
| **Borg** (2015) | Cell = Borgmaster (5 replicas, Paxos, one elected master) + separate scheduler on cached state + Borglets [1] DE | Median cell about 10k machines; >10,000 tasks/min arrivals in several cells; master 10-14 cores, up to 50 GiB [1] MV | Online pass over pending queue < 0.5 s; full cell from scratch "a few hundred seconds" vs >3 days without caching, equivalence classes, relaxed randomisation; task start median about 25 s [1] MV | Priority bands (no prod-on-prod preemption); quota at admission; reclamation (about 20% of workload) [1] | Heterogeneous machines, hard/soft constraints, fine-grained requests, cell-compaction metric [1] | "Ultimate scalability limit" unknown; blast radius limits cell size; package install 80% of start [1] |
| **Borg 2019 trace** | Same system plus batch queueing scheduler, Autopilot, alloc sets [3] DE | 8 cells, 96.4k machines (12.0k/cell) [3] MV | Mean 3,360 jobs/h per cell (3.5x 2011); tasks to schedule 7x; median scheduling delay lower, tail longer [3] MV | 5 tiers; evictions rare (<0.2% of prod collections) [3] | 7 hardware platforms, 21 shapes; abstract GCU [3] | Top 1% of jobs = 99.2% of CPU: mice stuck behind hogs [3] |
| **Omega** (2013) | Shared state, many parallel schedulers, optimistic commit [2] DE | Simulated on 3 Google clusters; up to 32 batch schedulers; clusters absorb 2.5x-9.5x load [2] MV (simulated) | t_job 0.1 s + t_task 5 ms from production; conflict fraction > 1 once service t_job reaches about 10 s [2] MV | Per-scheduler policy; cluster-wide via priority-range limits and audit; all-or-nothing transactions [2] | Batch (>80% of jobs) vs service (55-80% of resources) [2] | Conflicts with long decisions; offer-based two-level starves large jobs [2] |
| **Twine** (ex-Tupperware) | Region-wide control plane sharded by entitlement; Resource Broker, Scheduler, Allocator, ReBalancer; app-level schedulers [4] DE | Largest shard about 170K machines; entitlement about 60K; job about 15K tasks; 1M machines/region is a design target, not reached [4] MV | Large allocator about 1,000 job allocations/s at peak (avg 36 tasks/job); cache hit > 99% [4] MV | Entitlement quotas; no preemption described; TaskControl; rate-limited destructive ops [4] | One small SKU; 17 host profiles; RRU [4] | Entitlement fragmentation; global allocators wrong abstraction; 30-40% utilisation [4] |
| **RAS** (Meta) | Async MIP assigns servers to reservations; allocator places containers inside a reservation [5] DE | Millions of servers, thousands of reservations [5] MV | Solve SLO 1 h: mean 1.8K s, p95 2.2K s, p99 2.45K s (region of several hundred thousand servers); replacements < 1 min [5] MV | Reservations with guaranteed capacity; emergency out-of-band path [5] | RRU per hardware type; 9 types, 12 subtypes [5] | MIP ceiling about 10M variables; up to 1 h request delay; extra preemption [5] |
| **Shard Manager** (Meta) | App-level scheduler on Twine; mini-SM control-plane shards; solver with local search [6] DE | About 100M shards on > 1M servers; largest app about 19K servers, 2.6M shards [6] MV | Production allocator solve P90 about 10 s, P99 about 50 s [6] MV | Hard constraints plus soft goals batched by priority; emergency vs periodic [6] | Capacity and load models; shard load varies 20x [6] | Solver scalability; planned events 1000x unplanned [6] |
| **Multi-cluster** | KubeFed (retired); Karmada cluster selection over member clusters; MultiKueue [40][41][38] | Karmada: 100 simulated clusters x 5,000 nodes [41] MV (fake nodes) | Karmada cluster_schedule P50 12 ms, P99 32 ms; distribution P99 1,298 ms (push) [41] MV | n/p | n/p | KubeFed archived 2023 [40]; federation "adds complexity" [4] |

## Kubernetes (kube-scheduler, control plane, large clusters, multi-cluster)

Version context: the latest release at reading time is v1.37.1, published 2026-09-23 ([44]). Source-code
facts below are pinned to tag `v1.37.1` ([13]-[18], [45]) and design facts to the `kubernetes/enhancements`
commit `0c8d11e9a13e12d2ef75d2d78af5ac00d8189046` ([19]-[22]); statuses of features are those in that code
and those KEPs' `kep.yaml`. Number kinds are marked as elsewhere.

### kube-scheduler: how a placement is made (`DESIGN`)

- **One active scheduler, one decision at a time.** The scheduler is a controller that watches unassigned
  pods ([10]). By default it runs with leader election on (`LeaderElect` true, lease 15 s, renew deadline
  10 s, retry 2 s), so replicas are active/passive ([16]). `ScheduleOne` is documented as "serialized on the
  scheduling algorithm's host fitting" ([13]); the official docs say "Scheduling cycles are run serially,
  while binding cycles may run concurrently" ([11]). In the code, `scheduleOnePod` runs `schedulingCycle`
  synchronously and then `go sched.runBindingCycle(...)` ([13]). The KEP for async preemption states the
  consequence: "The scheduler is basically only one in a cluster, and hence scheduling throughput is the
  crucial metric", with pods scheduled "one by one within the scheduling cycle" ([21]).
- **Extension points** ([11]): PreEnqueue, QueueSort (one plugin), PreFilter, Filter, PostFilter (preemption
  is the typical PostFilter), PreScore, Score, NormalizeScore, Reserve/Unreserve, Permit (approve, deny, or
  wait with timeout), then PreBind, Bind, PostBind in the binding cycle; plugins are compiled in (the framework is "a set of
  'plugin' APIs that are compiled directly into the scheduler"). Filtering may run per node concurrently: the default
  `parallelism` is 16 goroutines ([15], [17]).
- **Reserve then bind asynchronously.** The scheduler "assumes" the pod on the chosen node in its cache so the
  next cycle accounts for it, then binds in the background ([13], [21]). Each bind is an API call, and the
  scheduler's API client is rate-limited: the scheduler "has an opinion about QPS/Burst", defaulting to 50 QPS
  and burst 100 ([15]).
- **Sampling instead of scoring every node.** `numFeasibleNodesToFind` stops searching once enough feasible
  nodes are found: below 100 nodes it checks all; otherwise `percentageOfNodesToScore` (0 means adaptive) is
  `50 - numNodes/125`, floored at 5%, and never fewer than 100 nodes ([13]). The docs give the same curve: "50%
  for a 100-node cluster and ... 10% for a 5000-node cluster. The lower bound ... is 5%" ([12]); by the formula
  a 130,000-node cluster scores 5% (6,500 nodes), which is arithmetic on [13], not a published figure. The
  code comment calls the 100-node/5% floors "semi-arbitrary" and says the minimum "in turn helps ensure a minimum
  level of spreading" ([13]). The search starts at a rotating index (`nextStartNodeIndex`) "to make sure that all the
  nodes have a chance to be searched" ([13]). The docs warn that "a Node which could possibly score a higher
  value for running the given Pod might not even be passed to the scoring phase. This would result in a less
  than ideal placement" ([12]). This is Borg's "relaxed randomization" ([1] §3.4) without Borg's score
  caching or equivalence classes, until opportunistic batching (below).
- **Queues** ([14]): `activeQ` (a heap ordered by the QueueSort plugin), `backoffQ` and `unschedulable`
  entities. Defaults: initial backoff 1 s, maximum backoff 10 s, an entity may sit as unschedulable for at most
  5 minutes before being retried, and a flush loop runs every 30 s. Since v1.34 *QueueingHint* callbacks
  (stable, on by default) requeue a rejected pod only when an event could make it schedulable ([11]);
  `SchedulerPopFromBackoffQ` (beta since 1.33) pops from the backoff queue when the active queue is empty
  ([18]). I found no capacity bound or overflow rule on these queues in [14]; the number of pending pods is
  bounded only by what the API server stores.
- **Cost model and the equivalence-class history.** SIG-Scheduling's own KEP states: "Today our scheduling
  algorithm is O(num pods x num nodes)" ([19]). Borg's equivalence classes ([1] §3.4) were tried in Kubernetes
  as the "Equivalence Cache (eCache)" in 2018 and "retracted because it became extremely complex", because it
  "was still O(num nodes)", was "tightly coupled with plugins" and its lookups "were, in many cases, more
  expensive than the actual computation" ([19]). The 2025-26 replacement is **opportunistic batching**
  (KEP-5598; beta since v1.35, on by default, GA targeted v1.38): a *pod scheduling signature* lets the
  scheduler reuse one pod's filter-and-score result as node hints for following pods with the same
  signature, with a batch discarded after `maxBatchAge` of 500 ms because "cached nodes are not re-filtered
  each cycle"; pods using `PodTopologySpread` or `InterPodAffinity` are "unsignable" and excluded; the KEP
  admits "No prior production history" ([19], [18]). Its best case is benchmarked in `scheduler_perf`
  (20,000 nodes, 20,000 one-pod-per-node pods, batch sizes up to 20,000) but that config sets no throughput
  threshold and I found no recorded result ([30]); the speed-up is therefore unmeasured in the sources read.
- **Async API calls.** Binding was already asynchronous; KEP-5229 makes the remaining API calls in the
  scheduling cycle asynchronous ("One of the bottlenecks is the API calls done during the scheduling cycle");
  it is beta since v1.34 but its gate `SchedulerAsyncAPICalls` is **off by default** in v1.37.1 ([22], [18]).

### Preemption: mechanism and cost

- Preemption is a PostFilter plugin. The dry run shortlists "numCandidates = max(numNodes *
  minCandidateNodesPercentage, minCandidateNodesAbsolute)" with defaults 10% and 100 ([45], [15]). Documented
  limits: victims keep their graceful-termination period, which "creates a time gap" before the preemptor can
  run; PodDisruptionBudgets are honoured only "best effort"; preemption does not consider inter-pod affinity
  to lower-priority pods and "scheduler does not perform cross-node preemption" ([25]).
- Cost: preemption makes API calls (status updates, deletes) inside the scheduling cycle, which "could block
  the scheduling throughput", and `scheduler_perf` "shows currently the preemption scenario takes too long time,
  compared to others" ([21]). `SchedulerAsyncPreemption` (alpha 1.32, beta and default-on since 1.33) moves
  that work off the loop ([21], [18]). In v1.37.1 the `PreemptionBasic` 5,000-node benchmark is commented out
  with "This test case always seems to fail" ([30]); the linked 2022 issue's author could not tell whether the
  cause was the scheduler or the harness, and reported that it "seems to take 1 minute to schedule 1 Pod" in
  that case on one machine ([31]).
  The measured floor for ordinary pods interleaved with preemptions at 5/s is 570 pods/s (5,000 nodes) ([30]).
- Operators who need fast, large preemption use a layer above: in Google's 130,000-node run "Kueue's advantage
  over kube-scheduler: preemption happens much faster, and switching between workloads is almost
  instantaneous"; at its peak 39,000 pods were preempted in 93 seconds ([38]) (`MEASURED-VENDOR`).
- Priorities are an int32 range up to 1,000,000,000 for users, and "In a cluster where not all users are
  trusted, a malicious user could create Pods at the highest possible priorities, causing other Pods to be
  evicted/not get scheduled"; ResourceQuota is the guard ([25]). Compare Borg, where priority bands are
  paired with quota-at-admission ([1] §2.5).

### Isolation, accounting and heterogeneity (`DESIGN`)

- **The scheduler accounts by requests, not usage.** "The scheduler ensures that, for each resource type, the sum of
  the resource requests of the scheduled containers is less than the capacity of the node", and it "still refuses to
  place a Pod on a node if the capacity check fails" even when measured usage is very low ([48]). Contrast Borg, which
  places non-production work against *reservations* that decay toward measured usage so that about 20% of the
  workload runs in reclaimed resources ([1] §5.5). Nothing in [48] describes usage-based reclamation in the
  default scheduler path.
- **Isolation is enforced on the node, not by the scheduler.** On Linux "the container runtime typically configures
  kernel cgroups"; a CPU limit is a hard throttle ceiling, a CPU request "typically defines a weighting", and the
  memory request "is mainly used during (Kubernetes) Pod scheduling" ([48]). Each pod gets a QoS class
  (Guaranteed, Burstable, BestEffort) from its requests and limits, and when a node runs out of resources "Kubernetes
  will first evict BestEffort Pods ..., followed by Burstable and finally Guaranteed Pods" ([49]). Borg's analogue
  splits compressible resources (throttled) from non-compressible ones (reclaimed by killing) and tunes the kernel
  CPU scheduler for latency-sensitive tasks ([1] §6.2).
- **Heterogeneity** is expressed through node labels, taints and affinity in Filter and Score plugins ([10], [11])
  and, for devices, through Dynamic Resource Allocation, stable since v1.35 (first available v1.30): device classes,
  CEL-expression filtering of device attributes, and sharing of a device among pods ([50]). There is no
  normalised compute unit in the sources read; Borg's GCU and Meta's RRU are the contrast ([3] §3, [5] §3.1).

### Gang scheduling: no longer absent, but new

Gang scheduling lived outside the core for years: KEP-583 "Coscheduling" (created 2018, still
`provisional`) chose to build it in `kube-batch` as CRDs ([23]); KEP-4671 lists Volcano, the Coscheduling
plugin, a Preferred Networks plugin and Kueue as external implementations ([20]); OpenAI hit exactly the
deadlock gang scheduling prevents ("if two experiments each requested 100% of the cluster's capacity ...
Kubernetes might schedule only half of each experiment's pods") and used the Coscheduling plugin ([36]).
**As of v1.37 this has changed:** the `Workload`/`PodGroup` API and a `GangScheduling` plugin exist in core as
**Beta since v1.37, disabled by default** (feature gate `GenericWorkload`; alpha v1.35; KEP targets GA in
v1.38) ([24], [18], [20]). The group is scheduled in one "PodGroup scheduling cycle" on a single cluster
snapshot, with a `minCount` quorum; if `minCount` cannot be met, "none of the Pods are scheduled" and the
group returns to the unschedulable queue ([24], [20]) (the KEP dropped its scheduling-timeout field for
alpha because "it won't be obvious for majority of users how to set it", [20]). `ScheduleOne` now handles a pod or a
pod group as one queue entity, still serially ([13]). The KEP records, under "Exacerbating the race window by
proceeding directly to binding", the accepted risk that a long group cycle on one snapshot may bind onto
nodes that failed meanwhile, and explicitly leaves fairness and
multiple queues out: "Bring fairness or multiple workload queues in kube-scheduler. Kueue and Volcano.sh
will continue to provide this" ([20]). Related and newer still: topology-aware workload scheduling (alpha
1.36, beta 1.38 target), workload-aware preemption (KEP-5710), composite pod groups (alpha 1.37) ([18], [24]).

### Documented limits and SLOs (sig-scalability)

- **Supported envelope** (`DESIGN`, [29], [26]): "Kubernetes v1.37 supports clusters with up to 5,000 nodes",
  no more than 110 pods/node, 150,000 total pods and 300,000 total containers; per-resource-type objects
  150,000; object size 1.5 MB; 3,000 pods per namespace. The thresholds "are NOT hard limits - crossing the
  limit results in degraded performance"; they assume "OSS distribution of Kubernetes setup running on sharded
  etcd", and release-blocking scalability tests have run on kops since December 2025 ([26]).
- **Reading the envelope against the large clusters** (my inference from [26], [29], [37], [38], not a statement in
  them): 5,000 nodes is the tested envelope of the open-source distribution on etcd; Google's 65,000- and
  130,000-node clusters changed the storage layer and added Kueue above the scheduler, and the two posts I read
  describe no change to the scheduling algorithm itself, so the evidence points at the control plane's storage and
  read paths, and at per-pod API round trips, as the binding limits rather than at node scoring.
- **SLOs** (`DESIGN`, [27]): mutating API calls on a single object, p99 per cluster-day <= 1 s; non-streaming
  reads p99 <= 1 s at resource scope and <= 30 s at namespace or cluster scope; stateless pod startup (creation
  to all containers started and observed via watch, excluding image pull and init containers) p99 <= 5 s.
  The SLOs bind only under two conditions: the "you promise, we promise" thresholds and "Cluster churn is <=
  20", where churn is "#(Pod spec creations/updates/deletions) + #(user originated requests) in a given
  second" ([27]). Only pods that are "schedulable ... without causing any preemption" count toward the startup
  SLI; time to pull images, run init containers and preempt is excluded ([28]). So the official SLO describes a
  cluster doing at most about 20 pod or user operations per second, far below the burst rates reported
  later in this section; those rates carry no SLO.
- **Measured scheduling throughput** (all `MEASURED-VENDOR` unless stated):
  - `scheduler_perf` is the project's scheduler integration benchmark ("Write scheduler integration test but focus
    on performance measurement"), driven with fake nodes (4 CPU, 32 GiB, 110 pods) and trivial pause pods, no
    kubelets; its README example pins a run to six CPUs ([30]). Its CI regression floors (the
    minimum of the averages over the last 250 runs, "set ... somewhat pessimistically") are 790 pods/s for
    5,000 nodes and 50,000 pods and 680 pods/s for 5,000 nodes and 10,000 pods ([30]). These are floors for
    CI, not peaks, on unspecified hardware.
  - In a full 5,000-node load test, SIG-Scheduling's expectation is "throughput around 100-150 pods/s" and
    `scheduling_algorithm_duration_seconds` averaging <= 10 ms ([22], citing perf-dash; the dashboard itself is
    JavaScript-rendered and was not opened).
  - Apache YuniKorn, a competing scheduler project, measured kube-scheduler at 50.5 pods/s in each of its
    5,000-pod KWOK tests against 833 pods/s for YuniKorn (best case), and measured the API server creating 5,000
    pods in 38 s, 131.6 pods/s ([39]) (`MEASURED-INDEPENDENT` of the Kubernetes project, not neutral). The constant 50.5
    pods/s is consistent with the scheduler's default 50 QPS client limit ([15]); the page does not say what
    QPS it configured, so this explanation is an inference.
  - Google's experimental 130,000-node GKE run sustained "up to 1,000 operations per second for both Pod
    creation and Pod binding" and about 750 pods/s average creation in the batch phase ([38]); see below.

### Control-plane bottlenecks: etcd, API server, watches, LIST

- etcd is one Raft group: "the minimum time to finish an etcd request is the network Round Trip Time (RTT)
  between members, plus the time fdatasync requires"; the default request limit is 1.5 MiB and the default
  storage quota 2 GiB (8 GiB suggested maximum) ([32], [33]). etcd's own published benchmark (3 x 8 vCPU
  GCE, etcd 3.2.0, 256-byte values, 1,000 clients) reached about 44,000-50,000 writes/s at 20-22 ms average
  latency ([33], `MEASURED-VENDOR`, 2017-era hardware); Kubernetes objects are larger and every write also feeds
  watches, so this is an upper bound for the store, not for the API server. Kubernetes advises storing Event
  objects in a separate etcd for large clusters ([29]).
- LIST is the memory hazard: "The kube-apiserver is vulnerable to memory explosion ... only a few LIST requests
  might cause serious disruption", with "around O(5*the_response_from_etcd) of temporary memory consumption";
  "Neither priority and fairness nor Golang garbage collection is able to protect the system"; recovery of a
  large cluster "can lead to a very expensive storm of LISTs" ([34]). The fix, streaming lists (`WatchList`),
  has been beta since v1.32 with stable targeted for v1.38 ([34]).
- Read amplification: in a 5,000-node cluster with 30 pods per node a kubelet's request for the pods on its node makes
  the API server list all 150,000 pods from etcd and filter to 30, "for each list request from each of the 5k
  kubelets"; serving consistent reads from the watch cache (KEP-2340, stable v1.34) removes that ([35]).
- Object size is a live design constraint, not a footnote: KEP-4671 split `PodGroup` out of `Workload` because "Large
  Workloads (i.e., large number of PodGroups) may easily hit the 1.5MB etcd object limit" and because updating one
  group's status "would require read-modify-write on the central massive Workload object" ([20]).
- Watch fan-out and per-node cost, from a real 7,500-node cluster (`MEASURED-VENDOR`, [36]): a Service whose
  endpoints include every node made each node join or leave fire a watch to every node, "$N^2$ and enormous,
  occasionally 1GB/s or more"; EndpointSlices "brought this load down 1000x"; API server heap reached "up to
  70GB ... per API Server"; adding hundreds of nodes at once "can overload API server capacity".
- GKE replaced etcd with a Spanner-backed store that implements the etcd API (65,000 nodes, [37]); at 130,000
  nodes node leases alone needed "13,000 QPS" and the cluster held "over 1 million objects" ([38]).

### Large-cluster reports (all operator-reported; none independent)

| Report | Scale and workload | What it says about the scheduler and control plane |
|---|---|---|
| OpenAI, Jan 2021 [36] | 7,500-node clusters; many jobs are one pod per node, MPI | "relatively low strain on the scheduler" because no bin-packing, but "strain on the kube-scheduler is spiky. A new job may consist of many hundreds of pods all being created at once"; 5 API servers and 5 etcd nodes, Events in their own etcd; "Traditional job scheduling systems have a lot of different features available to fairly run work between competing teams, which Kubernetes does not have", so they built team taints and balloon pods; gang scheduling by the Coscheduling plugin; clusters are isolated spokes that "cannot talk to one another" |
| GKE, Nov 2024 [37] | 65,000 nodes supported (was 15,000), over 250,000 accelerators in one cluster | etcd replaced by a Spanner-based store; control plane re-architected for "high churn"; no scheduler throughput numbers published |
| GKE, Nov 2025 [38] | 130,000 nodes "in experimental mode", twice the supported limit | "sustained Pod throughput of 1,000 Pods per second", 130,000 pods up in 3 min 40 s, 39,000 pods preempted in 93 s with Kueue, inference p99 pod startup about 10 s; job queueing, fairness and gang semantics come from Kueue, not kube-scheduler ("The default Kubernetes scheduler is designed to schedule individual Pods") |

### Multi-cluster

- **KubeFed** (federating the API) is retired: the repository now lives at `kubernetes-retired/kubefed`, is
  archived, last pushed 2023-03-24, and its README says it "has been archived and is no longer active
  development" ([40]). SIG-Multicluster now owns APIs rather than a federated control plane: `about-api`,
  `cluster-inventory-api`, `mcs-api`, `multicluster-runtime` and `work-api` ([43]).
- **Karmada** was accepted to CNCF on 2021-09-14, became incubating on 2023-12-12 and graduated on 2026-09-03
  ([42]). Its 2022 scale report (Karmada v1.3.0-4-g1f13ad97 on Kubernetes v1.23.10, guest post by a maintainer) tested 100 member clusters with 5,000
  nodes and 20,000 pods each, 500,000 nodes and 2,000,000 pods in total, but nodes and pods were simulated with
  `fake-kubelet` on `kind` control planes with single-instance components, so it measures control-plane object
  handling, not placement under load ([41], `MEASURED-VENDOR`). Results: cluster-selection ("cluster_schedule")
  P50 12 ms, P90 15 ms, P99 32 ms; resource distribution P50 706 ms, P90 899 ms, P99 1,298 ms in push mode (SLO
  2,000 ms); cluster registration P50 5,356 ms, P99 6,904 ms ([41]). The authors state the test "is not designed
  for stress testing Karmada".
- **MultiKueue**: Google names it as the tool it is investing in for AI platforms beyond about 100,000 nodes,
  because power limits push clusters "ideally distributed across multiple data centers" ([38]); the
  Twine paper's critique is that federation "adds complexity as a scale-out solution" because job metadata is
  split across cluster managers ([4] §3.3).

## Google Borg

Source: [1] (Verma et al., EuroSys 2015; Google, 18 pages; sections 1-8 read, figures not transcribed). All numbers below are reported by
the operator about its own production (`MEASURED-VENDOR`) unless marked `DESIGN`.

**Architecture** (`DESIGN`, [1] §3). A *cell* is a set of machines, one logically centralised *Borgmaster*
and a *Borglet* agent per machine. The Borgmaster is replicated five times; a single elected master is the
Paxos leader and the only state mutator, and every replica keeps most cell state in memory plus a Paxos
store on local disk ([1] §3.1). The scheduler is a separate process that works on a cached copy of cell
state, sends its assignments to the elected master, and the master "will accept and apply these assignments
unless they are inappropriate (e.g., based on out of date state)", which Borg itself calls "quite similar in
spirit to the optimistic concurrency control used in Omega" ([1] §3.4). The Borgmaster polls each Borglet
every few seconds, which "gives Borgmaster control over the rate of communication, avoids the need for an
explicit flow control mechanism, and prevents recovery storms"; stateless *link shards* aggregate and
compress Borglet reports to differences ([1] §3.3). Running tasks keep running if every Borgmaster replica
fails ([1] §3.3, §4).

**Scale reached.**
- "Our median cell size is about 10 k machines after excluding test cells; some are much larger"; a cluster
  "usually hosts one large cell" ([1] §2.2).
- "A single Borgmaster can manage many thousands of machines in a cell, and several cells have arrival rates
  above 10 000 tasks per minute. A busy Borgmaster uses 10-14 CPU cores and up to 50 GiB RAM" ([1] §3.4).
  This is an arrival rate for several cells; the paper gives no percentile distribution of per-task
  scheduling latency beyond the half-second pass figure below.
- Borg runs "hundreds of thousands of jobs, from many thousands of different applications" ([1] abstract);
  SREs manage "a few tens of thousands of machines per person" ([1] §2.6).
- Master election and failover "typically takes about 10 s, but can take up to a minute in a big cell because
  some in-memory state has to be reconstructed"; the Borgmaster achieves "99.99% availability in practice"
  ([1] §3.1, §4).

**How placement is decided** (`DESIGN`, [1] §3.2). The scheduler scans the pending queue "from high to low
priority, modulated by a round-robin scheme within a priority to ensure fairness across users and avoid
head-of-line blocking behind a large job". It works per task, in two parts: *feasibility* (constraints
plus "available" resources, counting resources held by lower-priority tasks that can be evicted) and
*scoring*, driven by the number and priority of preempted tasks, package locality, spread across power and
failure domains, and packing quality. Borg first used E-PVM scoring, which spread load and fragmented ("worst
fit"), and pure best fit, which punished bursty loads; the hybrid it uses now "provides about 3-5% better
packing efficiency ... than best fit for our workloads" ([1] §3.2). If the chosen machine lacks room Borg
preempts lower-priority tasks "from lowest to highest priority" and requeues them rather than migrating them.

**Why one scheduler keeps up** (`MEASURED-VENDOR`, [1] §3.4). Three techniques are named: *score caching*
(scores are cached until machine or task properties change; ignoring small resource changes reduces
invalidations), *equivalence classes* (feasibility and scoring are done once per group of tasks with identical
requirements) and *relaxed randomization* (machines are examined "in a random order until it has found
'enough' feasible machines to score", which the paper likens to Sparrow's batch sampling "while also
handling priorities, preemptions, heterogeneity and the costs of package installation"). The measured effect:
"scheduling a cell's entire workload from scratch typically took a few hundred seconds, but did not finish
after more than 3 days when the above techniques were disabled. Normally, though, an online scheduling pass
over the pending queue completes in less than half a second" ([1] §3.4). The earlier design was "a simple,
synchronous loop that accepted requests, scheduled tasks, and communicated with Borglets"; the fixes were to
split the scheduler into its own process, add threads for Borglet communication and read-only RPCs, and shard
those across the five replicas, which "keep the 99%ile response time of the UI below 1 s and the 95%ile of
the Borglet polling interval below 10 s" ([1] §3.4). Task start is slow by comparison: "median typically
about 25 s", of which package installation is "about 80%" ([1] §3.2).

**Prioritisation and isolation.**
- Priority bands, in decreasing order: monitoring, production, batch, best effort; "we disallow tasks in the
  production priority band to preempt one another" to remove preemption cascades ([1] §2.5).
- *Quota* decides admission, not scheduling: "jobs with insufficient quota are immediately rejected upon
  submission"; quota is a vector of resources at a priority for a period (typically months), priced higher at
  higher priority; "every user has infinite quota at priority zero"; quota "reduces the need for policies like
  Dominant Resource Fairness" ([1] §2.5).
- Resource reclamation: reservations decay toward measured usage after 300 s; prod tasks are placed against
  limits so "they never rely on reclaimed resources", non-prod against reservations; "about 20% of the
  workload ... runs in reclaimed resources in a median cell" ([1] §5.5).
- All tasks run in cgroup containers; latency-sensitive (LS) and batch appclasses; compressible resources
  (CPU, disk bandwidth) are throttled, non-compressible (memory, disk) are reclaimed by killing from lowest
  to highest priority; Google's CFS needed "substantial tuning to support both low latency and high
  utilization" ([1] §6.2).
- Sharing prod and non-prod: "segregating prod and non-prod work would need 20-30% more machines in the
  median cell"; adding a task raises other tasks' CPI by 0.3% and CPU performance is "about 3% worse in
  shared cells" (CPI samples explain about 5% of variance) ([1] §5.2).

**Heterogeneous hardware** ([1] §2.2-2.3, §5.1). Machines "are heterogeneous in many dimensions: sizes (CPU,
RAM, disk, network), processor type, performance, and capabilities such as an external IP address or flash
storage", and "Borg isolates users from most of these differences". Each resource dimension is requested
independently "at fine granularity; we don't impose fixed-sized buckets or slots" (Fig. 8 shows no bucket
size fits most tasks). Constraints are hard or soft. Efficiency is evaluated with *cell compaction* (the
smallest cell that still fits a workload, re-packed from scratch), because average utilisation is too crude
for heterogeneous machines and constrained jobs.

**Bottlenecks, failures and lessons the authors report.**
- "We are not sure where the ultimate scalability limit to Borg's centralized architecture will come from; so
  far, every time we have approached a limit, we've managed to eliminate it" ([1] §3.4). Cell size is
  limited by blast radius, not throughput: "These goals, not scalability limitations, are the primary
  argument against larger cells" ([1] §4).
- Failures are the norm; Borg rate-limits rescheduling after machine loss "because it cannot distinguish
  between large-scale machine failure and a network partition", and avoids repeating task:machine pairings
  that crash ([1] §4).
- Jobs as the only grouping mechanism was a mistake (no first-class multi-job service); one IP per machine
  forced port scheduling; the API grew about 230 parameters for power users and became hard for casual users
  ([1] §8.1). Introspection ("why pending?") and a kernel-like master with satellite services were wins
  ([1] §8.2).
- Borg is "one size fits all" in RPC, state machine and scheduler policy, which "have grown in size and
  complexity over time"; "scalability has not yet been a problem" ([1] §7).

## Google Omega

Source: [2] (Schwarzkopf, Konwinski, Abd-El-Malek, Wilkes; EuroSys 2013; read the architecture, simulation
and results sections). Omega is a scheduler architecture and an experiment, not a measured production fleet:
the numbers below come from the authors' simulators driven by Google production traces, with decision-time
parameters taken from production measurements (`MEASURED-VENDOR`, simulated).

**Architecture** (`DESIGN`, [2] §3). Three designs are compared. *Monolithic*: one scheduler, one code path
(Google's then-current scheduler "is effectively monolithic, although it has acquired many optimizations
over the years to provide internal parallelism and multi-threading"). *Two-level* (Mesos, YARN-like):
a central allocator offers resources to frameworks, with pessimistic concurrency control and no global view,
so a framework "cannot support preemption or policies requiring access to the whole cluster state".
*Shared state* (Omega): every scheduler gets "full access to the entire cluster", works on a private
frequently-refreshed copy of cell state, and commits placements to the master copy as an atomic
transaction; "at most one such commit will succeed in the case of conflict" and the loser resyncs and retries.
Schedulers may use incremental transactions (accept all but the conflicting changes) or all-or-nothing
transactions, which gives gang scheduling without resource hoarding ([2] §3.4). Cluster-wide policy (relative
priority, fairness) is enforced by limiting the priority range a scheduler may use and by post-hoc audit
rather than checks on the critical path ([2] §3).

**Why Google moved: engineering, not throughput.** "We have found that it is surprisingly difficult to support
a wide range of policies in a sustainable manner using a single-algorithm implementation. In the end, this
kind of software engineering consideration, rather than performance scalability, was our primary motivation"
([2] §3.1). The service scheduler places tasks to resist independent and coordinated failures, "an NP-hard
chance-constrained optimization problem with tens of failure domains", and the previous implementation "could
take tens of seconds"; a batch job queued behind it suffers head-of-line blocking ([2] §2.1).

**Workload facts** (`MEASURED-VENDOR`, May 2011 traces of three clusters, [2] §2.1): "Most (>80%) jobs are
batch jobs, but the majority of resources (55-80%) are allocated to service jobs" ([2] §2.1); of the
long-running high-priority service jobs, "20-40% of them run for over a month" ([2] §2.1); "a common
production service level objective (SLO) for job wait time is 30s" ([2] §4).

**Scheduling-cost model and results** (`MEASURED-VENDOR`, simulated, [2] §4). Decision time is modelled as
`t_job + t_task x tasks_per_job`, with "somewhat conservative estimates from measurements of our current
production system's behavior: t_job = 0.1 s and t_task = 5 ms". Reported outcomes:
- Monolithic: not scalable; even with a batch fast path, head-of-line blocking remains ([2] §4.4).
- Mesos-style two-level: with long service decisions, batch jobs starve while a framework holds an offer, and
  "a number of jobs are abandoned because they did not finish scheduling their tasks by the 1,000-attempt
  retry limit" ([2] §4.2).
- Omega: wait times "comparable to those for multi-path monolithic", no head-of-line blocking, "conflicts and
  interference are relatively rare" at default parameters. At cluster B (one of the largest, busiest) "once
  t_job (service) reaches about 10s, the conflict fraction increases beyond 1.0" and the 30 s wait SLO is
  missed even though the scheduler is not saturated; service-scheduler busyness is "about 40% higher" than in
  the no-conflict case. It "can support t_task (service) of 1 second (at a t_job (service) of 0.1s), resulting
  in a conflict fraction <= 0.2" ([2] §4.3, §5).
- Scaling the batch workload: cluster A "scales to about 2.5x the original workload before failing to keep
  up, while clusters B and C scale to 6x and 9.5x"; with load-balanced parallel batch schedulers, scheduler
  busyness improves "at least up to 32 batch schedulers" while conflict fraction rises ([2] §4.3).

**Gang scheduling and hoarding.** All-or-nothing placement "may be needed by some jobs (e.g., MPI programs),
but can unnecessarily delay the start of others that can make progress with only a fraction of their
requested resources (e.g., MapReduce jobs)"; incremental acquisition "can lead to deadlock if no backoff
mechanism is provided, while hoarding reduces cluster utilization and can also cause deadlock" ([2] §3).

## Google "Borg: the Next Generation" (2020 trace analysis)

Source: [3] (Tirmazi et al., EuroSys 2020), an analysis of Google's published May-2019 Borg trace against the
2011 trace. The numbers are `MEASURED-VENDOR` (Google analysing Google's own cells) but the trace is public, so
they are reproducible by a third party.

- **Scale of the trace** ([3] Table 1): 2011, 1 cell, 12.6k machines; 2019, 8 cells, 96.4k machines, "12.0k"
  machines per cell, 7 hardware platforms (was 3), 21 machine shapes (was 10), priority values 0-450.
- **Arrival rate** ([3] §6.1-6.3): the mean job arrival rate per cell rose "3.5x from 964 jobs per hour in
  2011 to 3360 jobs per hour in 2019"; the median "3.7x from 885 to 3309 jobs per hour"; "the number of tasks
  needing to be scheduled has increased by a factor of 7" (introduction) while cell size is comparable; the
  ratio of resubmitted to new tasks went "from 0.66:1 to 2.26:1", i.e. "considerably more 'churn'". Scheduling
  delay (job ready to first task running): "median scheduling delays have actually decreased, although the tail
  is longer for the last 28% of jobs", mostly best-effort-batch and mid-tier jobs, which have more tasks
  (80th percentile best-effort-batch job: 25 tasks; 95th percentile: 498). The paper gives these as CCDF
  figures; no p99.9 or maximum is quoted in the text.
- **Heavy tail** ([3] §7): "the largest 1% of jobs comprise 99.2% of the CPU load (99.1% of the memory load),
  and the largest 0.1% of jobs comprise 93.1% (92.6%) of the load"; squared coefficient of variation C^2 = 23k
  (CPU-hours) and 43k (memory-hours). The authors' conclusion for scheduler design: "one must find a way to
  schedule jobs so that the mice are either prioritized over the hogs or are isolated from them in some other
  way", because by Pollaczek-Khinchine mean queueing delay grows with C^2 "even when the system load is low".
- **Tiers** ([3] §1): free, best-effort batch (managed by a queued batch scheduler), mid, production and
  monitoring; "Borg will evict lower-tier jobs in order to ensure production tier jobs receive their expected
  level of service". Evictions are rare and SLO-capped: of 24.8M collections "only 0.79M of them (3.2%)
  experience any instance evictions, and 96.6% of those collections are in non-production tiers"; for the
  production tier "<0.2% of collections have any instances evicted" ([3] §5.2).
- **Heterogeneity** ([3] §3): "to help its users better handle machine heterogeneity, Borg has switched to using
  abstract Google Compute Units (GCUs) instead of physical CPU cores"; "an allocation of 1 GCU should provide
  about the same amount of computational power on any machine in the fleet".
- **Over-commitment** ([3] §4): the sum of limits is "consistently allocated well above 100% of the deployed
  capacity", i.e. "Borg is using statistical multiplexing to over-commit resources"; automatic vertical scaling
  (Autopilot) "reduc[es] the peak NCU slack by more than 25% for the vast majority of jobs" with
  unconstrained autoscaling.
- **Multiple schedulers** ([3] §3): "like Omega, Borg now supports multiple schedulers, including a batch
  scheduler that manages the aggregate batch job workload for throughput by queueing jobs until the cell can
  handle them, after which the job is given to the regular Borg scheduler".

## Meta Twine

Source: [4] (Tang et al., Facebook, OSDI 2020; read in full, references excluded). Numbers are operator-reported
(`MEASURED-VENDOR`); the paper contains no third-party measurement and gives most scale figures as
plots of P99 CPU usage of production scheduler shards over one week plus single largest-value statements, not
decision-latency percentiles.

**Architecture** (`DESIGN`, [4] §2). Facebook runs "12 geo-distributed regions"; a region is several
data-centre buildings, each divided into clusters of "tens of thousands of machines"; the fault-domain
hierarchy is region, data centre, main switchboard (MSB), power row, rack, machine ([4] §2). Twine is a
three-level system ([4] §8): a *Resource Broker* (per DC) owns machine state and unavailability events; the
*Scheduler* (the paper notes it "corresponds to the Kubernetes controllers") drives job and task lifecycle;
the *Allocator* ("corresponds to the Kubernetes scheduler", [4] Fig. 4) assigns machines to entitlements and
tasks to machines; *ReBalancer* runs asynchronously with a constraint solver to fix hot spots; a *Sidekick*
switches host profiles; the *Service Resource Manager* autoscales; and *application-level schedulers*
(Shard Manager, batch, ML, stream processing, video) sit above Twine for vertical workloads. Every stateful
component has "its own separate persistent store" and is sharded ([4] §3.2).

**The unit of capacity is the entitlement, not the cluster** ([4] §2.2). "Conceptually, an entitlement is a
pseudo cluster that uses a set of dynamically allocated machines to host jobs", with a quota expressed "as a
count of machines of certain types (e.g., 2,000 Skylake machines) or as Relative Resource Units (RRU)". A
machine is free or assigned to one entitlement and can be moved between them; "Twine binds a job to an
entitlement" rather than a physical cluster; tasks of a job spread across data centres and MSBs by default,
which cuts the fault-tolerance buffer from 1/12 (about 8.3%) for one DC's 12 MSBs to 1/60 (about 1.7%) across
five DCs ([4] §2.2).

**How placement is decided** ([4] §2.3). The allocator keeps "an in-memory index of all machines and their
properties" to answer hard-requirement queries (its example: "all Skylake machines with available CPU >= 2 RRU
and available memory >= 5 GB"), then applies *soft preferences*, each "a machine property to partition
machines into different bins" and "a strategy to allocate tasks to these machine bins" (for example, spread
across fault domains). It "uses multiple threads to perform concurrent allocations for different jobs, and
relies on optimistic concurrency control": before commit, a thread re-verifies that every impacted machine
still has room and otherwise "retries a different allocation". Results are cached per job, and "the cache hit
ratio is typically above 99%". The allocator is "optimized ... to make quick decisions when starting tasks;
this optimization limits computation time and leads to best-effort outcomes", and ReBalancer later improves
them by swapping machines between entitlements or moving tasks ([4] §2.3).

**Scale reached** (`MEASURED-VENDOR`, [4] §3). The design target is one control plane per region at the
region's power budget: "We designed Twine to manage all machines that can fit in a region's 150MW power budget.
Although none of our regions host one million machines yet, we are close". So *one million machines in one
region is a design capacity, not a reported production size*. What is reported:
- Scheduler shards are partitioned by entitlement; "the largest shard manages ~170K machines, using up to 40
  cores and 80GB memory" (P99 CPU of a shard over one week). A job must fit in one shard: the largest
  entitlement is "~60K machines" and the largest job "~15K tasks". Planned: 50K machines per shard, 20 shards
  per 1M machines ([4] §3.1).
- "At its peak, a large allocator performs ~1,000 job allocations per second, with an average job size of 36
  tasks"; "our largest global allocator currently manages more than one million machines across regions"
  ([4] §3.1). The paper does not report tasks placed per second; multiplying the two figures (about 36,000)
  would assume the 36-task average holds at the peak, which it does not say.
- Fleet: "millions of small machines" of one type (one 18-core CPU, 64 GB RAM) ([4] §2.8); the 50 largest
  services use about 70% of capacity ([4] §2.6).

**Why it scales: sharding and moving the short-lived work out** ([4] §3.2-3.3). Twine "avoid[s] Kubernetes'
centralized architecture where all components interact through one central API server and share one persistent
store. These centralized components become bottlenecks and limit Kubernetes' scalability to 5K machines" (a
`CLAIMED` characterisation of Kubernetes, not a measurement; the Kubernetes thresholds in [26] agree
on 5,000 nodes). Short jobs are kept away from the core: "the Twine scheduler and allocator do not directly
manage batch jobs, whose lifetime might last just a few seconds and cause high scheduling loads. The
application-level batch scheduler acquires resources from Twine in the form of Twine tasks. It reuses these
tasks over a long period of time to host different batch jobs". Sharding is preferred to federation because
federation "adds complexity as a scale-out solution" (split job metadata, a coordinating Federation Manager).

**Prioritisation and isolation.**
- Quota is the entitlement. The paper describes no job-priority or preemption mechanism (a text search of the
  paper finds neither "preempt" nor a priority scheme). "Twine does not overcommit CPU
  or memory by default, although a job owner can explicitly configure their job to do so. On the other hand,
  we overcommit power by default" ([4] §8). Power, not CPU, is the constrained resource.
- *TaskControl*: before disruptive lifecycle events the scheduler asks the application's TaskController to
  approve, delay or reorder task operations (a ZooKeeper deployment restarts followers first, leader last);
  maintenance cannot be blocked past a deadline ([4] §2.5). Demonstrated with 15,000 shards x 3 replicas on
  1,000 tasks: during a rolling update plus an MSB failure no shard lost its second replica ([4] §5.1).
- Opportunistic capacity is a second tier managed by an application-level scheduler (Optimus): in the paper's
  case study of one large product group, entitlements and private pools gave 55% of its machines and the other 45%
  came from "capacity buffers, machines freed up by autoscaling, and unused portions of other teams' entitlements",
  and "when opportunistic capacity is not available, some services gracefully degrade their quality of service"
  ([4] §6.3). Facebook did not yet offer SLOs on the availability of that tier (see below).
- Destructive operations are rate-limited in every component; in one incident the scheduler rate-limited task
  moves away from power-capped machines, which "halted the web tier's software upgrade and protected against
  further loss of capacity" ([4] §4).

**Heterogeneity is removed rather than modelled.** Facebook consolidates on one small-machine type to improve
fungibility ("as opposed to offering a variety of high-memory or high-CPU machine types"): a small-machine rack
holds 92 machines against 30 big ones, delivers "57% higher total compute capacity measured in RRU", and
gives "18% savings in power and 17% savings in total cost of ownership" fleet-wide ([4] §2.8, §5.4). What remains
heterogeneous is the machine *configuration*: 17 *host profiles* (kernel, sysctls, cgroup settings, hugepages,
NIC, Turbo) are applied just in time when a machine moves between entitlements; "P90 for enabling CPU Turbo
takes 3.0 seconds" and "for enabling HugePages takes 244 seconds"; a machine changes profile "once every two
days" on average ([4] §5.3). The web tier gains 11% and memcache 10.2% from profiles ([4] §7.2). Hardware
heterogeneity also survives as a hard constraint: services running A/B tests "need to explicitly configure the
processor generation for their tasks to prevent performance variations between hardware types from polluting
their test results" ([4] §6.3).

**Bottlenecks, failures and lessons reported.**
- Utilisation is modest: twshared's "average memory and CPU utilization are ~40% and ~30%" (Oct 2020), because the
  fleet "is dominated by user-facing services that provision capacity for peak load"; "we do not yet provide
  service-level objectives (SLOs) on the availability of opportunistic capacity" ([4] §6.1).
- *Entitlement fragmentation*: entitlements were overloaded as both fleet partitions and quota, producing
  many tiny ones; being split into "a materialization ... and a stackable reservation" ([4] §7.1).
- *Per-task customisation* (different binaries or flags per task) was, in the paper's words, something developers
  "abused ... to implement simple sharding"; it breaks autoscaling; being removed in favour of Shard Manager ([4] §7.2).
- *Global (cross-region) allocators were the wrong abstraction*: "machines in a region are largely fungible ...
  but this is not true for machines distributed across regions"; global jobs are 8% of jobs and are being
  replaced by a federation layer over regional Twine ([4] §7.3). So Twine's own experience is that a region is
  the natural unit for one control plane and cross-region needs explicit capacity decomposition.
- Blast radius: a regional control plane "may impact all jobs in a region", mitigated by sharding (each shard
  "no bigger than a traditional cluster"), replicas in different DCs, tasks surviving control-plane failure,
  and recurring large-scale failure tests ([4] §4).
- Adoption was slow: "twshared was created in 2013, but adoption was limited in its first six years"; it held 15%
  of the fleet in January 2019 and 56% in October 2020, after migration was made mandatory ([4] Fig. 3, §6.2).

## Meta Tupperware

Tupperware is Twine's earlier name: "Facebook's Tupperware cluster management system is now called Twine"
([7], June 2019). Twine was "first deployed ... in 2011", then grew "from one data center to a fleet of 15
geo-distributed data center locations" and, in 2019, "orchestrates containers on millions of servers to run
nearly all of our services" ([7]); the engineering post and the OSDI paper ([4]) are therefore the primary
description of its mature form, and there is no separate published evaluation of Tupperware under that name.
What the older sources add:
- **2011 architecture** (`DESIGN`, [7]): "every server cluster was managed by a separate, dedicated
  scheduler ... A scheduler could manage servers only within a single cluster, which meant a job could not
  span across clusters"; cluster decommissions then forced owners to migrate jobs by hand, which is what
  Resource Broker and entitlements later removed ([7], [4] Fig. 1: median 7.5 days and worst 87 days to
  empty a cluster).
- **2014 scale** (`MEASURED-VENDOR`, [9] slide 4, text read by local OCR of the slide image): "300,000+
  processes, spread over 15,000+ services". The same deck shows a scheduler plus a server database that tracks
  heartbeats and restarts tasks when a host dies (slides 8-10), per-host agents that start tasks in
  Linux-namespace containers with cgroup CPU/RAM/disk limits ("adaptive limits", slides 16-20), and the
  rationale for building it: Docker and CoreOS "didn't exist", and VMs carried a performance penalty and
  made debugging harder (slide 25).
- **A lesson the authors drew in 2014** ([9] slide 29): "Hard to understand why TW did something", and the
  useful question is "what should I do next?" - the same observation as Borg's "why pending?" ([1] §2.6).
- **Agent/scheduler split** (`DESIGN`, [8], 2015 USENIX abstract): "A scheduler controls how jobs are deployed
  to machines. An agent running on each machine is responsible for controlling the lifecycle of each process
  scheduled on it." The talk's detail is not published in the abstract page; no slides or video were opened.
- **Later control-plane facts** ([7] 2019): the scheduler is sharded per region with a proxy that preserves
  "the abstraction of a single control plane", "one allocator is scalable enough to handle an entire region
  without sharding" (by 2020 the paper says allocator sharding is being rolled out, [4] Fig. 10), "about 20
  percent of our servers are in the shared pool", and *elastic compute*: online services are downscaled at
  off-peak and "whole servers" are loaned to offline work, and Resource Broker "quickly revokes the loaned server" on a spike ([7]). Facebook
  chose whole servers as the unit of elastic capacity "because large services ... are heavily optimized to
  utilize entire servers" ([7]).

## Meta RAS

Source: [5] (Newell et al., SOSP 2021; read the architecture, solver, evaluation and lessons). RAS is the
server-to-reservation layer inside Twine. Numbers are operator-reported (`MEASURED-VENDOR`).

**Architecture** (`DESIGN`, [5] §1, §3). Resource allocation is split in two: (1) assign *servers to
reservations* off the critical path with an asynchronous mixed-integer-programming (MIP) solver, then (2) a
conventional Twine allocator places *containers onto servers within a reservation* in real time. A
reservation is "a set of servers dynamically assigned to a logical cluster" with guaranteed capacity
that embeds failure and maintenance buffers; it is expressed in *relative resource units* (RRUs), "defined
for each server type reflecting throughput of the particular server type", so a request for capacity is
decoupled from the hardware that fulfils it. The paper's argument for the split: Twine's earlier greedy
acquisition of free servers put region-wide server assignment "on the critical path of container
placement", forcing "simple heuristics ... which led to sub-optimal server assignment and could not provide
guaranteed capacity in the event of correlated failures" ([5] §1.1).

**Scale and timing** ([5] §3.2, §3.5, §4):
- Production: "continuously optimizing the allocation of millions of servers to thousands of reservations";
  capacity requests arrive "in the order of a few thousand per day" and range from one to more than 10,000
  servers, a few near 30,000 ([5] abstract, §2.4).
- The async solver has an SLO "of completing each solve within one hour". Measured over three months for "a
  region that hosts several hundreds of thousands of servers": mean allocation time 1.8K s, 95th percentile
  2.2K s, 99th percentile 2.45K s ([5] §4, Fig. 7). Past decisions are re-evaluated hourly.
- The solver's ceiling: "Empirically, we find that ≈10 million assignment variables is the upper limit"; a
  region with 1,000 reservations, 5 DCs, 10 MSBs per DC, 200 racks per MSB and 20 server types is "200 million
  assignment variables even after exploiting symmetry". RAS therefore merges identical servers into
  equivalence classes and solves in two phases (rack-level goals only in phase two, which adds variables until it
  covers 10% of reservations or reaches five million). By the authors' linear extrapolation an unphased problem
  "would require 75GB of memory" and "the time to just set up the largest full MIP problem would take ≈4000
  seconds, exceeding our one-hour SLO" (an extrapolation, not a measurement) ([5] §3.5.2, §4).
- Quality: "90% of the solutions are optimal within 200 server preemptions and 99% of the solutions are
  optimal in that all initially broken constraints are fixed"; a longer timeout "often tightens the above
  bounds but produces no new solution" ([5] §4).
- Container placement stays fast: the allocator gives "swift response times of seconds on the critical path of
  container allocation" ([5] §3.2). After an unplanned loss the Online Mover supplies replacement servers
  "within one minute" ([5] §3.2).
- Efficiency: RAS "achieves near fully allocated regions, where close to 94% of servers are allocated for
  workloads as guaranteed capacity, 2% are allocated for random failures, and the remaining 4% are allocated
  for large-scale correlated failures" ([5] §1.2).

**Heterogeneity** ([5] §2.2-2.4, `MEASURED-VENDOR`): one region shows "nine hardware types and a total of 12
subtypes" with large variation between MSBs; services gain very unevenly from newer CPUs (Web gains 1.47x and
1.82x on the second and third generations while the DataStore "do[es] not see any gains"), many requests can
only be served by the newest generation and some by 10-12 types. Hence the throughput-normalised RRU rather
than a uniform vCPU. Failure facts that drive the buffers: planned plus unplanned unavailability can exceed 5%
of regional capacity; unplanned events "usually account for less than 0.5% ... can spike to more than 3%"; one
correlated failure caused about 4% loss ([5] §2.5).

**Failure/limits the authors report** ([5] §5.3): capacity requests can take up to an hour, so an
out-of-band emergency path writes assignments directly to Resource Broker "without obeying all placement
guarantees" and later solves repair them; continuous optimisation causes extra service preemption that
sharded stateful services must absorb; a rigid reservation boundary means random failures beyond the planned
buffer wait for a slow solve; outages larger than one MSB exceed the model. The solver was judged practical
because most missed placement goals traced to "unrealistic and competing placement goals rather than
low-quality solutions".

## Meta Shard Manager

Source: [6] (Lee et al., SOSP 2021; read the introduction, placement/solver, scalability and evaluation).
Shard Manager (SM) is an *application-level scheduler on Twine* for sharded services, not a container
scheduler; it matters here because it shows how Meta places millions of small units on top of a coarse
container layer. Numbers are operator-reported (`MEASURED-VENDOR`).

- **Adoption and scale** ([6] abstract, §8.1): used by "hundreds of applications running on over one million
  machines, which account for about 54% of all sharded applications at Facebook"; "nearly 100M shards hosted
  on over one million servers", serving "billions of requests per second"; largest application deployments
  "~19K servers and ~2.6M shards"; 139 regional and 48 geo-distributed *mini-SMs* (control-plane shards), the
  largest managing "~50K servers and ~1.3M shards" on one 18-core 64 GB machine at P90 CPU about 30% and
  memory about 38%.
- **Planned events dominate**: software upgrades and maintenance "happen ~1000 times more frequently than
  unplanned failures", so SM integrates with Twine's TaskControl to avoid unsafe operations ([6] abstract,
  §4.1).
- **Placement by constraint solver, made fast by local search** ([6] §5): hand-written heuristics proved
  brittle; SM states placement as constraints and goals in the ReBalancer API and "reduces the allocator's
  lines of code to ~20% of the hand-crafted heuristics". But "MIP can solve an optimization problem with
  millions of assignment variables in tens of minutes, whereas SM needs to solve a problem with billions of
  variables in tens of seconds", so SM configures ReBalancer for local search, samples servers, evaluates
  large shards first and batches goals by priority. This matches RAS ([5]), which accepts MIP at a one-hour
  SLO because its decisions are off the critical path.
- **Two modes** ([6] §5): an *emergency* mode "tries to place unavailable shards as quickly as possible while
  satisfying hard constraints, but may temporarily deteriorate soft goals"; a *periodic* mode takes longer and
  "must not deteriorate soft goals". The separation "helps SM to perform near-realtime allocations at scale".
- **Measured solver behaviour** ([6] §8.4): in a stress test starting from a random assignment, 5x more
  problem (75K shards on 1K servers to 375K shards on 5K servers) took 6.8x longer (30 s to 205 s); in
  production "P90 and P99 at ~10 seconds and ~50 seconds"; without the domain-knowledge optimisation the
  75K-shard problem "cannot even finish in 300 seconds and the resulting solution requires 22% more shard
  moves".
- **Control plane scales by partitioning** ([6] §6): applications are split into non-overlapping partitions of
  thousands of servers and hundreds of thousands of shard replicas, each owned by a mini-SM; replicas of one
  shard stay inside one partition.
- **Why other frameworks were not adopted** ([6] §1.1): lack of geo-distribution support, inability to keep
  applications available during planned events, the burden of a one-size-fits-all framework, and brittle
  hand-crafted placement heuristics.

## Lessons for Hex

Hex's own targets, read from `docs/requirements.toml` in this repository [46]: AC-T7.4.2 (placement p99 at most
10 ms at 10,000 placements/s on 10,000 simulated nodes), AC-T7.11.2 (1,000,000 pending workloads placed on
10,000 simulated nodes in at most 120 s), AC-T7.11.3 (at least 10x kube-scheduler's throughput on the same
simulated cluster, against kwok), AC-T7.11.4 (at most 4 KiB of control-plane memory per workload) and AC-T7.12.x
(cells, three simulated regions). The best published rates, restated per second:

| Published sustained rate | Per second | Kind | Source |
|---|---|---|---|
| Borg, several cells: "above 10 000 tasks per minute" | about 167 tasks/s (derived) | MV | [1] §3.4 |
| Borg 2019, mean job arrival per cell: 3,360 jobs/hour | about 0.93 jobs/s (derived) | MV | [3] §6.1 |
| kube-scheduler `scheduler_perf` CI floor (5,000 nodes, 50,000 pods) | 790 pods/s | MV | [30] |
| SIG-Scheduling expectation, 5,000-node load test | 100-150 pods/s | MV | [22] |
| GKE 130,000-node experiment, pod creation and binding | up to 1,000/s | MV | [38] |
| Twine large allocator at peak | about 1,000 job allocations/s (36 tasks/job average) | MV | [4] §3.1 |
| Hex AC-T7.11.2, 1,000,000 in 120 s | 8,333/s average (derived) | target | [46] |

Hex's targets are roughly 8x the best published Kubernetes figure, 10x its CI floor (derived; so AC-T7.11.3's
10x and AC-T7.11.2's 8,333/s land in the same place) and 50x Borg's per-cell arrival rate. No source contradicts them and
no source validates them: they have to be earned in `hex-sim`. A serial loop at 10,000 decisions/s leaves 100
microseconds per decision (derived); SIG-Scheduling's own expectation is an average algorithm duration of up to 10
ms per decision at 100-150 pods/s ([22]), so a serial design needs about a hundredfold cheaper decisions, or
parallel schedulers or batching, which is what Borg, Omega and Twine turned to ([1] §3.4, [2], [4] §2.3).

### Adopt

1. **Make the decision a pure function of a cached snapshot and commit optimistically; keep I/O off the
   decision path.** Borg's scheduler works on "a cached copy of the cell state" and the master rejects
   assignments built on stale state ([1] §3.4); Omega's shared-state design measured the price of optimism
   (conflict fraction at most 0.2 even at 1 s per task, above 1 once a single decision takes about 10 s, [2]);
   Twine's allocator uses threads, optimistic concurrency and a re-verify-then-commit step ([4] §2.3). Kubernetes
   is the counter-example that is being repaired: a serial loop with API calls inside it, a 50 QPS default client
   ([15]), and KEPs 4832 and 5229 to move preemption and the remaining API calls off the loop ([21], [22]). Fits
   the sans-IO shape in CLAUDE.md §9. Keep individual decisions short, since conflicts grow with decision time
   ([2]).
2. **Scale placement with equivalence classes, sampling and caches, and state the staleness bound.** Borg: from
   "did not finish after more than 3 days" to "a few hundred seconds" ([1] §3.4); Twine's job-level allocation
   cache "typically above 99%" ([4] §2.3); Kubernetes samples 5-50% of nodes and trades placement quality for
   latency ([12]). Kubernetes' first cache (eCache, 2018) failed because it was fine-grained and coupled to
   plugins, and its replacement keys one *signature* per pod, caches the whole filter-and-score result and
   expires batches after 500 ms ([19]). For Hex: the scheduling signature belongs to the core (plugins declare
   which pod fields they read), the staleness bound is a named constant with a measured justification (CLAUDE.md
   §5), and a non-vacuity counter proves the cache is hit (CLAUDE.md §6). Agent boxes are highly homogeneous, so
   classes should pay off even more than for Kubernetes.
3. **Two-speed placement: a bounded-work fast path and an asynchronous optimiser.** Twine's allocator is
   "best-effort" for speed and ReBalancer fixes hot spots later with a constraint solver ([4] §2.3); RAS runs an
   hourly-SLO MIP off the critical path with an out-of-band emergency path "to grant immediate capacity without
   obeying all placement guarantees", which the next solve repairs ([5] §5.3); Shard Manager has an *emergency mode*
   and a *periodic mode* and had to abandon MIP for local search because MIP handles "millions of assignment
   variables in tens of minutes" while it needs "billions of variables in tens of seconds" ([6] §5). The
   optimiser's output is churn, so its moves must be priced and rate-limited (RAS minimises preemption of
   existing allocations, [5] §3.2; Twine rate-limits destructive operations in every component, [4] §4).
4. **Keep short-lived, bursty work off the fleet scheduler's critical path by leasing coarse capacity.** Twine does
   not schedule batch jobs "whose lifetime might last just a few seconds"; an application-level scheduler holds
   Twine tasks "over a long period of time to host different batch jobs" ([4] §3.2); RAS pre-allocates a
   reservation's capacity "ahead of time" so container placement "can immediately use a free server already in
   the reservation" ([5] §1.2); OpenAI used low-priority "balloon" pods, which the scheduler "can evict ... immediately to make room for actual
   work", to keep idle nodes from being scaled away ([36]). For Hex, agent boxes are the short-lived work: serve a burst from leased, pre-placed pool capacity
   and make the fleet-wide decision about pools, not boxes. The price is idle reserve: RAS holds about 6% of
   servers as failure buffer ([5] §1.2) and Twine's shared pool ran at about 30% CPU and 40% memory ([4] §6.1).
5. **Admit or refuse at the door (quota, typed refusal), then order by priority band; never let peers preempt
   peers.** Borg rejects over-quota jobs "immediately", offers unlimited priority-zero quota, forbids
   production-band tasks from preempting each other to stop cascades, and limits "the allowed rate of task
   disruptions" ([1] §2.5, §4); in Borg's 2019 cells fewer than 0.2% of production collections saw any
   eviction ([3] §5.2). Kubernetes has no
   fair-share or multi-queue layer in core (left to Kueue and Volcano, [20]), documents that PDBs are only
   "best effort" under preemption and that preemption cannot cross nodes ([25]), warns about untrusted priority
   ([25]), and excludes preempting pods from its own startup SLO ([28]). I found no capacity bound on its
   pending queues ([14]); Hex's rule that every queue has a bound and a typed overflow refusal (CLAUDE.md §2-3)
   goes further than the sources, whose only stated admission bounds are Borg's quota and its "admission control
   to avoid overload" ([1] §2.5, §4).
6. **Isolate mice from hogs.** In Borg's 2019 trace the top 1% of jobs use 99.2% of CPU and C^2 is 23k, so mean
   queueing delay is high "even when the system load is low" unless small jobs are "prioritized over the hogs or
   ... isolated from them" ([3] §7). The systems already do it structurally: a separate batch scheduler that queues
   best-effort batch work ([3] §3), separate batch and service schedulers in Omega ([2] §4), application-level
   batch schedulers in Twine ([4] §3.2). Hex's mix of thousands of tiny agent boxes and a few huge jobs wants
   separate queues and budgets per class (CP-T7.5's burst test, T-T7.5.1 [46]).
7. **Normalise heterogeneity into measured abstract units plus hard constraints; do not hide runtime-class
   differences.** Google moved to GCUs so "1 GCU should provide about the same amount of computational power on any
   machine" ([3] §3); Meta's RRUs are measured per workload because gains differ (Web 1.47x and 1.82x on newer CPUs,
   DataStore none, [5] §2.3); Twine expresses needs as hard requirements ("Skylake only") plus soft preferences
   with bins and strategies ([4] §2.3). Hex's `process`, `oci`, `firecracker` and `shards` classes are
   capability constraints, not speed ratios: model them as hard filters, model speed differences as RRU-like
   measured units, and price any reconfiguration (Twine's host-profile switches took P90 3.0 s to 244 s, [4]
   §5.3). Twine's other answer, standardising on one machine type, is an option for fleets Hex controls, not for
   those it does not.
8. **Cells are the unit of blast radius; one control plane per region at most; global is a thin layer.** Borg: cell
   size is limited by availability, "not scalability limitations" ([1] §4). Twine: shard the control plane by
   entitlement (a job must fit in one shard), keep the region as the largest control-plane scope, and decompose
   global capacity per region instead of letting a global allocator choose ([4] §3.1, §7.3). Kubernetes' tested
   envelope is 5,000 nodes per cluster ([29]) and its federation project is retired ([40]); Karmada's cluster
   selection is cheap (P99 32 ms) but was shown only on simulated nodes ([41]). Matches CP-T7.12.
9. **Say why something is pending, in words.** Borg's "why pending?" ([1] §2.6, §8.2) and Tupperware's own lesson
   ("Hard to understand why TW did something", [9] slide 29) are the same finding at two companies, and the
   core of Hex's AC-T7.4.1.
10. **Make groups atomic from the start.** Omega's all-or-nothing transaction is gang scheduling without hoarding
    ([2] §3.4); hoarding and incremental acquisition deadlock ([2] §3, [36]). Kubernetes retrofitted gang
    scheduling after years of external implementations (KEP-583 dates from 2018) and it is still Beta and serial
    ([23], [24], [20]), with the accepted risk that a long group cycle binds onto nodes that failed meanwhile
    ([20]). Hex's "matrix jobs are admitted whole" (AC-T7.5.1) should be a property of the core's commit, not a
    plugin.
11. **Measure placement quality, not just speed.** Borg judges policy by *cell compaction* (smallest cell that fits
    the workload, re-packed from scratch) because average utilisation misleads under constraints and
    heterogeneity, and found its hybrid scorer packs "about 3-5% better" than best fit ([1] §3.2, §5.1); pooling
    prod and non-prod saves 20-30% of machines ([1] §5.2). Hex's AC-T7.4.3 (stranded capacity) is the right kind
    of metric; compaction is a ready-made, reproducible way to run it in `hex-sim`.

### Avoid

- **A serial scheduling loop that performs API calls** ([13], [21], [22], [15]).
- **Fine-grained, plugin-coupled caches** (eCache, retracted, [19]).
- **Offer-based two-level scheduling for long or picky decisions**: in Omega's simulation Mesos-style offers made
  jobs hit the 1,000-attempt retry limit ([2] §4.2).
- **One abstraction doing two jobs** (Twine's entitlement was both partition and quota, so entitlements fragmented,
  [4] §7.1), **per-task customisation that breaks autoscaling** ([4] §7.2), and **a single API with about 230
  knobs** ([1] §8.1).
- **Cross-region allocators and federated per-cluster control planes as the scale-out mechanism** ([4] §7.3,
  [40]).
- **Unbounded LIST and watch fan-out between control plane and agents**: a few LISTs can exhaust API-server memory
  ([34]); one endpoints watch reached "1GB/s or more" ([36]). Borg's inversion (the master *polls* agents "which
  ... prevents recovery storms", [1] §3.3) and Twine's agent that keeps tasks running with no control plane ([4]
  §4) are the contrasting designs; Hex's protocol should have bounded, rate-controlled node reporting.
- **Treating published scale as capacity**: Twine's one million machines is a design capacity ([4] §3), Karmada's
  500,000 nodes are `fake-kubelet` objects ([41]), GKE's 130,000 nodes are "experimental" with a proprietary store
  ([38]), and Omega's numbers are simulated ([2]). The research README's rule (a claimed or vendor number is a lead, not
  a fact Hex designs against) applies to all of them ([46]).

## Open questions / gaps

- **No neutral third-party measurement of kube-scheduler throughput at 1,000+ nodes was found.** The nearest are
  Apache YuniKorn's KWOK runs (a competing project; the constant 50.5 pods/s it reports for kube-scheduler is
  consistent with the default 50 QPS client limit, but the page does not state the configured QPS, [39], [15]) and
  the Kubernetes project's own numbers ([30], [22]). The perf-dash dashboards that [22] cites are
  JavaScript-rendered and were not opened. SIG Scalability's raw CI output is public (the
  `k8s-infra-scalability-tests-logs` bucket, [47]), but the runs I listed held component logs (one kubemark-5000
  scheduler run had an 8.6 GB API-server audit log) and no summarised throughput file, and I did not parse
  them. Hex's AC-T7.11.3 needs its own `hex-sim` against `kwok` run.
- **Percentile coverage is thin.** No source reports p99.9 or maximum scheduling latency for Borg, Twine,
  RAS's allocator or kube-scheduler; Borg's paper gives one pass time ("less than half a second") and Borg's 2019
  trace analysis gives scheduling delay only as CCDF figures ([1] §3.4, [3] §6.3). Twine publishes P99 CPU of
  shards, not decision latency ([4] §3.1).
- **Twine's task-placement rate is not published**; only job allocations per second and average job size are, and
  the paper does not say they coincide ([4] §3.1).
- **Control-plane memory per workload** (Hex AC-T7.11.4, 4 KiB) has no reported counterpart: Borg gives master RAM
  without a task count at that moment, OpenAI gives API-server heap per node count ([1] §3.4, [36]).
- **Newer Meta numbers** (after the 2021 papers) and any Google Borg figures after the 2019 trace were not searched
  for, so they may exist.
- **Tupperware**: only the 2014 slides (read by local OCR of the slide images), a 2015 abstract and the 2019 post were
  found; the 2015 USENIX talk video and slides were not opened ([8], [9]).
- **The OpenAI post could not be fetched from openai.com (HTTP 403)**; the text was read from an Internet Archive
  copy dated 2023-02-13 whose page header says January 25, 2021 ([36]). It is older than the
  v1.37 sources above, and the scheduler and API server have changed since (for example KEPs 2340, 3157, 4832
  and 5229).
- **Karmada "peers"** (Open Cluster Management, Clusternet, Liqo, MultiKueue itself) were not studied; MultiKueue
  is mentioned only through Google's post ([38]).
- **Kubernetes workload-aware scheduling** (KEP-5710 preemption, KEP-5732 topology, KEP-6012 composite groups) was
  read only at feature-gate and `kep.yaml` level, not in design depth.
- **Pod startup cost beyond placement** (images, volumes, network) is outside the scheduler sources read; Borg's
  25 s median start with 80% in package installation ([1] §3.2) is the only figure here.
- **The KEP-5598 batching speed-up and the GKE benchmark figures are unmeasured in the text read**: the batching
  benchmark records no result ([30]), and the GKE post's per-phase charts are images whose values are not in its
  text ([38]).

## References

All accessed 2026-10-22. Dates are the document's own where it states one. Kinds of evidence: papers and
documents are primary; GitHub files are pinned to the named tag or commit.

1. Verma, Pedrosa, Korupolu, Oppenheimer, Tune, Wilkes (Google). "Large-scale cluster management at Google with
   Borg", EuroSys 2015, Bordeaux. Page: https://research.google/pubs/large-scale-cluster-management-at-google-with-borg/
   PDF read: https://storage.googleapis.com/gweb-research2023-media/pubtools/pdf/43438.pdf
2. Schwarzkopf, Konwinski, Abd-El-Malek, Wilkes. "Omega: flexible, scalable schedulers for large compute clusters",
   EuroSys 2013, Prague, April 15-17, 2013. PDF read: https://storage.googleapis.com/gweb-research2023-media/pubtools/pdf/41684.pdf
   (page: https://research.google/pubs/omega-flexible-scalable-schedulers-for-large-compute-clusters/)
3. Tirmazi, Barker, Deng, Haque, Qin, Hand, Harchol-Balter, Wilkes. "Borg: the Next Generation", EuroSys 2020,
   Heraklion, April 27-30, 2020, https://doi.org/10.1145/3342195.3387517 . PDF read:
   https://storage.googleapis.com/gweb-research2023-media/pubtools/5532.pdf (page: https://research.google/pubs/borg-the-next-generation/)
4. Tang et al. (Facebook). "Twine: A Unified Cluster Management System for Shared Infrastructure", OSDI 2020,
   November 4-6, 2020. https://www.usenix.org/conference/osdi20/presentation/tang  PDF read:
   https://www.usenix.org/system/files/osdi20-tang.pdf
5. Newell et al. (Facebook, CMU). "RAS: Continuously Optimized Region-Wide Datacenter Resource Allocation", SOSP 2021,
   October 26-29, 2021, https://doi.org/10.1145/3477132.3483578 . PDF read:
   https://research.facebook.com/file/4561236690664587/RAS-Continuously-Optimized-Region-Wide-Datacenter-Resource-Allocation.pdf
6. Lee et al. (Facebook). "Shard Manager: A Generic Shard Management Framework for Geo-distributed Applications",
   SOSP 2021, https://doi.org/10.1145/3477132.3483546 . PDF read:
   https://research.facebook.com/file/245575980870853/Shard-Manager-A-Generic-Shard-Management-Framework-for-Geo-distributed-Applications.pdf
7. Yu, Tang (Meta). "Efficient, reliable cluster management at scale with Twine", Engineering at Meta, June 6, 2019
   (carries the note "Facebook's Tupperware cluster management system is now called Twine").
   https://engineering.fb.com/data-center-engineering/tupperware/
8. Aivaliotis (Facebook). "Tupperware: Container Deployment at Scale", USENIX UCMS 2015, Washington D.C., November
   2015 (abstract page only). https://www.usenix.org/conference/ucms15/summit-program/presentation/aivaliotis
9. Narayanan (Facebook). "Tupperware: Containerized Deployment at FB", DockerCon 2014 slides (uploaded by Docker,
   Inc.). https://www.slideshare.net/Docker/aravindnarayanan-facebook140613153626phpapp02-37588997 ; slide images read
   from https://image.slidesharecdn.com/aravindnarayanan-facebook-140613153626-phpapp02-140801195157-phpapp02/75/Tupperware-Containerized-Deployment-at-FB-N-2048.jpg
   (N = 1..30) with local OCR; slides 4, 25 and 29 were also viewed directly and match the OCR. The page's own
   "AI-enhanced description" was not used as evidence.
10. Kubernetes documentation. "Kubernetes Scheduler" (page footer: last modified February 16, 2024).
    https://kubernetes.io/docs/concepts/scheduling-eviction/kube-scheduler/
11. Kubernetes documentation. "Scheduling Framework" (stable since v1.19; last modified July 04, 2025).
    https://kubernetes.io/docs/concepts/scheduling-eviction/scheduling-framework/
12. Kubernetes documentation. "Scheduler Performance Tuning".
    https://kubernetes.io/docs/concepts/scheduling-eviction/scheduler-perf-tuning/
13. Kubernetes source, tag v1.37.1. `pkg/scheduler/schedule_one.go`.
    https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/pkg/scheduler/schedule_one.go
14. Kubernetes source, tag v1.37.1. `pkg/scheduler/backend/queue/scheduling_queue.go`.
    https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/pkg/scheduler/backend/queue/scheduling_queue.go
15. Kubernetes source, tag v1.37.1. `pkg/scheduler/apis/config/v1/defaults.go`.
    https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/pkg/scheduler/apis/config/v1/defaults.go
16. Kubernetes source, tag v1.37.1. `staging/src/k8s.io/component-base/config/v1alpha1/defaults.go` (leader-election
    and client defaults). https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/staging/src/k8s.io/component-base/config/v1alpha1/defaults.go
17. Kubernetes source, tag v1.37.1. `pkg/scheduler/framework/parallelize/parallelism.go`.
    https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/pkg/scheduler/framework/parallelize/parallelism.go
18. Kubernetes source, tag v1.37.1. `pkg/features/kube_features.go` (feature-gate stages and owners).
    https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/pkg/features/kube_features.go
19. KEP-5598 "Opportunistic batching" (SIG Scheduling), kubernetes/enhancements @ 0c8d11e9a13e12d2ef75d2d78af5ac00d8189046.
    https://raw.githubusercontent.com/kubernetes/enhancements/0c8d11e9a13e12d2ef75d2d78af5ac00d8189046/keps/sig-scheduling/5598-opportunistic-batching/README.md
20. KEP-4671 "Gang Scheduling" (SIG Scheduling), same commit.
    https://raw.githubusercontent.com/kubernetes/enhancements/0c8d11e9a13e12d2ef75d2d78af5ac00d8189046/keps/sig-scheduling/4671-gang-scheduling/README.md
21. KEP-4832 "Asynchronous Preemption" (SIG Scheduling), same commit.
    https://raw.githubusercontent.com/kubernetes/enhancements/0c8d11e9a13e12d2ef75d2d78af5ac00d8189046/keps/sig-scheduling/4832-async-preemption/README.md
22. KEP-5229 "Asynchronous API calls during scheduling" (SIG Scheduling), same commit.
    https://raw.githubusercontent.com/kubernetes/enhancements/0c8d11e9a13e12d2ef75d2d78af5ac00d8189046/keps/sig-scheduling/5229-asynchronous-api-calls-during-scheduling/README.md
23. KEP-583 "Coscheduling" (created 2018-07-03, status provisional), same commit.
    https://raw.githubusercontent.com/kubernetes/enhancements/0c8d11e9a13e12d2ef75d2d78af5ac00d8189046/keps/sig-scheduling/583-coscheduling/README.md
24. Kubernetes documentation. "Gang Scheduling" (Beta since v1.37, disabled by default).
    https://kubernetes.io/docs/concepts/scheduling-eviction/gang-scheduling/
25. Kubernetes documentation. "Pod Priority and Preemption".
    https://kubernetes.io/docs/concepts/scheduling-eviction/pod-priority-preemption/
26. SIG Scalability. "Kubernetes Scalability thresholds", kubernetes/community @ f2d487985d7ccd129fc13fdb7be9600091012a99.
    https://raw.githubusercontent.com/kubernetes/community/f2d487985d7ccd129fc13fdb7be9600091012a99/sig-scalability/configs-and-limits/thresholds.md
27. SIG Scalability. "Kubernetes scalability and performance SLIs/SLOs", same commit.
    https://raw.githubusercontent.com/kubernetes/community/f2d487985d7ccd129fc13fdb7be9600091012a99/sig-scalability/slos/slos.md
28. SIG Scalability. "Pod startup latency SLI/SLO details", same commit.
    https://raw.githubusercontent.com/kubernetes/community/f2d487985d7ccd129fc13fdb7be9600091012a99/sig-scalability/slos/pod_startup_latency.md
29. Kubernetes documentation. "Considerations for large clusters" (v1.37).
    https://kubernetes.io/docs/setup/best-practices/cluster-large/
30. Kubernetes `scheduler_perf`, tag v1.37.1: `misc/performance-config.yaml`, `default_preemption/performance-config.yaml`,
    `batching/performance-config.yaml`, `README.md`, `util.go`, `templates/pod-default.yaml`, `templates/node-default.yaml`,
    under https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/test/integration/scheduler_perf/
31. kubernetes/kubernetes issue #108308, "scheduler_perf: Preemption and PreemptionPV workloads are not working
    successfully" (created 2022-02-23, closed). https://github.com/kubernetes/kubernetes/issues/108308
32. etcd documentation. "System limits". https://etcd.io/docs/latest/dev-guide/limit/
33. etcd documentation. "Performance". https://etcd.io/docs/latest/op-guide/performance/
34. KEP-3157 "Allow informers for getting a stream of data instead of chunking" (SIG API Machinery), same enhancements commit.
    https://raw.githubusercontent.com/kubernetes/enhancements/0c8d11e9a13e12d2ef75d2d78af5ac00d8189046/keps/sig-api-machinery/3157-watch-list/README.md
35. KEP-2340 "Consistent Reads from Cache" (SIG API Machinery), same commit.
    https://raw.githubusercontent.com/kubernetes/enhancements/0c8d11e9a13e12d2ef75d2d78af5ac00d8189046/keps/sig-api-machinery/2340-Consistent-reads-from-cache/README.md
36. OpenAI (Jan 25, 2021, per the page). "Scaling Kubernetes to 7,500 Nodes". Read from the Internet Archive copy
    http://web.archive.org/web/20230213011050/https://openai.com/blog/scaling-kubernetes-to-7500-nodes/ ;
    original https://openai.com/index/scaling-kubernetes-to-7500-nodes/ (HTTP 403 to automated fetch).
37. Bradstock, Różacki (Google Cloud). "65,000 nodes and counting: Google Kubernetes Engine is ready for
    trillion-parameter AI models", November 13, 2024.
    https://cloud.google.com/blog/products/containers-kubernetes/gke-65k-nodes-and-counting
38. Massri, Różacki (Google Cloud). "How Google Does It: Building the largest known Kubernetes cluster, with 130,000
    nodes", November 21, 2025. https://cloud.google.com/blog/products/containers-kubernetes/how-we-built-a-130000-node-gke-cluster/
39. Apache YuniKorn documentation (v1.10.0). "Evaluate YuniKorn Performance with KWOK".
    https://yunikorn.apache.org/docs/performance/evaluate_perf_function_with_kwok
40. Kubernetes SIG Multicluster. KubeFed README ("Kubernetes Cluster Federation (archived)"):
    https://raw.githubusercontent.com/kubernetes-sigs/kubefed/master/README.md ; repository metadata (archived, last
    push 2023-03-24, moved to kubernetes-retired/kubefed): https://api.github.com/repos/kubernetes-sigs/kubefed
41. Wang (Karmada maintainer), CNCF guest post. "Support for 100 Large-Scale Clusters: Test Report on Karmada",
    November 29, 2022. https://www.cncf.io/blog/2022/11/29/support-for-100-large-scale-clusters/
42. CNCF. Karmada project page (accepted 2021-09-14, incubating 2023-12-12, graduated 2026-09-03).
    https://www.cncf.io/projects/karmada/
43. Kubernetes SIG Multicluster README (subprojects), kubernetes/community @ f2d487985d7ccd129fc13fdb7be9600091012a99.
    https://raw.githubusercontent.com/kubernetes/community/f2d487985d7ccd129fc13fdb7be9600091012a99/sig-multicluster/README.md
44. GitHub releases API, kubernetes/kubernetes latest release (v1.37.1, published 2026-09-23).
    https://api.github.com/repos/kubernetes/kubernetes/releases/latest
45. Kubernetes source, tag v1.37.1. `staging/src/k8s.io/kube-scheduler/config/v1/types_pluginargs.go` (`DefaultPreemptionArgs`).
    https://raw.githubusercontent.com/kubernetes/kubernetes/v1.37.1/staging/src/k8s.io/kube-scheduler/config/v1/types_pluginargs.go
46. Hex repository, `docs/requirements.toml` (CP-T7.4, CP-T7.5, CP-T7.11, CP-T7.12), `docs/design/README.md` and
    `docs/research/README.md` (method and number kinds), working tree on branch `cp/t7-runtime`, read 2026-10-22.
47. SIG Scalability CI artifact bucket `k8s-infra-scalability-tests-logs` (public object listing, jobs including
    `ci-kubernetes-kubemark-gce-scale-scheduler` and `ci-kubernetes-e2e-gci-gce-scalability-1-35`; listed, not parsed).
    https://storage.googleapis.com/storage/v1/b/k8s-infra-scalability-tests-logs/o?delimiter=/
48. Kubernetes documentation. "Resource Management for Pods and Containers".
    https://kubernetes.io/docs/concepts/configuration/manage-resources-containers/
49. Kubernetes documentation. "Pod Quality of Service Classes". https://kubernetes.io/docs/concepts/workloads/pods/pod-qos/
50. Kubernetes documentation. "Dynamic Resource Allocation" (stable since v1.35).
    https://kubernetes.io/docs/concepts/scheduling-eviction/dynamic-resource-allocation/
