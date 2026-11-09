# G5's constants, audited against Ada's rule

Ada, 2026-11-06: "We do not support arbitrary constants. We support learned and data derived values."
Latency and throughput come before memory: a memory cut that costs either isn't taken.

Every constant `cp/t7-sched-g5` adds or relies on, by kind:

- **(a)** learned at runtime from what the system sees;
- **(b)** derived from measured data, with its record;
- **(c)** a platform or spec limit, cited at its source;
- **(d)** anything else: picked, or another system's default. A published number counts as (d): citing
  it isn't deriving it.

Each (d) has its way out: how it becomes (a)-(c), or why it can't yet. There are no tolerated
false-alarm rates (Ada: "Why any of these? We don't allow arbitrary values. Learned, data-derived, and
adaptive."): each threshold that decides a verdict (drain counts, the slow bar, CPI²'s 2σ and 0.35)
becomes a cost rule, acting when the expected loss of waiting passes that of acting, with learned
inputs: P(at fault | observed) from the learned healthy distribution and a learned prior, a false
action priced at the measured re-placement, waiting at the work blocked. The disk probe is the first. "Requirement" marks a number
that is the bar being met, not a tuning; "bound" marks a resource bound (CLAUDE.md §2), whose overflow
is a typed refusal.

## Converted on this branch

| Name | Was | Now | File | Kind now |
|---|---|---|---|---|
| `swim::gossip_room` | 8 entries, picked | what QUIC's minimum datagram (1,200 B) leaves beside the widest ack | `crates/hex-sim/src/swim.rs` | (c) RFC 9000 §14.1, as hyper-swim's owner sizes it |
| `disk_probe::RECORD_BYTES` | 4,096, a page | 1 byte: the least write that dirties a block | `crates/hex-node/src/disk_probe.rs:22` | the minimum; nothing chosen |
| `probe::INITIAL_TIMEOUT_NS` | 1 s, RFC 6298 §2.1's prior | gone: before any answer the probe judges by its host class's recorded healthy answers | `crates/hex-node/src/disk_probe.rs` `recorded` | (b) macOS: the APFS record's third run, 23,764 answers by the probe's buckets, `bench/health/records/2026-11-06-probe-floor-apfs.md`; Linux and Windows: recorded first, refused until then; the drill judges by its own disk's recorded baseline |
| `probe::MAX_TIMEOUT_NS` | 60 s, RFC 6298 §2.5's "MAY" | gone: no ceiling; a disk steadily slower than 60 s is judged by its own latency | `crates/hex-health/src/probe.rs` | (a) |
| `probe::MIN_TIMEOUT_NS`, `DRAIN_TIMEOUTS`, `SLOW_STREAK`, RFC 6298's timer; then the learned-wait rule's `BOUND_FACTOR` | 1 s, 3, 3; then 2 | gone: Suspect past the slowest healthy answer; Drain at the age where waiting costs more than acting, `(1 - pi) S(T) C + pi T` least, `S` the healthy answers' tail (Jeffreys half-count past the slowest), `pi` the Jeffreys posterior of a stall per probe, `C` the caller's measured re-placement time | `crates/hex-health/src/probe.rs` | (a)/(b), Ada's cost rule (2026-11-06). Replayed in Rust on APFS run 2 against run 3's record (`crates/hex-health/examples/replay_probe.rs`): at 2 ms a workload, 2 false drains, a stall drained at 67.1 ms; at 194 ms, 0 false drains, drained at 335.5 ms; the exact form (Python) 69.5 and 275.2 ms, the difference the histogram's 25%; the 1 s floor drained at 3,000 ms. A disk that keeps answering ever more slowly raises its own ages: fail-slow is the peer grade's (F2). |
| `penalty::MAX_EJECTION_PER_MILLE` | 10%, Envoy `max_ejection_percent` | gone: a node is ejected only when the rest of the cell re-places its every workload at that moment (`admitted_anywhere`, then the fast path, on a copy of the cell with the node fenced); else `Refusal::NoRoom` and it stays in placement, `Suspect`. The purpose kept: a correlated fault drains no cell | `crates/hex-health/src/penalty.rs`, `crates/hex-sim/src/faults.rs` `correlated` | (a): measured on the cell each time. 1,000 seeds, loaded cells of G2's node models, a third to two fifths slow at once: 9,211 ejected, 30,627 refused, 14,356 workloads re-placed, 0 stranded (up to 41.5% of a cell out at once where it had room); the check off strands 67,244 workloads on 843 seeds |

## hex-health, the detectors and penalties

| Name | Value | File:line | Kind | Way out |
|---|---|---|---|---|
| `grade::WINDOW_NS` | 1 s | `crates/hex-health/src/grade.rs:26` | (d) picked to fit Panorama's 7 s | (a): the time a node's requesters take to report `MIN_OBSERVATIONS`, at each node's learned report rate. |
| `grade::MIN_OBSERVATIONS` | 5 | `grade.rs:28` | (d) picked | (b)/(a): the sample size at which a window's median separates a node at the slow bar from its peers at the owner's error rate, from the learned spread of healthy medians. |
| `grade::MIN_PEERS` | 3 | `grade.rs:31` | (c) logic: the fewest nodes in which a majority can single one out | — |
| `grade::SLOW_FACTOR_MILLI` | 2× | `grade.rs:33` | (d) picked | **Stays (d), to the lead.** The cost form was tried (`bench/health/records/2026-11-06-grade-cost-rule.md`): with no recorded gray incidents its Jeffreys prior makes waiting near free, and no gray node drained within 4 windows at either price (today's rule: all within 4, no wrong verdict), with 122 false drains at 2 ms. It wants the fleet's record of gray incidents (how often, how slow); none exists. |
| `grade::STAY_FACTOR_MILLI` | 1.5× | `grade.rs:35` | derived from the slow bar and the histogram's resolution | Moves with `SLOW_FACTOR_MILLI`. |
| `grade::DRAIN_WINDOWS` | 3 | `grade.rs:37` | (d) CPI²'s 3 flags | **Stays (d), to the lead**, with `SLOW_FACTOR_MILLI`: the cost form chooses the bar and the run length together, and needs the same record. |
| `histogram` 4 sub-buckets per power of two | 25% resolution | `crates/hex-health/src/histogram.rs:8` | (d) picked | Derived from what must be told apart (the slow bar against the healthy spread). Memory only: Ada's rule says take the finer resolution if a test shows it helps detection. |
| `penalty::BASE_EJECTION_NS` | 30 s | `crates/hex-health/src/penalty.rs:20` | (d) Envoy | (a): the cell's learned time from a node's drain to its healthy return. |
| `penalty::MAX_EJECTION_NS` | 300 s | `penalty.rs:22` | (d) Envoy | (a): as above, its learned high quantile. |
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
| `antagonist::SIGMAS` | 2 | `antagonist.rs:30` | (d) CPI² | **Stays (d), to the lead.** The cost form needs a CPI record (healthy tasks' CPI by job, and antagonist incidents) and Hex records no CPI yet: no counters are read on any host class. It is the grade's case with less data. |
| `antagonist::VICTIM_FLAGS`, `FLAG_WINDOW_NS` | 3 in 5 min | `antagonist.rs:32-33` | (d) CPI² | **Stays (d)**, with `SIGMAS`. |
| `antagonist::CORRELATION_WINDOW_NS` | 10 min | `antagonist.rs:35` | (d) CPI² | (a): the window that holds the learned samples the correlation needs. |
| `antagonist::MIN_CORRELATION_MILLI` | 0.35 | `antagonist.rs:37` | (d) CPI² | **Stays (d)**, with `SIGMAS`: its null distribution is the bystanders' correlations in a CPI record. |
| `antagonist::IDENTIFY_GAP_NS` | 1 s | `antagonist.rs:39` | (d) CPI² | Bound on the identifications' CPU; (b) from their measured cost. |
| `antagonist::BEST_EFFORT_CAP_MILLI`, `CAP_MILLI` | 0.01, 0.1 CPU | `antagonist.rs:42-43` | (d) CPI² | (a): the cap that brings the victims' CPI back under their spec, learned per throttle. |
| `antagonist::THROTTLE_NS` | 5 min | `antagonist.rs:44` | (d) CPI² | (a): until the victims' CPI is back under spec. |
| `antagonist::HISTORY` | 64 samples | `antagonist.rs:47` | bound | Derived from the correlation window at the sampling rate. |
| `agent::REPEAT_LIMIT` | 3 | `crates/hex-health/src/agent.rs:21` | (d) picked | **Stays (d).** kiln keeps every session's tool calls, and `conversations_search` returns them, but each with its arguments: the rule counts *identical* calls (same tool, same arguments) between progress events, and telling identical calls apart needs the arguments, which are content, ruled out (the lead, 2026-11-06). Tool names alone can't: consecutive calls of one tool are mostly different calls. A source that gives each call's argument hash without its content (kiln could compute it) would make it (b). |
| `agent::MAX_DISTINCT_CALLS` | 256 | `agent.rs:24` | bound, picked | **Stays (d)**, for the same reason: distinct calls are counted by their arguments. |
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
