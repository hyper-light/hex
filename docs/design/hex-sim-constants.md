# G2's constants, audited against Ada's rule

Ada, 2026-11-06: "We do not support arbitrary constants. We support learned and data derived values."
Latency and throughput come before memory: a memory cut that costs either isn't taken.

Every constant `cp/t7-sched` adds or relies on, by kind:

- **(a)** learned at runtime from what the system sees;
- **(b)** derived from measured data, with its record;
- **(c)** a platform or spec limit, cited at its source;
- **(d)** anything else: picked, or another system's default. A published number counts as (d): citing
  it isn't deriving it.

"Requirement" marks a number that is the bar being met, not a tuning. "Rule" marks the lead's two rules
(2026-11-06), which other goals apply too. The **input-bound rule**: a bound on an input is the most any
recorded input holds, times 2. **Hex's rule for learned waits**: a wait that can't end on a fact is bounded
at twice the slowest on record, then twice the slowest of the record's and its own, only rising. Their 2
is the rule's margin. Units and encodings (per-mille, millionths, a kind's index) aren't listed.

## Converted on this branch

| Name | Was | Now | File | Kind now |
|---|---|---|---|---|
| `index::Shapes::MAX`, and what passes it | 16; past it, `Shapes::of` silently kept the smallest, claiming a smaller shape fits wherever a larger one does. That's false for the Pareto-minimal shapes a sample holds (none fits wherever another does), so a dropped shape could make a usable leftover read as stranded: a §3 drop | twice the census's most minimal shapes, 3 (one a kind) = 6; past it, refused, typed | `crates/hex-sim/src/index.rs:69` | (b), input-bound rule, census in `crates/hex-sim/src/census.rs` |
| `model::Overhead::MAX_SIZES` | 64 sizes a class | gone: `replay::Limits::max_overheads`, twice the census's most overhead rows, 16 = 32 | `crates/hex-sim/src/replay.rs:47` | (b), input-bound rule |
| `replay::Limits::default` | 64 MiB, 100,000 nodes, 1,000,000 legs, 100,000 owners, 1,000,000 groups, picked with "room to spare" | `Limits::RECORDED`: twice the census of the committed traces: 3,544,456 bytes, 100 nodes, 50,590 legs, 678 owners, 0 groups, 32 overhead rows | `crates/hex-sim/src/replay.rs:47` | (b), input-bound rule. A doc-truth test reads every committed trace, so a trace that holds more names the figure to raise. No committed trace names a group, so the bound is none; tests with groups pass their own |
| `packing::EARLY_KEPT` and "below half load" | the first 8 arrivals passed over below 50% load kept with their needs, the rest counted | gone: every arrival passed over is counted by its kind, and the load at the first refusal and at the first for room are kept | `crates/hex-sim/src/packing.rs` (`PackReport`) | nothing chosen |
| `sched_packing`'s `LADDER` | 14 speeds, 10x to 2,000x, picked; the slowest of them reaching 90% on every seed | gone: the slowest whole speed at which Hex and both best fits reach 90% on every seed. Doubling from 1x finds one at which they do, then every speed below it is packed, so no assumption that reaching 90% gets easier as the speed rises (it doesn't always: at 124x `MostAllocated` reaches it on 58 of the fleet's mix's seeds, at 125x on 57) | `crates/hex-bench/benches/sched_packing.rs` (`slowest`) | (b), from the packs themselves |
| the packing seeds | 16 starts spread evenly over the day | `packing::SEEDS` = 59 starts drawn uniformly (`packing::start`): the least `n` with `0.95^n < 0.05`, so every seed meeting the bar claims at least 95% of starts do, at 95% confidence | `crates/hex-sim/src/packing.rs:33` | (b), from the claim; the claim's levels are G3's claim rule (`docs/design/hex-sim.md` on cp/t7-sched-g3): Ada's to confirm for `AC-T7.4.3` |
| `sched_fast_path::FILL_PER_MILLE` | 850, "about 85%" | `packing::LOAD_PER_MILLE`, 90%: the load `AC-T7.4.3` holds the scheduler to | `crates/hex-bench/benches/sched_fast_path.rs` | requirement |
| `sched_fast_path`'s launches | the fill estimated from the mean need, plus a quarter to spare | exactly the launches the run takes: the fill counted once on the index alone, which places as the timed run does | `crates/hex-bench/benches/sched_fast_path.rs` (`cluster`) | nothing chosen |
| the fast path's starts | `seed × day / runs` | `packing::start` | `crates/hex-bench/benches/sched_fast_path.rs` | as the packing seeds |
| `Tie::Narrowest`'s 3 | a literal | `Classes::ALL.count()` | `crates/hex-sim/src/index.rs` | structural |
| non-vacuity floors in `tests/sim.rs` | placements > 50,000; sampling changed > 20 of 200 runs; > 15,000 crashes; > 10,000 runs through a crash | the claim each test states: some run placed, sampling changed some run, every seed crashed (kept), some work ran through a crash | `crates/hex-sim/tests/sim.rs:113, :148, :517-518` | the claim's own |
| the class re-check bound's 3 | a literal | the test's own demands, `demands.len()` | `crates/hex-sim/tests/sim.rs:607` | structural |
| `huawei_trace.py`'s `MIN_LIFE_NS` | 1 ms, read as picked | kept: the logs' times are milliseconds (three decimals), so 1 ms is the least nonzero lifetime they can show; 69 sandboxes take it | `bench/sched/huawei_trace.py:102` | (c), the data's resolution |
| `kiln_export.py`'s `MAX_RUNS`, `MAX_PAGES` | 200,000 runs, 2,000 pages | twice the 1,488 runs of the export on record; pages: those runs and one more, since a page goes on only with a fresh run | `bench/sched/kiln_export.py:23-29` | (b), input-bound rule |
| `shards_overhead.py`'s boot deadline | 60 s | twice the slowest boot to ready on record for KVM on fedora, 6,088.2 ms, then twice the slowest seen, only rising | `bench/sched/shards_overhead.py` | (b)/(a), Hex's rule for learned waits |
| its handshake timeout | 5 s a try | the time its boot has left | same | derived |
| its stop | SIGTERM, 10 s, then SIGKILL | SIGKILL, waited for: the kernel ends it | same | (c), signal(7) |
| its reply cap | 64 bytes | the relay's longest answer, `OK 4294967295\n`, 14 bytes | same | (c), the handshake's format |
| Shards' overhead figure | the VMM's own PSS beside 5 idle guests, plus the kernel's per guest: its text shared five ways, so the 5 set the figure | the VMM's own private memory plus the kernel's per guest: what one more guest costs a node. The text is a cost per node, charged nowhere: at most the binary's size once per node running a Shards microVM | `bench/sched/shards_overhead.py`, `huawei_trace.py` | (b), measured; independent of how many guests share |
| Shards' overhead's source | 5 MiB stand-in, then debug-build figures | Shards' release build, as it ships, debug beside it as context | `bench/sched/shards_overhead.md` | (b) |

