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
  mainline Firecracker passes no device through. Shards' VMM does give a guest a GPU, shared through
  Venus or, on Linux, a whole PCI function by VFIO (AW[58][59]), but shards-api, the one seam Hex reaches
  it through, names no device to ask for (AW[60]);
- it has the room;
- it isn't a benchmark beside a running build, or a build beside a running benchmark, whatever their
  kinds;
- it isn't a session on a node whose CPU in use is above 90%.

The equivalence class (H8) is need, role and kind (`Demand`). Each filter's refusals of nodes that had the room are counted
(`Report::refused`). `spec::check_filters` replays the log against the cluster's facts alone and counts
every placement that broke a filter, apart from the index's own verdicts.

**Sources.** `scheduler-architecture.md` S3 (hard filters on facts, the declare-and-assign contract, GPU
access a property of the node and runtime class); `AC-T7.6.3`'s 90%; a GPU is a fact of the node (AW[21]);
mainline Firecracker passes no device through (AW[25]-[28]); Shards' device model, read in its source
at `a0cced87` (AW[58]-[60]): Venus virtio-gpu and VFIO in its VMM, no device in shards-api's `GuestSpec`. Where each kind runs: `bench/sched/huawei_workload.md`.

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
Firecracker release that passes a GPU through, or a shards-api that carries Shards' VFIO or Venus device
to Hex, would make the GPU rule refuse work that could be placed; `Class::takes_gpus` changes with it,
and the node then says which kernels and devices it has.

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
  Shards by guest size, 2.26 to 4.77 MiB, as measured under KVM (`bench/sched/shards_overhead.md`), one
  trace row per size (`replay`'s `overhead,<class>,<bytes>,<for_mem_bytes>`).

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
- Shards' overhead measured idle: a busy guest, or one with Shards' network or share process beside it,
  costs its host more.
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
- On the fleet's mix with no completions the least cut is 53.0% against best fit by CPU and 52.7% against
  `MostAllocated`. Everywhere else on the fleet Hex strands nothing, against the best fits' 36-647
  per-mille.
- Every arm reaches 90% on every seed. Under completions, some launches end on the way on every seed.
- Hex passes over no more launches than either best fit in any set: 2,556 across the seeds on the fleet's
  mix with no completions, against 6,679 and 11,322, and none anywhere else. It passes over none below
  half load and none for room.
- The bar doesn't hang on the chosen speeds. Of the 179 ladder rows at which every arm reaches 90% on every
  seed, Hex meets it on every seed of 177. In the other two, one seed has `MostAllocated` stranding
  nothing either, so no cut is possible.
- As context, on the fleet as it is (10 hosts), Hex meets the bar at each set's clocks on every seed
  where a best fit strands something. Along the ladder, Firecracker alone at 500x misses on one seed:
  Firecracker has two hosts there, and which of them holds the last few hundred millicores decides
  whether 91 or 30 GiB of memory is stranded (`packing.md`).

The bench measures the arms not taken beside it (`packing.md`), and every fragment-aware arm meets the bar.
- Least stranded then most allocated is Hex's tie without the kinds. On m5d.metal, where every node runs
  every kind, it is Hex's score. On the fleet it strands a little less on the mix with no completions (a
  least cut of 53.5%), but it passes over 11,195 launches there, 950 of them below half load: Firecracker
  microVMs wait for Linux room that the other kinds took.
