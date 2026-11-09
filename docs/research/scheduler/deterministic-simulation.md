# Deterministic simulation: one seed, virtual time, a nemesis, and an oracle

Hex's scheduler has to be correct under crashes, partitions and reordering at a scale no lab holds, so
`hex-sim` is a deterministic simulator of the cluster scheduler. It has a seeded PRNG, virtual time, a
single-threaded event loop, a nemesis that injects a crash at every control-plane write, an oracle (a
serial specification) for model-based testing, and a kwok-like cluster of fake nodes. This note is the
evidence base for that design. It covers FoundationDB's deterministic simulation, TigerBeetle's VOPR,
Jepsen's nemesis and Elle's history checking, kwok's fake nodes, stateful model-based testing
(QuickCheck, proptest-state-machine) and the sans-IO pattern. It follows the method in
[`../README.md`](../README.md#method): every claim and number is cited to a source opened and read for
this note (accessed 2026-11-04), every number carries a kind, and a gap is written as a gap.

Kinds: `MI` MEASURED-INDEPENDENT, `MV` MEASURED-VENDOR (the authors or operators measured their own
system), `CL` CLAIMED (no stated method), `DE` DESIGN (a fact from a document, code or specification, not
a speed). `n/p` = not published, or not found in what was read. Citations are `DS[n]` (References at the
end). Repositories are cited at a pinned commit, by `path:line`; PDFs by page of the copy read.

## Summary

1. **Determinism is built in from the start, not bolted on.** FoundationDB wrote its simulator before
   the database, runs every process of a cluster in one single-threaded discrete-event simulation, and
   abstracts every source of nondeterminism (network, disk, time and the PRNG) behind it DS[1] (DE).
   TigerBeetle stubs the clock, network and disk the same way, and replays any failure from its seed and
   Git commit DS[2][4] (DE).
2. **Faults come from the seed, in many kinds, at tuned rates.** FoundationDB injects machine, rack and
   datacenter failures, network faults, disk corruption of unsynchronized writes, and randomized event
   times, at rates tuned so that faults do not shrink the state space. "Buggification" lets the code
   itself inject unusual but legal behavior, and swarm testing varies cluster size, workload, faults and
   tuning parameters on every run DS[1] (DE). The VOPR derives every fault probability (packet loss,
   partitions, clogging, crashes) from one PRNG seeded by one number DS[4] (DE). Its storage injects
   faults at the target of a pending write during a crash DS[5] (DE).
3. **Virtual time is what makes it cheap.** Discrete-event simulation fast-forwards to the next event,
   so bugs that take a long time to play out are found many times faster than in real-world tests DS[1]
   (DE). TigerBeetle claims that "One minute of VOPR time is equivalent to days of real-world testing"
   DS[2] (CL, no method stated).
4. **Correctness is judged by oracles and checkers, not by eye.** FoundationDB's workloads carry
   assertions and invariants and check recoverability by restoring a recoverable environment DS[1]
   (DE). The VOPR checks safety (no split-brain while replicas crash and restart) and then liveness (a
   connected core converges) DS[4], and checks that caught-up replicas' data files are byte-for-byte
   identical DS[2] (DE). Jepsen records a history of every operation and hands it to a checker DS[6]
   (DE); Elle infers isolation anomalies from such a history DS[8] (DE).
5. **The nemesis is a process the generator schedules.** In Jepsen a nemesis "introduces faults into the
   system" and is "scheduled by the generator" like any client DS[6]; its protocol is `setup!`,
   `invoke!` and `teardown!`, and nemeses compose by the operations they handle DS[7] (DE). Jepsen's
   own faults are not deterministic: FoundationDB's paper says such approaches "lack deterministic
   reproducibility" DS[1] (DE).
6. **A serial model is the oracle.** Hughes models the system's state abstractly, computes the model
   state at every step, and checks each call's result against it in a postcondition DS[10] (DE). The
   bugs come from "comparing two independent descriptions" of the behavior DS[10]. On AUTOSAR this found
   "more than 200 problems" DS[10] (MV). proptest-state-machine runs a reference state machine beside the
   system under test, shrinks a failing sequence of transitions, and saves the seed that repeats it
   DS[11] (DE).
7. **Fake nodes make cluster scale cheap.** kwok simulates the lifecycle of fake nodes and pods with no
   kubelet and claims 1k nodes and 100k pods on a laptop and 20 nodes or pods created per second DS[9]
   (CL).
8. **What simulation can't do.** FoundationDB's simulation "is not able to reliably detect performance
   issues" and can't test code outside Flow DS[1] (DE). TigerBeetle says the same of formal methods
   from the other side: unlike model checking, simulation "exercises a specific implementation" DS[3]
   (DE).

## 1. FoundationDB

FoundationDB (FDB) built the simulator first. "before building the database itself, we built a
deterministic database simulation framework" that simulates "disk, process, network, and
request-level failures and recoveries, all within a single physical process" DS[1] p. 2 (DE).

**Single-threaded and deterministic.** "All database code is deterministic; accordingly multithreaded
concurrency is avoided (instead, one database node is deployed per core)" DS[1] p. 8, §4 (DE). The
simulator abstracts "all sources of nondeterminism and communication", "including network, disk, time,
and pseudo random number generator" DS[1] p. 8 (DE). Many servers run in one simulation: "The simulator
process is able to spawn multiple FDB servers that communicate with each other through a simulated
network in a single discrete-event simulation" DS[1] p. 8 (DE). The seam to the real system is thin:
"The production implementation is a simple shim to the relevant system calls" DS[1] p. 8 (DE).

**Reproducibility.** The real code runs "together with randomized synthetic workloads and fault
injection, in a deterministic discrete-event simulation", and "determinism guarantees that every bug
found this way can be reproduced, diagnosed, and fixed" DS[1] p. 8 (DE). Observing a run doesn't change
it: "Adding additional logging, for instance, generally does not affect the deterministic ordering of
events, so an exact reproduction is guaranteed" DS[1] p. 11, §6.2 (DE). A bug found in production was
first reproduced in simulation: "the debugging process was almost always first to improve the
capabilities or the fidelity of the simulation until the issue could be reproduced there" DS[1] p. 11
(CL). Gap: the paper says the PRNG is abstracted, but not how a run's seed is chosen or recorded.

**Test oracles.** "FDB uses a variety of test oracles to detect failures in simulation" DS[1] p. 8 (DE).
Workloads "have assertions built in to verify the contracts and properties of the database" DS[1] p. 9
(DE). Liveness is checked by repair: "returning the modeled hardware environment ... to a state in which
recovery should be possible and verifying that the cluster eventually recovers" DS[1] p. 9 (DE; the
ellipsis drops a parenthetical).

