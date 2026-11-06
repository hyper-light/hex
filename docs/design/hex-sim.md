# hex-sim: the deterministic simulated cluster

hex-sim is the measurement vehicle `scheduler-architecture.md` builds first (its "hex-sim" section and
implementation goal 0): a simulated cluster of fake nodes in which the scheduler's correctness bars
(`AC-T7.3.3`, `T-T7.4.1`) are checked and its other bars will be measured. This records its decisions,
each with its sources, evidence and falsifier (CLAUDE.md §5). Sources are
`docs/research/scheduler/deterministic-simulation.md`, cited `DS[n]`.

## What it is now

`crates/hex-sim`, a pure core (CLAUDE.md §9) with no I/O, no threads and no clock:

- `model`, `generate`: nodes (CPU, memory, GPUs and the kinds they run) and workloads (need, arrival, run
  time, owner, and kind: an OCI container, a Firecracker microVM or a Shards microVM), generated from a
  seed; every workload's kind and need are drawn against one node, so it fits somewhere, and GPU work is
  drawn only against GPU nodes, as OCI containers (H9).
- `index`: best fit as one total order over (free CPU, free memory, free GPUs, node id), so the
  scheduler and its specification can't disagree on a tie. Best fit is the baseline S1 scores against; a
  sample ranks the nodes it admits by a `Scorer`, and Hex places by `Scorer::HEX` (H11).
- `store`: the control plane's durable records, changed only by compare-and-set (S6): a placement names
  the record version it read and the node's capacity is checked in the same write, so a repeated or stale
  commit is a conflict, never a second start or an overcommit.
- `sim`: the event loop. Clients submit and resubmit under one id; the control plane admits, decides on
  `workers` parallel deciders, and commits through one in-order committer that late-binds each decision
  (S2); node agents start what the store's feed shows them and report it done until recorded; the
  nemesis crashes the control plane at a chosen write.
- `spec`: the serial specification (one scheduler, one cycle) as a log oracle, a direct serial run, and
  the end-state count of workloads lost or started twice.
- `replay`, `packing`: a recorded workload's text, read bounded and typed (H10), and `AC-T7.4.3`'s
  measure, a workload packed to a load and what it strands counted (H11).

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

### H9. Hard filters: kind, GPU, benchmark against build, and the 90% CPU rule

**The choice.** A workload declares its kind, an OCI container, a Firecracker microVM or a Shards microVM
(`Class`), and its role: plain, a build (it changes its host), a benchmark (interference-sensitive) or an
agent session (`Kind`). A node carries the kinds it runs and its background CPU load (co-tenants and the
host), beside what Hex placed. A node admits a workload only when:
- it runs the workload's kind;
- the workload needs no GPU, or the node has GPUs and the workload's kind can be given one: only an OCI
  container can (`Class::takes_gpus`), so a Firecracker or Shards microVM that asks for one is admitted
  nowhere and refused at admission. The lead approved this on 2026-11-06, until Shards says otherwise:
  mainline Firecracker passes no device through, and Shards' API names none;
- it has the room;
- it isn't a benchmark beside a running build, or a build beside a running benchmark, whatever their
  kinds;
- it isn't a session on a node whose CPU in use is above 90%.

The equivalence class (H8) is need, role and kind (`Demand`). Each filter's refusals of nodes that had the room are counted
(`Report::refused`). `spec::check_filters` replays the log against the cluster's facts alone and counts
every placement that broke a filter, apart from the index's own verdicts.

**Sources.** `scheduler-architecture.md` S3 (hard filters on facts, the declare-and-assign contract, GPU
access a property of the node and runtime class); `AC-T7.6.3`'s 90%; a GPU is a fact of the node (AW[21]);
mainline Firecracker passes no device through (AW[25]-[28]); Shards' guest spec names disks, shares and
vsock ports, and no device (shards-api draft, hyper-light/shards `a0cced87`,
`crates/api/src/lib.rs:41-62`). Where each kind runs: `bench/sched/huawei_workload.md`.

