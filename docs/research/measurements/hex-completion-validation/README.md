# Scheduler completion acceptance tooling (AC-T7.3.1, AC-T7.4.1)

Implementer `01a1c7676cfc7297a41236a8e71ee188`, lane L4. ROOT coordinator
`01a1aa77778471339bea3beff90d6bbd` owns native runs, VM/fault/mutation campaigns,
profiling, benchmarking, integration, audits, gates, merges and pushes. ROOT
acknowledged this lane's policy readback before edits. Base is exactly
`b06b1760bb690c9e29a31f99efdb998cf8279c36`; policy revision
`0342b85d9c808e3ced041a6c3ef0e9bf70438272`; rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.

Owned paths are new `scripts/scheduler-completion/**`,
`bench/scheduler-completion/**`, this directory, standalone
`crates/hex-cli/tests/completion_*` and `crates/hex-gpu/tests/completion_*`.
No production picker, inventory, reservation ledger or runtime is duplicated.
The fixture observes the existing runtime's capacity/records and uses the ordinary
control and Runtime sockets. Root capabilities and the VM executable must agree
with the typed backend; a scheduler label and a generic OCI guest do not suffice.

The Python checker accepts exactly nine ordered native stages per backend and
topology: initialized capability, durable binding, checked compute, live foreign
denial, wrong-ID refusal, scheduler recovery, runtime recovery after catalog/load
reordering, complete physical cleanup, and counted completion. An exact selected
libtest count of one is required per invocation. Ignored-only, missing hardware,
zero-selected, unsupported, simulated and compile-only paths cannot pass.
Receipt artifacts have content hashes, sizes and paths retained inside the run;
symlinks and path escapes refuse. A failed invocation preserves its state.

`scripts/scheduler-completion/run.py` runs only the named standalone native test,
under ROOT's explicit authorization, caller-provided finite deadlines and output
bounds. It records the actual binary, command, source, PID, exit, reap and raw log.
Its timeout can stop only its exact child handle and never claims descendant/VM
cleanup from that fact. ROOT must inspect the preserved scope's PID journal when
a fixture cannot finish its ordinary cleanup.

Primary method sources: [Python subprocess](https://docs.python.org/3/library/subprocess.html)
defines timeout/wait and return-code behavior; [Python resource](https://docs.python.org/3/library/resource.html)
is Unix-only, so unavailable counters remain unmeasured; the
[Linux cgroup v2 interface](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html)
supplies hierarchical CPU/memory/kernel/page-fault counters; the
[Vulkan device identity interface](https://docs.vulkan.org/spec/latest/chapters/devsandqueues.html)
distinguishes device UUID, Windows LUID and pipeline-cache UUID. UUID alone is not
a persisted physical key: correlate it with independently observed PCI/native
identity for this run and keep the existing runtime's native key authoritative.

Alternatives considered: trusting log keywords permits vacuous counts; checking
only GPU enumeration permits metadata without dispatch; a separate fixture picker
would duplicate production authority; `RUSAGE_CHILDREN.ru_maxrss` is the largest
child's peak on Linux and cannot establish parent/kernel/tree cost. Raw workload
and external resource observers are separate evidence, joined by source/run IDs.

This checkpoint is source-first and has no native pass or performance claim.
Initial source has not yet been checked. All actual backend/VM/foreign-device,
crash/recovery, mutation/restoration, allocation/copy/fault, competing-baseline
and supported-target results remain owed by ROOT. Light harness self-checks will
run only on admitted d132 with one CPU, 4 GiB memory, 12 GiB disk, nice 10,
jobs/test threads one, and unique private source/target/cache/temp.
