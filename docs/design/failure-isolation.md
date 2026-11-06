# Failure detection and isolation: a bound per fault class

Scheduler implementation goal 5 (`scheduler-architecture.md` S8): how Hex finds bad nodes and
workloads, how fast, and how it isolates them without letting a wrong verdict, or a right one about
many nodes at once, take a cell down. Each decision has its sources, evidence and falsifier (CLAUDE.md
§5). Sources are `docs/research/scheduler/failure-isolation.md` (`FI[n]`) and
`proximity-and-failure-containment.md` (`PF[n]`).

## What it is

- `crates/hex-health`, a pure core (CLAUDE.md §9): every detector and penalty, fed observations stamped
  with the caller's own time. It has no clock and no I/O, so hex-sim drives it in virtual time and a node
  agent drives it with real probes, on one code path (§8).
- `crates/hex-sim/src/membership.rs`: SWIM with Lifeguard, modelled, for the crash class and Lifeguard's
  false-positive bar.
- `crates/hex-sim/src/faults.rs`: the nemesis that injects each fault class into a simulated cell driving
  hex-health, and measures each fault's detection and isolation time against its bound below.
- The node's disk probe and drain, shown by a dm-delay drill (`AC-T7.10.3`).

One graded level per node (`hex_health::Level`): `Suspect` stops new placements, `Drain` moves work
off. Every verdict goes through the penalties (F6), which cap how much of a cell it may take.

## The bounds

Detection is the first instant the scheduler acts on the fault (stops placing on the node, holds the
account, stops the session); isolation is the instant the fault can do no more (work moved, antagonist
capped, session or workload stopped). Times are from the fault's start, in the model's own parameters,
each named below.

| Class | Injected | Detected when | Isolated when | Bound |
|---|---|---|---|---|
| Crash | a node stops answering | a member's own probe first suspects it (F1) | a member condemns it (hyper-swim; memberlist: declares it dead) and its work is moved | hyper-swim: condemnation within the bound its condemner states, `(2(2m − 1) + 1)` periods + its measured wait (F1); memberlist, the competitor: suspicion ≤ 6 probe periods + probe timeout at p99, ≤ 2(n−1) periods + timeout at max, dead ≤ suspicion + suspicion max timeout + 1 period |
| Gray / fail-slow | a node's work runs ≥ 2.5x slower than its peers' | `Suspect` (F2) | `Drain` | ≤ 2 windows (2 s) and ≤ 4 windows (4 s): under Panorama's 7 s (FI[4]) |
| Disk stall | a node's probe write stops completing | `Suspect` (F3) | `Drain` | ≤ 1 timeout + 1 probe period and ≤ 3 timeouts + 1 period; the timeout is at least 1 s (1 s on a healthy fast disk) |
| Antagonist | a co-tenant's CPU use doubles its neighbours' CPI | a victim with an antagonist correlated ≥ 0.35 (F4) | the antagonist capped | ≤ 3 CPI samples + 1 s (CPI²'s cadence: one a minute, so ≤ 181 s) |
| Stuck tool loop | a session repeats a call without progress | the third identical call (F5) | the session stopped at it | at the 3rd call: 0 after it |
| Quota shown as slowness | a provider answers rate limit or quota exhausted | the first typed error (F5) | the account held | at the first error; node suspicions = 0 |
| Partial provider outage | a provider fails for most of its callers | the provider marked degraded (F5) | no work needing it placed | ≤ 2 windows (2 s); node suspicions = 0 |
| Dangerous | a seccomp kill, ungranted egress or refused privilege gain, in an OCI container, a Firecracker or a Shards microVM | the signal (F7) | stopped; an OCI container restarted as the first microVM its node runs that holds it (Firecracker, then Shards), else refused; a microVM's trip refused | at the signal: 0 after it |

And across classes (F6):

| Bound | Value |
|---|---|
| nodes ejected or draining at once in a cell | ≤ max(1, ⌊10% of the cell⌋) for a cell of 2 or more; 0 for a cell of 1 |
| nodes whose work is moved after failure | ≤ 0.1/s; ≤ 0.01/s when 55% (and at least 3) are down in a cell over 50 nodes; 0 when that happens in a cell of 50 or fewer |
| load growth from retries | ≤ 1.1x the requests, ≤ 3 attempts each |
| a crash-looping workload's restart delay | 10 s doubling to 300 s, reset after 10 minutes running |

## Decisions

### F1. Crash: hyper-swim's detector, with memberlist's SWIM and Lifeguard as the competitor

**Which detector the node agent runs.** None yet: today's node agent (`hex-node`) runs no membership.
The design gives it hyper-swim (S8, `scheduler-architecture.md`), hyper-raft's SWIM with Lifeguard's
buddy system, nacks and local health, and so the crash class's bound is hyper-swim's, measured on
hyper-swim's own `Detector`, vendored unchanged (`vendor/MANIFEST.toml` [projects.hyper-raft] at
6a236ae, 0 recorded changes) and driven by hex-sim (`swim.rs`). It is not memberlist's: hyper-swim
times each pair as an NFD-E detector from its measured round trips (an RFC 6298 timer of 1-60 s until
an estimator configures), its period is what its probe needs or the owner's detection budget's share
of one, and only a member's own probes condemn, never a gossiped suspicion.