**Evidence.** `the_hard_filters_keep_every_placement_in_bounds`: 500 generated clusters of Linux, Windows
and macOS nodes, each running the kinds its platform can, with GPU nodes, background load, every role and
every kind, and one microVM in fifty asking for a GPU. Workloads on a node that can't run their kind, GPU
work placed where it gets none, benchmarks beside builds and hot sessions are all 0; every microVM that
asked for a GPU was refused at admission; every filter refused nodes that had room; and each kind placed
over 100 benchmarks and over 100 sessions. `a_class_the_node_cant_run_is_refused` and
`only_an_oci_container_is_given_a_gpu` check the two kind filters alone. The 1,000-cluster,
10,000-crash-seed and 10,000-node tests check the same on every run. Mutants that switch off the hot, the
benchmark/build or the kind filter, or that let a microVM be given a GPU, fail the test.

**What would falsify it.** A run whose replay counts a violation. A session refused everywhere while
some node is under 90% with room would also falsify it, and `check_log`'s `LeftPending` refuses that. A
Firecracker release, or a Shards API, that passes a GPU through would make the GPU rule refuse work that
could be placed; `Class::takes_gpus` changes with it.

### H10. The recorded workload is a day of serverless sandbox launches, each one of Hex's three kinds

**The choice.** Hex's scheduler places microVMs (Ada, 2026-11-06: "Your scheduler is for *microvms*."), of
three kinds: OCI containers, Firecracker microVMs and Shards microVMs ("we work with OCI containers,
Firecracker, and Shards Microvms", the same day). T7's plan, version 4, measures `AC-T7.4.3` "by hex-sim
with a recorded workload of OCI containers, Firecracker microVMs and Shards microVMs". That workload is a
day of sandbox launches from a production cloud: Huawei Cloud's public serverless traces 2025, Region 5, day
30 (`bench/sched/huawei-r5-day30.trace`, and `huawei-r5-day30.keepalive.trace` with Huawei's one-minute
keep-alive added), converted by `bench/sched/huawei_trace.py` from raw files it checks by sha256.
- Each cold start is one request: its arrival, its lifetime (to the end of its last request, plus the
  keep-alive in the second trace), its resource pool (millicores and MiB) and its owner (`userID`). The
  sandboxes are Huawei's pods, which may be containers, taken as requests by shape.
- Each request's kind is its owner's, the owners split so that each kind has a third of the day's launches.
- Two node models (the lead, 2026-11-06). The main one is 10 EC2 m5d.metal hosts (48 cores, 384 GiB),
  Linux x86_64 with KVM, each running all three kinds. Beside it, to exercise where each kind may land,
  are Hex's own fleet's hosts, five of each: Linux runs all three kinds, x86_64 Windows OCI containers and
  Shards microVMs, and arm64 macOS Shards microVMs alone.
- Each kind's runtime overhead is added to its requests' memory: OCI 0.42 MiB, Firecracker 5 MiB, and
  Shards 5 MiB as a stand-in.

The trace's text, with G4's owner fields and the kinds, is parsed by `replay::parse`, bounded and typed
(`Limits`, `TraceError`). kiln's own CI, recorded on 2026-11-05 (`bench/sched/kiln_workload.md`), is kept as
a side measurement, not of record.

**Sources.**
- Joosen et al., "Serverless Cold Starts and Where to Find Them", EuroSys 2025, and its data at
  github.com/sir-lab/data-release (CC BY 4.0); its §2 for the one-minute keep-alive.
- The m5d.metal host: Agache et al., NSDI 2020 §6, the host the Firecracker paper measured on.
- Where each kind runs: Hex's oci class (Linux, and Windows over HCS), Firecracker (Linux with KVM), and
  Shards' API draft (`a0cced87`, `crates/api/src/lib.rs:172-178`).
- Overheads: `bench/competitive/RESULT.md:95` for OCI; Firecracker's specification and Agache et al., NSDI
  2020 (`docs/research/runtime/substrates-and-sandboxes.md` §3.3).
- A kind per owner: a pod names its runtime class in its spec (Kubernetes, "Runtime Class").
- `bench/sched/huawei_workload.md` gives the sources, the mapping of each field, the kind rule, the hosts,
  the counts, and why this trace over the Azure Functions traces.

**Evidence.**
- `a_recorded_workload_replays_the_same_twice`: 25,295 launches of 339 owners, all four traces (each node
  model, with and without the keep-alive). The same hash, report and log come twice from one seed; every
  launch is done once; `check_log` and `check_filters` are clean. The kind filter refuses nodes on the
  fleet, and none on m5d.metal, where every node runs every kind.
- `huawei_trace.py` run again on the recorded raw files gives all four traces byte for byte.
- The parser reads the kinds and overheads (`classes_are_read_and_default_to_every_class_and_oci`).
  `malformed_traces_are_refused_by_line`, `traces_past_their_bounds_are_refused` and its proptests refuse
  malformed, oversized and out-of-order text with a typed error, and none panics.

**What would falsify it.**
- Hex's own microVM launches packing differently from Huawei's sandboxes: larger, longer, or with devices,
  none of which this day has.
- The kinds are given, not recorded. A real split far from thirds, or by function rather than owner, would
  change the mix's measure; each kind alone doesn't depend on it.
- Shards' own overhead differing much from the 5 MiB stand-in.
- The pools' limits are taken as requests; a platform that packs by usage would see smaller ones.
- A second region or day that packs differently would bound how far this one stands for microVM work.

### H11. Hex packs by least stranded leftover, then the node running the fewest kinds, then most allocated

**The choice.** `Scorer::HEX`, which is `Scorer::Fragment(Tie::Narrowest)`. Among the nodes a sample admits:
1. the one whose leftover after placing the request still fits some request shape of a kind the node runs
   (or is empty), so no CPU or memory is left that nothing can use;
2. between those, the node that runs the fewest kinds, so a node only some kinds can use is spent before
   one every kind can;
3. then the node most allocated once the request is placed (Kubernetes' `MostAllocated`), so what isn't
   stranded packs;
4. then the node's key.

`AC-T7.4.3` is measured by `packing::pack` on the recorded workload (H10), on both node models, to 90%
load, the mix of kinds and each kind alone (`packing::alone`: the whole day as that kind, each need moved
to its overhead).
- Load is the dominant share placed (the larger of CPU's and memory's) over the nodes the requests may use.
- Stranded capacity is the free CPU and memory on nodes where no request shape of a kind the node runs
  fits, each as a share of those nodes', summed (`PackReport::stranded_per_mille`). That is Borg's
  stranding, with "fully allocated" read against each kind's Pareto-minimal shapes.
- The seeds are 16 starting arrivals spread evenly over the day, each wrapping round it.
- Three clocks. With no completions (`Clock::Held`), lifetimes don't enter. With completions
  (`Clock::Compressed`), once with lifetimes as recorded and once with the keep-alive. At the recorded
  rate the day never loads the nodes to 90%, so arrivals come faster: for each node model and set, the
  slowest speed on `sched_packing`'s ladder (10x to 2,000x) at which Hex and both best fits reach 90% on
  every seed, so lifetimes enter as far as the nodes allow.
- Best fit alone is measured two ways: by free CPU first (`Scorer::BestFit`) and across resources
  (`Scorer::MostAllocated`, Kubernetes' bin packing).

The lead approved four of these on 2026-11-06, each for its reason:
1. **The fewest-kinds tie** keeps eligibility from stranding: a node only some kinds can use is spent on
   them first, leaving the nodes every kind can use for the kinds that need them. With most allocated as
   the tie, the fleet's Shards microVMs spread over Linux and Windows, and 11,195 launches of the mix with
   no completions found no node. Every early refusal the bench lists for it is a Firecracker microVM,
   which only Linux runs.
2. **GPUs only for OCI containers**, until Shards says otherwise (H9).
3. **The completions ladder.** At one fixed speed, 2,000x, the one-minute keep-alive outlived the whole
   fill, so lifetimes never entered. The slowest speed at which every arm reaches 90% on every seed lets
   them enter as far as the nodes allow.
4. **The wrap-around.** Seeds that started late in the day ran out of arrivals before 90%; that was the
   earlier model's shortfall at 500x. Each seed now draws the whole day, its wrapped arrivals a day's
   span later.

**Sources.** Borg's stranding, resources "that cannot be used because another resource on the machine is
fully allocated", and its hybrid of best fit and worst fit, which packs "about 3-5% better" than best fit
(`docs/research/scheduler/cluster-managers-at-scale.md`, [1] §3.2). Dominant share (DRF,
`placement-algorithms-and-fairness.md` [18]). Kubernetes' `LeastAllocated` and `MostAllocated` (same note,
[22]). Tetris's alignment, measured beside it ([19]).

**Evidence.** `the_recorded_workload_strands_at_least_30_percent_less_than_best_fit` and
`bench/sched/packing.md`. On every one of the 16 seeds of the mix and of each kind alone, on both node
models, under all three clocks, Hex's scorer strands at least 30% less than both best fits at 90% load.
- On m5d.metal, Hex strands nothing on any seed, against the best fits' 87-804 per-mille.
- On the fleet's mix with no completions the least cut is 52.9% against best fit by CPU and 52.6% against
  `MostAllocated`. Everywhere else on the fleet Hex strands nothing, against the best fits' 36-646
  per-mille.
- Every arm reaches 90% on every seed. Under completions, some launches end on the way on every seed.
- Hex passes over no more launches than either best fit in any set: 2,556 across the seeds on the fleet's
  mix with no completions, against 6,679 and 11,322, and none anywhere else. It passes over none below
  half load and none for room.
- The bar doesn't hang on the chosen speeds. Of the 179 ladder rows at which every arm reaches 90% on every
  seed, Hex meets it on every seed of 176. In the other three, one seed has `MostAllocated` stranding
  nothing either, so no cut is possible.
- As context, on the fleet as it is (10 hosts), Hex meets the bar at each set's clocks on every seed
  where a best fit strands something. Along the ladder, Firecracker alone at 500x misses on one seed:
  Firecracker has two hosts there, and which of them holds the last few hundred millicores decides
  whether 91 or 30 GiB of memory is stranded (`packing.md`).

The bench measures the arms not taken beside it (`packing.md`), and every fragment-aware arm meets the bar.
- Least stranded then most allocated is Hex's tie without the kinds. On m5d.metal, where every node runs
  every kind, it is Hex's score. On the fleet it strands a little less on the mix with no completions (a
  least cut of 54.1%), but it passes over 11,195 launches there, 950 of them below half load: Firecracker
  microVMs wait for Linux room that the other kinds took.
- Least stranded then Tetris's alignment (the plan's) passes over 2,397 on the fleet's mix with no
  completions. Its cuts fall as low as 66.8% on m5d.metal and 31.7% on the fleet (Firecracker alone,
  lifetimes as recorded), and it passes over launches in every set and clock on m5d.metal, and in all but
  one on the fleet (Shards alone, lifetimes as recorded).

