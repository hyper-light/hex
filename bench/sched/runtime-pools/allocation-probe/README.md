# Standalone FitIndex measurement tool

Root compiled/clippied the same Rust source against exact63ec2758 and935f6b21
(r377), validated both modes and ran the loaded allocation census(r390). The parent
README and JSON records carry measured scope and remaining quiet-latency work.
The package is benchmark tooling, never installed or linked into Hex production.

The original constructor mode requires --nodes, --cpu-milli, --mem-bytes and
--rounds. It measures only FitIndex::new and explicit drop; original inputs remain
live through all rounds and output. No workload defaults are supplied.

For --workload runtime-pool-cycles additionally supply all of:

- --pool-classes (comma-separated oci,firecracker,shards without duplicates),
  --pool-cpu-milli and --pool-mem-bytes: one shared pool per node.
- --demand-class, --demand-cpu-milli and --demand-mem-bytes: a healthy demand,
  included in the pool and fitting both host and pool. GPU/disk are zero.
- --decision verdict|fit and --scorer best-fit|most-allocated|align|hex.
- --repeats and --warmup-repeats: explicit measured/unmeasured cycle counts;
  warmup0 is accepted, repeats must be positive.

Verdict times a rotating single-node admission check. Fit times actual global
FitIndex::fit with prebuilt Pick/Shapes and charges its selected node. Every take
and give is checked; give must restore the exact prior Key. Sequential cycles
never intentionally overlap reservations. A failed/refused operation produces a
nonzero diagnostic result, not a healthy timing sample.

Every operation has cumulative before/after allocator snapshots and an Instant
interval. Sample-vector capacity is reserved before construction. JSON/statistics
run after index drop. Summaries sum individual SUT deltas; no counter reset or
per-call JSON occurs within the measured spans. Reported percentiles use nearest
rank and include cold samples. Timings include instrumentation/clock overhead;
there is no overhead subtraction. copied_bytes is null/unmeasured.

The System GlobalAlloc wrapper forwards exact pointers/layouts, tracks successful
calls and emits a best-effort allocation-free Unix stderr marker on null. It cannot
catch an OOM abort, OS kill or double panic. Ordinary unwindable errors/panics are
labeled; unexplained termination remains unknown. Preserve external exit status
and stderr. Zero retained bytes after index drop does not prove process-wide leak
freedom. Counters omit allocator metadata, direct foreign allocation, stack and
kernel memory. Atomic snapshots assume the pure synchronous SUT and no unrelated
background threads; concurrent allocation would invalidate attribution.

Wrap the executable in /usr/bin/time -l for separate whole-process CPU/RSS/faults.
Those values include sample and reporting buffers; do not call them index memory.
Use identical inputs, locks, toolchains/flags and quiet approved hardware for any
latency comparison. The constructor old b948 binary uses its historical source;
this extension requires the pool APIs present in63ec and935f.
