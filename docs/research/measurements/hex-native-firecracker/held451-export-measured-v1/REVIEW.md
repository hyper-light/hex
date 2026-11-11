# Explicit measurement copy: source only, unrun

This is a changed measurement source, not a semantically identical timing sample
or a faster export algorithm. ROOT requested the copy and retains R14 profiling,
measurement interpretation, next admission and any quiet-window request. No remote
transfer/staging, measurement-module import, run, profiling or benchmark occurred.

The frozen439-line exporter remains byte-identical at SHA
`83586fc225fbe7209eb250a6c8d21b8fb70dff97f82dd7dccb1f4f47bde00ad3`.
`instrumentation-from-83586.patch` is the complete diff to this separate copy.
Its source SHA is
`1f89937a6171918295932c4aa3dbba5658b55218c19bbb7d8a4d902e41bab3ec`.
The prior1dc/aa08 failures, current transport cache provenance, original event
cursor/raw/rounded drafts and b144 source review remain unchanged.

## Four existing fsync sites

| Fixed label | Existing descriptor and position |
| --- | --- |
| `source_event` |Event-log descriptor, after every existing write+flush, including a failed event |
| `archive` |Partial archive descriptor, after existing flush and chmod, before rehash/publication |
| `manifest` |Manifest descriptor, after its existing write+flush |
| `output_directory` |Existing output directory descriptor, after archive/manifest publication |

Each site invokes the explicit `measured_fsync` wrapper. It calls `os.fsync(fd)`
once, with the original descriptor, at the original sequence position. The wrapper
does not catch or suppress that call's exception. A finally block records the API
attempt, error status and cumulative `monotonic_ns` duration, including failed
attempts. All original event flush/fsync frequency, archive rehash, publication,
directory synchronization, receipts, file selection and refusal guards remain.
Other helper/receipt fsync calls are unchanged and are not counted as one of these
four labelled sites.
The clock returns integer nanoseconds; retain untouched JSON and Python integer
parsing for these values. [Python monotonic_ns](https://docs.python.org/3.14/library/time.html#time.monotonic_ns).

`source_event_samples_ns` retains attempt durations, including failures, in call
order. The cap is exactly `COUNT*2+1` =3,193, derived from the fixed1,596-object
protocol plus one allowance requested for measurement. On overflow the original
fsync still runs, the fixed counters still update, and the profile explicitly
records typed refusal `SourceEventSamplesExceeded`; quantiles remain null. It
does not silently publish prefix percentiles as a full measurement or replace an
original I/O error. This is a measurement refusal, not new artifact authority or
a changed export-success policy.

Quantiles use nearest rank `ceil(n*p/100)`, one-based, without interpolation, for
p50/p90/p99; max is the largest sample. An empty or refused sample set has null
quantiles. Raw attempt samples and the sample count remain in the evidence.
These are Python `os.fsync` API timings/counts, not kernel syscall retries,
filesystem work, device latency or a general RuntimeCost.

## Phase and own-CPU evidence

The fixed phase labels are `control_mappings_before`, `operation` and
`control_mappings_after`; each records calls and cumulative monotonic duration,
also on a raised exception. Zero calls means not reached. Control-mapping phases
time the existing mapping function, excluding the following JSON evidence write.
The operation phase is inclusive of nested fsync timings and instrumentation;
the phase and fsync durations must not be added as independent costs.

Own `os.times()` user/system snapshots and deltas cover the worker interval after
the original deadline setup/tick through profile finalization. These are the
worker's own user/system CPU fields, excluding parent/child CPU; they are not
cgroup-wide or whole-host kernel/device accounting. The final worker-evidence
JSON write and its fsync occur after the end snapshot. `worker_interval_ns` starts
just before the CPU start snapshot and ends in the same finalization function.
No child CPU is relabelled as own CPU. [Python os.times](https://docs.python.org/3.14/library/os.html#os.times).
These observations would be partial worker
evidence, never an accepted cold-provider cost or whole-node allowance.

New measurement setup follows the original175s deadline calculation/tick, so it
does not extend that clock. Scope180s, outer200s+3s, Kiln210s and caller220s stay
unchanged. Timers, counters, allocations, sorting and serialization add overhead
and can move the deadline cursor. No overhead equivalence, zero-cost property,
speedup, fsync bottleneck share or live result is claimed.

The profile is nested under `measurement_profile` in the existing owned
`worker-final.json`, inside its existing finally path. The whole JSON remains
bounded by the unchanged16MiB `write_json` limit; labels/phases are fixed and the
only new growing collection has the3,193-sample cap. A hard kill, earlier pre-try
failure, allocation/I/O error or inability to write final evidence can leave the
profile absent/incomplete. The unchanged per-event source journal is not a timing
sample journal. No added signal handler, writer thread, service or persistence
manager is introduced. ROOT must retain these failure/kill uncertainties.

## Source cohort and proposed command

Imports are exactly the frozen imports. New calls use the already imported
`time`/`os` builtins and sorted/list/dict primitives. No module, mapped-control
input, package or tool fallback is added. Input14e35, controls d5e8 (including the
same CURRENT transport-only loader-cache pin), IOe9a19 and all old source/raw
remain byte-identical. Actual future mappings are still subject to the unchanged
strict control verification; this source check is not a native mapping result.

The four-file runtime seal is `held451-export.seal.json`, SHA
`27102cb1c12a42dd247d97e820895cb1b63e961bcf6d67924400e4c4b75c031c`.
`next-command.json` contains complete remote/API/CLI argv. It proposes the fresh
bundle `/home/adalundhe/.cache/hex-native-firecracker/held451-export-measured-bundle-v1`,
root `.../held451-export-measured-v1-01`, and unit
`hex-native-fc-held451-export-measured-v1-01`, with the same normal451 Linux amd64
1CPU/4GiB/12GiB, nice10, swap0, pids256, jobs1/threads1 envelope.

The admission template is false with null actual ACK/deadline. This proposal does
not stage sources, request a window or authorize any run. The existing parent API
does not echo the nested profile; ROOT would separately retrieve exact final
worker evidence following its own next admitted execution. Collection,
materialize/preflight/build and GPU lanes receive no release here.

`source-validation.json` records AST/syntax/import/descriptor-label/guard/input
checks and normalized baseline operation-graph comparison, without importing or
executing any measurement function. That comparison strips only the declared
instrumentation and is not a runtime timing/exception/compatibility proof. No
quantile helper, profiler, export, fault injection or test campaign was run.
Current7d7 policy/rulesdf2 and assigned native docs ownership apply. Existing
R01–R16 evidence remains inherited; this checkpoint adds only source/metadata
proof. ROOT owns the required live validation and R14 decision.