**Fault injection.** "The FDB simulator injects machine, rack, and data-center level fail-stop failures
and reboots, a variety of network faults, partitions, and latency problems, disk behavior (e.g. the
corruption of unsynchronized writes when machines reboot), and randomizes event times" DS[1] p. 9 (DE).
Rates are tuned: "Fault injection distributions are carefully tuned to avoid driving the system into a
small state-space caused by an excessive fault rate" DS[1] p. 9 (DE).

**Buggify.** "FDB itself cooperates with the simulation in making rare states and events more common,
through a high-level fault injection technique informally referred to as “buggification”" DS[1] p. 9
(DE). Its examples: "unnecessarily returning an error from an operation that usually succeeds,
injecting a delay in an operation that is usually fast, choosing an unusual value for a tuning
parameter" DS[1] p. 9 (DE). "Randomization of tuning parameters also ensures that specific performance
tuning values do not accidentally become necessary for correctness" DS[1] p. 9 (DE).

**Swarm testing and coverage.** "Each run uses a random cluster size and configuration, random
workloads, random fault injection parameters, random tuning parameters, and enables and disables a
different random subset of buggification points" DS[1] p. 9 (DE). Conditional coverage macros
(`TEST( buffer.is_full() );`) count "how many distinct simulation runs achieved that condition" DS[1]
p. 9 (DE): a non-vacuity counter.

**Virtual time.** "Discrete-event simulation can run arbitrarily faster than real-time if CPU
utilization within the simulation is low, as the simulator can fast-forward clock to the next event"
DS[1] p. 9 (DE). "Randomized testing is embarrassingly parallel" DS[1] p. 9 (DE).

**Measured outcome.** "CloudKit [59] has deployed FDB for more than 0.5M disk years without a single
data corruption event" DS[1] p. 11 (MV: Apple's own fleet; the paper attributes it to simulation, which
is not a controlled comparison).

**Limits.** "Simulation is not able to reliably detect performance issues, such as an imperfect load
balancing algorithm. It is also unable to test third-party libraries or dependencies, or even
first-party code not implemented in Flow" DS[1] p. 9 (DE). On other methods: Jepsen-style
non-deterministic fault injection approaches "lack deterministic reproducibility", and model checking
"can only verify the correctness of a model rather than of the actual implementation" DS[1] p. 12 (DE).

