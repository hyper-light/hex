# Runtime capacity is part of admission (AC-T7.4.1)

Coordinator record, 2026-11-10 UTC. This fixes an observed gap in adapting kiln's
runner ledger to Hex's VM-backed runtime. It adds a constraint to the existing
index and receiver admission path; it does not introduce another picker.

## Reproduction

At `b948976184b73f047565cd873d83144ed8a05f52`, lead run r329 on the arm64 Mac
Studio used an ordinary `hex node`, a real Shards guest configured with 4 vCPUs
and 632 MiB, and the production scheduler locally and over forced-key OpenSSH.
A demand reserving 633 MiB was assigned, physically created and started twice.
Independent Runtime inspection verified each running box. Both were deleted and
the owned guest ended as `Stopped`. This is an admission counterexample, although
the lifecycle harness itself returned success. The explicit negative admission
assertion then failed with exit1.

Raw record: `~/.cache/hx/e-mqkw3q2u/{result.json,owned-guest.json,pids.jsonl}` on
runner `d1323898b255a3ed435eb83fa0f7b0cf`. Lead assertion log:
`~/.cache/hex-lead/logs/scheduler-guest-capacity-red-b9489761.{log,exit}`.
The same fixture's valid 64 MiB case passed before this reproduction at
`~/.cache/hx/e-o9ksvgks/result.json`. These runs use actual guest workloads;
neither establishes a competitive performance result.

## Sources and alternatives

The ported kiln ledger already admits only within measured room in every
dimension (`crates/hex-sim/src/lb/runner.rs`, upstream provenance in that file's
header, and `lb/mod.rs::within`). The missing input was the smaller backing
runtime's room. The old shell supplied the host's free memory and disk even for
an already-configured guest.

