# hex-sim: the deterministic simulated cluster

hex-sim is the measurement vehicle `scheduler-architecture.md` builds first (its "hex-sim" section and
implementation goal 0): a simulated cluster of fake nodes in which the scheduler's correctness bars
(`AC-T7.3.3`, `T-T7.4.1`) are checked and its other bars will be measured. This records its decisions,
each with its sources, evidence and falsifier (CLAUDE.md §5). Sources are
`docs/research/scheduler/deterministic-simulation.md`, cited `DS[n]`.

## What it is now

`crates/hex-sim`, a pure core (CLAUDE.md §9) with no I/O, no threads and no clock:

- `model`, `generate`: nodes (CPU, memory, GPUs) and workloads (need, arrival, run time), generated from a
  seed; every workload's need is drawn inside one node's capacity, so GPU work is drawn only against GPU
  nodes.
- `index`: best fit as one total order over (free CPU, free memory, free GPUs, node id), so the
  scheduler and its specification can't disagree on a tie. Best fit is the baseline S1 scores against;
  implementation goal 2 replaces the order with the goodput score.
- `store`: the control plane's durable records, changed only by compare-and-set (S6): a placement names
  the record version it read and the node's capacity is checked in the same write, so a repeated or stale
  commit is a conflict, never a second start or an overcommit.
- `sim`: the event loop. Clients submit and resubmit under one id; the control plane admits, decides on
  `workers` parallel deciders, and commits through one in-order committer that late-binds each decision
  (S2); node agents start what the store's feed shows them and report it done until recorded; the
  nemesis crashes the control plane at a chosen write.
- `spec`: the serial specification (one scheduler, one cycle) as a log oracle, a direct serial run, and
  the end-state count of workloads lost or started twice.

## Decisions

### H1. One seed, virtual time, one thread

**The choice.** A run is a function of its cluster, its configuration and one `u64` seed: the generator
draws from SplitMix64 seeded once; time is virtual nanoseconds advanced only by the event queue; events
at equal times run in the order they were scheduled; maps are B-trees, never hash maps, so iteration
order is fixed. Each run reports an FNV-1a hash of every event it handled, in order, and of every store
write's time.

**Sources.** FoundationDB runs every process of a cluster in one single-threaded discrete-event
simulation with network, disk, time and the PRNG behind it DS[1]; TigerBeetle's VOPR draws every fault
from one PRNG seeded by one number and replays a failure from its seed and commit DS[2][4].

**Evidence.** `ten_thousand_nodes_run_deterministically` runs 10,000 nodes and 100,000 workloads twice
from one seed and gets the same hash, report and store log, and a different hash from another seed.

**What would falsify it.** Two runs of one seed differing in hash on one build: a source of
nondeterminism the loop doesn't own, found and moved behind it.

### H2. The nemesis crashes the control plane at every write, both sides of it

**The choice.** Every store write the control plane makes (submit, placement, completion) is a crash
point, numbered in order. A run names one: `Before` (the write is never applied) or `After` (it is
applied and its writer never hears). A crash loses everything the control plane holds in memory: its
view, queue, proposals and in-flight work, and every event it had scheduled (an epoch voids them). It
restarts after `restart_ns` and rebuilds from the store. The store's feed to node agents and the agents
themselves survive, as Amphora and the hexlets do. The sweep runs a cluster once per write and side.

**Sources.** FoundationDB injects machine failures, among other faults, from inside the simulation DS[1];
TigerBeetle's simulated storage injects faults at the target of a pending write during a crash DS[5];
Jepsen's nemesis is a process scheduled like a client DS[6][7].

**Evidence.** `a_crash_at_every_control_plane_write_loses_and_doubles_nothing`: every one of the clean
run's writes, both sides, leaves a log the specification accepts and zero lost or doubled, with exactly
one crash per run (crashes before writes = writes seen, and crashes after writes = writes seen), and more than half of the crashed histories differ from
the clean one (the crash had effect). With node agents told of a placement only when its writer survives (the feed
moved behind the crash), the sweep fails at write 3, `After`: one workload lost (same unit, mutant
`feed-after-crash`).