**hyper-swim's crash bound, derived.** A member's rounds probe each of the `m` members its view holds
once, in a shuffled order, so its next probe of a crashed member is at most `2m − 1` periods away
(SWIM §4.3, `PF[8]`); unanswered, it suspects. Its next probe of the suspect tells it so (Lifeguard's
buddy system) and starts at most as far again; when that period ends unanswered the condemnation is
pending. It is made once another member has answered, proof that this member's own network works: a
wait nothing bounds in advance, which the member measures. So from the victim's last answer to its
condemnation: at most `(2(2m − 1) + 1)` periods, each the longest this member's periods have run or an
unanswered probe's deadlines allow, plus the measured wait. Each member states that bound itself
(`Detector::detection_bound`, hyper-raft `crates/hyper-swim/src/detector.rs:1914-1930` and `bound_periods`, `:1950-1957`, at 6a236ae), so
the bar is that every condemnation comes within the bound its condemner stated.

**Measured** (hex-sim `tests/swim.rs`, 1,000 seeds at each pace of 8-128 members on a LAN of
0.1-2 ms one-way, one member crashed after warm-up; fedora unit
`hex-sched-g5-probe-swim3-20261105T124924Z`): every crash suspected and condemned, every condemnation
within its stated bound, no live member condemned.

| | first suspicion p99 / max | first condemnation p99 / max | stated bound p50 / max | probes per member-second |
|---|---|---|---|---|
| hyper-swim, periods at need (budget 0) | 0.69 / 1.09 s | 1.62 / 1.90 s | 320 / 656 s | ~47 |
| hyper-swim, one probe a second (budget `(2(2m − 1) + 1)` s) | 5.49 / 7.99 s | 12.1 / 16.0 s | 365 / 1,090 s | ~1 |
| memberlist SWIM + Lifeguard (the competitor, `membership.rs`) | 5.60 / 8.59 s | 13.4 / 15.7 s (dead) | | 1 |

At memberlist's load, hyper-swim condemns as fast as memberlist declares dead, faster at the p99. Its
stated bound is another matter: 50 to 100 times what it measured, because it is the round-robin worst
case, `4m − 1` periods for `m` peers, times the longest period the member has run (the first periods,
before an estimator configures, run to RFC 6298's initial second). It is not the 60 s RTO cap: no run
here doubled a timer that far. A finding for hyper-swim's owner: the bound a member states is sound
(no condemnation passed it) but too loose to plan an owner's failover budget on at a hundred members.

**memberlist, the competitor.** The model (`membership.rs`) is memberlist's LAN defaults (FI[2]), the
reference implementation of SWIM with Lifeguard: a probe a second, a 500 ms probe timeout, 3
indirect probes, a suspicion timeout from `4 × max(1, log10 n)` periods up to 6 times that, shrinking as
independent confirmations arrive (FI[3]), and Lifeguard's local health multiplier up to 8. A suspicion
is a hint: the scheduler stops placing on a suspected node at once, and moves its work only once it is
declared dead, through F6's caps. It takes none of hyper-swim's code or constants; it is kept as
the competitor, for Lifeguard's false-positive bar and memberlist's crash timing.

**memberlist's crash bound, derived.** Each period every live member probes one other, so a given member goes
unprobed in a period with probability about `(1 − 1/(n−1))^(n−1) ≈ e^−1` (`PF[8]` §3.1: the first
probe comes `e/(e−1)` periods after a crash on average), and in `k` periods with about `e^−k`. Its
prober suspects it when the indirect probes' wait ends, a period after its probe began (memberlist's
`probeNode`). The p99 of the first suspicion is then `ln(100) + 1 ≈ 5.6` periods: at most 6 periods and
the probe timeout. hex-sim measured 5.60 s over 1,000 seeds of 8-128 members, with Lifeguard and
without. The bound was 5 periods until 2026-11-05: that left out the period from the probe to the
suspicion, so no SWIM on memberlist's protocol could meet it. A target selection that probes every
member every period (a shared ring per period) would bound it at 2 periods; that is hyper-swim's
protocol, not Hex's, and a possible later improvement for its owner.