[Kubernetes Node Allocatable](https://kubernetes.io/docs/tasks/administer-cluster/reserve-compute-resources/)
(read 2026-11-10, live documentation) similarly distinguishes workload capacity
from the machine's total resources. No Kubernetes reserve constant is borrowed.
[Firecracker's v1.13.1 machine configuration](https://raw.githubusercontent.com/firecracker-microvm/firecracker/v1.13.1/src/firecracker/swagger/firecracker.yaml)
defines guest vCPU and memory independently of the host. Hex's existing
`GuestConfig::spec` likewise passes explicit guest CPU and memory limits.

Keeping host facts alone is disproved by r329. Lowering the whole node to its
smallest guest would unnecessarily restrict an independent native backend.
Operator-entered duplicate limits could drift from the running guest. The chosen
input is therefore a typed capacity report through the existing Runtime seam:
actual free and total CPU, memory and writable volume, with a backing-pool ID.
A GuestRuntime asks the actual guest node, bounds that answer by its configured
CPU/memory, and identifies the actual VM by PID plus start identity. A native
same-host implementation can retain the existing host budget explicitly.

## State, ownership and bounds

The core retains its global node budget and adds bounded runtime pools, each
covering a disjoint set of placement classes. A reservation must fit both. The
maximum number of pools follows from the number of classes. The existing
take/give path changes host and pool accounting atomically. Refresh validates
held reservations before changing any state. No clock, I/O or additional
scheduling algorithm enters the pure core.

The shell groups reports of one backing pool before adding measured memory held
by its own physical boxes. Different reports of the same pool use the smaller
componentwise room. It does not credit requested caps, invent disk usage, or
extend total capacity. Reports and pool identities remain bounded by the common
wire frame. Backend calls run outside core/record locks. There is no new cache or
periodic probe: capacity joins the existing admission/Describe refresh points.

Recovery may floor only its private boot ledger to reservations of boxes actually
found alive, including their class pools. That floor is never advertised as free
room; new local admission remains blocked until an actual refresh succeeds.
Deletion and duplicate handling retain their cleanup authority.

## Validation and remaining work

The original actual633/632 admission counterexample is fixed. Root r338 atcfc54769
and both debug/release boundaries at935f6b21 each refused both oversized demands,
ran/deleted both valid64MiB controls and stopped the owned guest. The same
constraints remain after rejecting the storage-only candidate: signedcc7c2190's
production tree is exactly63ec2758's6b87c98d4b982aaf16b3a441138b5ad1689019a6.

Root r347 passed all seven target clippies, production checks, the full macOS
workspace and vendoring at63ec; root r363 passed those at935f too. Windows root
r342 passed531tests with0failures/3ignored and workspace native clippy at962;
r351 passed158sim tests/native clippy at63ec; r367 passed160sim tests/native
clippy at935f. Ignored cases are not claimed executed. Native Linux/root-lab
validation remains owed while the approved runners lack declared CPU headroom.

The actual935f release and debug censuses each completed1,000local+1,000SSH
physical creates/deletes, empty baselines and identity-checked guest shutdown.
Their107,546+129,854census files were SHA256-verified, all named scheduler rows
met their1,000sample targets, each profile had4,004capacity observer answers
attributed to successful processes, and no malformed/truncated rows or recorded
wait expirations occurred. Reports separate raw multiplexed rows from successful
single-call/process outcomes. These records name935f, whose storage candidate
was subsequently reverted; they are not mislabeled ascc runs or installed seeds.

The allocation/quiet latency comparison and exact standalone harnesses are in
[the benchmark record](../../../bench/sched/runtime-pools/README.md). Although
side storage saved800,000bytes with10,000finite pools, quiet50,000sample/version
fit p99 rose from162,875to189,292ns. Four of five paired p99 values worsened.
Root restored inline state under the user's latency-first instruction. No speed
improvement or external competitor ratio is claimed.

## Open acknowledgement defect

Freshcc debug boundary r410 passed. The release case refused both633MiB asks but
its remote64MiB control remained unconfirmed until the original120s operator
cancellation deadline. Receiver row was running; owner row remainedat1 and its
inboundpeer-1order record was absent. Both the failed release guest and passed
debug guest were stopped with exact identity proofs. The reported cleanup_error
was incomplete census controls after the failure, not missing VM retirement.
Raw release evidence: root runnerd132 ~/.cache/hx/e-i37a7f_n/result.json and
its o.state/r.state. This run is failed, not a passing final E2E.

A targeted cold remote-Busy regression passed, falsifying that condition alone.
A traced real repeat and a four-extra-reader Status-pressure repeat also passed,
so neither is claimed to fix the intermittent failure. The latter trace did
expose retry measurement including its own earlier pauses: within one refresh
queue,2-5microsecond Busy checks requested4.8/16.7/62.7ms delays. An isolated
virtual-clock regression reproduces that sampling defect; its reset-after-pause
fix is signed in a98824f7. The test was first run red at94711111 (root96a86a73),
with requested pauses [1,2,4]ns, and passes with the reset after each pause,
requesting [1,1,1]ns. The cold-Busy regression and focused clippy also pass.
Root independently repeated exact guard removal/restoration in r452: removing the
reset restored the exact96a86a73 scheduler source and failed with [1,2,4]ns
against [1,1,1]ns (cargo101,0.18s). Restoring exact a988 source passed (cargo0,
0.21s). The earlier r449 removed the same guard while retaining two comments;
it also failed/passed, but is not labeled byte-identical to96a86a73. Source
hashes, commands and logs for the exact check are retained in
bench/sched/runtime-pools/retry-reset-root-mutation-a98824f7.json.

Root r430 then replayed the unchanged real boundary fixture once in debug and
five times in release using uninstrumented a98824f7 host binaries. All six runs
passed both oversized refusals, both valid physical creates/deletes, the restored
empty baseline and identity-checked guest shutdown. Root r429 passed the four
focused scheduler cases and workspace native Windows clippy. The combined
seven-target/workspace gate r427 is still running at this evidence checkpoint;
its intermediate success is not a final gate result.

Root r439 repeated the same four-reader diagnostic with only the retry reset
added to the earlier trace source. The first owner refresh sampled6,697,375ns
of real outer work; its next eight Busy attempts sampled1,125-1,875ns and kept
the same6,697,375ns requested pause. A second sequence held4,702,292ns across
subsequent1,583/1,375ns samples. The receiver similarly kept781,691,875ns after
its next1,167ns sample. The reset excludes earlier pauses while preserving the
maximum actual work. Both physical controls passed, all eight reader scopes
joined/reaped every child, and VM8146/start12180467610293 ended Stopped.

The trace source is a temporary cc7c2190 copy plus the same minimal reset and
tagged instrumentation; it is not presented as the uninstrumented a988 artifact.
The production source contains no debug trace. The exact summaries, input hashes,
trace observations and source-preserved diagnostic tools are in
[the retry record](../../../bench/sched/runtime-pools/retry-reset-a98824f7.json)
and census/. Queue ordinals are explicitly inferred from queue-enter boundaries,
not invented actual IDs. Initial Describe/open work remains legitimate outer time.

These bounded replays verify the timing defect and its fix. They do not establish
that it was the sole cause of the original intermittent acknowledgement stall.
No wait seed or longer deadline was used to hide that failure; calibration and
the native Linux/root-lab validation remain owed.
The Windows arm64 VM remains paused. Firecracker/WHP/GPU VM coverage is unverified.