- Least stranded then Tetris's alignment (the plan's) passes over 2,397 on the fleet's mix with no
  completions. Its cuts fall as low as 66.4% on m5d.metal and 31.7% on the fleet (Firecracker alone,
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

**Open.** Hex's own launch records replace the given kinds once Hex runs microVMs for its users, and a
shards-api that carries a device would let Shards microVMs ask for GPUs (H9).

### H20. A modelled WAN, and hyper-swim's own coordinates learned over it

**The choice.** `wan` models a WAN of eight regions whose round trips are Azure's published P50s
between them (the larger of each pair's two directions), with three zones a region. In the WAN of
record (`Access::Bound`) every pair of zones is at Azure's stated bound, about 2 ms, which is the
worst case Azure allows. Nodes have no access height and probes no jitter, because nothing measured
states either. So the record's claim covers inter-region and inter-zone structure only, and two nodes
of one zone are 0 apart. `Access::Modelled` is used only by a labelled sensitivity run, never the
record: zone terms drawn below the bound, heights of 25-250 µs and probes stretched up to 10%, each
modelled.

Every node runs hyper-swim's Vivaldi engine (`vivaldi`) on hyper-swim's own schedule: one probe a
protocol period, and each round a fresh shuffled permutation of the other members, so every member is
probed once a round (its detector's `RandomizedOrder`, SWIM §4.3). Rounds run with scale-free patience
(the lead, 2026-11-06), so no round count is chosen. The lowest median relative error over every pair
so far is tracked, and convergence stops once as many rounds have passed without a new low as it took
to reach that low. The coordinates the study places with are the last round's, as a running cluster's
are. Choosing the lowest round would read the true RTTs, which no node knows, and would make proximity
look better than production can be. The stop rule itself measures error against the truth; that is
the simulation's convergence clock only, since a running cluster never stops converging.

Bounds follow the input-bound rule, the largest value in the census times a stated margin of 2:
- `MAX_NODES` = 100: the fleet trace's 50 node rows.
- `MAX_ROUNDS` = 12,700: the longest convergence of the record and its sensitivity runs, 6,350 rounds.
Doc-truth tests tie the census to the traces.

The engine is hyper-swim's `coordinates.rs`, its tests with it, vendored through `scripts/vendor.py` at
hyper-raft `6a236ae4c73e` (`vendor/MANIFEST.toml` `[projects.hyper-raft]`, `take` naming only the files
hex-sim compiles). It is included unchanged by path (`crates/hex-sim/src/lib.rs`, `#[path]`), so
`vendor.py check`, one of the gates, proves the bytes are upstream's; `vendor/hyper-raft/UPSTREAM.md`
records no changes.

Why not the alternatives:
- Hex reaches another product only through its `-api` crate (CLAUDE.md §9), and hyper-raft has none.
- A model of the engine would measure the model, not what Hex's nodes will carry.
- A pasted copy has nothing holding it to upstream.
hex-sim's SWIM timing (implementation goal 5) takes its hyper-swim files through the same entry, so
hyper-raft is vendored once.

**What replaces it:** when hyper-raft publishes the engine behind an `-api` crate, hex-sim depends on
that by name and version (patched to kiln's copy in `[patch.crates-io]`, CLAUDE.md Vendoring), and the
`hyper-raft` entry and its `#[path]` include go.

**Sources.**
- Dabek, Cox, Kaashoek and Morris, *Vivaldi*, SIGCOMM 2004 (`PF[1]`: 11% median relative error over
  1,740 Internet hosts).
- Ledlie, Gardner and Seltzer, *Network Coordinates in the Wild*, NSDI 2007 (`PF[2]`: real latencies
  violate the triangle inequality, which Azure's table also does in a few triples).
- hyper-swim (`PF[3][6]`; its detector's probe order at hyper-raft `6a236ae`,
  `crates/hyper-swim/src/detector.rs`, `RandomizedOrder`).
- Das, Gupta and Motivala, *SWIM*, DSN 2002 (§4.3, round-robin probe target selection).
- "Azure network round-trip latency statistics" (learn.microsoft.com/azure/networking/azure-network-latency,
  2026-07-30: monthly P50s of backbone probes taken each minute).
- "What are Azure availability zones?", Inter-zone networking.
- Kiln research `01a1a9977b2273d0860ac510b8aa0a69`.

**Evidence.**
- `wan::a_seed_names_one_set_of_coordinates`: one seed gives the same coordinates bit for bit, and
  another seed others; each round offers `n·(n−1)` probes, and every probe of a pair apart is a sample.
- `wan::coordinates_learn_the_wan`: on both node models' WANs the patience rule stops at twice the
  best round, and the median relative error ends at most Vivaldi's 11%. A node's own region is
  predicted nearer than another's in more trials than chance; `unconverged_coordinates_fail_the_same_measure`
  shows the measure fails coordinates that learned nothing.
- `patience_runs_past_a_noise_uptick`: AllKinds seed 15, whose error rose once by noise (25.4% at
  round 3, then 26.4%), ran past the uptick; the first-uptick rule had stopped it at round 4, and the
  test failed on that rule.
- `the_coordinates_held_are_the_last_rounds`: the study places with the last round's coordinates. It
  failed on a best-round snapshot, which held 25.4% where the last round was 29.2%.
- `true_round_trips_follow_the_model` and `the_census_is_the_traces`.
- hyper-swim's own six tests pass on the vendored file.
- Over the record's 118 studies (fedora, commit `272b220`, unit
  `hex-sched-g3-record-20261106T204635Z`, virtual time), the final median error is at least 3.1%, 6.0% at the median and at
  most 29.2% (seed 15, 6 rounds). `bench/sched/proximity.md` lists each seed's rounds and final error.

**What would falsify it.** hyper-swim's coordinates measured on a real WAN (the open gap `PF[6]`)
predicting worse than they do here. Then the model is too kind, and hex-sim's proximity numbers (H21)
are an upper bound until they are refit to the measurement. Inside a zone, where the record claims
nothing, access heights and jitter are what a measurement would add.

### H21. Proximity is measured against blind placement, on the recorded day as it is

**The choice.** `proximity` runs G2's recorded day (H10: Huawei Cloud's Region 5, day 30) as it is, in
virtual time, on each of its node models:
- `huawei-r5-day30.trace`'s ten m5d.metal hosts, each running all three kinds.
- `huawei-r5-day30.fleet.trace`'s Hex fleet hosts, some running only some kinds.
Every one of its 25,295 launches runs at its recorded arrival, with:
- its owner;
- its kind (an OCI container, a Firecracker microVM or a Shards microVM, by G2's rule in the trace: a
  kind per owner, in thirds of the launches);
- its need, with its kind's overhead as the trace states it;
- its recorded lifetime as its compute.
A launch is placed only on a node that runs its kind. Each launch talks to a home node, drawn
uniformly (the record claims nothing about where homes sit; skewed homes are a labelled sensitivity
run). Its round trips are log-normal through TraceLab's p50 25 and p99 1,438 tool calls a session, and
their waits, each the WAN's true RTT from its node to its home, are added to its compute. So the
RTT-to-compute ratio is the data's.

Three policies choose from the same sampled window (`FitIndex::sampled_fit`):
- Blind best fit.
- Proximity: the least RTT hyper-swim's coordinates predict, best fit breaking ties. It is a soft
  score that never removes a candidate, and a node is predicted 0 from itself.
- An oracle on the true RTT.
Goodput is the launches finished by the day's last arrival, per second. The tail is the p99 of arrival
to end. Both are reported over every launch and for each kind.

The seeds carry a stated claim: at least 95% of seeds show the gain, at 95% confidence. If a fraction
`f` of seeds would not, `n` seeds all showing it happens with chance `(1−f)^n`, so
`n = ⌈ln 0.05 / ln 0.95⌉ = 59` seeds a node model, 118 studies. `MAX_WORKLOADS` follows the census
rule: 25,295 launches × 2.

**Sources.**
- Scheduler design S1 (goodput, not balance) and S4 (proximity a soft score, its falsifier).
- H10, for the kinds, their overheads, the recorded workload and the two node models.
- `agentic-workloads.md` §1 (TraceLab's tool calls a session, AW[5]; tool calls 30-85% of request
  latency, AW[34]).
- `proximity-and-failure-containment.md` §1.5 (Hyperscale's `rtt_ucb`, PF[33]).
- Kiln research `01a1a9977b2273d0860ac510b8aa0a69`.

**Evidence.** `bench/sched/proximity.md` (the run of record: fedora, commit `272b220`, unit
`hex-sched-g3-record-20261106T204635Z`, virtual time; adas-mac-studio-2.local gives the same tables
digit for digit). Proximity cuts the completion time's p99 and p50, and mean RTT,
against blind on all 118 studies, over all launches and for each kind:

| | AllKinds | Mixed |
|---|---|---|
| p99, all launches | at least 5.1% | at least 5.8% |
| p99, OCI | at least 61.2% | at least 27.6% |
| p99, Firecracker | at least 2.5% | at least 2.1% |
| p99, Shards | at least 0.004% | at least 2.3% |
| p50, all launches | at least 93.8% | at least 91.5% |

Goodput does not move. At the day's own load nothing queues (blind's queue never holds more than one
launch), so every launch finishes within the day under either policy, and goodput is equal on every
study. This replaces the earlier reading of a goodput gain on every study, which came from loads that
were picked, not recorded. Proximity is within 0.01% of the oracle's goodput.

Tests:
- `proximity_beats_blind` asserts what holds on every seed (p99 and p50 lower, goodput no lower). The
  mutant `no-proximity-term` fails it, and that mutant is its non-vacuity, with no share asserted.
- `proximity_and_the_oracle_place_nearer_than_blind`.
- `jobs_and_nodes_are_the_recorded_ones` and `kinds_are_g2s_by_owner_in_thirds` (the trace's 8,432,
  8,432 and 8,431).
- `a_kind_no_node_runs_is_refused_not_misplaced`.
- `a_node_is_predicted_zero_from_itself`.
- `hostile_recorded_workloads_are_refused`, `every_policy_runs_every_job_deterministically` and
  `blind_never_moves_from_itself`.

**Why it is this way: four measured reasons.**
- **The prediction is not widened (`K_SIGMA = 0`).** Hyperscale's bound, `rtt × (1 + K_SIGMA·mean
  error)` with its `K_SIGMA = 2`, was measured against the plain prediction on 120 studies of recorded
  nodes and launches (commit `5920b50`, adas-mac-studio.local-2, virtual time).
  - The least gains over blind were identical: all launches goodput +3.1% and p99 −9.3%; OCI +3.0% and
    −6.8%; Firecracker +3.1% and −6.5%; Shards +2.7% with −4.0% (`K = 2`) against −3.9% (`K = 0`).
  - The bound's only edges were the tie-break nearer than blind on 116 against 112 studies, and the
    oracle gap at 0.88% against 0.91%.
  - A copied multiplier that buys no least gain is not taken (Ada, 2026-11-06: "We do not support
    arbitrary constants"; the lead's ruling).
- **No tie-break policy.** Proximity only among best fit's exact ties was measured against blind on the
  same 59 seeds a fleet (commit `4cadcb3`, adas-mac-studio-2.local, virtual time). It worsened the
  all-launch p99 on both fleets:
  - AllKinds: worse on 49, better on 4 (mean −3.2%).
  - Mixed: worse on 42, better on 16 (mean −1.2%).
  - Only Shards on Mixed improved (better on 39, +0.8%), and that fleet's tail was still worse.
  - It is not the coordinates' error: scored by the true RTT it was worse still (AllKinds worse on 49,
    Mixed on 41).
  - It is path dependence. It changes about 13 decisions a day on the identical m5d hosts (185 on the
    fleet), yet it shifts mean RTT across every placement, because best fit packs later launches onto
    whichever node those few choices filled. It was dropped (the lead's rule).
- **A node is predicted 0 from itself.** Vivaldi's estimate of a node to itself is `2·height`.
  - On AllKinds seed 15 that put node 6 (Brazil South, alone in its region) 246 ms from itself and
    195 ms from node 4 (Southeast Asia; true RTT 331 ms).
  - So proximity never placed a launch on its own home there. All 3,166 launches homed on node 6 went
    to Southeast Asia, and Shards' p99 came out 1.6% worse than blind's: the one per-kind miss of
    117/118.
  - A launch on its own home has a true RTT of 0, so the study predicts 0. With that, the miss is gone
    (118/118).
- **Patience, with the last round's coordinates.** The first-uptick stop halted seed 15 at round 4 on
  noise. Patience runs on, and the study places with the last round's coordinates (29.2% on seed 15),
  not the lowest round's (25.4%), which only the truth could pick (H20). 118/118 holds with them.

**What would falsify it.**
- Recorded microVM sessions whose round trips don't go to one place.
- A day whose load queues while proximity's tail cut vanishes. At the recorded load nothing queues, so
  the record says nothing about goodput under contention.
- A real WAN on which hyper-swim's error is far above this WAN's (H20).
- Shards' measured overhead, once it replaces the stand-in in the recorded workload, changing which
  nodes fit a Shards microVM enough to cut its gain. The run is repeated then.
Bandwidth, unmodelled here, is A2's open gap.

### H5. Bounded everywhere, no recursion

**The choice.** The store holds at most the run's workloads and three log entries each; the event queue
`max_events`; the change window `change_window`; the run `max_steps`; clients and node agents a counted
number of attempts. Past each, a typed `SimError`. The committer's pass visits each pending workload at
most once; handlers schedule events, never call each other's handlers in a cycle.

**Sources.** CLAUDE.md §2 and §11.

**Evidence.** `bounds_refuse_typed`: each bound, exceeded, returns its error.

## What it doesn't do yet

Each goes with the implementation goal that needs it: the nemesis's faults other than crashes (gray, fail-slow, mass
loss, the agent-specific faults; goal 5), inter-node latency inside the control-plane loop `sim` (H20-H21
measure proximity in a study of their own), provider
quotas (A3), priorities and fair share (goal 4), sharded control planes, cells and regions (goals 6-7),
heap accounting per workload (`AC-T7.11.4`; `Store::record_bytes` is the record's layout only), and
wall-clock throughput and tail of the real fast path (goal 2). The virtual costs in `Config::default`
are chosen for tests, not measured; goal 2 sets them from the measured fast path.

Under saturation each completion sends the committer back to the front of the queue and every pending
workload ahead of a placement is decided again, as the specification itself does; the equivalence
classes of S2 (goal 2) are what make that cheap, so hex-sim's own runs are sized below saturation
until then.
