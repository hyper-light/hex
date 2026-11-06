# G5's constants, audited against Ada's rule

Ada, 2026-11-06: "We do not support arbitrary constants. We support learned and data derived values."
Latency and throughput come before memory: a memory cut that costs either isn't taken.

Every constant `cp/t7-sched-g5` adds or relies on, by kind:

- **(a)** learned at runtime from what the system sees;
- **(b)** derived from measured data, with its record;
- **(c)** a platform or spec limit, cited at its source;
- **(d)** anything else: picked, or another system's default. A published number counts as (d): citing
  it isn't deriving it.

Each (d) has its way out: how it becomes (a)-(c), or why it can't yet. Many wait on one requirement
from Ada: how often Hex may wrongly stop placing on, or drain, a healthy node. The thresholds that
decide a verdict (drain counts, the slow bar, CPI²'s 2σ and 0.35, the probe's warm-up) are each the
point at which a healthy node is wrongly flagged at that rate, computed from what the system learns. "Requirement" marks a number
that is the bar being met, not a tuning; "bound" marks a resource bound (CLAUDE.md §2), whose overflow
is a typed refusal.

## Converted on this branch

| Name | Was | Now | File | Kind now |
|---|---|---|---|---|
| `swim::gossip_room` | 8 entries, picked | what QUIC's minimum datagram (1,200 B) leaves beside the widest ack | `crates/hex-sim/src/swim.rs` | (c) RFC 9000 §14.1, as hyper-swim's owner sizes it |
| `disk_probe::RECORD_BYTES` | 4,096, a page | 1 byte: the least write that dirties a block | `crates/hex-node/src/disk_probe.rs:22` | the minimum; nothing chosen |
| `probe::INITIAL_TIMEOUT_NS` | 1 s, RFC 6298 §2.1's prior | gone: the first probe is a calibration write whose latency seeds the timer; nothing is judged and the node takes no work until it completes | `crates/hex-health/src/probe.rs`, `crates/hex-node/src/disk_probe.rs` | (a) |
| `probe::MAX_TIMEOUT_NS` | 60 s, RFC 6298 §2.5's "MAY" | gone: no ceiling; a disk steadily slower than 60 s is judged by its own latency | `crates/hex-health/src/probe.rs` | (a) |

## hex-health, the detectors and penalties

