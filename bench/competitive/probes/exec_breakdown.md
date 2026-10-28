# Where the seam's exec and start spend their time, and the floor under exec

Goal 3 of the runtime design (decision 3 of `docs/design/runtime-architecture.md`; kiln plan
`01a17c0ccc6f721e84990c5ca370de3e`, step 1) is judged by two ratios read from one run: Hex's exec p50 through the
`Runtime` seam at most a quarter of libcrun's in-process exec p50, and Hex's start p50 within 10% of libcrun's
(argument A3). Before step 2 is judged against the first, this probe measures where Hex's exec and start spend their
time today, part by part, and the floor under any exec into a running container, against a quarter of libcrun's exec
in the same run.

## How it runs

`exec_breakdown.sh <hex> <seam_oci>` builds `exec_floor.c`, copies every binary into the run's directory, and runs
`exec_breakdown_inside.sh` in the bench container with host.sh's OCI-pass flags (privileged, a private cgroup
namespace, no network, pinned to CPUs 4-31, the host's `/usr`), on the OCI pass's bundles. After a small probe of each
item, each round runs these in a shuffled order:

- **libcrun**: `oci_bench.py`'s lifecycle phase on libcrun in process (create, start, exec of `true` and delete of a
  `sleep` container, each timed alone). Its exec p50 is what the target is a quarter of.
- **floor**: `exec_floor.c`, right before or after libcrun's lifecycle, so the two meet the same host.
- **seam**: `seam_oci`'s lifecycle phase, the `oci` class through the seam in process: Hex's measured p50s.
- **traced**: the same with the breakdown's tracer on.

`exec_breakdown.py summarize` then writes `summary.md` and `summary.json`. `check` holds the probes to reading whole:
no recorded error, no lost trace event, every traced call cut into its parts, every cut the file it stands for.

### The floor

`exec_floor.c` is a prepared, single-threaded process doing what goal 3's seed would (plan step 2), and nothing else.
Each cycle it sets its children's pid namespace to the container's (`setns`), clones into the container's cgroup
with a pidfd (`clone3`, `CLONE_INTO_CGROUP | CLONE_PIDFD`), and the child joins the container's other namespaces
that differ from the probe's own (ipc, uts, net and mnt here), takes `/` as its working directory and execs
`/usr/bin/true`. The probe waits on the pidfd (`waitid(P_PIDFD)`). No state is read, no config parsed, no user or
capabilities set: no exec of `true` into that container can cost less. The container is a `sleep` that crun makes
from the same bundle.

Beside it, interleaved cycle by cycle in the same process (the order rotated each cycle), the lesser floors it is
made of, and a gauge:

- `spawn`: `posix_spawn` and wait of `/usr/bin/true`, as `oci_bench.py` spawns every call (from C here, where
  `oci_breakdown.md` §2 timed it from Python);
- `clone`: `clone3` with a pidfd, exec and wait, with no cgroup and no namespaces;
- `cgroup`: the same, born in the container's cgroup;
- `spin`: 100,000 steps of a dependent multiply-add chain with no system call, a gauge of the clock the host gives
  a burst of work at that moment.

Each cycle is timed alone on `CLOCK_MONOTONIC`, from before its first system call to after its wait returns.

### How a call is cut into parts

The tracer is an ftrace instance of the run's own (`/sys/kernel/tracing/instances/hexbd-<pid>`), on
`CLOCK_MONOTONIC` (`trace_clock` mono), its events kept only for the run's processes' names. `seam_oci`'s
`HEXBENCH_TRACE` brackets the start and exec of every other timed lifecycle with `trace_marker` lines,
`hexbd B <op> <id>` and `hexbd E <op> <id> <ns>`, and turns the instance's recording on only between them. The
lifecycles between run untraced in the same process: each traced call's twins met the host as it did.

A part runs from one event of the call's critical path to the next. The cuts are `exec_cuts` and `start_cuts` in
`exec_breakdown.py`, from the marker before the call to the one after, so a call's parts cover it whole. They follow
the call across processes: the node agent (`seam_oci`), the `hex __oci-exec` helper it spawns
(`crates/hex-node/src/node.rs`, `exec_oci`), the child the helper clones (`crates/hex-runtime/src/linux/oci/exec.rs`),
and the program.

