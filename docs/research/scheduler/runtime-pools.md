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

## Validation still owed

The implementation is in progress. Focused pure-core red tests and the live r329
counterexample are retained; no combined fixed-tree pass is claimed here yet.
The lead must run the fixed real 633/632 refusal case, valid local/SSH lifecycles,
restart and shared-pool cases, guard-removal evidence, all seven gates, native
Windows checks and the Linux root lab when approved headroom is available.
New capacity-call waits and scheduler waits require real debug/release census
records before seeding. Allocation, fault, CPU and latency comparisons remain
unmeasured for this change. The Windows arm64 VM remains paused.