## 2. TigerBeetle's VOPR

The VOPR is TigerBeetle's deterministic simulator. "Because our simulator is deterministic based on a
_seed_ number and the Git commit, we can perfectly reproduce any bugs discovered in testing" DS[2]
`docs/internals/vopr.md:7-8` (DE). "In the simulator, all non-deterministic parts of the system are
stubbed out. This includes the clock, network, and disk operations" DS[2] `vopr.md:29-30` (DE). "The
VOPR uses a random seed to tune parameters for injecting different types of faults into the
simulation" DS[2] `vopr.md:32-33` (DE). On failure, "the seed and Git commit hash can be used to replay
back the exact simulation and bug" DS[2] `vopr.md:39-40` (DE). Speed: "One minute of VOPR time is
equivalent to days of real-world testing" DS[2] `vopr.md:9-10` (CL).

**Single thread, swarm, scale.** The VOPR "can run an entire cluster on a single thread, injecting
various storage faults and infinitely speeding up time" DS[3] `docs/ARCHITECTURE.md:315-316` (DE). It
"combines a smart workload generator, swarm testing and a thousand CPU cores" DS[3]
`ARCHITECTURE.md:316-317` (CL: the core count is stated, not recorded). "Determinism supercharges
randomized testing. Any test failure can be reliably reproduced by sharing a seed that led to the
failure" DS[3] `ARCHITECTURE.md:306-307` (DE).

**One PRNG from one seed.** The seed comes from the command line or from a random draw, and every
option is then drawn from one generator: `var prng = stdx.PRNG.from_seed(seed);` DS[4]
`src/vopr.zig:150` (DE). In swarm mode the replica count, client count, network delays, loss, clogging,
replay and partition probabilities are all drawn from it, e.g. `.packet_loss_probability =
ratio(prng.int_inclusive(u8, 30), 100),` DS[4] `src/vopr.zig:447` (DE); sub-simulators get their own
seeds from it (`.seed = prng.int(u64),` at `src/vopr.zig:415` and `:443`).

**Safety, then liveness, each on a tick budget.** "Safety: replicas crash and restart; at any given
point in time arbitrarily many replicas may be crashed, but each replica restarts eventually. The
cluster must process all requests without split-brain" DS[4] `src/vopr.zig:263-265` (DE). The loop is
`while (tick < cli_args.ticks_max_requests) : (tick += 1) {` DS[4] `src/vopr.zig:271`, bounded by
`ticks_max_requests: u32 = 40_000_000,` at `:83` (DE). Then "Liveness: a core set of replicas is up and
fully connected. The rest of the replicas might be crashed or partitioned permanently. The core should
converge to the same state" DS[4] `src/vopr.zig:315-317` (DE), on its own budget (`ticks_max_convergence`,
`:84`). A failure prints `"you can reproduce this failure with seed={}"` DS[4] `src/vopr.zig:349` (DE).

**Crash at a pending write.** The simulated storage permits "An additional fault is permitted at the
target of a pending write during a crash" DS[5] `src/testing/storage.zig:11` (DE), and fault placement
is bounded so the cluster can still recover: faults are distributed "to ensure that at least one
replica will have a valid copy to help others repair" DS[5] `storage.zig:14-15` (DE).

**Checkers.** "the simulator also includes a variety of additional checkers that verify the correctness
of the cluster's state" DS[2] `vopr.md:59-60` (DE); replicas' data files "are designed to be
byte-for-byte identical across caught-up nodes in the cluster" DS[2] `vopr.md:60-61` (DE).

## 3. Jepsen and Elle

Jepsen's test loop: "A *generator* generates new operations for each process to perform" and "The start
and end of each operation is recorded in a *history*. While performing operations, a special *nemesis*
process introduces faults into the system--also scheduled by the generator" DS[6] `README.md:26-30`
(DE). Then "Jepsen uses a *checker* to analyze the test's history for correctness" DS[6]
`README.md:32-33` (DE).

The nemesis is a protocol with three operations DS[7] `jepsen/src/jepsen/nemesis.clj:16-21` (DE):
`(invoke! [this test op] "Apply an operation to the nemesis, which alters the` / `cluster.")`, beside
`setup!` and `teardown!`. Nemeses compose: `compose` "Combines multiple Nemesis objects into one" and
"routes to the appropriate child nemesis" by the op's `:f` DS[7] `nemesis.clj:396` and `:401` (DE).
Jepsen runs against real nodes over SSH (`README.md:20-22`), so its fault timing is not reproducible
from a seed; FDB's paper names exactly that gap DS[1] p. 12.