On kiln's CI trace, a side measurement since Ada's correction, every node runs every kind, so Hex's tie
there is most allocated too. That arm met the bar on 8 of 16 seeds of kiln's fourteen days, with no
completions (`packing.md`, the record at `ad3053c`); least stranded then least allocated met it on all 16.

**What would falsify it.**
- A recorded microVM workload on which Hex's scorer strands no less than best fit, or passes over more
  arrivals on the way to the same load.
- The narrowest-node tie spends first the nodes few kinds can use. A workload whose kinds skew against the
  fleet's could exhaust those nodes and refuse the next launches of their one kind.
- Where every node runs every kind, the tie can't tell nodes apart and is most allocated. That strands
  nothing on m5d.metal, but met the bar on only half of kiln's CI seeds.

**Open.** Shards' own overhead and device model replace the stand-ins when Shards states them, and Hex's
own launch records replace the given kinds once Hex runs microVMs for its users.

### H5. Bounded everywhere, no recursion

**The choice.** The store holds at most the run's workloads and three log entries each; the event queue
`max_events`; the change window `change_window`; the run `max_steps`; clients and node agents a counted
number of attempts. Past each, a typed `SimError`. The committer's pass visits each pending workload at
most once; handlers schedule events, never call each other's handlers in a cycle.