Only events that cost a call little are traced. On this kernel an open's or an execve's entry event fetches its path
from user memory. In the trials, tracing those entries cost an exec about 10% and a start about 20% at p50, about 4 µs
an event. So the rounds trace the opens' exits, which carry only the descriptor, and an open is told by its place on
the call's path, which is the same every time: the node's first `openat` in an exec is the box's record, its second
the container's `state.json`. The probe step adds a detailed trace that also records the entries with their paths, and
`check` holds every such cut to the file it stands for (20 of 20 traced calls in the run of record's probe). rustix
opens with `open(2)` on x86_64, std with `openat(2)`, so both are traced. tracefs prints the clock to the microsecond.

## Run 1 (2026-10-28, fedora): of record for plan step 1

- **Host:** the "fedora" kiln runner (`kiln-a094a12d`, hostname `fedora-2.attlocal.net`), Intel Core i9-14900HX (32
  CPUs: P-core threads 0-15, E-cores 16-31; the container pinned to 4-31), kernel 6.19.10-300.fc44.x86_64, 62 GiB.
- **When and load:** 2026-10-28T01:57:35Z to about 02:03Z. The 1-minute load was 0.62 at the start and 19.47 at the
  end: other work started a minute in. The host's I/O pressure (`/proc/pressure/io`, some, avg10) was 97.6% at the
  start and 65.9% at the end, from a stall outside this run, with swap full. Each item's load and pressure as it began
  are in `probe.out`. Not of record by CLAUDE.md §7 for absolute numbers (`LOAD_GATE=0`). The comparisons below are
  within this run: the floor against the libcrun exec that ran beside it, and each traced call against its twins.
- **Binaries:** `hex 0.1.0 (f59dd3f)`, sha256 cf7a58c57e5198dd, static-pie, built on fedora against Fedora's
  glibc-static 2.43-9.fc44. `seam_oci` of 33e82d1, sha256 c49c25012efc2607. `exec_floor` of a99e00f, sha256
  d7ef60723b79b9a7, GCC 16.2.1. crun 1.30.1, sha256 86d1e6a0e7694597. libcrun 1.30.1 (commit 079ff6a7) in process,
  driver sha256 5c356f2139adc232.
- **Harness:** `bench/competitive` at a99e00f. The summary was made again at 44cd1a2, which adds each round's parts;
  the first is kept as `summary.first.md`. Out dir on fedora:
  `~/.cache/hex-bench/out/exec-breakdown-20261028T015735Z/`.
- **Parameters:** 3 rounds. libcrun, seam and traced: 1,000 lifecycles after 100 warm-ups each; traced recorded 500
  of them. Floor: 1,000 cycles of each mode after 100. Every item recorded no error, every traced call was cut whole,
  and no trace event was lost.

### 1. The floor under exec, against a quarter of libcrun's exec

p50 in ms. Each round's floor ran right beside its libcrun lifecycle.

| Round | libcrun exec | Target (a quarter) | Floor | Floor over target | spawn | clone | cgroup | spin |
|---|---|---|---|---|---|---|---|---|
| r1 | 2.221 | 0.555 | 0.463 | 0.83 | 0.381 | 0.459 | 0.458 | 0.105 |
| r2 | 2.045 | 0.511 | 0.411 | 0.80 | 0.323 | 0.399 | 0.402 | 0.137 |
| r3 | 2.183 | 0.546 | 0.409 | 0.75 | 0.319 | 0.392 | 0.398 | 0.155 |

Best of 3: libcrun's exec 2.045 ms, so the target is 0.511 ms; the floor is 0.409 ms, 0.80x the target. Floor p99:
2.548, 0.660 and 0.616 ms; max 3.250, 17.152 and 0.869 ms. r1's floor ran at load 13.7, with some task waiting for a
CPU 16% of the time (`/proc/pressure/cpu`).

- **The target stands on fedora.** In every round the floor is under a quarter of the libcrun exec beside it, by 17 to
  25%.
- **That's the margin step 2 works in:** 0.09 to 0.14 ms over the floor for everything an exec through the seam must
  do beyond it (find the box, become the container's process, report the end).
- **The floor's own parts:** a fork-style clone costs 0.07-0.08 ms more than `posix_spawn`'s vfork-style one (spawn
  against clone). Birth in the container's cgroup adds at most 0.006 ms (clone against cgroup). The namespaces, four
  joined by the child and the pid namespace by the parent, add 0.005 to 0.011 ms (cgroup against floor).

The day's earlier passes, before this one's design was settled, met the host in a slower state. Within each, the
floor stayed under a quarter of libcrun's exec:

| Pass | n | libcrun exec p50 | Floor p50 | Floor over target | The host |
|---|---|---|---|---|---|
| `exec-breakdown-20261028T014541Z` (trial) | 50 | 2.139 | 0.325 | 0.61 | load 4 |
| `exec-breakdown-20261028T014653Z` r1 | 1,000 | 3.938 | 0.821 | 0.83 | load 4, slow state |
| `exec-breakdown-20261028T014653Z` r2 | 1,000 | 3.754 | 0.823 | 0.88 | load 3, slow state |
| `exec-breakdown-20261028T015457Z` (trial) | 50 | 3.307 | 0.790 | 0.96 | load 0.6, slow state |

In the slow state every part of a call took about twice as long, user-space ones included: the helper reading and
parsing `config.json` took 0.086 ms in this run's first traced round against 0.046-0.047 in the other two. That holds
on the same kind of core: 80% of the slow trial's trace events were on E-cores, against 98% of a fast trial's. The
floor slowed 2.0-2.5x there and libcrun's exec 1.5-1.8x, so the margin narrowed to 4% at worst. The slow state came
with the host otherwise idle (load 0.6, its I/O stalled). This run's first traced round met it, at 2.96 ms against
1.43 in the other two. No floor item met it, so the `spin` gauge doesn't say what it is.

### 2. Exec through the seam, in parts

p50 in ms, by traced round. r1 met the slow state; r2 and r3 the loaded host that followed.

| Group (its parts' p50s, summed) | r1 | r2 | r3 |
|---|---|---|---|
| the node's bookkeeping | 0.136 | 0.074 | 0.068 |
| helper spawn and re-exec | 0.838 | 0.427 | 0.435 |
| request and config | 0.086 | 0.047 | 0.046 |
| namespace joins | 0.208 | 0.104 | 0.097 |
| the clone | 0.210 | 0.089 | 0.087 |
| the child's set-up | 0.109 | 0.049 | 0.063 |
| the program's exec | 0.782 | 0.366 | 0.369 |
| the wait | 0.475 | 0.191 | 0.206 |

**Every part is timed, and the parts sum to the measured p50 within 10% in each round:**

| Round | Parts' p50s, summed | The calls between, p50 | Sum over it | The traced calls as timed, p50 | Sum over it | Recording's cost |
|---|---|---|---|---|---|---|
| r1 | 2.844 | 2.851 | 0.997 | 2.962 | 0.960 | 1.039x |
| r2 | 1.347 | 1.411 | 0.955 | 1.432 | 0.941 | 1.015x |
| r3 | 1.371 | 1.422 | 0.964 | 1.430 | 0.959 | 1.006x |

The untraced seam rounds measured exec at p50 1.342, 1.298 and 1.413 ms. The parts, p50 in ms:

| Part | Group | r1 | r2 | r3 |
|---|---|---|---|---|
| the call reaches the node; the box's record opened | the node's bookkeeping | 0.012 | 0.006 | 0.005 |
| the record read and decoded; the container's state opened | the node's bookkeeping | 0.026 | 0.013 | 0.012 |
| the state read and parsed; the init's stat opened | the node's bookkeeping | 0.020 | 0.009 | 0.008 |
| the init checked alive; the helper's request made; its stdio's first /dev/null opened | the node's bookkeeping | 0.046 | 0.025 | 0.024 |
| the rest of its stdio opened; the spawn prepared | the node's bookkeeping | 0.032 | 0.021 | 0.019 |
| the helper's clone (vfork), until the child runs | helper spawn and re-exec | 0.123 | 0.059 | 0.076 |
| the child's stdio set; the helper's execve (kernel) | helper spawn and re-exec | 0.222 | 0.110 | 0.109 |
| the helper's binary starts: relocated, its pages faulted in, libc's start-up | helper spawn and re-exec | 0.303 | 0.162 | 0.157 |
| Rust's start-up; the request read and parsed; config.json opened | helper spawn and re-exec | 0.190 | 0.096 | 0.093 |
| config.json read and parsed; the container's cgroup opened | request and config | 0.086 | 0.047 | 0.046 |
| the cgroup's procs and the init's namespaces opened, each compared with the helper's | namespace joins | 0.183 | 0.091 | 0.085 |
| the namespaces joined (setns) | namespace joins | 0.025 | 0.013 | 0.012 |
| the signals forwarded; the lifeline made | the clone | 0.021 | 0.012 | 0.011 |
| the clone into the container's cgroup (kernel) | the clone | 0.100 | 0.056 | 0.054 |
| the child first runs | the clone | 0.089 | 0.021 | 0.022 |
| the child becomes the container's process (user, capabilities, lifeline) | the child's set-up | 0.109 | 0.049 | 0.063 |
| the program resolved; its execve (kernel) | the program's exec | 0.344 | 0.151 | 0.147 |
| the program runs: the dynamic loader, libc, main | the program's exec | 0.430 | 0.210 | 0.217 |
| the program's exit (kernel, to its exit event) | the program's exec | 0.008 | 0.005 | 0.005 |
| its teardown, and the helper's poll wakes | the wait | 0.148 | 0.062 | 0.079 |
| the helper reaps it | the wait | 0.022 | 0.009 | 0.011 |
| the helper ends itself as the program did | the wait | 0.047 | 0.021 | 0.021 |
| the helper's exit (kernel, to its exit event) | the wait | 0.008 | 0.004 | 0.004 |
| its teardown, and the node's poll wakes | the wait | 0.207 | 0.076 | 0.074 |
| the node reaps the helper | the wait | 0.033 | 0.014 | 0.013 |
| the node returns the exit | the wait | 0.010 | 0.005 | 0.004 |

What it shows, from r2 and r3:

- **Hex's exec, 1.41-1.42 ms at p50 (the traced calls' untraced twins), is 3.4x the floor's 0.41.**
- **The helper process accounts for about half of it, 0.68 ms.** That's its spawn and re-exec (0.43), its read and
  parse of `config.json` (0.047), its opens and comparisons of the container's namespaces (0.09), and the hop its own
  end adds between the program's end and the node's (0.11: its reap of the program, its status, its exit, its
  teardown and the node's wake). Plan step 2's seed removes the helper. The node still waits for the program's end
  and reaps it.
- **The largest single part of the re-exec is the static-pie binary's start-up before its first system call,
  0.16 ms**, its relocations and first page faults. Hex is 14.7 MB; `true`'s whole run takes 0.21 ms.
- **The namespace joins themselves cost 0.012 ms** for five `setns` calls. Finding the namespaces costs seven times
  that: opening the cgroup's procs and each of the init's seven namespaces, and comparing each with the helper's own.
- **The program's exec and run take 0.37 ms** (its execve 0.15, the loader, libc and `main` 0.21), against the
  floor's whole cycle of 0.41. One difference between them: Hex's program is a fork of the 14.7 MB helper, whose
  address space its execve tears down, and the floor's is a fork of a 22 KB probe.

### 3. Start through the seam, in parts

p50 in ms, by traced round.

| Group (its parts' p50s, summed) | r1 | r2 | r3 |
|---|---|---|---|
| state reads | 0.092 | 0.051 | 0.050 |
| the FIFO | 0.099 | 0.051 | 0.055 |
| state write | 0.223 | 0.128 | 0.116 |
| hooks' config read | 0.064 | 0.036 | 0.035 |

| Round | Parts' p50s, summed | The calls between, p50 | Sum over it | The traced calls as timed, p50 | Sum over it | Recording's cost |
|---|---|---|---|---|---|---|
| r1 | 0.478 | 0.465 | 1.027 | 0.484 | 0.988 | 1.040x |
| r2 | 0.266 | 0.249 | 1.069 | 0.258 | 1.032 | 1.036x |
| r3 | 0.256 | 0.241 | 1.064 | 0.252 | 1.017 | 1.047x |

The untraced seam rounds measured start at p50 0.287, 0.261 and 0.260 ms; libcrun's start was 0.111, 0.101 and
0.099. The parts, p50 in ms:

| Part | Group | r1 | r2 | r3 |
|---|---|---|---|---|
| the call reaches the node; the box's record opened | state reads | 0.013 | 0.007 | 0.007 |
| the record read and decoded; the container's state opened | state reads | 0.017 | 0.009 | 0.009 |
| the state read and parsed, its init checked (the node's); the state opened again | state reads | 0.032 | 0.018 | 0.017 |
| the state read and parsed, its init checked again (the oci runtime's start); the FIFO opened | state reads | 0.030 | 0.017 | 0.017 |
| the init, blocked opening the FIFO, wakes and writes its byte; the poll returns | the FIFO | 0.099 | 0.051 | 0.055 |
| the byte read; the state encoded as running; its temp file made | state write | 0.095 | 0.060 | 0.054 |
| the state written; renamed into place | state write | 0.128 | 0.068 | 0.062 |
| config.json opened for the poststart hooks | hooks' config read | 0.015 | 0.008 | 0.008 |
| config.json read and parsed; the call returns | hooks' config read | 0.049 | 0.028 | 0.027 |

- **Half of Hex's start is writing `state.json` as running:** 0.12-0.13 ms of 0.25-0.27, the temp file and its
  rename. libcrun's start writes no state (plan step 3).
- **The FIFO itself, which both runtimes do, is 0.05 ms:** the open that wakes the init, and its byte.
- **The container's state is read twice,** once by the node to check it's created and once by the oci runtime's
  start, 0.017-0.018 ms each, and `config.json` is read and parsed for poststart hooks the bundle doesn't have
  (0.035 ms).
- **Plan step 3 removes the state write and the second read, about 0.14 ms.** About 0.12 ms is left, against
  libcrun's 0.10.