Elle (Kingsbury and Alvaro) checks a recorded history without a serial oracle: it "infers an
Adya-style dependency graph between client-observed transactions" and "can detect every anomaly in
Adya et al’s formalism [2] (except for predicates)" DS[8] p. 1 (DE; pdftotext of the copy read drops the hyphen in
"Adya-style" and "NP-complete" at line breaks). It names the cost of the general alternative: linearizability checkers
are limited "by the NP-complete nature of linearizability checking" DS[8] p. 1 (DE). For hex-sim, Elle
matters where the oracle can't be serial: a history of concurrent placements checked for the isolation
the control plane claims.

## 4. kwok: fake nodes

"[KWOK] is a toolkit that enables setting up a cluster of thousands of Nodes in seconds" DS[9]
`README.md:7` (CL). "Under the scene, all Nodes are simulated to behave like real ones" DS[9]
`README.md:8` (DE). `kwok` is "responsible for simulating the lifecycle of fake nodes, pods, and other
Kubernetes API resources" DS[9] `README.md:15` (DE). Its scale: "Currently, KWOK can reliably maintain
1k nodes and 100k pods easily" DS[9] `README.md:25` and "Currently, KWOK can create 20 nodes or pods per
second" DS[9] `README.md:27` (both CL: no hardware or method stated). Nodes are configurable: "You can
configure different node types, labels, taints, capacities, conditions, etc." DS[9] `README.md:30`
(DE). Gap: kwok is not deterministic and runs against a real API server; it is the model for hex-sim's
fake nodes, not for its event loop.

## 5. Model-based testing against a serial model

Hughes describes QuickCheck's stateful testing: "we also model the state of the system abstractly, and
define model state transitions for each operation in the API" and "We define postconditions for each
API call, relating the actual result of the call to the model state" DS[10] p. 3 (DE). "A test passes if
all postconditions hold" DS[10] p. 3 (DE). The model can be wrong too: "Errors are often in the model,
rather than the code" DS[10] p. 9 (DE). Why it works: "we find bugs by comparing two independent
descriptions of the desired behaviour—the implementation, and the specification" DS[10] p. 17 (DE). On
Volvo's AUTOSAR acceptance tests they formalized the standard "in 20,000 lines of QuickCheck code" DS[10]
p. 11, "finding more than 200 problems—of which well over 100 were ambiguities or inconsistencies in the
standard itself" DS[10] p. 11 (MV: Quviq's own project).

In Rust, proptest-state-machine "automates the checking of properties of a system under test (SUT)
against an abstract reference state machine definition" and shrinks to "a minimal sequence of
transitions that still reproduce the issue" DS[11] `book/src/proptest/state-machine.md:7` (DE). "It will
capture regressions file with a seed that can be used to deterministically repeat the found case"
DS[11] `state-machine.md:25` (DE). The test replicates "the same transitions as the reference state
machine to find any possible divergence" DS[11] `state-machine.md:36` (DE).

## 6. Sans-IO

A sans-IO implementation "contains no code that does any form of network I/O or any form of
asynchronous flow control" and is "defined entirely in terms of synchronous functions returning
synchronous results" DS[12] `#what-is-an-i-o-free-protocol-implementation` (DE). That is what lets the
same scheduler core run under hex-sim's event loop and under the production shell, as FDB's "simple
shim to the relevant system calls" does DS[1] p. 8.

## 7. What this means for hex-sim

- **Determinism from one seed.** Every random choice in a run (cluster shape, workload, fault kinds,
  rates and timing, tuning values) is drawn from one PRNG seeded by one `u64`, and sub-generators are
  seeded from it, as the VOPR does DS[4]. A failure prints the seed and commit that replay it DS[2][4].
- **Single-threaded event loop on virtual time.** All scheduler and node code runs on one thread in a
  discrete-event loop that fast-forwards to the next event; clock, network, disk and randomness sit
  behind the sim's seams DS[1][2][3]. The core is sans-IO so the production shell and the sim drive the
  same code DS[12][1].
- **Nemesis at every control-plane write.** Crashes are injected at every control-plane write, including
  at the target of a pending write, and the nemesis also partitions, delays and reorders, scheduled by
  the generator like a client DS[1][5][6][7]. Fault rates are tuned so faults don't collapse the state
  space DS[1].