**Sources.** SWIM's deterministic bound from round-robin probing (`PF[8]`); Lifeguard's 50-100x fewer
false positives at a cost of about 1 s at p99 (FI[1]); memberlist's constants (FI[2], FI[3]).

**Confirmations count only from members in good local health.** Lifeguard's dynamic suspicion
shortens a suspicion's timeout with each other member's suspicion of the same target, as if each
were independent evidence (FI[3]). They aren't when they come from slow members: each slow member
suspects healthy peers on its own probes and gossips it, the others take that as a confirmation,
and the shortened timeout runs out before the slow member hears the refutation. So a gossiped
suspicion confirms only when its sender's local health multiplier was 0 as it sent it (the
suspicion carries that bit): Lifeguard's own premise, that a member's LHM is its evidence of its own
slowness (FI[1]), applied to the suspicions it gossips. hyper-swim goes further, and lets no
gossiped suspicion condemn: "A gossiped suspicion is a hint: only a member's own probes condemn"
(hyper-raft `crates/hyper-swim/src/detector.rs:44` at 6a236ae). The model keeps Lifeguard as
published beside it as the competitor (`Config::health_gated_confirmations` off).

**Evidence.** `membership.rs`'s tests: the crash bound over 1,000 seeds, unchanged by the gate (first
suspicion p99 5.60 s, first dead p99 13.4 s). With 4% of members slow, over 1,000 seeds of 32-128
members, healthy members declared dead: SWIM 190,359; Lifeguard as published 103 (0.05% of SWIM's,
under its 2% bar); with confirmations health-gated, 0 (fedora unit
`hex-sched-g5-probe-gate1-20261105T123854Z`). Before the gate, a trace of the published variant's
false positives (unit `hex-sched-g5-probe-chk1-20261105T113256Z`) found each declared by a slow
member, with exactly one confirmation, from another slow member. With one slow member delayed
1-30 s, no false positives with Lifeguard either way (SWIM: 16,388), since nobody confirms.

**What would falsify it.** A crash suspected or declared dead past its bound; with confirmations
health-gated, any healthy member declared dead because of slow members; or Lifeguard as published
above 2% of SWIM's rate.

### F2. Gray and fail-slow: requesters' observations compared across peers

**The choice.** Every piece of work a node does is reported by its requester as a slowdown: its time
over what its class expects. Each 1 s window, a node with at least 5 observations whose median slowdown
is at least twice the median of its peers' medians (at least 3 peers with evidence) is slow for the
window: `Suspect` at once, `Drain` after 3 slow windows in a row (CPI²'s 3 flags, FI[9]), healthy again
on a good window. Once suspected, a node's window is still slow at 1.5x its peers': the 2x bar less the
histogram's 25% resolution, so a window whose median dips under the bar but within what the histogram
can tell from it is no evidence of recovery. Without it, a 2.5x node whose ten-odd pieces of work in a
window happen to land low resets its count and drains a window or more late (the nemesis's seed 287:
5.2 s). Medians come from a fixed-size log histogram (4 buckets per power of two, so within 25%),
so a node's history costs 1 KiB however busy it is. The median and the 25% quantisation together mean a
node 2.5x or more slower is always caught; one between 2x and 2.5x may be.

**Sources.** Gray failure is what requesters see and heartbeats don't (`PF[10]`, `PF[12]`); IASO and
Perseus compare peers (FI[7], `PF[14]`); a graded verdict each consumer thresholds (FI[5]).

**Evidence.** `hex-health` `grade` tests: a 3x-slow node suspected within 2 windows and drained within
4; a cell-wide slowdown suspects nobody; too little evidence decides nothing. `faults.rs` measures it over
1,000 seeds.

**What would falsify it.** A ≥ 2.5x-slow node with evidence not suspected within 2 s or drained within 4
s; or a healthy node suspected when the slowdown is cell-wide.

### F3. Disk stall: the node's own write-and-flush probe, timed as RFC 6298 times a segment

**The choice.** The node agent writes and flushes a record on its state disk every probe period. The
write's timeout is RFC 6298's `SRTT + max(G, 4·RTTVAR)` over the probe's own completed latencies, at
least 1 s and at most 60 s (FI[8]); a probe that outlived its timeout is no sample (Karn). The node is
`Suspect` when the outstanding write outlives one timeout and `Drain` at the timer's second expiry, the
first doubled (three timeouts), or after 3 completed probes in a row each slower than the timeout. A
node that can't write its own disk stops being trusted as a crashed one does (hyper-raft's heartbeat
after flush, `PF[26]`).