**What would falsify it.** A crash point whose run loses or doubles a workload: a defect in recovery,
fixed at its cause (CLAUDE.md §4). This sweep is one crash per run on one cluster; `AC-T7.3.3`'s 10,000
crash seeds, several crashes each, are H6; the nemesis's other faults are implementation goal 5.

### H3. Decide in parallel, commit in order, late-bound

**The choice.** Deciders take pending workloads lowest queue position first and decide against the
current view; one committer takes the queue in order. At commit a decision stands only if no node
changed since it was made that could now come first or that it chose; otherwise it is made again
against the view (a conflict, counted). Decisions older than the last `change_window` changes are made
again (stale, counted). A completion gives capacity back and sends the committer to the front of the
queue again; a commit already sent then is checked again before it is written.

**Sources.** Late binding and sampling (Sparrow PA[1]), optimistic commit on shared state (Omega CM[2],
Gödel's single binder PA[4a]), in `scheduler-architecture.md` S2 and S6.

**Evidence.** Free capacity changes only through recorded changes, so a node unchanged since a decision
keeps its place in the order; the check is exact. `placements_equal_the_serial_specification_on_generated_clusters`
holds it over 1,000 generated clusters with 1-8 deciders and windows of 1-64, with conflicts and stale
decisions both reached (non-vacuity). With the check turned off (every decision kept), the same test fails:
the store refuses a stale placement as overcommitting its node (`UnexpectedConflict`): fedora unit `hex-sched-all-20261104T084349Z` at 3b1345c, mutant `no-late-binding`.

**What would falsify it.** A run whose log the specification refuses. Its cost, not its truth, is what
implementation goal 2 measures: the in-order committer is the serial point the 10,000/s bar tests.

### H4. The serial specification is the oracle, checked on the log

**The choice.** The specification: workloads queue in the order the store accepted them; at every
change the scheduler takes the queue in order and puts each on its best-fit node, passing over one no
node holds. `check_log` replays the store's log and refuses the first write it couldn't have made: a
placement not the best fit, a placement while an earlier workload had a node, a completion off its node,
or work left pending at the end with room. `serial_place` runs it directly when capacity only shrinks,
and the executor's placements equal it workload for workload. `check_end` counts lost (acknowledged, then
neither placed nor waiting, or placed and never recorded done) and started twice.

**Sources.** QuickCheck's state-machine testing checks every step's result against a model state DS[10];
proptest-state-machine runs a reference machine beside the system under test DS[11]; Jepsen hands a
recorded history to a checker DS[6].

**Evidence.** The oracle's own non-vacuity: hand-made histories it must refuse (`NotSerialChoice`,
`JumpedQueue`, `LeftPending`) are refused, and a double start and a lost workload are each counted.

**What would falsify it.** A history the oracle accepts that the specification can't make, shown by a
hand-made case; or a case where concurrency forces a divergence that isn't a defect, which would then be
stated as the specification's allowance.

### H6. AC-T7.3.3's crash seeds: the nemesis's faults are a function of its seed

**The choice.** A crash seed draws a cluster, a control plane (deciders, costs, change window,
restart time) and a nemesis from one generator. The nemesis (`Nemesis::Random`) crashes the plane at a
write the seed picks among the submits, so every run has at least one crash, then at each later write
with a seeded probability (0.5% to 20%), at most a seeded 1 to 8 crashes, each before or after its write
by a fair draw from the nemesis's own generator. A seed is one run; 10,000 seeds are `AC-T7.3.3`'s
10,000.

**Sources.** FoundationDB draws its faults, and randomises its tuning parameters, from the simulation's
seed, and tunes fault rates so they don't shrink the state space DS[1]; the VOPR draws every fault
probability from one generator seeded by one number DS[4].