**Sources.** CLAUDE.md §2 and §11.

**Evidence.** `bounds_refuse_typed`: each bound, exceeded, returns its error.

### H60. The control plane as shards: a Plane per node slice, one code path

Implementation goal 6 (CP-T7.11). Sources for H60-H63 are cited by path and line in
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
another slice could hold them, above 1% of the 1,000,000 at H63's bench. That would make the partition's
lost goodput the cost Hydra's falsifier names (`placement-algorithms-and-fairness.md:1335-1336`), and
routing would then take free capacity into account.

### H61. Shard loss and lease takeover: detection plus rebuild within 5 s

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

### H62. Control-plane memory: a counting allocator over the store and the planes

**The choice.** `AC-T7.11.4`'s bytes per workload are measured, not computed from layouts
(`Store::record_bytes` is the layout alone). The test binary `crates/hex-sim/tests/memory.rs`
registers a counting global allocator over `System` that keeps live bytes (allocated minus freed). It
runs 1,000,000 workloads on 10,000 nodes to H63's end state. It then drops the run's store and planes
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

### H63. The 1,000,000-placement bench and the kwok comparison

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

### H64. Cells and regions: a federation over independent cells, one code path

Implementation goal 7 (CP-T7.12, S7). `CR[n]` refers to `docs/research/scheduler/cells-and-regions.md`.