**Sources.** FI[8]; `PF[26]`; FI[6] for why waiting for a human doesn't do (fail-slow found in hours to
months).

**Evidence.** `hex-health` `probe` tests, and `AC-T7.10.3`'s dm-delay drill (`bench/health/`), which
stalls the node's disk under running work and records when it was marked, drained, and whether any work
failed.

**What would falsify it.** A drill in which work fails before the node is drained, or a healthy disk's
ordinary flush latency suspected.

### F4. Antagonists: CPI² as written

**The choice.** CPI² (FI[9]) unchanged: a job's CPI spec from its tasks' samples (no management under 5
tasks or 100 samples a task; tasks under 0.25 CPU-sec/sec ignored), an outlier above the 2σ point, a
victim at 3 outliers in 5 minutes, at most one identification a second, the antagonist the co-tenant
whose CPU use correlates best with the victim's CPI over 10 minutes by CPI²'s correlation, at least 0.35,
capped at 0.01 CPU-sec/sec (best effort) or 0.1 for 5 minutes. The spec learns only from samples that
aren't outliers, so interference doesn't teach the spec to accept it.
A node's samples of one minute are taken together, as CPI²'s per-machine agent gathers them (§3), and
a victim is judged only once all are in: judged at its own sample, it would correlate against its
co-tenants' samples one minute short, and the cap would come a sample late (the nemesis's seed 12: 240 s).
The bound is for the class the table states, CPI at least doubled: the correlation over CPI²'s 10
minutes then passes 0.35 by the third sample after the onset. A weaker antagonist takes longer, as
CPI²'s correlation says it must: over 1,000 seeds at 1.8-3x, every one at 1.915x or more was capped
within the bound, and the 44 at 1.80-1.91x at the fourth or fifth sample after the onset (240-300 s;
hex-sim, 2026-11-05, unit `hex-sched-g5-main-meas-20261105T081731Z`).

**Evidence.** `hex-health` `antagonist` tests; `faults.rs` over 1,000 seeds.

**What would falsify it.** An antagonist not capped within 3 samples and a second, or a bystander capped
(correlated below 0.35 or not on the victim's node).

### F5. Agent faults are charged to what caused them, never to the node

**The choice.** These are faults of an agent session, the workload running inside an OCI container, a
Firecracker microVM or a Shards microVM; the detectors read the session's calls and its provider's
errors, which are the same in each kind, so the bars hold for all three.
- A stuck tool loop: a session that makes the same call (its tool and arguments, hashed) a third time
  with no progress between is stopped, as is one that makes 256 different calls with none (the memory
  bound, and itself a verdict). The repeat limit is chosen, not measured: MAST names step repetition and
  its share (FI[16]) but gives no threshold, and kiln's recorded agent traces are what would set it (A4).
- Quota shown as slowness: a provider's typed rate-limit or quota error holds the account (until its
  retry-after, or until a call succeeds) at the first error, and is never a node observation.
- A partial provider outage: overload, server errors and timeouts on a majority of a provider's callers
  (at least 3) in a window mark the provider degraded. Failing half or more of its calls (at least 5)
  is charged to a node's own path only when fewer than 3 of the provider's callers saw any error and
  at least 3 saw none, so an outage's first window, when only some callers have failed yet, charges
  nobody. Provider errors never feed F2.

**Sources.** FI[16]; FI[17] (typed 429s and 529s; overload "across all users").

**Evidence.** `hex-health` `agent` tests; `faults.rs` injects each over 1,000 seeds and counts node
suspicions from quota and outages, which must be 0.

**What would falsify it.** A healthy session's trace with a third identical call and no progress; a node
suspected for its provider's or account's fault; a provider outage not marked within 2 windows.

### F6. Penalties escalate, forgive and are capped