- **Buggify and swarm.** The scheduler exposes buggify points (spurious errors, delays, unusual tuning
  values) and each run enables a random subset, with a random cluster and workload DS[1][4].
- **Oracle comparison.** A serial specification of the scheduler runs beside it; every operation's
  result is checked against the model state, and a failing run shrinks to a minimal sequence DS[10][11].
  Where concurrency makes a serial oracle unsound, the recorded history is checked Elle-style DS[8][6].
- **Safety, then liveness, each on a counted budget.** A run checks safety while faults run, then heals a
  core and checks it converges within a tick budget, as the VOPR does DS[4]; coverage counters prove the
  rare states were reached DS[1].
- **Fake nodes.** A kwok-like cluster of fake nodes with configurable capacity, labels, taints and
  conditions gives the scheduler thousands of nodes on a laptop, but inside the deterministic loop
  rather than against a live API server DS[9].
- **What it won't show.** hex-sim proves correctness, not speed: performance claims still come from
  recorded benchmarks on real hardware DS[1].

## 8. Gaps

- FDB's paper does not say how a run's seed is chosen, stored or replayed; TigerBeetle's code does
  DS[1][4].
- No source read gives an independent (`MI`) measurement of how many bugs deterministic simulation
  finds against another method; FDB's 0.5M disk years and Hughes' 200 problems are the authors' own.
- kwok's scale figures carry no hardware or method DS[9].
- The Elle copy read is arXiv v1 (2020-03-23), not the VLDB camera-ready.

## References

[DS1] J. Zhou, M. Xu, A. Shraer et al. (Apple, Snowflake), "FoundationDB: A Distributed Unbundled
Transactional Key Value Store", SIGMOD '21, https://doi.org/10.1145/3448016.3457559. Copy read:
https://www.foundationdb.org/files/fdb-paper.pdf (14 pages; page numbers are the PDF's, which carries
none printed). Kinds: DE; MV for the CloudKit figure; CL for the debugging-practice claim.
- p. 2: "before building the database itself, we built a deterministic database simulation framework"
- p. 8: "All database code is deterministic; accordingly multithreaded concurrency is avoided"
- p. 8: "including network, disk, time, and pseudo random number generator"
- p. 8: "determinism guarantees that every bug found this way can be reproduced, diagnosed, and fixed"
- p. 9: "a high-level fault injection technique informally referred to as “buggification”"
- p. 9: "the simulator can fast-forward clock to the next event"
- p. 11: "so an exact reproduction is guaranteed"
- p. 11: "for more than 0.5M disk years without a single data corruption event" (MV)
- p. 12: "All of these approaches lack deterministic reproducibility."

[DS2] TigerBeetle, `docs/internals/vopr.md`, at commit 0450c32b2c3c3ab6114f3402cc601341cc8192aa.
https://raw.githubusercontent.com/tigerbeetle/tigerbeetle/0450c32b2c3c3ab6114f3402cc601341cc8192aa/docs/internals/vopr.md
Kinds: DE; CL for the speed claim.
- `:29-30`: "In the simulator, all non-deterministic parts of the system are stubbed out. This includes the clock, network, and disk operations."
- `:39-40`: "the seed and Git commit hash can be used to replay back the exact simulation and bug"
- `:9-10`: "One minute of VOPR time is equivalent to days of real-world testing." (CL)

[DS3] TigerBeetle, `docs/ARCHITECTURE.md` (Determinism, Simulation Testing), at commit
0450c32b2c3c3ab6114f3402cc601341cc8192aa.
https://raw.githubusercontent.com/tigerbeetle/tigerbeetle/0450c32b2c3c3ab6114f3402cc601341cc8192aa/docs/ARCHITECTURE.md
Kinds: DE; CL for the core count.
- `:315-316`: "can run an entire cluster on a single thread, injecting various storage faults and infinitely speeding up time"
- `:306-307`: "Any test failure can be reliably reproduced by sharing a seed that led to the failure."
- `:320`: "the simulation testing exercises a specific implementation"

[DS4] TigerBeetle, `src/vopr.zig`, at commit 0450c32b2c3c3ab6114f3402cc601341cc8192aa.
https://raw.githubusercontent.com/tigerbeetle/tigerbeetle/0450c32b2c3c3ab6114f3402cc601341cc8192aa/src/vopr.zig
Kind: DE.
- `:150`: `var prng = stdx.PRNG.from_seed(seed);`
- `:263-265`: "Safety: replicas crash and restart; at any given point in time arbitrarily many replicas may be crashed, but each replica restarts eventually."
- `:315-317`: "Liveness: a core set of replicas is up and fully connected."
- `:349`: "you can reproduce this failure with seed={}"

