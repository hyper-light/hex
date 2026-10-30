# Where the seam's create spends its time

Goal 5 of the runtime design (decision 5 of `docs/design/runtime-architecture.md`; kiln plan
`01a1877e05c7748ca8865548aab76445`, step 1) is judged by one ratio read from one run: Hex's create p50 through the
`Runtime` seam at most a quarter of libcrun's in-process create p50 (and create+start of `true` likewise). Before
its levers are built, this probe measures where a seam create spends its time today, part by part, beside libcrun's
create in the same run, so each lever's share is measured rather than guessed: the box's record off create's path
(plan step 2), the seed in place of the shim's second exec of the hex binary (plan steps 3 and 4), and a pool of
cgroups made ahead (plan step 5), whose step this probe decides.

## How it runs

`create_breakdown.sh <hex> <seam_oci>` copies every binary into the run's directory and runs
`create_breakdown_inside.sh` in the bench container with host.sh's OCI-pass flags (privileged, a private cgroup
namespace, no network, pinned to CPUs 4-31, the host's `/usr`), on the OCI pass's bundles. Inside, the cgroup root is
vacated to a leaf and its controllers (cpu, memory, pids, io) handed down, as `oci_inside.sh` leaves it, so each box's
cgroup is made with its controllers and bounds as in the pass of record: the kiln run's own cgroup on fedora has no
controllers, and a cgroup made there costs what one without them does. After a small probe of each item, each round
runs these in a shuffled order:

- **libcrun**: `oci_bench.py`'s lifecycle phase on libcrun in process (create, start, exec of `true` and delete of a
  `sleep` container, each timed alone). Its create p50 is what goal 5's target is a quarter of.
- **seam**: `seam_oci`'s lifecycle phase, the `oci` class through the seam in process: Hex's measured create p50.
- **traced**: the same with the breakdown's tracer on around create.

`create_breakdown.py summarize` then writes `summary.md` and `summary.json`. `check` holds the probes to reading
whole: no recorded error, no lost trace event, every traced create cut into its parts, every positional cut the file
it stands for.

The node's state is where `seam_oci` keeps it in the OCI pass, under `$HOME/.cache/hex-t7/ociseam` in the container:
the container's overlay, whose upper layer is on fedora's btrfs. Its records are written as `crates/hex-node/src/
store.rs` writes them today: an oci box's record names the container's state directory, so it is fsynced, file and
directory, at each of its two writes in a create (kiln research `01a1877e05cc73b495447bf5113ebda3`, f4).

### How a create is cut into parts