**The choice.** A topology is `regions` regions of `cells` cells each. A cell is today's hex-sim run
(`sim::run`) over its own nodes, with its own store and its own shards (H60, H61): nothing in one cell
reads or writes another's store, plane or nodes. The nodes are split into regions, and each region's
into cells, as `Layout` splits slices: contiguous ranges of node ids. A pure core,
`federation::Federation` (sans-IO, CLAUDE.md §9), routes each workload once, when it arrives:
- to its home region, `id mod regions`, a fixed per-region decomposition of capacity, as Twine's
  Federation does instead of a global allocator making ad hoc choices (CR[2] §7.3 p. 14,
  `cells-and-regions.md:31-38`);
- in that region, to the first cell, from `(id / regions) mod cells` on, whose empty nodes admit it
  (H60's rule one level up; the quotient, so that with as many cells as regions, or any shared factor,
  a region's work still spreads over all its cells). None does: it tries the other regions in order; none does: it's refused, typed.

Each cell's workloads keep their arrival order and are numbered densely within it, as `Layout` numbers
a slice's nodes. With one region of one cell the federation hands the cluster to `sim::run` unchanged,
so one cell through the federation is today's hex-sim, bit for bit (CLAUDE.md §8): one node is that
with a one-node cluster, and three regions add only the routing.

**Sources.** Cells as independent failure domains, the reason for bounding a cell, not its scale:
Borg CR[1] §4 p. 6 (`cells-and-regions.md:18-23`). A regional plane widens the blast radius, which
Twine bounds by shards that each manage a small fraction (CR[2] §4 p. 9, `cells-and-regions.md:24-30`).
Explicit per-region capacity over regional planes: CR[2] §7.3 (`cells-and-regions.md:31-38`). Karmada
spreads across clusters by topology groups (CR[7] `:446-461`, `cells-and-regions.md:62-63`).

**Evidence.** H60's routing is already held by `tests/shards.rs`: a pure function of the spec and the
cluster, which needs no shared state, so the federation's is too. The one-code-path claim is a test
(H67): one cell through the federation and `sim::run` give the same report and trace hash.

**What would falsify it.** One cell through the federation differing from `sim::run` on any seed. Or
any write, read or decision of one cell's touching another's nodes or store.

### H65. A cell's control plane lost: no other cell's workload disturbed

**The choice.** A cell-loss drill loses every shard of one cell for good (`Nemesis::Lose` for each, with
no live shard left to take over). No other cell's control plane takes over: a cell can't act on
another's nodes (H64). The lost cell's running work keeps running (H7: node agents outlive the plane),
and its pending work waits; the drill doesn't re-route it, because a cell's loss is a repair, not a
failover (the region's loss is, H66). `AC-T7.12.2`'s count of other cells' workloads disturbed is
measured two ways: each other cell's run is the same, report and trace hash, as the same run without
the loss; and `spec::check_kept_running` over them counts 0.

**Sources.** Borg's cells are independent "to minimize the chance of correlated operator errors and
failure propagation" and its running tasks outlive every Borgmaster replica (CR[1] §4 p. 6,
`cells-and-regions.md:18-23`). Twine: "even if all Twine components fail, existing tasks continue to
run" (CR[2] §4 p. 9, `cells-and-regions.md:29-30`).

**Evidence.** The drill's test. Its non-vacuity: a mutant that re-routes a lost cell's pending work to
the next cell changes that cell's run, and the test fails.

**What would falsify it.** Any other cell's report or trace differing with the loss, or a running
workload in the lost cell stopped.

### H66. A region lost: its work re-placed within the others' capacity, checked before it moves

**The choice.** A region is lost wholesale at time `at`: its nodes and its cells' planes. The
federation sees it after `region_grace_ns` without a report (default 40 s, cited: Karmada's
`cluster-monitor-grace-period`, CR[3] `:34`; `T-T7.12.1` sets no time bar, so the competitor's default
is the drill's). Then:
- **What moves:** every workload the federation routed to the lost region and not reported ended. A lost
  region is a total loss, as MultiKueue treats a lost worker (CR[11] `:351-356`). The region is fenced by
  an epoch: the federation refuses its reports after the loss, so a region that comes back can't start a
  second copy (H61's rule one level up).
- **Where it goes:** the federation reads each surviving region's per-node free capacity at the moment
  it acts, from that region's own store, not an estimate. It deducts first what is already pending
  there, then each moved workload as it admits it ("assume and deduct", CR[9] `:14`). A moved workload
  goes, in id order, to the first surviving region, from its home on, whose free nodes hold it by best
  fit. None does: it's refused, typed (`Refusal::NoCapacity`), and counted. Nothing moves that doesn't
  fit, so no region is overcommitted.
- Admitted workloads arrive in their new region's cells at `at + region_grace_ns`, as new submissions.

**Sources.** Karmada re-places only on an explicit taint, keeps its spread rules and paces evictions;
its estimator reads a stale snapshot and "may over-commit", which "assume and deduct" closes (CR[4]-[9],
`cells-and-regions.md:39-63`). MultiKueue re-admits against a global quota that may exceed the workers'
(CR[10] `:33-38`, `cells-and-regions.md:64-75`). Neither checks that the lost work fits before moving it
(`cells-and-regions.md:96-100`): `T-T7.12.1` asks for that check.

**Evidence.** `T-T7.12.1`'s test, at two capacities: with room, every moved workload is placed in a
surviving region and none refused; when tight, exactly those for which no surviving node had room are
refused, which a brute-force check confirms. Both: no node over capacity, each surviving cell's log
accepted by the serial specification, and lost-or-double 0 across the federation. Mutant: admitting
without deducting the pending work overcommits, the test fails.

**What would falsify it.** A moved workload refused while a surviving region could hold it, a node
over capacity, or a workload started in two regions.

### H67. The differential suite: one node, one cell, three regions, on one specification

**The choice.** `AC-T7.12.3`'s suite runs each of a set of generated seeds (H1's generator) at three
scales: one node (the seed's first node, all its workloads), one cell (one region of one cell, the seed's
nodes) and three regions (three regions of two cells, the same nodes). Each runs clean and with H6's
random crash nemesis. Every run must hold what the specification fixes, the same at every scale:
lost-or-double 0 (`spec::check_end`), every cell's log accepted by the serial specification
(`spec::check_slices`), the hard filters (`spec::check_filters`), and every workload placed, refused
typed, or waiting where no node of its routing has room. Where a node lands is not compared across
scales, because capacity per domain differs by configuration: Csmith compares "within an equivalence
class but not across classes" (CR[14] §2.6 p. 5, `cells-and-regions.md:76-83`). And one cell through
the federation must equal `sim::run`, report and trace hash, on every seed (H64).

**Sources.** Differential testing needs comparable, deterministic implementations, and a difference,
loop or crash is a candidate bug (McKeeman CR[13] p. 1; Csmith CR[14] §2.1 p. 2,
`cells-and-regions.md:76-83`). No source applies it across scales of one system
(`cells-and-regions.md:114-116`): Hex compares configurations of one implementation against one
specification, the same rule on another axis.

**Evidence and falsifier.** The suite itself; any seed where a scale breaks an invariant, or one cell
differs from `sim::run`, falsifies H64's one code path.

### H68. Cell selection against Karmada's

**The choice.** `crates/hex-bench/benches/sched_cells.rs` times the federation's selection on 100
simulated cells, Karmada's 100 member clusters: each workload's route to a region and a cell with room,
read from each cell's live free capacity and deducted as it goes (H66's check, the comparable step to
Karmada's capacity-weighted `cluster_schedule`). 2,000,000 workloads, Karmada's report's total, each
timed alone. The record gives p50, p90, p99, p99.9 and max, release, fedora CPUs 4-15 under the timed
lock, best of 5, in `bench/sched/cells.md`. The bar: p99 at most a fifth of Karmada's 32 ms.

**Sources.** Karmada's 2022 report: 100 clusters of 5,000 simulated nodes and 20,000 pods each,
`cluster_schedule` P50 12 ms, P90 15 ms, P99 32 ms (`cluster-managers-at-scale.md:239-245`, [41], MV;
its nodes are `fake-kubelet` objects and its authors say it isn't a stress test).

**What would falsify it.** Hex's p99 over 6.4 ms. Karmada's step includes its API objects' handling,
Hex's selection is in-process; the record says so, as `bench/sched/kube_scheduler_kwok.md` does.

### H69. G6 and G7 over Hex's three classes: every workload a class, every node, cell and region declaring its own

**Why.** Hex's scheduler places OCI containers, Firecracker microVMs and Shards microVMs (Ada,
2026-11-06: "Your scheduler is for *microvms*." and "we work with OCI containers, Firecracker, and Shards
Microvms."). G6 and G7 were first measured on workloads of no class; this puts every one of their bars on
the three.

**The choice.**
- **Each workload's class is G2's rule** (`bench/sched/huawei_workload.md:79-82`): all of an owner's
  workloads are one class, and owners are split so each class gets a third of the launches, the owner
  with the most launches first, each to the class with the fewest so far, ties OCI, Firecracker,
  Shards. `generate::by_owner` is that rule in Rust, beside `huawei_trace.py`'s `classes_by_owner`.
  Generated workloads take owners at the recorded day's 75 launches each (25,295 of 339 owners,
  `huawei_workload.md:81-82`), drawn uniformly (`generate::owners`).
- **Each class's overhead is in its workload's memory** (`huawei_workload.md:122-127`): OCI 440,402 bytes
  (0.42 MiB), Firecracker 5 MiB, and Shards 5 MiB, **a stand-in**, until Shards states its own
  (`generate::overheads`).
- **The nodes are G2's two node models** (`huawei_workload.md:92-120`; the lead, 2026-11-06: the main
  model and the fleet beside it), in `generate::NodeModel`:
  - **EC2 m5d.metal, the main one**: 48,000 millicores, 384 GiB, no GPU, Linux with KVM, so every class
    on every node (Agache et al., NSDI 2020 §6). Every G6 and G7 figure is of record on it.
  - **Hex's fleet, beside it**: its ten hosts from kiln's runner facts exactly as G2's fleet model has
    them (`huawei_trace.py:83-94`: adacachy and fedora Linux, jupiter Windows, seven Macs), repeated in
    that order to the figure's node count, as `sched_fast_path` repeats them. Each runs its OS's classes
    (`huawei_workload.md:112-116`): Linux all three, Windows OCI and Shards, macOS Shards alone. So
    Firecracker may use two nodes in ten, OCI three, Shards all ten. Each figure is measured on it too
    where it applies, and says when it doesn't (a million pause pods don't fit the fleet's OCI and
    Firecracker capacity).