**The choice.**
- A node's `Drain` ejects it from placement for 30 s times its consecutive ejections, up to 300 s,
  forgiven one multiple per 30 s healthy; never past 10% of the cell, at least one in a cell of two or
  more (Envoy's defaults and `always_eject_one_host`, FI[10]). A refused ejection leaves the node
  `Suspect`: no new work, its running work kept.
- Moving work off failed nodes is rate-limited: 0.1 nodes/s, 0.01/s once 55% (and at least 3) of a cell
  over 50 nodes are down, none when that happens in a smaller cell (Kubernetes' defaults, FI[11]),
  because a mass failure can't be told from a partition (Borg, FI[12]).
- A crash-looping workload restarts after 10 s doubling to 300 s, reset after 10 minutes running
  (FI[13]).
- Retries: at most 3 attempts a request, and a client retries only while its retries in the last two
  minutes are under 10% of its requests: load grows at most 1.1x (FI[14]).

**Evidence.** `hex-health` `penalty` and `retry` tests; `faults.rs`'s correlated faults (a cell-wide
gray fault, a partition losing most of a cell) never exceed the cap or the rate.

**What would falsify it.** A run in which a cell has more nodes ejected or draining than the cap, or
moves work faster than the rate; retries growing load past 1.1x.

### F7. Dangerous workloads move to a stronger kind or are refused

**The choice.** Hex places three kinds of workload: OCI containers, Firecracker microVMs and Shards
microVMs (G2's `model::Class`; Ada, 2026-11-06). A workload that trips a runtime security signal is
stopped at once. A tripped OCI container starts again only as a microVM on its node: the first of
Firecracker, then Shards, that the node runs and that will hold it, its own need plus that microVM's
overhead in what the node has free once the container's need and overhead are given back
(`hex_sim::danger`). A GPU workload fits neither, since neither microVM takes a device. Otherwise it is
refused, typed: its node runs no microVM (`NoMicroVmOnNode`), or none it runs holds it (`DoesNotFit`). A
trip inside a Firecracker or Shards microVM is refused (`InMicroVm`): no kind Hex places isolates more
strongly. What trips: a seccomp kill, an egress attempt no grant names, a refused privilege gain. A call
failed with an errno doesn't: profiles answer calls they don't support that way, and programs fall back.

**How the two microVMs rank.** Firecracker first. Both run their own guest kernel under KVM, unlike an OCI
container on its host's (FI[19]). Firecracker's isolation surface is published and measured: a minimal
device model (Agache et al., NSDI '20; `docs/research/runtime/substrates-and-sandboxes.md` [SUB-11]).
Shards' device model is not public in the sources read (`docs/research/scheduler/agentic-workloads.md`,
its device table). Their overheads don't decide it: Firecracker's is 5 MiB by its specification and
Shards' 5 MiB is a stand-in until Shards' own is measured (`docs/design/hex-sim.md`, the recorded
workload). Shards first would need its device model shown smaller than Firecracker's.

**Which bars are per kind.** Only this one: a trip's outcome depends on the kind it ran in. The agent
faults (F5) read a session's own tool calls and provider errors, which are the same inside any of the
three kinds; gray and fail-slow (F2), the disk stall (F3), antagonists (F4) and the penalties (F6) are
the node's. So those are measured once, for every kind on the node.

**Sources.** seccomp's typed actions (FI[18]); containers share the host kernel and microVMs don't
(FI[19]); Firecracker's device model [SUB-11]; the kinds and their overheads, G2 (`hex-sim.md`). Which
signals should trip is unresearched (FI §8): this is Hex's own choice.

**Evidence.** `hex-sim` `danger` tests (the move counts both overheads, to the byte; refusals typed);
`faults.rs` over 1,000 seeds of each kind against an oracle from the rule's statement, no wrong verdict
(fedora unit `hex-sched-g5-main-kinds2-20261106T130519Z`): OCI, 500 moved to Firecracker, 209 to
Shards, 152 refused for a node without a microVM, 139 for none that holds it; Firecracker and Shards,
1,000 refused each, in a microVM.

**What would falsify it.** A tripped workload started again as an OCI container, or in a kind its node
doesn't run or that doesn't hold it; a microVM's trip moved; a benign workload refused for errno'd calls.

## Constants

Each constant in hex-health, with its value and source; `doc_truth` in `crates/hex-health/tests/`
holds this table to the code.

| Constant | Value | Source |
|---|---|---|
| `probe::INITIAL_TIMEOUT_NS` | 1000000000 | RFC 6298 §2.1 (FI[8]) |
| `probe::MIN_TIMEOUT_NS` | 1000000000 | RFC 6298 §2.4 (FI[8]) |
| `probe::MAX_TIMEOUT_NS` | 60000000000 | RFC 6298 §2.5 (FI[8]) |
| `probe::DRAIN_TIMEOUTS` | 3 | RFC 6298 §5.5, the timer doubled once (FI[8]) |
| `probe::SLOW_STREAK` | 3 | CPI²'s 3 flags (FI[9]) |
| `grade::WINDOW_NS` | 1000000000 | chosen: 2 and 4 windows fit Panorama's 7 s (FI[4]) |
| `grade::MIN_OBSERVATIONS` | 5 | chosen: the fewest a median of which one outlier can't move |
| `grade::MIN_PEERS` | 3 | chosen: two nodes can't say which of them is slow |
| `grade::SLOW_FACTOR_MILLI` | 2000 | chosen; falsified by a missed 2.5x node or a suspected healthy one |
| `grade::STAY_FACTOR_MILLI` | 1500 | derived: `SLOW_FACTOR_MILLI` less the histogram's 25% resolution |
| `grade::DRAIN_WINDOWS` | 3 | CPI²'s 3 flags (FI[9]) |
| `penalty::BASE_EJECTION_NS` | 30000000000 | Envoy `base_ejection_time` (FI[10]) |
| `penalty::MAX_EJECTION_NS` | 300000000000 | Envoy `max_ejection_time` (FI[10]) |
| `penalty::MAX_EJECTION_PER_MILLE` | 100 | Envoy `max_ejection_percent` (FI[10]) |
| `penalty::EVICTION_MILLI_PER_S` | 100 | Kubernetes `--node-eviction-rate` (FI[11]) |
| `penalty::SECONDARY_EVICTION_MILLI_PER_S` | 10 | Kubernetes `--secondary-node-eviction-rate` (FI[11]) |
| `penalty::UNHEALTHY_PER_MILLE` | 550 | Kubernetes `--unhealthy-zone-threshold` (FI[11]) |
| `penalty::UNHEALTHY_MIN_NODES` | 3 | Kubernetes `--unhealthy-zone-threshold` "minimum 3" (FI[11]) |
| `penalty::LARGE_CELL_NODES` | 50 | Kubernetes `--large-cluster-size-threshold` (FI[11]) |
| `penalty::BACKOFF_BASE_NS` | 10000000000 | Kubernetes CrashLoopBackOff (FI[13]) |
| `penalty::BACKOFF_CAP_NS` | 300000000000 | Kubernetes CrashLoopBackOff (FI[13]) |
| `penalty::BACKOFF_RESET_NS` | 600000000000 | Kubernetes CrashLoopBackOff (FI[13]) |
| `retry::MAX_ATTEMPTS` | 3 | SRE ch. 21 (FI[14]) |
| `retry::RETRY_PER_MILLE` | 100 | SRE ch. 21 (FI[14]) |
| `retry::BUCKET_NS` | 10000000000 | chosen: 12 buckets make SRE ch. 21's two minutes (FI[14]) |
| `antagonist::MIN_JOB_TASKS` | 5 | CPI² §3 (FI[9]) |
| `antagonist::MIN_TASK_SAMPLES` | 100 | CPI² §3 (FI[9]) |
| `antagonist::MIN_CPU_MILLI` | 250 | CPI² §3 (FI[9]) |
| `antagonist::SIGMAS` | 2 | CPI² §3 (FI[9]) |
| `antagonist::VICTIM_FLAGS` | 3 | CPI² §3 (FI[9]) |
| `antagonist::FLAG_WINDOW_NS` | 300000000000 | CPI² §3 (FI[9]) |
| `antagonist::CORRELATION_WINDOW_NS` | 600000000000 | CPI² §4.1 (FI[9]) |
| `antagonist::MIN_CORRELATION_MILLI` | 350 | CPI² §4.1 (FI[9]) |
| `antagonist::IDENTIFY_GAP_NS` | 1000000000 | CPI² §4.2 (FI[9]) |
| `antagonist::BEST_EFFORT_CAP_MILLI` | 10 | CPI² §4.2 (FI[9]) |
| `antagonist::CAP_MILLI` | 100 | CPI² §4.2 (FI[9]) |
| `antagonist::THROTTLE_NS` | 300000000000 | CPI² §4.2 (FI[9]) |
| `agent::REPEAT_LIMIT` | 3 | chosen, to be set from kiln's agent traces (A4; FI §8) |
| `agent::MAX_DISTINCT_CALLS` | 256 | the memory bound per session |