| Name | Value | File:line | Kind | Way out |
|---|---|---|---|---|
| `probe::MIN_TIMEOUT_NS` | 1 s | `probe.rs:23` | (d) RFC 6298 §2.4, made for TCP's delayed ACKs | (a): floor at the longest probe the disk has completed without a drain. Replayed on a recorded healthy trace (`bench/health/records/2026-11-06-probe-floor-apfs.md`): the floor removed drains a healthy disk 5 times in 10 minutes; learned, a stall is suspected at 30 ms and drained at 89 ms, 30x faster, with one warm-up false drain. The warm-up's length follows from the tolerated false-drain rate: **waits on Ada's rate.** |
| `probe::DRAIN_TIMEOUTS` | 3 | `probe.rs:28` | (d) RFC 6298 §5.5 doubling | (a): the count `k` with `p^k` under the owner's tolerated false-drain rate, `p` the learned rate of a healthy disk's probes outliving one timeout. |
| `probe::SLOW_STREAK` | 3 | `probe.rs:31` | (d) CPI²'s 3 flags | Same as `DRAIN_TIMEOUTS`. |
| `grade::WINDOW_NS` | 1 s | `crates/hex-health/src/grade.rs:26` | (d) picked to fit Panorama's 7 s | (a): the time a node's requesters take to report `MIN_OBSERVATIONS`, at each node's learned report rate. |
| `grade::MIN_OBSERVATIONS` | 5 | `grade.rs:28` | (d) picked | (b)/(a): the sample size at which a window's median separates a node at the slow bar from its peers at the owner's error rate, from the learned spread of healthy medians. |
| `grade::MIN_PEERS` | 3 | `grade.rs:31` | (c) logic: the fewest nodes in which a majority can single one out | — |
| `grade::SLOW_FACTOR_MILLI` | 2× | `grade.rs:33` | (d) picked | (a): a node is slow when its window is outside the learned distribution of healthy nodes' window medians at the owner's error rate (a robust z-score over the cell's history). |
| `grade::STAY_FACTOR_MILLI` | 1.5× | `grade.rs:35` | derived from the slow bar and the histogram's resolution | Moves with `SLOW_FACTOR_MILLI`. |
| `grade::DRAIN_WINDOWS` | 3 | `grade.rs:37` | (d) CPI²'s 3 flags | As `DRAIN_TIMEOUTS`, with `p` the learned false-slow rate per window. |
| `histogram` 4 sub-buckets per power of two | 25% resolution | `crates/hex-health/src/histogram.rs:8` | (d) picked | Derived from what must be told apart (the slow bar against the healthy spread). Memory only: Ada's rule says take the finer resolution if a test shows it helps detection. |
| `penalty::BASE_EJECTION_NS` | 30 s | `crates/hex-health/src/penalty.rs:20` | (d) Envoy | (a): the cell's learned time from a node's drain to its healthy return. |
| `penalty::MAX_EJECTION_NS` | 300 s | `penalty.rs:22` | (d) Envoy | (a): as above, its learned high quantile. |
| `penalty::MAX_EJECTION_PER_MILLE` | 10% | `penalty.rs:25` | (d) Envoy | (a): eject only as many nodes as the cell's measured free capacity can re-place the work of (the fast path's own `admitted_anywhere`). |
| `penalty::EVICTION_MILLI_PER_S` | 0.1 nodes/s | `penalty.rs:27` | (d) Kubernetes | (a): the scheduler's measured re-placement throughput in the cell. |
| `penalty::SECONDARY_EVICTION_MILLI_PER_S` | 0.01 nodes/s | `penalty.rs:29` | (d) Kubernetes | (a): as above, scaled by the share of the cell still healthy. |
| `penalty::UNHEALTHY_PER_MILLE` | 55% | `penalty.rs:31` | (d) Kubernetes | (a): the share at which the healthy nodes' free capacity can no longer take the rest's work. |
| `penalty::UNHEALTHY_MIN_NODES` | 3 | `penalty.rs:32` | (d) Kubernetes | Falls away with the capacity rule. |
| `penalty::LARGE_CELL_NODES` | 50 | `penalty.rs:34` | (d) Kubernetes | Falls away with the capacity rule. |
| `penalty::BACKOFF_BASE_NS` | 10 s | `penalty.rs:37` | (d) Kubernetes CrashLoopBackOff | (a): the workload's own measured start time. |
| `penalty::BACKOFF_CAP_NS` | 300 s | `penalty.rs:38` | (d) Kubernetes | (a): from the workload's learned run lengths. |
| `penalty::BACKOFF_RESET_NS` | 600 s | `penalty.rs:39` | (d) Kubernetes | (a): a run longer than its learned crash interval's high quantile forgives. |
| `retry::RETRY_PER_MILLE` | 10% | `crates/hex-health/src/retry.rs:13` | derived from the requirement: growth ≤ 1.1× means retries ≤ 10% | Requirement. |
| `retry::MAX_ATTEMPTS` | 3 | `retry.rs:11` | (d) the SRE book | (a): the attempts after which a retry's learned success rate is below the budget's worth. |
| `retry::BUCKET_NS`, `BUCKETS` | 10 s × 12 | `retry.rs:15-16` | (d) the SRE book's 2 minutes | (a): the window over which the learned request rate gives enough requests to hold 10%. |
| `antagonist::MIN_JOB_TASKS`, `MIN_TASK_SAMPLES` | 5, 100 | `crates/hex-health/src/antagonist.rs:25-26` | (d) CPI² | (b)/(a): the samples at which the learned spec's 2σ point is stable to the owner's error. |
| `antagonist::MIN_CPU_MILLI` | 0.25 CPU | `antagonist.rs:28` | (d) CPI² | (a): below the CPU at which a task's CPI is learned to be noise. |
| `antagonist::SIGMAS` | 2 | `antagonist.rs:30` | (d) CPI² | (a): the outlier point at the owner's false-flag rate, from the learned spec. |
| `antagonist::VICTIM_FLAGS`, `FLAG_WINDOW_NS` | 3 in 5 min | `antagonist.rs:32-33` | (d) CPI² | As `DRAIN_TIMEOUTS`. |
| `antagonist::CORRELATION_WINDOW_NS` | 10 min | `antagonist.rs:35` | (d) CPI² | (a): the window that holds the learned samples the correlation needs. |
| `antagonist::MIN_CORRELATION_MILLI` | 0.35 | `antagonist.rs:37` | (d) CPI² | (a): above the learned null distribution of bystanders' correlations. |
| `antagonist::IDENTIFY_GAP_NS` | 1 s | `antagonist.rs:39` | (d) CPI² | Bound on the identifications' CPU; (b) from their measured cost. |
| `antagonist::BEST_EFFORT_CAP_MILLI`, `CAP_MILLI` | 0.01, 0.1 CPU | `antagonist.rs:42-43` | (d) CPI² | (a): the cap that brings the victims' CPI back under their spec, learned per throttle. |
| `antagonist::THROTTLE_NS` | 5 min | `antagonist.rs:44` | (d) CPI² | (a): until the victims' CPI is back under spec. |
| `antagonist::HISTORY` | 64 samples | `antagonist.rs:47` | bound | Derived from the correlation window at the sampling rate. |
| `agent::REPEAT_LIMIT` | 3 | `crates/hex-health/src/agent.rs:21` | (d) picked | (b): kiln's recorded agent traces (A4); none recorded yet. **Can't convert without them.** |
| `agent::MAX_DISTINCT_CALLS` | 256 | `agent.rs:24` | bound, picked | (b): the distinct calls between progress events in kiln's traces. **Same.** |
| `agent` "half its calls failing", "a majority of callers" | ½ | `agent.rs:227,230` | (c) logic for the majority; (d) for "half" | (a): above the learned per-node error rate. |