The tracer is an ftrace instance of the run's own (`/sys/kernel/tracing/instances/hexcb-<pid>`), on
`CLOCK_MONOTONIC` (`trace_clock` mono), its events kept only for the run's processes' names: `seam_oci` (the node
agent, and the child its spawn clones until that execs) and `hex` (the shim, and the container's init it clones). With
`HEXBENCH_TRACE_OPS=create`, `seam_oci`'s `HEXBENCH_TRACE` brackets the create of every other timed lifecycle with
`trace_marker` lines, `hexbd B create <id>` and `hexbd E create <id> <ns>`, and turns the instance's recording on only
between them. The lifecycles between run untraced in the same process: each traced create's twins met the host as it
did.

A part runs from one event of the create's critical path to the next. The cuts are `create_cuts` in
`create_breakdown.py`, from the marker before the call to the one after, so a create's parts cover it whole. They
follow the call across the node agent (`crates/hex-node/src/node.rs`, `create_oci`; `store.rs`), the oci runtime
(`crates/hex-runtime/src/linux/oci/run.rs`, `create_in_cgroup`), the shim it spawns (`hex __oci-init`,
`oci/init.rs`'s `shim`) and the container's init the shim clones into the box's cgroup (`init.rs`'s `container`), up to
its ready signal, the agent's read of it and the record's second write. The cuts are system calls whose count in a
create is fixed: the record store's four `fsync`s, two `mkdir`s (the box's cgroup, then the container's state
directory, each held to its path, which the kernel fetches with every `mkdir` entry), the FIFO's `mknodat`, the device
program's first `bpf` call (held to `BPF_PROG_LOAD`) and its last before the spawn (held to `BPF_PROG_ATTACH`), the
spawn's `clone3`, the shim's exec, its first `poll` (Rust's start-up checks its stdio), its `setns` into the
namespace made ahead (held to `CLONE_NEWNET`), its `clone3` of the init, the init's `pivot_root` and its first `write`
(held to the two bytes of its ready signal), the agent's reaping of the shim (`wait4`) and the rename of the
container's state. Six cuts are opens told by their place: the node's first `openat` (the box's record, looked for),
the last before each record write's `fsync` (the record's temp file), the shim's last `openat` before its first
`open` (`config.json`), and the agent's first after the ready signal and second after the state's rename (the init's
`/proc/<pid>/stat`). The probe step's detailed trace records the opens' entries with their paths, and `check` holds
every such cut to the file it stands for.

The parts fall in six groups, as decision 5 weighs them: **the box's record** (its two durable writes), **the
cgroup, made and bounded** (the box root listed for its count of boxes, the `mkdir`, the bounds written), **the shim's
spawn and exec** (the clone, the hex binary's exec and start-up, to its `config.json`), **the init's set-up, to the
exec FIFO** (the spec read, the namespaces joined and made, the init cloned into its cgroup, its root mounted and
pivoted, the process become, its ready signal), **the handshake** (the agent woken by the ready signal, the init's
start time read, the shim reaped), and **the node's and the runtime's own work** (the call's checks, the container's
state directory, `config.json` and the device plan, the FIFO, the device program loaded and attached, the pipes, the
namespace taken from the pool, `state.json` written and read back, the init watched).

Only events that cost a call little are traced: an open's entry fetches its path from user memory, about 4 µs an event
(`exec_breakdown.md`), so the rounds trace the opens' exits and tell an open by its place. rustix opens with
`open(2)` on x86_64, std with `openat(2)`, so both are traced. Percentiles are nearest-rank. tracefs prints the clock to
the microsecond.

## Run 1 (2026-10-30, fedora): of record for plan step 1

- **Host:** the "fedora" kiln runner (`kiln-a094a12d`, hostname `fedora-2`), Intel Core i9-14900HX (32 CPUs: P-core
  threads 0-15, E-cores 16-31; the container pinned to 4-31), kernel 6.19.10-300.fc44.x86_64.
- **When and load:** 2026-10-30T00:08:22Z to 00:12:35Z. The 1-minute load was 7.17 at the start and 6.85 at the end,
  from other work on the host; some task was stalled on CPU 0.20% and 0.63% of the last 10 s at the start and end
  (`/proc/pressure/cpu`, some, avg10), on I/O 1.16% and 3.41%. Each item's load and pressure as it began are in
  `probe.out`. `LOAD_GATE=0`: by Ada's rule a busy host is the default, and the comparisons are within this run (each
  traced create against its untraced twins, Hex's create against libcrun's beside it). The host's clock
  (`probes/host_clock.sh`, before and after the run): a burst took 0.238 and 0.223 ms at rest on a P-core against 0.114
  held awake, so the run met the host in its at-rest state, where short calls run at about half the speed (`exec_ab.md`,
  "The host's state").
- **What else ran:** just before the timed run (about 00:08Z), no other bench was going on fedora. There was no
  hexbench or hexpass container: `docker ps`, with Focal's `s5-focal*` test containers left out of the listing, showed
  four containers up for 18 hours, issue556oss-control-plane, buildx_buildkit_kagent-builder-v0.23.00, kind-registry
  and kiln-t10-minio. There was no host.sh, seam_oci, oci_bench, create_breakdown, exec_breakdown, docker_pass, cargo or
  rustc process, and the 1-minute load was 10.54. Kiln's queue there wasn't read. The functional run before it (00:07Z,
  not timed) had no such check.
- **Binaries:** `hex 0.1.0 (ef64dbc)`, sha256 596969cb3a612644, static-pie, built in the Fedora 44 pass image against
  its glibc-static. `seam_oci` of ef64dbc with this probe's `HEXBENCH_TRACE_OPS`, sha256 0656cae0a8464d57. libcrun
  1.30.1 (079ff6a7) in process, driver sha256 5c356f2139adc232.
- **The node's state:** under `/root/.cache/hex-t7/ociseam` in the container, on the container's overlay, whose
  layers are on fedora's btrfs (`/dev/nvme1n1p3`, `compress=zstd:1`, as Docker's root). The cgroup root's controllers:
  cpu, io, memory and pids.
