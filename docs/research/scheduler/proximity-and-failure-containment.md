# Proximity and failure containment: network coordinates, failure detection, gray failure, and containing bad workloads

Hex must place work near what it talks to, notice nodes that fail (including the ones that only slow
down), and catch failing, dangerous or faulty workloads fast, then deprioritize and isolate them. This
note is the evidence base for all three. It covers network coordinates (Vivaldi, Serf and Consul, and
hyper-swim, the engine Hex would reuse), failure detection (SWIM, Lifeguard, φ-accrual and hyper-raft's
detectors), gray failure and fail-slow faults, and the mechanisms production systems use to contain a
bad workload or node. It follows the method in [`../README.md`](../README.md#method): every claim and
number is cited to a source opened and read for this note (accessed 2026-10-22), every number carries a
kind, and a gap is written as a gap.

Kinds: `MI` MEASURED-INDEPENDENT, `MV` MEASURED-VENDOR (the authors or operators measured their own
system), `CL` CLAIMED (no stated method), `DE` DESIGN (a fact from a document, code or specification, not
a speed). `n/p` = not published, or not found in what was read. Citations are `[n]` (References at the
end). Hyper-light code is cited at hyper-raft commit 6a236ae.

*Verified (2026-10-22):* 147 quoted figures and phrases were checked against the saved text of their
sources: 143 by script, and 4 by reading them where a code comment or a hyphenated line splits them.

## Summary

1. **Coordinates predict latency well enough to rank, not to promise.** Vivaldi's 2-D coordinates with a
   height predict round-trip time with an 11% median relative error over 1,740 Internet hosts [1] (MV).
   In the wild, on Azureus's million-node system, errors were higher, 83% of pairs violated the triangle
   inequality, and churn was higher; six techniques (latency and update filters, neighbor decay,
   coordinate memory, gravity, violator exclusion) improved accuracy 43% and stability by four orders of
   magnitude [2] (MV). Coordinates measure latency only: no source read predicts bandwidth from them.
2. **Hex already has the engine.** hyper-swim runs Dabek's update exactly as published, in two
   dimensions plus a height, with no gravity, adjustment term or latency filter. It replaced the
   Serf-style engine that came from Hyperscale (eight dimensions, a height share, an adjustment term
   clamped at ±1 s, 0.99 gravity) [3][4]. Today it ranks only the relays for indirect probes [5], and its
   accuracy has been checked only on loopback and container links [6]. Hyperscale's gate is the precedent for
   placement: it scores each datacenter on a blend of a quality-weighted RTT upper confidence bound and the
   latency it has observed. A datacenter with no evidence is scored as far away as the farthest known one,
   and health is never traded for latency [33][34][35] (DE).
3. **Consul shows what coordinates are for in operations**: sorting service results by estimated RTT
   (`?near=`), failing prepared queries over to the next-closest datacenter, and an `rtt` command. LAN and
   WAN coordinates are separate and cannot be compared [7] (DE).
4. **SWIM's cost per member does not grow with the cluster**: both the expected time to first detect a
   failure and the expected message load per member are independent of group size [8] (DE). Its weak
   point is the slow member. One overloaded member already causes false positives, and 4 overloaded members
   (4% of the cluster) produce hundreds of them at healthy members. Lifeguard's local-health extensions
   cut false positives 50-100x. The cost: median detection is 0.1 s slower, the 99.9th percentile up to
   1.5-1.8 s slower, and it sends 11% more messages but 2% fewer bytes [9] (MV).
5. **Gray failure is the common failure.** Microsoft reports that gray failure is "behind most cloud
   incidents" and that its defining trait is differential observability: the system's own failure
   detector does not see what its clients see [10] (CL). In 101 fail-slow incidents from 12
   institutions, pinpointing the cause took hours to months. A single degraded NIC made jobs lock slots on
   healthy machines and collapsed a whole cluster's performance [11] (MI, a community study).
6. **Detectors that look at peers and requesters catch gray failure in seconds to minutes.** Panorama
   detected all 15 reproduced gray failures in under 7 s, where existing approaches caught one within
   300 s [12]. IASO isolates a slow node within minutes from timeout signals across 39,000 nodes [13].
   Perseus found 304 fail-slow drives among 248K, and isolating them cut node-level p99.99 latency by 48%
   [14]. OmegaGen's generated watchdogs caught 20 of 22 partial failures, with a median detection time of
   4.2 s [15]. All four are `MV`.
7. **Bad neighbors are caught by correlation and throttled before they are evicted.** Google's CPI²
   flags a task whose cycles per instruction exceed 2σ of its job's learned distribution at least 3
   times in 5 minutes. It then finds the antagonist by correlating the victim's CPI with suspects' CPU
   use and caps the antagonist at 0.01 or 0.1 CPU-seconds per second for 5 minutes. It runs in all of
   Google's shared compute clusters [16] (MV).
8. **Repeat offenders get escalating penalties, with a forgiveness rule and a cap on how much is
   ejected.** Kubernetes backs off a crashing container at 10 s, 20 s, 40 s, … up to 300 s, and resets
   after 10 minutes of clean running [17]. Envoy ejects an outlier host for its base time multiplied by
   its consecutive ejections, never ejects more than a configured share of the cluster, and decays the
   multiplier while the host stays healthy [18]. Shuffle sharding limits any one customer's blast radius:
   with 8 workers and 2 per customer, a problem reaches 1/28 of customers, 7x better than plain sharding
   [19] (DE).
9. **Retries turn overload into outage.** In 22 metastable failures from 11 organizations, retries
   sustained more than 50% of them, and outages lasted 1.5 to 73.53 hours [20] (MI, public incident
   reports). Three retries at each layer of a five-deep stack multiply load 243x [21]. Google bounds retries
   at 3 per request and a 10% retry ratio per client, which caps the growth at 1.1x, and sheds load by
   criticality [22] (DE).

## 1. Network proximity

### 1.1 Vivaldi

Vivaldi gives each host synthetic coordinates such that the distance between two hosts' coordinates
predicts their round-trip time, so a host that knows another's coordinates need not measure it [1]. It
is fully distributed, with no landmarks or distinguished hosts, and "a new host can compute good
coordinates for itself after collecting latency information from only a few other hosts". Because it
needs little communication, it "can piggy-back on the communication patterns of the application" [1]
(DE). Its evaluation simulated a network whose latencies were measured among 1,740 Internet hosts. A 2-D
Euclidean model with height vectors embedded them with an 11% median relative error in predicted RTT [1]
(MV). The two data sets' median RTTs were 76 ms (PlanetLab) and 159 ms (King) [1]. Its design choices,
as hyper-raft's note verified against the paper: an adaptive timestep weighted by the two nodes' error
estimates (`c_c = 0.25` empirically), two to three effective dimensions with no significant gain past
three, and height vectors that model the access link and predict better than 2-D or 3-D alone [1][6]
(DE). Its stated limit: "Vivaldi defends against high-error nodes, but not malicious nodes" [1].

### 1.2 In the wild

Ledlie, Gardner and Seltzer studied the Azureus BitTorrent network's live coordinate system, which ran
on a million nodes, with a test branch used by about ten thousand clients. They found "higher relative
errors, more triangle inequality violations, and higher churn" than on PlanetLab or in simulation [2].
83% of Azureus pairs, 85% of MIT King pairs and 68% of the PlanetLab subset violated the triangle
inequality (some third node gave a faster path). 37% of Azureus pairs and 22% of King pairs had a path
at least 20% faster [2] (MI for the data sets, MV for the fixes). Because Azureus maintained its
coordinates only on top of other traffic, each node's neighbors were skewed to a small part of the
network and its coordinate "failed to become globally accurate". Six techniques fixed most of it:
latency filters, update filters, neighbor decay, coordinate memory, gravity and violator exclusion. In
Azureus they gave "a 43% improvement in accuracy and a four order-of-magnitude improvement in
stability" [2] (MV).

### 1.3 Serf and Consul

Serf's `coordinate` package implements Vivaldi with Ledlie's additions. Its defaults are 8 dimensions
plus a height ("Per [2] we chose 8 dimensions plus a non-Euclidean height", where [2] is Ledlie), a
maximum error of 1.5, `c_e = c_c = 0.25`, an adjustment window of 20 samples, a minimum height of 10 µs,
a 3-sample moving-median latency filter per peer, and gravity `ρ = 150` [4][23] (DE). The file itself
says these defaults are "suitable for basic testing of the algorithm, but not tuned to any particular
type of cluster" [4]. Consul uses Serf's coordinates in four ways: `consul rtt` estimates the RTT
between two nodes; catalog and health queries sort results by RTT from a given node (`?near=`);
prepared queries "fail over services to other Consul datacenters based on network round trip times";
and the raw coordinates are exposed for other applications. LAN and WAN gossip pools keep separate,
incompatible coordinates [7] (DE).

### 1.4 hyper-swim, the engine Hex would reuse

hyper-swim (hyper-raft's SWIM crate) carries its own Vivaldi engine. It descends from slates' engine,
which "followed hyperscale's": error folded into a relative estimate floored at 0.05, eight dimensions,
a separate height share, an adjustment term with a ±1 s clamp, and 0.99 gravity. It was then rebuilt as
Dabek's Fig. 3 update, whole, in two dimensions plus a height, with `c_c = 0.25` from the paper and
`c_e = 2/(m + 1)` derived from one probing round of `m` members [3][6] (DE). It removed gravity, the
adjustment term and the error bounds. Drift moves every coordinate alike, so it leaves every
prediction unchanged, and the engine only predicts between coordinates refreshed each round [3][6].
Coordinates are kept only for members this node probes and are dropped at death, so they are bounded by
the membership [3]. Today the detector uses them to choose indirect-probe relays "nearest the target in
coordinate space" [5] (DE). The repository has never measured them against a wide-area round-trip
matrix: "its loopback and container round trips are one link's, all alike" [6].

### 1.5 Hyperscale: coordinates in routing

Hyperscale, hyper-light's Python framework for performance testing and scalable unit and integration
testing, runs its own Vivaldi engine in distributed mode. It has 8 dimensions, `c_e = 0.25`, a height,
an adjustment term and a small centering gravity, and keeps a coordinate's error as a moving average of
its absolute prediction error in seconds [33] (DE). Its gate routes jobs across datacenters with those
coordinates in four steps [34][35][36] (DE):

- **RTT as an upper confidence bound.** `rtt_ucb = clamp(rtt_hat + K_SIGMA·σ, RTT_MIN, RTT_MAX)` with
  `K_SIGMA = 2`, σ bounded to 1-500 ms and the estimate to 1 ms-10 s. A coordinate earns full quality
  (has converged) at 10 samples and an error of at most 20 ms, and expires after 300 s [33].
- **Blend with what was observed.** A datacenter's estimate is `confidence·observed + (1 − confidence)·
  (quality·rtt_ucb + (1 − quality)·prior)`. Here `observed` is an EWMA of its time to accept a
  dispatch, with confidence growing with its sample count. The prior is "the farthest latency evidenced
  for any datacenter". While the gate's own coordinate has not converged, "Vivaldi distances measured from
  it are not evidence" [35][36].
- **Health first.** Unhealthy and initializing datacenters are excluded, as are those with no managers or
  with every manager's circuit open, each with its reason recorded. "A missing coordinate never
  excludes." The rest are bucketed HEALTHY, BUSY and DEGRADED, "in that order, never traded for latency".
  Within a bucket, `score = latency × load_factor × health_severity_weight × slo_routing_factor`, where the
  load factor adds utilization, queue depth and circuit pressure at one third each [34].
- **A datacenter that failed a job cools down for that job.** It is routed after every other eligible
  datacenter for `CIRCUIT_BREAKER_HALF_OPEN_AFTER`, but not excluded, since "it may be the only one left".
  Ties break on a rendezvous hash of the job id [34].

The record states its own gap: the criterion of under 10 s for losing a datacenter "is **unverified**:
no test asserts it", and the only tested bound holds death classification to at most 30 s [34]. AD-35
also dropped scaling failure-detection timeouts by RTT ratio. Probe timeouts pad only for the node's own
unreliability (Lifeguard's local health), because "the base probe timeout (1s) already exceeds any
terrestrial round trip" [32].

### 1.6 What coordinates do not give


- **Bandwidth.** Vivaldi, Ledlie and Serf model latency only [1][2][4]. None of the sources read
  predicts bandwidth from coordinates; bandwidth-aware placement is a gap here.
- **Endpoints outside the membership.** A coordinate exists only for a node that runs the protocol. A
  model provider's API, an object store or a registry is not a member, so the RTT to it must be measured
  directly. Consul's separate LAN and WAN pools show that even two pools of the same system do not
  share a space [7].
- **Trust.** Vivaldi tolerates nodes in error but not malicious ones [1]. A tenant-controlled node
  should not be able to move other nodes' coordinates.

## 2. Failure detection

### 2.1 SWIM

SWIM separates failure detection from membership dissemination. Members probe each other in a
randomized round-robin, and updates ride on the probes and acknowledgements. "Both the expected time to
first detection of each process failure, and the expected message load per member, do not vary with
group size" [8] (DE). Members suspect before they declare, which "allows the system to discover and
rectify false failure detections", and the protocol "guarantees a deterministic time bound to detect
failures" [8]. That bound is at most twice the group size in protocol periods, from round-robin
probing (SWIM §4.3, as hyper-raft's note records [6]).

### 2.2 Lifeguard: the slow member

Lifeguard is HashiCorp's set of extensions to SWIM as run in memberlist, Serf and Consul. It starts
from the observation that "slow message processing can cause SWIM to mark healthy members as failed"
[9]. The scenarios it names include "web servers that were provisioned for the steady state, but
experience bursts of much heavier traffic" [9], which describes a bursty agent node. In the authors'
experiments one overloaded member was enough to cause false positives, and "as few as 4 overloaded
members (representing 4% of the cluster) is enough to produce hundreds of false positives at healthy
members" [9] (MV). Lifeguard has each member doubt its own detector when it is slow (local health),
and adds a nack and a buddy system. Together these reduced false positives "by a factor of between 50x
and 100x", and at healthy members by 10x to 100x, to under 2% of SWIM's baseline [9] (MV). The price
is detection latency: the median rises 0.1 s, the 99th and 99.9th percentiles about 1 s (6-7%) for
first detection, and 1.5-1.8 s (7-9%) at the 99.9th percentile. It sends about 11% more messages but
about 2% fewer bytes [9] (MV). In memberlist the suspicion timer starts at its maximum and drives
toward its minimum as independent confirmations arrive [24] (DE).

### 2.3 φ-accrual

Instead of a binary verdict, the φ-accrual detector outputs a suspicion level
`φ(t_now) = −log10(P_later(t_now − T_last))`, from the distribution of past heartbeat inter-arrivals.
Suspecting at `φ ≥ 1` is wrong about 10% of the time, at 2 about 1% and at 3 about 0.1% [25] (DE). Each
consumer picks its own threshold on one shared output, so a scheduler can stop placing new work on a
node at a low φ while a membership layer waits for a high one.

### 2.4 hyper-raft's detectors

hyper-raft assigns failure detection to node pairs, not groups. There is one detector per ordered pair
of nodes, shared by every group they share. Each is Chen, Toueg and Aguilera's NFD-E, configured from
measured heartbeat lateness with a Cantelli bound, so a crash is detected within `E(D) + α + η` [26]
(DE). A heartbeat is sent only after the sender's log device took a write and flush within the period,
so "a node whose disk stalls stops being trusted, as a crashed one does" [26]. That is a fail-slow
defense built into liveness. hyper-swim treats each member's probes of a peer as that pair's NFD-E
detector, keeps SWIM's and Lifeguard's mechanisms, and derives every timing constant from measurement
[26]. A host suspected because it is slow may ask for more time with a progress witness it cannot fake,
but "an overloaded host is never extended: shedding load heals it, and extending it would hide a host
that cannot keep up". Grants halve, and all grants together never exceed one base window [27] (DE). Its
verdicts are hints to each owner's committed membership, never durable decisions [5].

## 3. Gray failure, fail-slow and partial failure

- **Differential observability.** Huang et al. (Microsoft) argue that the major availability breakdowns
  in cloud systems come from gray failure rather than fail-stop failure: "gray failure is behind most
  cloud incidents" (CL, first-hand experience). Its key feature is that "the system's failure detectors
  may not notice problems even when applications are afflicted by them" [10].
- **Fail-slow hardware is common and slow to find.** 101 reports from 12 institutions show every
  hardware type failing slow, from internal causes (firmware, wear) and external ones. 39% of root
  causes are external: configuration, environment, temperature, power. Faults convert from fail-stop,
  fail-partial and fail-transient into fail-slow. Symptoms are permanent, transient, partial, or
  occasional stops. Root-cause chains are long: a dead fan made the other fans run at full speed, and
  their vibration degraded a disk. Impact cascades: "a degraded NIC made many jobs lock task
  slots/containers in healthy machines, hence new jobs cannot find enough free slots". Finding the faulty
  part "can take hours to months" [11] (MI). The authors' advice to designers: software handles the
  binary fail-stop model, and fail-slow needs more research [11].
- **Detect what requesters see.** Panorama turns each component into an observer that reports the
  errors it handles. It detected all 15 real-world gray failures the authors reproduced in under 7 s,
  where existing approaches detected one of them within 300 s [12] (MV).
- **Compare peers.** IASO (Nutanix) converts timeout signals into a fail-slow score per node. Deployed for
  over 1.5 years across 39,000 nodes, it isolates a slow node "within minutes". It caught 232 fail-slow
  incidents in 7 months, and it measured a fail-slow annual failure rate of 1.02% [13] (MV). Perseus
  (Alibaba) fits a light regression per drive. In 10 months on 248K drives it found 304 fail-slow cases,
  and isolating them cut node-level 99.99th-percentile latency by 48% [14] (MV).
- **Watch the program itself.** In 100 partial failures from five mature systems, Lou et al. found
  defects that need production conditions to trigger. OmegaGen generates watchdogs from the program. On 22
  real cases they detected 20, with a median detection time of 4.2 s, and pinpointed the failure scope for
  18 [15] (MV).

## 4. Containing bad workloads and nodes

### 4.1 Antagonists: detect by correlation, throttle first

CPI² uses cycles-per-instruction from hardware counters. It learns normal behavior by aggregating the
tasks of the same job, so outliers are judged against their own job, not a global threshold [16]. A
task is a victim when its CPI is above the 2σ point of the predicted distribution, and flagged at least
3 times in a 5-minute window. Likely antagonists are found passively, by correlating the victim's CPI
with the suspects' CPU usage. The antagonist is then capped: "0.01 CPU-sec/sec for low-importance ('best
effort') batch jobs and 0.1 CPU-sec/sec for other job types", applied 5 minutes at a time [16] (DE). CPI²
was "rolled out to all of Google's shared compute clusters" [16] (MV). The machine-protection side
(PSI-driven killers, `memory.min`, IO control) is in
[`../runtime/cgroup-v2-resource-control.md`](../runtime/cgroup-v2-resource-control.md).

### 4.2 Repeat offenders: escalate, forgive, cap

- **Crash loops.** The kubelet restarts a failing container "with an exponential backoff delay (10s,
  20s, 40s, …), that is capped at 300 seconds (5 minutes)", and resets the timer once the container has
  run 10 minutes without problems. An alpha feature lowers the defaults to 1 s and 60 s [17] (DE).
- **Outlier ejection.** Envoy ejects a host it finds to be an outlier, but only while the number of
  ejected hosts is below `max_ejection_percent`. The ejection lasts `base_ejection_time` multiplied by the
  host's consecutive ejections, up to `max_ejection_time`. The multiplier decrements at each healthy check,
  and a load balancer in its "panic" scenario uses ejected hosts anyway [18] (DE).
- **Borg.** "If a Borglet does not respond to several poll messages its machine is marked as down", and
  its tasks are rescheduled elsewhere. If the machine comes back, the Borgmaster tells it to kill the
  rescheduled tasks, to avoid duplicates [37]. Borg also spreads a job's tasks across failure domains
  (machines, racks, power domains), and limits both the rate of task disruptions and how many of a job's
  tasks may be down at once during maintenance. It "rate-limits finding new places for tasks from
  machines that become unreachable, because it cannot distinguish between large-scale machine failure and
  a network partition", and it "avoids repeating task::machine pairings that cause task or machine
  crashes" [37] (DE). Running tasks keep running when the Borgmaster or their Borglet is down [37]. The
  rest of Borg is in [`cluster-managers-at-scale.md`](cluster-managers-at-scale.md).
- **Nodes.** Kubernetes taints a not-ready or unreachable node `NoExecute`, and pods get an automatic
  toleration of `tolerationSeconds=300`. "The control plane limits the rate of adding new taints to nodes",
  which bounds evictions when many nodes become unreachable at once, for example in a network
  disruption [28] (DE). node-problem-detector reports a permanent problem that makes a node unavailable as
  a `NodeCondition` and a temporary one as an `Event` [29] (DE).

### 4.3 Blast radius: shuffle sharding

AWS invented shuffle sharding for Route 53. Each customer gets a near-unique combination of workers, so
a "poisonous request, or a flood of requests" from one customer impacts its own shard and at most one
worker of any other. With eight workers and shards of two there are 28 combinations, so "the scope of
impact due to a problem is just 1/28th. That's 7 times better than regular sharding" [19] (DE).

### 4.4 Retry storms and overload

- **Metastable failures.** Bronson et al. describe failures that persist after their trigger is gone,
  held in place by a sustaining effect [30][20]. A study of public incident reports found 22 such failures
  in 11 organizations, including at least 4 of 15 major AWS outages in a decade. "By far, the most common
  sustaining effect is due to the retry policy, affecting more than 50% of the studied incidents". Outages
  lasted 1.5 to 73.53 hours, and recovery "often requires reducing load" [20] (MI).
- **Retry amplification.** In a five-deep call stack with three retries at each layer, a failing
  database sees 243x its load. AWS limits retries locally with a token bucket (in the AWS SDK since 2016),
  and adds jitter so clients do not retry in lockstep [21] (DE). The agentic note records the same
  pattern in agents: Copilot's tool failures start retry loops that raise compute up to 4x
  ([`agentic-workloads.md`](agentic-workloads.md)).
- **Google's rules.** Clients throttle themselves adaptively. Over the last two minutes they track the
  requests they attempted and the ones the backend accepted, and once requests exceed `K` times accepts
  (e.g. 2) they reject new requests locally with rising probability. Every request carries one of four
  criticalities: `CRITICAL_PLUS`, `CRITICAL`, `SHEDDABLE_PLUS` or `SHEDDABLE`. A request is tried at
  most three times, and a client retries only while retries are under 10% of its requests, which "reduces
  the growth to just 1.1x" [22] (DE). Retries use randomized exponential backoff, and deadlines propagate
  down the call tree [31] (DE).

## 5. Lessons for Hex

Each lesson is a recommendation (`DE`, a design input) with its evidence and what would falsify it.
These feed the scheduler design (CP-T7.4 scoring, CP-T7.5 quotas and fair share, CP-T7.6 agents as
workloads) and node health; the decisions go in `docs/design/` with Hex's own measurements.

| Lesson | Evidence | Falsified if |
|---|---|---|
| **Use hyper-swim's coordinates as a soft latency score, weighted by each node's error estimate**: for placing an agent near its gang, its data or its caller, never as a hard filter | 11% median error in Vivaldi's evaluation [1]; higher error and heavy triangle-inequality violation in the wild [2]; the engine exists and is bounded [3][5]; Hyperscale blends a quality-weighted RTT upper bound with observed latency under a conservative prior, and never trades health for latency [34][35] | On Hex's own fleet, coordinate-predicted RTT ranks candidate nodes no better than random against measured RTT |
| **Measure the RTT to non-member endpoints directly** (provider APIs, registries, object stores), per node and passively from real calls | Coordinates cover members only; Consul's two pools do not share a space [7] | Providers expose a coordinate-compatible latency signal |
| **Bandwidth comes from topology and measured transfers, not coordinates** | No source read predicts bandwidth from coordinates [1][2][4] | A coordinate-based bandwidth predictor is found with measured accuracy |
| **Node liveness is hyper-swim's (SWIM + Lifeguard + NFD-E)**; its verdicts are hints, and the scheduler's committed membership decides | Constant per-member load [8]; 50-100x fewer false positives under slow members [9]; measured-timing detectors [26] | Hex's bursty nodes still show false suspicions at a rate that churns placement |
| **Health is graded, not binary**: stop new placements at a low suspicion level and evict only at a high one, on one φ-style signal | φ's per-consumer thresholds [25]; gray failure escapes binary detectors [10][11] | A binary verdict, tuned on Hex's traces, misses no gray failure that a graded one catches |
| **Every placement outcome is an observation of the node**: start latency, exit status, provider errors and request latency, compared across peers | Panorama's requester view [12]; IASO's peer timeouts [13]; Perseus's per-device outliers [14] | Peer comparison on Hex's fleet flags no node that liveness missed |
| **Antagonists: detect by correlation within a workload class, throttle with a cap first, then move** | CPI²'s 2σ, 3 times in 5 minutes, then a 5-minute cap at 0.01 or 0.1 CPU [16] | Throttling does not restore the victims' performance on Hex workloads |
| **Repeat offenders: escalating backoff with a forgiveness window, and a cap on the share of nodes or workloads quarantined at once** | CrashLoopBackOff [17]; Envoy's escalating ejection and max-ejection share [18]; rate-limited node taints [28] | Escalation hides recoverable workloads, by measured lost goodput |
| **Shuffle-shard tenants over nodes and quota accounts** so that a poisonous workload reaches only its own shard | 1/28 blast radius at 8 workers and shards of 2 [19] | Hex's tenant count is too small for shards to differ |
| **Retry budgets, deadline propagation and criticality-based shedding everywhere Hex retries** (placements, provider calls, restarts) | Retries sustain most metastable failures [20]; 243x amplification [21]; Google's 3-per-request and 10%-per-client budgets [22][31] | Hex's measured retry rate never exceeds its budget under injected overload |
| **Isolate dangerous workloads by runtime class**: a workload that trips a security signal moves to a stronger class or is refused, typed | The runtime notes' class matrix ([`../runtime/matrix.md`](../runtime/matrix.md)) | n/p here; security signals were not researched in this note |

## 6. Open questions and gaps

- **Coordinates on Hex's own fleet.** hyper-swim's accuracy has been checked only on links that are all
  alike [6]. Its error on the real mixed fleet (macOS, Windows and Linux; LAN, VPN and WAN) is
  unmeasured, and so is its value for placement.
- **Bandwidth.** No source read gives a decentralized bandwidth predictor with measured accuracy.
- **Grading.** Lifeguard measured false positives at its authors' cluster size [9]. No source measures
  a graded health signal driving placement, as opposed to membership.
- **Agent-specific faults.** None of the gray-failure work covers agent workloads: stuck tool loops,
  quota exhaustion presented as slowness, or a model provider's partial outage. These need Hex's own
  traces.
- **Security signals.** Which runtime events (seccomp denials, egress attempts, privilege changes)
  should quarantine a workload, and how fast, was not researched here.

## References

Accessed 2026-10-22 unless stated; every source was opened and read for this note. arXiv entries name
the version read.

[1] F. Dabek, R. Cox, F. Kaashoek, R. Morris (MIT CSAIL), "Vivaldi: A Decentralized Network Coordinate System", SIGCOMM '04. https://pdos.csail.mit.edu/papers/vivaldi:sigcomm/paper.pdf

[2] J. Ledlie, P. Gardner, M. Seltzer (Harvard, Aelitis), "Network Coordinates in the Wild", NSDI '07. https://www.usenix.org/legacy/event/nsdi07/tech/full_papers/ledlie/ledlie.pdf

[3] hyper-raft (github.com/hyper-light/hyper-raft) at 6a236ae, `crates/hyper-swim/src/coordinates.rs` (module documentation) and `crates/hyper-swim/ORIGIN.md` (change 7).

[4] HashiCorp, serf `coordinate/config.go`, master branch. https://github.com/hashicorp/serf/blob/master/coordinate/config.go

[5] hyper-raft (github.com/hyper-light/hyper-raft) at 6a236ae, `crates/hyper-swim/src/detector.rs` (module documentation, lines 74-77) and `src/lib.rs`.

[6] hyper-raft (github.com/hyper-light/hyper-raft) at 6a236ae, `docs/research/swim.md`, "The network coordinates" and the SWIM entries (its sources verified 2026-10-02).

[7] HashiCorp, "Network coordinates", Consul documentation. https://developer.hashicorp.com/consul/docs/architecture/coordinates

[8] A. Das, I. Gupta, A. Motivala (Cornell), "SWIM: Scalable Weakly-consistent Infection-style Process Group Membership Protocol", Proc. 2002 International Conference on Dependable Systems and Networks (venue as Lifeguard's reference list gives it). https://www.cs.cornell.edu/projects/Quicksilver/public_pdfs/SWIM.pdf

[9] A. Dadgar, J. Phillips, J. Currey (HashiCorp), "Lifeguard: Local Health Awareness for More Accurate Failure Detection", arXiv:1707.00788v2, 3 Apr 2018. https://arxiv.org/abs/1707.00788

[10] P. Huang, C. Guo, L. Zhou, J. R. Lorch, Y. Dang, M. Chintalapati, R. Yao (Microsoft Research, Microsoft Azure), "Gray Failure: The Achilles' Heel of Cloud-Scale Systems", HotOS '17. https://www.microsoft.com/en-us/research/wp-content/uploads/2017/06/paper-1.pdf

[11] H. S. Gunawi et al. (University of Chicago and 11 other institutions), "Fail-Slow at Scale: Evidence of Hardware Performance Faults in Large Production Systems", FAST '18. https://www.usenix.org/system/files/conference/fast18/fast18-gunawi.pdf

[12] P. Huang et al., "Capturing and Enhancing In Situ System Observability for Failure Detection" (Panorama), OSDI '18. https://www.usenix.org/system/files/osdi18-huang.pdf

[13] B. Panda, D. Srinivasan, H. Ke, K. Gupta, V. Khot, H. S. Gunawi (Nutanix, University of Chicago), "IASO: A Fail-Slow Detection and Mitigation Framework for Distributed Storage Services", USENIX ATC '19. https://www.usenix.org/system/files/atc19-panda_0.pdf

[14] R. Lu, E. Xu et al. (SJTU, Alibaba, Xiamen University), "Perseus: A Fail-Slow Detection Framework for Cloud Storage Systems", FAST '23. https://www.usenix.org/system/files/fast23-lu.pdf

[15] C. Lou et al., "Understanding, Detecting and Localizing Partial Failures in Large System Software" (OmegaGen), NSDI '20. https://www.usenix.org/system/files/nsdi20-paper-lou.pdf

[16] X. Zhang et al. (Google), "CPI²: CPU performance isolation for shared compute clusters", EuroSys '13. https://static.googleusercontent.com/media/research.google.com/en//pubs/archive/40737.pdf

[17] Kubernetes, "Pod Lifecycle", Kubernetes documentation. https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/

[18] Envoy, "Outlier detection", Envoy architecture overview (latest). https://www.envoyproxy.io/docs/envoy/latest/intro/arch_overview/upstream/outlier

[19] Amazon Web Services, "Workload isolation using shuffle-sharding", Amazon Builders' Library. https://aws.amazon.com/builders-library/workload-isolation-using-shuffle-sharding/

[20] L. Huang et al., "Metastable Failures in the Wild", OSDI '22. https://www.usenix.org/system/files/osdi22-huang-lexiang.pdf

[21] Amazon Web Services, "Timeouts, retries, and backoff with jitter", Amazon Builders' Library. https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/

[22] Google, "Handling Overload", Site Reliability Engineering (the SRE book), chapter 21. https://sre.google/sre-book/handling-overload/

[23] HashiCorp, serf `coordinate/client.go`, master branch. https://github.com/hashicorp/serf/blob/master/coordinate/client.go

[24] HashiCorp, memberlist `suspicion.go`, master branch. https://github.com/hashicorp/memberlist/blob/master/suspicion.go

[25] N. Hayashibara, X. Défago, R. Yared, T. Katayama (JAIST), "The φ Accrual Failure Detector", research report IS-RR-2004-010, 10 May 2004. https://dspace.jaist.ac.jp/dspace/bitstream/10119/4784/1/IS-RR-2004-010.pdf

[26] hyper-raft (github.com/hyper-light/hyper-raft) at 6a236ae, `docs/timing.md`, sections 2.1, 2.2 and 2.7.

[27] hyper-raft (github.com/hyper-light/hyper-raft) at 6a236ae, `crates/hyper-swim/src/extension.rs` (module documentation).

[28] Kubernetes, "Taints and Tolerations", Kubernetes documentation. https://kubernetes.io/docs/concepts/scheduling-eviction/taint-and-toleration/

[29] Kubernetes, node-problem-detector `README.md`, master branch. https://github.com/kubernetes/node-problem-detector/blob/master/README.md

[30] N. Bronson, A. Aghayev, A. Charapko, T. Zhu, "Metastable Failures in Distributed Systems", HotOS '21. https://sigops.org/s/conferences/hotos/2021/papers/hotos21-s11-bronson.pdf

[31] Google, "Addressing Cascading Failures", Site Reliability Engineering (the SRE book), chapter 22. https://sre.google/sre-book/addressing-cascading-failures/

[32] hyper-light, Hyperscale `docs/architecture/AD_35.md`, "AD-35: Vivaldi Network Coordinates with Role-Aware Failure Detection" ("As built", 2026-10), at commit 9e7fcda. https://github.com/hyper-light/hyperscale/blob/9e7fcda/docs/architecture/AD_35.md

[33] hyper-light, Hyperscale `hyperscale/distributed/models/vivaldi_config.py` and `hyperscale/distributed/swim/coordinates/coordinate_engine.py`, at commit 9e7fcda. https://github.com/hyper-light/hyperscale/tree/9e7fcda/hyperscale/distributed

[34] hyper-light, Hyperscale `docs/architecture/AD_36.md`, "AD-36: Vivaldi-Based Cross-Datacenter Job Routing" ("As built", 2026-10), at commit 9e7fcda. https://github.com/hyper-light/hyperscale/blob/9e7fcda/docs/architecture/AD_36.md

[35] hyper-light, Hyperscale `hyperscale/distributed/routing/datacenter_latency_estimator.py`, at commit 9e7fcda. https://github.com/hyper-light/hyperscale/blob/9e7fcda/hyperscale/distributed/routing/datacenter_latency_estimator.py

[36] hyper-light, Hyperscale `docs/architecture/AD_45.md`, "AD-45: Adaptive Route Learning", at commit 9e7fcda. https://github.com/hyper-light/hyperscale/blob/9e7fcda/docs/architecture/AD_45.md

[37] A. Verma, L. Pedrosa, M. Korupolu, D. Oppenheimer, E. Tune, J. Wilkes (Google), "Large-scale cluster management at Google with Borg", EuroSys '15. https://storage.googleapis.com/gweb-research2023-media/pubtools/pdf/43438.pdf