## Requirements and platform limits, kept

| Name | Value | File:line | Source |
|---|---|---|---|
| `index::HOT_PER_MILLE` | 900 | `crates/hex-sim/src/index.rs:17` | `AC-T7.6.3`'s 90% |
| `spec::over_ninety` | `busy × 10 > capacity × 9` | `crates/hex-sim/src/spec.rs:286` | `AC-T7.6.3`, stated apart in the oracle so it doesn't check the index against itself |
| `packing::LOAD_PER_MILLE` | 900 | `crates/hex-sim/src/packing.rs:28` | `AC-T7.4.3`'s 90% load |
| the 30% bar | `hex × 10 ≤ base × 7` | `crates/hex-sim/tests/sim.rs`, `sched_packing.rs` (`cut`) | `AC-T7.4.3` |
| `sched_fast_path` `NODES`, `RATE_PER_S`, the 10 ms bar | 10,000, 10,000/s, 10 ms | `crates/hex-bench/benches/sched_fast_path.rs:46-47` | `AC-T7.4.2` |
| the quantiles reported | p50, p99, p99.9 | `crates/hex-sim/src/trace.rs:62-64` | p99 is `AC-T7.4.2`'s; p50 to p99.9 and max are the bench bar's |
| the crash seeds | 10,000 | `crates/hex-sim/tests/sim.rs` | `AC-T7.3.3` |
| the 10,000-node determinism run | 10,000 nodes | `crates/hex-sim/tests/sim.rs` | `AC-T7.4.2`'s scale |
| SplitMix64's constants | `0x9E37…`, `0xBF58…`, `0x94D0…` | `crates/hex-sim/src/rng.rs:15-19` | Steele, Lea and Flood, OOPSLA 2014 |
| the keep-alive | 60 s | `bench/sched/huawei_trace.py` | Huawei's own setting, Joosen et al. EuroSys 2025 §2 |
| EC2 m5d.metal | 48,000 millicores, 384 GiB | `bench/sched/huawei_trace.py:88` | Agache et al. NSDI 2020 §6 |
| the fleet's hosts | threads, memory, GPUs, OS | `bench/sched/huawei_trace.py:90-101` | kiln's runner facts, 2026-11-05 (b) |
| the kinds each OS runs | Linux all, Windows OCI and Shards, macOS Shards | `bench/sched/huawei_trace.py:86` | Shards' hosts (`crates/api/src/lib.rs:172-178` at a0cced87); Hex's oci class over HCS |
| OCI's overhead | 440,402 bytes | `bench/sched/huawei_trace.py:80` | (b) Hex's oci class, `bench/competitive/RESULT.md:95` |
| a pool's vCPUs | its CPU in whole vCPUs, rounded up | `bench/sched/shards_overhead.py` | a guest has at least its pool's CPU |
| the boots per run | 100 | `bench/sched/shards_overhead.py` | the least whose nearest-rank p99 isn't the maximum |
| the store's log | three writes a launch | `crates/hex-sim/src/sim.rs:382`, `sched_fast_path.rs` | a submit, a placement and a completion |
| the pins | SHA-256s, commits | `bench/sched/shards_guest.sh`, `huawei_trace.py` | what was measured |
| `-j 4`, CPUs 4-15, the timed lock | | `bench/sched/shards_guest.sh` | fedora's rules for every job (Ada) |