- **Each slice, cell and region declares the union of its nodes' classes** (`Layout::classes`,
  `CellRun::classes`), and routing passes over one that doesn't declare a workload's class before it
  asks whether its empty nodes admit it (H60, H64). `Capacity::admit` skips a cell that doesn't run the
  class (H66). The index already refuses a node that doesn't (H9), and the specification counts any such
  placement (`FilterReport::class_off_node`).
- **Cells and regions are cut from those nodes** in id order, as H60 and H64 cut them, so on the fleet
  every cell holds the fleet's mix and a lost region's work of each class moves only to nodes of that
  class elsewhere. Regions are places, each holding a mix of hosts (the lead, 2026-11-06).
  `T-T7.12.1` is of record on the main model, where every node runs every kind; beside it on the fleet,
  a lost region's Firecracker goes to the other regions' Linux hosts within their capacity. The typed
  refusal is its own case (`a_kind_no_surviving_node_runs_is_refused_typed`): three regions each of
  one OS's fleet hosts, so losing the Linux one leaves no surviving node that runs Firecracker, and
  every Firecracker microVM is refused, typed.
- **Plain best fit strands the scarce class; Hex's score doesn't.** With classes, the 1M mixed row on
  one plane under best fit left 4,678 Firecracker microVMs unplaced: OCI and Shards work filled the
  nodes that alone run Firecracker. G2's `Scorer::HEX` (H11: least stranded against each class's
  smallest needs, then the node running the fewest classes) placed all 1,000,000, the spec accepting
  the run. Every G6/G7 bench places by it, and the finding is evidence for H11 recorded in
  `bench/sched/million.md`.