**Evidence.** `ten_thousand_crash_seeds_lose_and_double_nothing`: 10,000 seeds, every one crashed,
lost or started twice exactly 0, every log one the serial specification makes, and running work
undisturbed (H7).

**What would falsify it.** A seed whose run loses or doubles a workload, or one the specification
refuses: a recovery defect, fixed at its cause and kept as a named regression seed.

### H7. A control-plane restart leaves running work running

**The choice.** Node agents and the store outlive the control plane. A restarted plane rebuilds its
view from the store, counting placed workloads as running on their nodes, never as pending; node agents
keep running what they run and resend each completion until it is recorded. A workload that ran is kept
when it was started once, on the node it was placed on, ran its whole run time there and is recorded done
there (or is still placed there, for one that never ends): `spec::check_kept_running`.

**Sources.** Borg: "already-running tasks continue to run even if the Borgmaster or a task's Borglet goes
down" (Verma et al., EuroSys 2015, p. 6; `docs/research/scheduler/cluster-managers-at-scale.md` [1] §3.3).
The desired/observed split with the store as the record is `scheduler-architecture.md` S6.

**Evidence.** `a_control_plane_restart_leaves_running_workloads_running`: up to 6 crashes over 60
long-running workloads, and again with workloads that never end; dozens running through crashes, none
disturbed. The 10,000 seeds check the same on every run.

**What would falsify it.** A workload running through a crash that is started again, moved, cut short,
or never recorded done. The mutant that rebuilds placed workloads as pending fails both tests.

### H8. The fast path samples per equivalence class and skips a class that holds nowhere