## hex-sim's models and nemesis

| Name | Value | File:line | Kind | Way out |
|---|---|---|---|---|
| memberlist's LAN config (probe 1 s, timeout 500 ms, 3 indirect, mult 4 and 6, awareness 8) | | `crates/hex-sim/src/membership.rs:112-117` | the competitor's shipped configuration | The model is memberlist; it must run memberlist's numbers. Not Hex's tuning. |
| Lifeguard's nack point | 80% | `membership.rs:118` | the competitor's (Lifeguard §IV) | Same. |
| LAN one-way delay | 0.1-2 ms | `membership.rs:119-120`, `crates/hex-sim/src/swim.rs:81-82` | (d) picked | (b), not yet: measured adacachy → fedora (`bench/health/records/2026-11-06-lan-rtt.md`) with both hosts at load 57-70, so the tail is host delay, which the models keep as their own fault. Wants two wired hosts at rest, or a cell's own hyper-swim's learned round trips, recorded. |
| `swim` crash time, run end | 60 s, 1 h | `swim.rs:84-85` | test bounds | The run ends at the condemnation; the end is a step budget. |
| `max_events`, `max_steps`, `max_relayed`, `MAX_MEMBERS`, `MAX_SUSPICION_MULT` | 2²², 2³², 64, 2,048, 64 | `membership.rs:63,67,123-125`, `swim.rs:86-88` | bounds | Typed refusals past them; sized to the runs. |
| `swim` paces: budget 0, and `(2(2m−1)+1)` s | | `crates/hex-sim/tests/swim.rs` | 0 is hyper-swim's "at need"; the other matches memberlist's one probe a second | Requirement (a like-for-like load). |
| nemesis draws: cell sizes, onsets, slowdown 2.5-8×, noise 0.7-1.5×, CPI ×2-3, CPU ×10, loop and session counts, provider shares, cells of 4-32 nodes and their loads | | `crates/hex-sim/src/faults.rs:79-688` | (d) test-only | (b) where data exists: slowdowns and noise from Panorama's and IASO's published distributions; the kinds' loads from G2's recorded workload (`hex-sim.md` H10). The fault classes' definitions (2.5×, doubles) are the design's. |
| `kind_overheads` | OCI 0.42 MiB, Firecracker 5 MiB, Shards 5 MiB | `faults.rs:442-449` | (b) OCI measured; (d) Firecracker's spec figure; Shards a stand-in | (b): Shards measured on fedora/KVM (2.26-4.77 MiB by size) has landed for the sched agent; take it at the next re-merge. Firecracker the same way. |

## Tests and the drill

| Name | Value | File | Kind |
|---|---|---|---|
| seeds per class | 1,000 | `crates/hex-sim/tests/faults.rs:24` | Requirement (the plan's N). |
| gray: suspect ≤ 2 windows, drain ≤ 4, < 7 s | | `faults.rs` tests | Derived from the window; 7 s is Panorama's, the competitor bar. |
| antagonist ≤ 3 samples + 1 s | 181 s | `faults.rs` tests | Derived from CPI²'s sampling; moves with it. |
| crash p99 ≤ 6 periods | ln(100)+1 | `tests/membership.rs` | Derived (F1). |
| Lifeguard ≤ 2% of SWIM | | `tests/membership.rs` | The competitor's own claim. |
| drill: 8 units, 10 s deadline, 100 ms probe period, 60 s stall, 10 s warm-up | | `bench/health/dm_delay_drill.sh:57` | (d) test scenario. The deadline is the work's requirement in the scenario; the period should be derived from it (the period at which drain fits the deadline). |