- **The bench's `/run` was overlayfs:** the container's `/run` is the overlay too (only `/dev/shm` is a tmpfs there,
  as goal 5's step 2 found), and `oci_bench.py` keeps libcrun's state in `/run/libcrun` (`oci_bench.py:70`). So both
  runtimes' state sat on overlayfs, where a host's `/run` is a tmpfs: today's code in today's container, the baseline.
  Step 2 mounts a tmpfs at `/run` in host.sh's and docker_pass.sh's containers, so a pass matches a real host.
- **Run:** `~/.cache/hex-bench/out/create-breakdown-20261030T000822Z` on fedora, the harness at ef64dbc with this
  probe's changes. 3 rounds of 1,000 timed lifecycles after 100 untimed, each round's libcrun, seam and traced runs in
  a shuffled order. 1,500 traced creates, each cut into its 35 parts; no trace event lost, no line unread. The probe
  step's 10 detailed creates held every positional cut to its file, as the functional run before it did
  (`create-breakdown-20261030T000701Z`).

### Create p50 (ms)

`traced: recording` is the traced runs' creates the tracer recorded, every other one; `traced: between` the creates
between them, in the same process with the tracer's events armed.

| | r1 | r2 | r3 | best | pooled p50 | pooled p99 |
|---|---|---|---|---|---|---|
| libcrun | 5.055 | 4.814 | 5.183 | 4.814 | 5.015 | 6.951 |
| seam | 17.915 | 18.220 | 17.760 | 17.760 | 18.019 | 32.993 |
| traced: recording | 18.573 | 18.470 | 18.758 | 18.470 | 18.624 | 29.057 |
| traced: between | 18.185 | 17.997 | 18.573 | 17.997 | 18.297 | 27.884 |

Each round's tail, its p99.9 and max (ms): seam 36.240 and 48.150, 36.007 and 36.919, 55.114 and 62.278; libcrun
7.400 and 7.667, 14.212 and 17.143, 7.376 and 7.446.

### Create through the seam, in parts (1,500 traced creates)

| # | part | group | r1 p50 | r2 p50 | r3 p50 | pooled p50 | pooled p99 | pooled mean |
|---|---|---|---|---|---|---|---|---|
| 1 | the call reaches the node; the spec checked; the box's record looked for | the node's and the runtime's own work | 0.024 | 0.023 | 0.023 | 0.023 | 0.041 | 0.023 |
| 2 | its cgroup looked for; a watch slot reserved; the record encoded, its temp file made | the box's record | 0.099 | 0.096 | 0.095 | 0.097 | 0.152 | 0.095 |
| 3 | the record written | the box's record | 0.023 | 0.022 | 0.023 | 0.023 | 0.043 | 0.023 |
| 4 | the record's fsync (kernel) | the box's record | 3.001 | 2.998 | 3.143 | 3.041 | 5.719 | 3.245 |
| 5 | the record closed and renamed into place; the store's directory opened | the box's record | 0.200 | 0.189 | 0.192 | 0.194 | 0.355 | 0.198 |
| 6 | the directory's fsync (kernel) | the box's record | 2.988 | 2.953 | 3.031 | 2.987 | 6.333 | 3.285 |
| 7 | the box root listed, its boxes counted | the cgroup, made and bounded | 0.094 | 0.091 | 0.096 | 0.094 | 0.147 | 0.095 |
| 8 | the cgroup made (kernel) | the cgroup, made and bounded | 0.282 | 0.271 | 0.279 | 0.278 | 0.661 | 0.291 |
| 9 | its bounds written: memory.max, memory.swap.max, pids.max | the cgroup, made and bounded | 0.101 | 0.101 | 0.099 | 0.101 | 0.195 | 0.102 |
| 10 | the container's state directory made; config.json read and parsed; its device policy planned | the node's and the runtime's own work | 0.174 | 0.168 | 0.166 | 0.169 | 0.260 | 0.166 |
| 11 | the exec FIFO made; the device program's instructions made | the node's and the runtime's own work | 0.067 | 0.066 | 0.066 | 0.067 | 0.106 | 0.066 |
| 12 | the device program loaded (kernel: the verifier) | the node's and the runtime's own work | 0.496 | 0.484 | 0.492 | 0.491 | 0.691 | 0.479 |
| 13 | its identity read; the hints read, the program they name looked for, the hints written; attached | the node's and the runtime's own work | 0.095 | 0.091 | 0.092 | 0.092 | 0.943 | 0.124 |
| 14 | the pipes made; a namespace made ahead taken; the shim's spawn prepared | the node's and the runtime's own work | 0.071 | 0.069 | 0.070 | 0.070 | 0.109 | 0.070 |
| 15 | the shim's clone (vfork, kernel), until its child runs | the shim's spawn and exec | 0.085 | 0.083 | 0.086 | 0.084 | 0.199 | 0.092 |
| 16 | the child's set-up; the hex binary's execve (kernel) | the shim's spawn and exec | 0.259 | 0.254 | 0.255 | 0.256 | 0.421 | 0.248 |
| 17 | the hex binary starts: relocated, its pages faulted in, libc's start-up | the shim's spawn and exec | 0.357 | 0.350 | 0.353 | 0.354 | 0.494 | 0.339 |
| 18 | Rust's start-up; the shim reads its environment; config.json opened | the shim's spawn and exec | 0.187 | 0.181 | 0.184 | 0.184 | 0.268 | 0.176 |
| 19 | config.json read and parsed; the cgroup, its procs and the namespace made ahead opened | the init's set-up, to the exec FIFO | 0.185 | 0.185 | 0.185 | 0.185 | 0.272 | 0.176 |
| 20 | the namespace made ahead joined (setns, kernel) | the init's set-up, to the exec FIFO | 0.007 | 0.007 | 0.007 | 0.007 | 0.011 | 0.007 |
| 21 | the container's other namespaces made: pid, mount, ipc, uts (unshare) | the init's set-up, to the exec FIFO | 0.181 | 0.175 | 0.178 | 0.178 | 0.721 | 0.180 |
| 22 | the init's clone into its cgroup (kernel), until it runs | the init's set-up, to the exec FIFO | 0.167 | 0.163 | 0.162 | 0.164 | 0.607 | 0.180 |
| 23 | the init's root mounted: the rootfs bound, /usr, /proc and /dev | the init's set-up, to the exec FIFO | 0.972 | 0.959 | 0.964 | 0.966 | 1.935 | 0.930 |
| 24 | pivot_root (kernel) | the init's set-up, to the exec FIFO | 0.885 | 0.863 | 0.877 | 0.873 | 1.101 | 0.827 |
| 25 | the old root unmounted; the hostname set; the process become (capabilities, user); ready sent | the init's set-up, to the exec FIFO | 0.137 | 0.133 | 0.134 | 0.135 | 0.414 | 0.155 |
| 26 | the agent wakes to the ready signal and reads it; the init's stat opened | the handshake | 0.072 | 0.075 | 0.075 | 0.074 | 0.174 | 0.082 |
| 27 | the init's start time read; the shim reaped | the handshake | 0.069 | 0.068 | 0.068 | 0.068 | 0.101 | 0.066 |
| 28 | the container's state written (state.json, renamed into place) | the node's and the runtime's own work | 0.198 | 0.195 | 0.196 | 0.196 | 0.283 | 0.191 |
| 29 | the state read back (state.json); the init's stat opened | the node's and the runtime's own work | 0.064 | 0.064 | 0.062 | 0.063 | 0.097 | 0.062 |
| 30 | the init checked; the record encoded, its temp file made | the box's record | 0.090 | 0.090 | 0.087 | 0.089 | 0.131 | 0.086 |
| 31 | the record written | the box's record | 0.021 | 0.021 | 0.021 | 0.021 | 0.039 | 0.020 |
| 32 | the record's fsync (kernel) | the box's record | 3.154 | 3.110 | 3.206 | 3.158 | 6.048 | 3.268 |
| 33 | the record closed and renamed into place; the store's directory opened | the box's record | 0.232 | 0.230 | 0.223 | 0.230 | 0.416 | 0.232 |
| 34 | the directory's fsync (kernel) | the box's record | 2.977 | 2.989 | 3.028 | 3.001 | 6.203 | 3.305 |
| 35 | the init watched (its pidfd, its stat checked, the epoll set); the box's info made; the call returns | the node's and the runtime's own work | 0.197 | 0.194 | 0.193 | 0.194 | 0.302 | 0.192 |

| group, its parts' p50s summed | r1 | r2 | r3 | pooled | its share of the pooled sum | pooled means |
|---|---|---|---|---|---|---|
| the box's record | 12.785 | 12.698 | 13.049 | 12.841 | 70.5% | 13.757 |
| the cgroup, made and bounded | 0.477 | 0.463 | 0.474 | 0.473 | 2.6% | 0.488 |
| the shim's spawn and exec | 0.888 | 0.868 | 0.878 | 0.878 | 4.8% | 0.855 |
| the init's set-up, to the exec FIFO | 2.534 | 2.485 | 2.507 | 2.508 | 13.8% | 2.454 |
| the handshake | 0.141 | 0.143 | 0.143 | 0.142 | 0.8% | 0.148 |
| the node's and the runtime's own work | 1.386 | 1.354 | 1.360 | 1.365 | 7.5% | 1.373 |

| round | the parts' p50s, summed | the creates between, p50 | the sum over it | the traced creates as timed, p50 | the sum over it | recording's cost |
|---|---|---|---|---|---|---|
| r1 | 18.211 | 18.185 | 1.001 | 18.573 | 0.981 | 1.021x |
| r2 | 18.011 | 17.997 | 1.001 | 18.470 | 0.975 | 1.026x |
| r3 | 18.411 | 18.573 | 0.991 | 18.758 | 0.981 | 1.010x |

Pooled, the parts' p50s sum to 18.207 ms, and their means to 19.076 ms, the creates' mean span from marker to marker
(19.076 ms). Against the creates between the traced ones, p50 18.297 ms, the sum is 0.995; against the traced creates
as seam_oci timed them, p50 18.624 ms, 0.978; against the untraced seam rounds, p50 18.019 ms pooled, 1.010.
Recording cost the traced creates 1.018x at p50.

**Plan step 1's criterion,** the parts summed within 10% of create's p50 in the same run: 99.5% of the creates
between, met; 100.1%, 100.1% and 99.1% in each round.

### What each lever takes (pooled part p50s, against the creates between, p50 18.297 ms)

| lever | its parts | ms | share of create |
|---|---|---|---|
| the record off create's path (plan step 2) | the record's four fsyncs | 12.187 | 66.6% |
| | the box's record, whole | 12.841 | 70.2% |
| the seed (plan steps 3 and 4) | the shim's spawn and exec | 0.878 | 4.8% |
| a pool of cgroups made ahead (plan step 5) | the cgroup, made and bounded | 0.473 | 2.6% |
| | of it, the making: the box root listed and the mkdir | 0.372 | 2.0% |

Once the record's fsyncs and the shim's spawn and exec are gone, the parts left sum to 5.142 ms (before whatever a
seed's clone costs in the shim's place); the cgroup, made and bounded, is 0.092 of that, its making alone 0.072. Goal
5's target, a quarter of libcrun's create p50 (5.015 ms pooled), is 1.254 ms: what's left is 4.101x it.

### What follows from it

Each durable record write's two fsyncs, the file's and the directory's, took 6.028 ms (the first write) and 6.159 ms
(the second) at p50 on the overlay, against 7.138 and 7.481 ms for the same pair on fedora's `/home` in the fsync probe
(f6). What's left once the record's fsyncs and the shim's spawn and exec are gone, 5.142 ms, is 1.025 of libcrun's
create in the same run, part by part (its share of the 5.142 ms):

| what's left | parts | ms | share |
|---|---|---|---|
| the init's root: mounted, then pivot_root | 23, 24 | 1.839 | 35.8% |
| the init's other set-up: the shim's config read, the namespaces joined and made, the init's clone, the process become | 19, 20, 21, 22, 25 | 0.669 | 13.0% |
| the record's temp files, writes and renames, without their fsyncs | 2, 3, 5, 30, 31, 33 | 0.654 | 12.7% |
| the device program, loaded and attached | 12, 13 | 0.583 | 11.3% |
| the node's and the runtime's other work | 1, 10, 11, 14, 35 | 0.523 | 10.2% |
| the cgroup, made and bounded | 7, 8, 9 | 0.473 | 9.2% |
| the container's state written (state.json) and read back | 28, 29 | 0.259 | 5.0% |
| the handshake | 26, 27 | 0.142 | 2.8% |

Take away as well the init's root, the device program, the record's and the state's writes and the cgroup's making,
each whole, and 1.435 ms is left: 1.145 of the target. A seed's serial work per create, what today's shim does after
its exec up to the init's first run (parts 19, 21 and 22: the spec read and parsed, the namespaces made, the clone into
the cgroup), is 0.527 ms at p50; the init's own set-up after that clone (parts 23 to 25) is 1.974 ms, which runs in the
init, beside a seed's next clone.