**The choice.** A workload's need is its equivalence class. Its decision scans nodes in id order
from a start that the class and a seed fix (`Sample::start`), stops at the `want`-th node that holds the
need, and takes the best fit among those (`FitIndex::sampled_fit`). `want` is at least `min_feasible` and
`per_mille` of the nodes, by default 100 and 5%: kube-scheduler's floors. Late binding voids a decision
when any node inside its scanned window changed since it was made (a node outside can't move the
scan's stop or any key it saw), and it is made again (a conflict). When a class is found to hold on no
node, it is marked until capacity comes back: the committer passes its other workloads over undecided
(`class_skips`), and deciders don't decide them. So a completion under saturation re-checks each pending
class once (`class_checks`), not each workload. The serial specification chooses with the same sample,
so `T-T7.4.1` tests the fast path itself.

**Sources.** Borg: "Borg only does feasibility and scoring for one task per equivalence class", and
it "examines machines in a random order until it has found 'enough' feasible machines to score" (EuroSys
2015 §3.4); kube-scheduler: `minFeasibleNodesToFind = 100`, `minFeasibleNodesPercentageToFind = 5`
(pkg/scheduler/schedule_one.go:57,62 at v1.37.1); Sparrow's late binding comes "within 5% (4ms) of an
omniscient scheduler" (SOSP 2013 §3, p. 5).

**Evidence.** `placements_equal_the_serial_specification_on_generated_clusters`: 1,000 generated
clusters with samples of 1-8 nodes or up to 30%. `with_capacity_only_shrinking_placements_equal_a_serial_run`
also shows that sampling chose differently from a whole-cluster best fit in some runs.
`a_completion_under_saturation_rechecks_each_class_once`: at most 3 class checks per stretch between
completions over 600 workloads in 3 classes, and the rest of each class passed over undecided.

**What would falsify it.** A run whose log the sampled specification refuses (the late-binding check
missed a change that mattered), or a class checked more than once in a stretch with no completion. The
window check is conservative, so it can count more conflicts than strictly needed. Step 5's bench is
what says whether that costs too much.

### H9. Hard filters: GPU, benchmark against build, and the 90% CPU rule

**The choice.** A workload declares its kind: plain, a build (it changes its host), a benchmark
(interference-sensitive) or an agent session. A node carries its background CPU load (co-tenants
and the host), beside what Hex placed. A node admits a workload only when:
- the workload needs no GPU, or the node has GPUs;
- it has the room;
- it isn't a benchmark beside a running build, or a build beside a running benchmark;
- it isn't a session on a node whose CPU in use is above 90%.

The class is need and kind (`Demand`). Each filter's refusals of nodes that had the room are counted
(`Report::refused`). `spec::check_filters` replays the log against the cluster's facts alone and counts
every placement that broke a filter, apart from the index's own verdicts.

**Sources.** `scheduler-architecture.md` S3 (hard filters on facts, the declare-and-assign contract);
`AC-T7.6.3`'s 90%; a GPU is a fact of the node (AW[21]).

**Evidence.** `the_hard_filters_keep_every_placement_in_bounds`: 500 generated clusters with GPU
nodes, background load and every kind. Hot sessions, GPU work off GPU nodes and benchmarks beside builds
are all 0, and every filter refused nodes that had room. The 1,000-cluster, 10,000-crash-seed and
10,000-node tests check the same on every run. Mutants that switch off the hot or the benchmark/build
filter fail the test.

**What would falsify it.** A run whose replay counts a violation. A session refused everywhere while
some node is under 90% with room would also falsify it, and `check_log`'s `LeftPending` refuses that.

### H5. Bounded everywhere, no recursion

**The choice.** The store holds at most the run's workloads and three log entries each; the event queue
`max_events`; the change window `change_window`; the run `max_steps`; clients and node agents a counted
number of attempts. Past each, a typed `SimError`. The committer's pass visits each pending workload at
most once; handlers schedule events, never call each other's handlers in a cycle.

**Sources.** CLAUDE.md §2 and §11.

**Evidence.** `bounds_refuse_typed`: each bound, exceeded, returns its error.

### H10. The control plane as shards: a Plane per node slice, one code path

Implementation goal 6 (CP-T7.11). Sources for H10-H13 are cited by path and line in
`docs/research/`. `SS[n]` refers to `docs/research/scheduler/scale-and-shards.md`.

**The choice.** The nodes are split into `slices` contiguous ranges of node ids. Each slice is a
`Plane`: today's plane over that slice's nodes, with its own view, queue, deciders and in-order
committer (H3, H8). A workload is routed to a slice by a pure function of its spec and the cluster:
start at slice `id mod slices` and take the first slice, in order, holding a node that would admit the
workload when empty (its capacity and hard filters, H9, not its current use). So routing needs no
shared state and never changes once made. A shard is a control-plane process holding one or more
slices. Every plane commits to the one store by compare-and-set, as now (S6). Slices share no nodes,
so two planes never contend for a node. The serial specification (H4) applies per slice, in the
store's order of that slice's workloads. With `slices = 1`, routing sends every workload to the one
plane, which is today's plane: one code path (CLAUDE.md §8), with no sharded variant beside it.

**Sources.** Gödel's dispatcher "partitions the nodes among shards", each shard checking "its own
node partition first", with one binder detecting conflicts
(`docs/research/scheduler/placement-algorithms-and-fairness.md:288,292`). Twine partitions its scheduler
shards by entitlement, and "a job must fit in one shard" (largest ~170K machines; planned 50K per shard,
20 per million: `docs/research/scheduler/cluster-managers-at-scale.md:479-482`). Shard Manager's
control plane is non-overlapping partitions of thousands of servers, each owned by a mini-SM
(`cluster-managers-at-scale.md:646-647,667-668`). Hydra shards placement into sub-clusters of 2-3
thousand nodes to pass one RM's ~1k decisions/s (`placement-algorithms-and-fairness.md:1315-1316`).

**Evidence.** The per-slice plane is the plane H1-H9 already hold, by the same tests. At 1,000,000
workloads on 10,000 nodes, eight slices give 1,250 nodes and 125,000 workloads per plane, inside every
measured partition above. Step 2 (`crates/hex-sim/tests/shards.rs`) shows the rest: placements equal the
per-slice serial specification at 1-8 slices; `slices = 1` gives today's hash; and nothing is lost or
doubled under H6's nemesis with sharded planes.

**What would falsify it.** A sharded run whose log the per-slice specification refuses, or one losing or
doubling a workload. Or routing that strands work: a count of workloads pending on their slice while
another slice could hold them, above 1% of the 1,000,000 at H13's bench. That would make the partition's
lost goodput the cost Hydra's falsifier names (`placement-algorithms-and-fairness.md:1335-1336`), and
routing would then take free capacity into account.

### H11. Shard loss and lease takeover: detection plus rebuild within 5 s

**The choice.** A shard holds each of its slices under a lease in the store: a record naming the
holder, an epoch, and the expiry. The shard renews every 100 ms. The lease lapses when `lease_ns`
(1 s) passes without a renewal. A live shard then takes the slice by compare-and-set on the lease
record, bumping its epoch, and rebuilds the slice's plane from the store with `Plane::rebuild` (H7):
placed workloads count as running and pending ones queue again. Every placement write names its slice's
epoch, and the store refuses a write from an older epoch. A lost shard that comes back and writes is
refused (a conflict), never a second start. Node agents and the store outlive the shard (H7): agents keep
running what they run and keep resending completions until recorded. The store holds the one clock that
decides a lapse, as Chubby's master holds the authoritative lease timer. In hex-sim that clock is virtual
time (H1).

The bound, from `T-T7.11.1`'s 5 s: takeover = detection + rebuild. Detection is at most `lease_ns` after
the last renewal, so 1 s. That leaves 4 s for the rebuild of the lost shard's slices.

**Sources.**
- The detection terms are Raft's defaults: etcd heartbeats every 100 ms and starts an election after
  1,000 ms of silence, a timeout "at least 10 times the round-trip time"
  (`docs/research/scheduler/scale-and-shards.md:35-37`, SS[11] `tuning.md:14,16,20`).
- Coarser leases cost minutes, not seconds. Chubby's default session extension is 12 s
  (`scale-and-shards.md:38-39`), and its master elections took 4-30 s (`scale-and-shards.md:26-27`).
  kube-scheduler's leader lease is 15 s, renew 10 s, retry 2 s: the competitor's failover
  (`scale-and-shards.md:33-34`). Borg's takeover is 10 s to a minute, because in-memory state has to
  be reconstructed (`cluster-managers-at-scale.md:277-278`).
- A lease is a time-bounded right; a successor waits out the holder's term; leases depend on clocks of
  bounded drift (Gray and Cheriton, `scale-and-shards.md:14-19`).
- The epoch that fences the old holder is Chubby's client epoch and lock generation number
  (`scale-and-shards.md:20-25`).
- Rebuilding from the durable record while clients ride out the fail-over is Chubby's §2.9 and Borg's
  running tasks (`scale-and-shards.md:28-31`, H7).

**Evidence.** The rebuild path is H7's, held by its tests over 10,000 crash seeds: a rebuilt plane
leaves running work running. The rebuild's cost is not cited but measured, by step 2:
`Plane::rebuild` wall-clock at 125,000 records (one of eight slices at 1,000,000), on fedora CPUs 4-15,
recorded in `bench/`. The same step's test kills a shard at 1,000,000 workloads and checks two things:
its workloads keep running (`spec::check_kept_running`), and a live shard commits on the lost slices
within `lease_ns` plus the measured rebuild, in virtual time.

**What would falsify it.** A measured detection plus rebuild over 5 s at 1,000,000 workloads. Or a
lost shard's write accepted after its slice was taken (a fencing defect). Or a workload lost, doubled
or disturbed by a takeover. The first is fixed by making the rebuild faster or the slices smaller,
never by moving the bar (CLAUDE.md §4).

### H12. Control-plane memory: a counting allocator over the store and the planes

**The choice.** `AC-T7.11.4`'s bytes per workload are measured, not computed from layouts
(`Store::record_bytes` is the layout alone). The test binary `crates/hex-sim/tests/memory.rs`
registers a counting global allocator over `System` that keeps live bytes (allocated minus freed). It
runs 1,000,000 workloads on 10,000 nodes to H13's end state. It then drops the run's store and planes
(`Run.store`, `Run.planes`) and reads the count before and after. The difference, divided by the
workloads, is the bytes per workload, against 4 KiB.

The counter is a crate with a safe API, a dev-dependency of `hex-sim`: `cap`
(`static A: Cap<System> = Cap::new(System, usize::MAX)`, read by `A.allocated()`) or `stats_alloc`
(`&INSTRUMENTED_SYSTEM`, read by a `Region`'s `change()`). Step 3 takes whichever passes clippy clean
on Linux, macOS and windows-msvc. The `unsafe impl GlobalAlloc` stays inside the crate, so Hex's code
gains no `unsafe`: the test carries no `#![allow(unsafe_code)]` and CLAUDE.md §12's list is
unchanged.

The store's history log is counted with it. In production, the store is Amphora's and the log is its
own, so counting both is conservative: a pass with them is a pass without. The counter sees the bytes
requested from the allocator, not the allocator's own per-chunk overhead.

**Sources.** `GlobalAlloc` is "A memory allocator that can be registered as the standard library's
default", and `#[global_allocator]` routes "all default allocation requests to a custom object". The
trait is `unsafe`, and std's own example forwards to `System` (`scale-and-shards.md:40-46`, SS[4]
`global.rs:7,189`, SS[5] `alloc.rs:12-14,21-32` at Rust 1.99.1, Hex's toolchain). `cap` 0.1.2
(https://docs.rs/cap/0.1.2/cap/) and `stats_alloc` 0.1.10
(https://docs.rs/stats_alloc/0.1.10/stats_alloc/) each wrap `System` behind a safe registration and
reader, with the `unsafe impl` inside (`scale-and-shards.md:46-52`, SS[12] `lib.rs:5,15-16,22,138,179`,
SS[13] `lib.rs:14-15,18-20,80-82,233,251`). No competitor publishes control-plane memory per workload.
The nearest are whole-process figures: Twine's 80 GB shard for ~170K machines, Shard Manager's 64 GB
mini-SM at ~38%, and a busy Borgmaster's 50 GiB (`scale-and-shards.md:74-76`,
`cluster-managers-at-scale.md:272,479-480,646-647`).

**Evidence.** Step 3 measures it and records the number. The counter's own non-vacuity: allocating a
known buffer moves it by exactly that buffer's bytes, and dropping the run gives back what building it
took.

**What would falsify it.** A count that misses heap the planes or store hold: for example, a structure
whose growth with workloads the counter doesn't show when the workload count doubles. Or the process's
resident memory growing by more than 4 KiB per workload while the count stays under. That would mean the
allocator's overhead matters, and the bar would be re-measured at resident memory.

### H13. The 1,000,000-placement bench and the kwok comparison

**The choice.** One cluster shape serves both benches, so the comparison is class for class
(CLAUDE.md §7). The pod is `scheduler_perf`'s pause pod: 100m CPU and 500Mi. The node is
`scheduler_perf`'s node sized to hold its own 110-pod limit of that pod: 11 CPU and 55 GiB, 110 pods.
10,000 such nodes have 1,100,000 slots, so 1,000,000 fill them to 91%.

- **`AC-T7.11.2`, 1,000,000 on 10,000** (step 4, `crates/hex-bench/benches/sched_million.rs`,
  record `bench/sched/million.md`):
  - All 1,000,000 workloads are submitted pending before the clock starts, and none ends while it runs.
  - The time runs from the first decision to the 1,000,000th commit, wall-clock, on a release build.
  - It runs at `slices` 1 and 8, on fedora CPUs 4-15 under the timed lock, on a noisy host whose load
    is recorded. Best of 5, as `bench/sched/fast_path.md`.
  - A second row draws needs from hex-sim's generator into the same 1,100,000-slot budget, so a
    one-class best case (H8 decides one class once) never stands alone.
- **`AC-T7.11.3`, against kube-scheduler on kwok** (step 5, `bench/sched/kube_scheduler_kwok.md`):
  - The cluster: 10,000 kwok nodes of the shape above, under the `kwokctl` binary runtime, with
    kube-scheduler v1.37.1.
  - The load: 50,000 pause pods (`scheduler_perf`'s `5000Nodes_50000Pods` count), all created before
    the scheduler starts, so creation doesn't cap it.
  - The scheduler runs twice: at its defaults (QPS 50, burst 100) and raised to QPS 5,000, burst
    10,000.
  - Throughput is pods bound from first bind to last. Best of 3 for each setting.
  - hex-sim places the same 50,000 on the same 10,000 nodes, and the bar is Hex's rate at least 10x
    kube-scheduler's raised rate, the stronger of its two.

**Sources.** The shapes and the method:
- `scheduler_perf`'s node is 4 CPU, 32 GiB, 110 pods, and its pod 100m/500Mi
  (`scale-and-shards.md:61-63`, SS[7] `node-default.yaml:7-10`, SS[8] `pod-default.yaml:11-17`).
- Its floors are 790 and 680 pods/s, each the minimum average of 250 runs (`scale-and-shards.md:63-65`,
  `cluster-managers-at-scale.md:183`). It pins runs to CPUs and repeats them (`scale-and-shards.md:65-67`).

The QPS settings:
- The scheduler's own default is 50/100 (`scale-and-shards.md:53-55`, SS[6] `defaults.go:149-154`).
- YuniKorn's KWOK runs measured a flat 50.5 pods/s, consistent with that cap
  (`cluster-managers-at-scale.md:189-193`). So a default-only comparison would measure the client
  limit, not the scheduler.
- Gödel's benchmark raised QPS and burst to 10,000 on 30,000 KWOK nodes and got ~300 pods/s
  (`placement-algorithms-and-fairness.md:311-316`).

kwok's own claims: 1k nodes and 100k pods "reliably", and 20 creates/s (`deterministic-simulation.md:331-332`, CL). Pre-creating the pods keeps that rate out of the measured window.

**Evidence.** The bars' arithmetic: `AC-T7.11.2`'s 120 s is 8,333/s, or 120 µs per placement on one
plane. The fast path decides and commits single-threaded at p50 50.8-54.2 µs and p99 81-92 µs
(`bench/sched/fast_path.md`), inside 120 µs, but at 85% fill with one workload in flight, not 1,000,000
pending. Steps 4 and 5 record the measured runs. The ratio needs kube-scheduler measured on fedora beside
Hex, which no cited floor replaces (`scale-and-shards.md:77-78`).

**What would falsify it.**
- kube-scheduler at the raised setting within 10x of Hex on the same cluster: the bar is missed and stated so.
- kwok failing to hold 10,000 nodes beyond its claimed 1k: then the same shapes run on `scheduler_perf`
  in-process, and the record says why.
- A best-of-N whose runs spread by more than the margin to the bar: the record then reports every run,
  not the best.

Hex's side commits to an in-process store and kube-scheduler's to an API server and etcd. When Hex's
store is Amphora over its seam, the comparison is run again.

## What it doesn't do yet

Each goes with the implementation goal that needs it: the nemesis's faults other than crashes (gray, fail-slow, mass
loss, the agent-specific faults; goal 5), inter-node latency and Vivaldi coordinates (goal 3), provider
quotas (A3), priorities and fair share (goal 4), sharded control planes (decided in H10-H13, built by goal 6), cells and regions (goal 7),
heap accounting per workload (`AC-T7.11.4`, H12; `Store::record_bytes` is the record's layout only), and
wall-clock throughput and tail of the real fast path (goal 2). The virtual costs in `Config::default`
are chosen for tests, not measured; goal 2 sets them from the measured fast path.

Under saturation each completion sends the committer back to the front of the queue and every pending
workload ahead of a placement is decided again, as the specification itself does; the equivalence
classes of S2 (goal 2) are what make that cheap, so hex-sim's own runs are sized below saturation
until then.
