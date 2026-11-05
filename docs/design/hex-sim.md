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

### H10. The recorded kiln workload: what kiln measured, read through its API

**The choice.** `AC-T7.4.3`'s recorded kiln workload is fourteen days of kiln's own CI
(`bench/sched/kiln-ci-20261022-20261105.trace`), read from the running kiln's HTTP API by
`bench/sched/kiln_export.py` and turned into the trace by `bench/sched/kiln_trace.py`. A leg's need is
what kiln measured it using (mean cores and peak RSS), capped at the machine it ran on; a leg kiln
didn't measure takes the median of its own job's measured legs. The nodes are the runners kiln has
CPU and memory facts for that ran a leg. The trace's text is parsed by `replay::parse`, bounded and
typed (`Limits`, `TraceError`).

**Sources.** kiln's `GET /api/runs`, `/api/runs/:id` and `/api/agents` on adacachy, exported
2026-11-05T21:39:09Z (`bench/sched/kiln_workload.md`, with the export beside it). kiln's jobs carry no
declared need, so its measurements are the only need on record.

**Evidence.** `a_recorded_kiln_workload_replays_the_same_twice`: 4,655 legs on 10 nodes, the same hash,
report and log twice from one seed, every leg done once, `check_log` and `check_filters` clean.
`kiln_trace.py` run again on the committed export gives the committed trace byte for byte. The parser's
`malformed_traces_are_refused_by_line`, `traces_past_their_bounds_are_refused` and two proptests
(`any_text_is_read_or_refused`, `any_trace_shaped_text_is_read_or_refused`) refuse malformed,
oversized and out-of-order text with a typed error, and none panics.

**What would falsify it.** kiln's measurements standing far from what legs used: 26 legs measured
more CPU than their machine has and are capped, so some measurements are wrong. A sample of legs
re-measured on their runner that differs systematically from kiln's record would mean the trace's needs
aren't kiln's work.

### H11. Hex packs by least stranded leftover, then least allocated

**The choice.** `Scorer::HEX`, which is `Scorer::Fragment(Tie::LeastAllocated)`: among the nodes a sample
admits, the one whose leftover after placing the request still fits some request shape (or is empty), so no
CPU or memory is left that nothing can use; between those, the node least allocated once the request is placed
(Kubernetes' `LeastAllocated`); then the node's key. `AC-T7.4.3` is measured by `packing::pack` on the
recorded kiln workload (H10) as the requests its legs would declare (`packing::whole_requests`: measured use
rounded up to whole cores and GiB, at least one of each), packed in arrival order with no completions to 90%
load, the load being the dominant share placed (the larger of CPU's and memory's). Stranded capacity is then
the free CPU and memory on nodes where no request shape fits, each as a share of the cluster's, summed
(`PackReport::stranded_per_mille`). The seeds are 16 starting arrivals spread evenly over the recording. Best
fit alone is measured two ways: by free CPU first (`Scorer::BestFit`) and across resources
(`Scorer::MostAllocated`, Kubernetes' bin packing).

**Sources.** Borg's stranding, resources "that cannot be used because another resource on the machine is fully
allocated", and its hybrid of best fit and worst fit, which packs "about 3-5% better" than best fit
(`docs/research/scheduler/cluster-managers-at-scale.md`, [1] §3.2). Dominant share (DRF,
`placement-algorithms-and-fairness.md` [18]). Kubernetes' `LeastAllocated` and `MostAllocated` (same note,
[22]). Tetris's alignment, measured beside it ([19]). kiln's declarations: whole cores and GiB, the smallest
`{ cpu: "1", memory: 1Gi }` (kiln `.kiln/ci.yaml:107` at `0fb5b21309e`).

**Evidence.** `the_recorded_kiln_workload_strands_at_least_30_percent_less_than_best_fit` and
`bench/sched/packing.md`: on every one of the 16 seeds, Hex's scorer strands at least 63.2% less than best fit
by CPU and 51.4% less than `MostAllocated` at 90% load. It reaches 90% on every seed, where best fit stalls
short of it on 5. On the way it passes over 152 arrivals across the seeds, against best fit's 8,597 and
`MostAllocated`'s 2,267. The bench measures the arms not taken beside it (`packing.md`, the arms). The plan's
score, least stranded then Tetris's alignment (`Fragment(Tie::Align)`), strands more than a best fit on 4
seeds and meets the bar on 12 of 16. Least stranded then most allocated (`Fragment(Tie::MostAllocated)`) keeps
best fit's empty nodes, refusing nothing below half load, but meets the bar on only 8 of 16. Tetris's
alignment alone meets it on 7.

**What would falsify it.** A recorded workload on which Hex's scorer strands no less than best fit, or passes
over more arrivals on the way to the same load. The known cost: spreading spends the empty nodes best fit
keeps, so arrivals are refused while the cluster is mostly idle. There are 37 below half load across the
seeds, against none under either best fit. 8 are for room, all whole-machine legs of 32 cores, one at 13.8%
load; 29 are by the benchmark-beside-build filter, the largest being kiln's 42-GiB `quic-transport`
benchmarks. In a schedule those legs wait for a node to empty rather than being passed over. A workload whose
large legs matter more than its count of legs would weigh that against Hex's score. Holding room for them, or
preempting for them, is G4's.

### H5. Bounded everywhere, no recursion

**The choice.** The store holds at most the run's workloads and three log entries each; the event queue
`max_events`; the change window `change_window`; the run `max_steps`; clients and node agents a counted
number of attempts. Past each, a typed `SimError`. The committer's pass visits each pending workload at
most once; handlers schedule events, never call each other's handlers in a cycle.

**Sources.** CLAUDE.md §2 and §11.

**Evidence.** `bounds_refuse_typed`: each bound, exceeded, returns its error.

## What it doesn't do yet

Each goes with the implementation goal that needs it: the nemesis's faults other than crashes (gray, fail-slow, mass
loss, the agent-specific faults; goal 5), inter-node latency and Vivaldi coordinates (goal 3), provider
quotas (A3), priorities and fair share (goal 4), sharded control planes, cells and regions (goals 6-7),
heap accounting per workload (`AC-T7.11.4`; `Store::record_bytes` is the record's layout only), and
wall-clock throughput and tail of the real fast path (goal 2). The virtual costs in `Config::default`
are chosen for tests, not measured; goal 2 sets them from the measured fast path.

Under saturation each completion sends the committer back to the front of the queue and every pending
workload ahead of a placement is decided again, as the specification itself does; the equivalence
classes of S2 (goal 2) are what make that cheap, so hex-sim's own runs are sized below saturation
until then.
