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
one crash per run (injected = 2 × writes seen), and more than half of the crashed histories differ from
the clean one (the crash had effect). With node agents told of a placement only when its writer survives (the feed
moved behind the crash), the sweep fails at write 3, `After`: one workload lost (same unit, mutant
`feed-after-crash`).

**What would falsify it.** A crash point whose run loses or doubles a workload: a defect in recovery,
fixed at its cause (CLAUDE.md §4). This sweep is one crash per run on one cluster; `AC-T7.3.3`'s 10,000
crash seeds, with the nemesis's other faults, are implementation goal 1.

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

### H5. Bounded everywhere, no recursion

**The choice.** The store holds at most the run's workloads and three log entries each; the event queue
`max_events`; the change window `change_window`; the run `max_steps`; clients and node agents a counted
number of attempts. Past each, a typed `SimError`. The committer's pass visits each pending workload at
most once; handlers schedule events, never call each other's handlers in a cycle.

**Sources.** CLAUDE.md §2 and §11.

**Evidence.** `bounds_refuse_typed`: each bound, exceeded, returns its error.

## What it doesn't do yet

Each goes with the implementation goal that needs it: the nemesis's other faults (gray, fail-slow, mass
loss, the agent-specific faults; goal 5), inter-node latency and Vivaldi coordinates (goal 3), provider
quotas (A3), priorities and fair share (goal 4), sharded control planes, cells and regions (goals 6-7),
heap accounting per workload (`AC-T7.11.4`; `Store::record_bytes` is the record's layout only), and
wall-clock throughput and tail of the real fast path (goal 2). The virtual costs in `Config::default`
are chosen for tests, not measured; goal 2 sets them from the measured fast path.

Under saturation each completion sends the committer back to the front of the queue and every pending
workload ahead of a placement is decided again, as the specification itself does; the equivalence
classes of S2 (goal 2) are what make that cheap, so hex-sim's own runs are sized below saturation
until then.