pivot_root's 0.873 ms is the kernel's walk over every thread on the host, which it makes to move each one whose root or
working directory is the old root: `pivot_root` calls `chroot_fs_refs` (Linux v6.19 `fs/namespace.c:4526,4606`),
which takes `read_lock(&tasklist_lock)` and runs `for_each_process_thread(g, p)` (`fs/fs_struct.c:60-61`). The host had
3,310 to 3,752 threads during the run (`/proc/loadavg`'s count at its start and end), so the part grows with what else
runs on a node, its own containers included. libcrun's create makes the same call: `ret = pivot_root (".", ".");`
(crun 1.30.1, `src/libcrun/linux.c:2567` at 079ff6a7), the bench's driver leaving `no_pivot` unset
(`bench/competitive/libcrun/driver.c:79-91`).

## Run 2 (2026-10-30, fedora): a repeat with the pre-run check, not of record

- **Why:** run on the lead's word, which reached me after run 1 and had crossed with the slot going to step 2.
  Decision 5 rests on run 1.
- **The check before each run:** the functional run `create-breakdown-20261030T004109Z` at 00:41:09Z, then the timed
  run at 00:41:37Z.
  - Kiln showed fedora's 8 slots idle (`ci.runners.list`, at 00:40:58Z and 00:41:27Z).
  - `docker ps`, every container listed, showed the same four long-lived ones as before: no hexbench, hexpass or
    Focal `s5-focal*` container.
  - Kiln's own builds ran beside it: a traced `cargo build` of kiln, a `cargo test -p kiln --no-run`, and rustc of
    amphora_engine and turso_core.
  - The 1-minute load was 9.66, and CPU and I/O stalled 0.08% and 0.24% of the last 10 s (`/proc/pressure`, some,
    avg10).
- **The overlap:** step 2's pass, handed fedora's slot meanwhile, ran beside it from 00:42:30Z to about 00:44Z. It made a
  niced static release build of hex on all cores (00:42:30-00:42:55Z), ran host_clock.sh (to 00:42:58Z) and fetch.py (to
  about 00:44Z), and stopped before any bench of its own. This run went from 00:41:40Z to about 00:44:55Z, so the overlap
  falls in its first two rounds; the third ran clear of it.
- **Binaries:** run 1's. hex sha256 596969cb3a612644; seam_oci rebuilt at 9ff1ddb came out byte for byte run 1's
  (0656cae0a8464d57).
- **What it shows beside run 1:** the parts summed to 102.3% of the p50 of the creates between the traced ones, so the
  step's criterion was met again. The host ran held awake by the builds beside it: a burst took 0.095 and 0.107 ms at
  rest on a P-core (host_clock.sh before and after), against run 1's 0.238 and 0.223. Its tables aren't quoted here;
  they are in its run directory on fedora, `~/.cache/hex-bench/out/create-breakdown-20261030T004140Z`.