[DS5] TigerBeetle, `src/testing/storage.zig` (module documentation), at commit
0450c32b2c3c3ab6114f3402cc601341cc8192aa.
https://raw.githubusercontent.com/tigerbeetle/tigerbeetle/0450c32b2c3c3ab6114f3402cc601341cc8192aa/src/testing/storage.zig
Kind: DE.
- `:11`: "An additional fault is permitted at the target of a pending write during a crash."

[DS6] Jepsen, `README.md` (Design Overview), at commit 08e3e33964808876c2c9bbe4f3f9f948295a108e.
https://raw.githubusercontent.com/jepsen-io/jepsen/08e3e33964808876c2c9bbe4f3f9f948295a108e/README.md
Kind: DE.
- `:29-30`: "a special *nemesis* process introduces faults into the system--also scheduled by the generator."
- `:32-33`: "Jepsen uses a *checker* to analyze the test's history for correctness"

[DS7] Jepsen, `jepsen/src/jepsen/nemesis.clj`, at commit 08e3e33964808876c2c9bbe4f3f9f948295a108e.
https://raw.githubusercontent.com/jepsen-io/jepsen/08e3e33964808876c2c9bbe4f3f9f948295a108e/jepsen/src/jepsen/nemesis.clj
Kind: DE.
- `:16`: `(defprotocol Nemesis`
- `:19`: `(invoke! [this test op] "Apply an operation to the nemesis, which alters the`
- `:396`: "Combines multiple Nemesis objects into one."

[DS8] K. Kingsbury (Jepsen), P. Alvaro (UC Santa Cruz), "Elle: Inferring Isolation Anomalies from
Experimental Observations", arXiv:2003.10554v1, 23 Mar 2020 (published in PVLDB 14(3), 2020).
https://arxiv.org/abs/2003.10554 Kind: DE.
- p. 1: "Elle can detect every anomaly in Adya et al’s formalism [2] (except for predicates)"

[DS9] Kubernetes SIGs, kwok `README.md`, at commit 07f188f7020515b90266612f32bbb4d062678d31.
https://raw.githubusercontent.com/kubernetes-sigs/kwok/07f188f7020515b90266612f32bbb4d062678d31/README.md
Kinds: DE; CL for the scale figures.
- `:15`: "responsible for simulating the lifecycle of fake nodes, pods, and other Kubernetes API resources"
- `:25`: "Currently, KWOK can reliably maintain 1k nodes and 100k pods easily." (CL)
- `:27`: "Currently, KWOK can create 20 nodes or pods per second." (CL)

[DS10] J. Hughes (Chalmers University, Quviq AB), "Experiences with QuickCheck: Testing the Hard Stuff
and Staying Sane", in *A List of Successes That Can Change the World* (WadlerFest), LNCS 9600, Springer,
2016. Copy read: author's preprint (18 pages, dated 2016-01-12),
https://www.cs.tufts.edu/~nr/cs257/archive/john-hughes/quviq-testing.pdf. Kinds: DE; MV for the AUTOSAR
figure.
- p. 3: "A test passes if all postconditions hold."
- p. 9: "Errors are often in the model, rather than the code."
- p. 11: "finding more than 200 problems—of which well over 100 were ambiguities or" (MV)
- p. 17: "we find bugs by comparing two independent descriptions"

[DS11] proptest, *The Proptest Book*, `book/src/proptest/state-machine.md`, at commit
70cf37d46b7d83393b6ed94c611f8c52209f585d.
https://raw.githubusercontent.com/proptest-rs/proptest/70cf37d46b7d83393b6ed94c611f8c52209f585d/book/src/proptest/state-machine.md
Kind: DE.
- `:7`: "State machine testing automates the checking of properties of a system under test (SUT) against an abstract reference state machine definition."
- `:25`: "It will capture regressions file with a seed that can be used to deterministically repeat the found case."

[DS12] Sans I/O project (no author named on the page), "Writing I/O-Free (Sans-I/O) Protocol Implementations", Sans I/O 1.0.0
documentation. https://sans-io.readthedocs.io/how-to-sans-io.html#what-is-an-i-o-free-protocol-implementation
Kind: DE.
- "defined entirely in terms of synchronous functions returning synchronous results"