## Still (d), with the way out

| Name | Value | File:line | Why not yet | Way out |
|---|---|---|---|---|
| `Sample::default`'s floors | 100 nodes or 5% | `crates/hex-sim/src/index.rs:149-150` | kube-scheduler's defaults (`schedule_one.go:57,62`). At the packing scale (10 and 50 nodes) they make every scan whole, so `AC-T7.4.3`'s tables don't hang on them; `AC-T7.4.2`'s 10,000-node bench and the property tests do | (b): the smallest sample whose stranding at 10,000 nodes is within the seeds' spread of a whole scan, checked against `AC-T7.4.2`: a whole-scan pack at 10,000 nodes, hours on fedora. Or (a): the shell sizes its sample from its own decision cost against the arrival rate it sees |
| `sim::Config::default` | 4 deciders; decide 50 µs, commit 20 µs, feed 10 µs; restart 5 ms; client and report retries every 10 ms, 16 times; a change window of 4,096; 2^22 events, 2^32 steps | `crates/hex-sim/src/sim.rs:96-107` | Its own comment: "chosen for test runs, not measured". G4 and G6 rewrote `sim.rs` (990 and 938 lines), so changing its defaults here would collide with both re-merges and shift their tests' timing | decide and commit: (b) from the fast path's measured split (a pass beside the timed one); feed: (b) Amphora's watch latency; restart: (b) `Plane::rebuild` timed at the census's size; retries: the rule for learned waits on those; attempts: until a measured restart has passed; change window: (a) the changes a decision can see in flight; events and steps: the input-bound rule over the runs on record. One change, after G4 and G6 land |
| `generate.rs`'s draws | 4-64 cores, 2-8 GiB a core, 1-8 GPUs; 6 in 10 nodes Linux, 2 Windows, 2 macOS; background up to the node's CPU; GPU work half the time on a GPU node; 40% plain, 25% builds, 15% benchmarks, 20% sessions; gaps and lifetimes uniform to twice their mean | `crates/hex-sim/src/generate.rs:40-119` | A property test's input space: chosen for coverage, which the tests' non-vacuity counters show. The recorded day has no GPUs, builds, benchmarks or sessions, which the filters' tests need. Every G3-G6 test that generates a cluster draws from it, so a change moves all their inputs at once | (b) where data exists: nodes from the census of node models, needs, gaps and lifetimes from the recorded day's distributions, the platform split from the fleet's. The kinds and GPUs keep their draws, with the counters as evidence. After the re-merges |
| the property tests' ranges and sizes | 1-24 nodes, 1-160 workloads, deciders 1-8, decide 0-200 µs, and so on; 1,000 and 200 runs | `crates/hex-sim/tests/sim.rs:29-80` | as `generate.rs` | as `generate.rs`; run counts by a stated claim, as the seeds |
| exact-count floors in `tests/sim.rs` | crashes ≥ 2, ≥ 20 running through, ≥ 40 kept; skips > 10× checks; > 100 GPU and filter cases; > 20,000 placements | `crates/hex-sim/tests/sim.rs:548-559, :612, :693-697` | "Several" and "dozens" are the tests' words; the numbers are picks. Pinning the measured counts would fail at G4's and G6's re-merges, whose `sim.rs` changes move them | (b): pin each to its measured value once G4 and G6 have landed, or restate the claim as "some" |
| the node counts | 10 m5d.metal; the fleet five times | `bench/sched/huawei_trace.py` (`--nodes`) | The lead's calls (2026-11-06 09:18Z, 09:46Z). `AC-T7.4.3` names no cluster size and the trace names no hosts; any count trades against the speed, which is now exact for the count | a cluster size `AC-T7.4.3` states |
| the day | day 30 of Region 5 | `bench/sched/huawei_trace.py` (`--day`) | the day G2 took | every day of the release, or a day the requirement names |
| owners to kinds | a third of the launches each | `bench/sched/huawei_trace.py` (`classes_by_owner`) | The trace names no runtime. Approved by the lead | Hex's kinds' recorded mix, once Hex runs a workload |
| `sched_fast_path`'s runs, placements | best of 5; 100,000 a run | `crates/hex-bench/benches/sched_fast_path.rs:228-229` | Best of N is CLAUDE.md §7's; N is picked. The least sample whose nearest-rank p99.9 isn't the maximum is 1,000; 100,000 is that times 100 | the precision the bar's claim states |
| Firecracker's overhead | 5 MiB | `bench/sched/huawei_trace.py:80` | Its specification's bound (and NSDI 2020's "less than 5MB"): a published number | (b): measure it as Shards' was, on fedora under KVM. Shards' x86_64 kernel has `CONFIG_VIRTIO_MMIO` and vsock, so Hex's guest may boot under Firecracker; not yet tried |
| `shards_overhead.py`'s runs, poll, idle | best of 3; READY tried every 1 ms; 3 s idle | `bench/sched/shards_overhead.py` | N picked. The poll sets the reading's resolution: under 1 ms on 99-169 ms. A busy poll would take CPU from the guest's vCPUs in its cpuset, and the host can't learn the guest's listen without asking. The idle wait is the reading's moment | runs: as the fast path's; idle: wait until each guest's resident memory holds across its own boot time |
| `kiln_export.py`'s `TIMEOUT_S` | 60 s | `bench/sched/kiln_export.py` | No response time is recorded. The kiln traces it made carry no G2 bar | the rule for learned waits, from a recorded export |
| the rules' own numbers | the margin 2; the claim's 95% and 95% | `crates/hex-sim/src/census.rs:7`, `packing.rs:33` | the lead's and G3's rules | Ada's |