- **kube-scheduler is held to the same constraint** (H63's comparison): each kwok node carries
  `hex.dev/<class>: "true"` for each class it runs, and each pod a required node affinity on its class's
  label, and its memory request carries its class's overhead. That is Kubernetes' own way to keep a pod
  on nodes of its runtime (CR[15] `:111-113`, CR[16] `:144-147`, `cells-and-regions.md:85-92`).
  RuntimeClass would merge the same selector in admission and add the same overhead (CR[16]
  `:162-167`); the bench writes them on the pod so the API server's admission path is the same in both
  configurations.

**Evidence.** Every figure of G6 and G7 is re-measured with classes, per class where it applies, and
recorded beside the generic figure it replaces: `bench/sched/million.md`, `kube_scheduler_kwok.md`,
`takeover_rebuild.md`, `cells.md`, and `tests/cells.rs`, `tests/shards.rs`, `tests/memory.rs`.

**What would falsify it.** A workload placed on a node that doesn't run its class, at any scale. A class
whose work a region or cell is sent but can't run. A moved workload refused while a surviving node of
its class had room. Or Shards' own overhead, once stated, far from 5 MiB: then the overhead changes and
every figure here is measured again.

## What it doesn't do yet

Each goes with the implementation goal that needs it: the nemesis's faults other than crashes (gray, fail-slow, mass
loss, the agent-specific faults; goal 5), inter-node latency and Vivaldi coordinates (goal 3), provider
quotas (A3), priorities and fair share (goal 4), sharded control planes (decided in H60-H63, built by goal 6), cells and regions (decided in H64-H68, built by goal 7; in Hex's three classes, H69),
heap accounting per workload (`AC-T7.11.4`, H62; `Store::record_bytes` is the record's layout only), and
wall-clock throughput and tail of the real fast path (goal 2). The virtual costs in `Config::default`
are chosen for tests, not measured; goal 2 sets them from the measured fast path.

Under saturation each completion sends the committer back to the front of the queue and every pending
workload ahead of a placement is decided again, as the specification itself does; the equivalence
classes of S2 (goal 2) are what make that cheap, so hex-sim's own runs are sized below saturation
until then.
