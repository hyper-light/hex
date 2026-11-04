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

**The seed path** (goal 5 plan step 4, from 72812eb on): a seam create no longer spawns a shim. The node agent hands a
seed (`hex __oci-seed`, `crates/hex-node/src/createpool.rs`) the request over its socket, and the seed clones the init
straight into the box's cgroup and the namespaces the spec makes (`oci/seed.rs`'s `serve_create`). The init then joins
the network namespace made ahead itself and sets up its root as before (`init.rs`'s `clone_child`). `create_cuts` takes
that path when no create forks from the agent (`ops_for`), and cuts it into 30 parts: the shim path's first 13 (to the
device program's attach) and its last 6 (from the state's read-back), and between them the request sent and the seed's
open of the box's cgroup, the seed's `clone3`, the init's first run, its `setns` (held to `CLONE_NEWNET`), its root
mounted, its `pivot_root`, its ready signal, the agent's read of it and the state written. The seed is told by its fork:
the first after the device program's load by a process other than the agent whose child goes on to `pivot_root`. Its
open of the box's cgroup is held to that cgroup's path by the detailed trace. With no shim there is no `wait4`, and the
namespaces the shim made by `unshare` are made by the seed's clone, so they fall in the init's clone. The parts' groups
are the shim path's, with **the seed: the request and its clone's preparing** in place of the shim's spawn and exec.

**The device program held** (goal 5 extended, step 1, from cp/t7-g5x-s1 on): the node keeps each policy's program
open once loaded, and on a root node the default policy's sits on the oci boxes' root, so a create loads only a policy
the node holds no program for, and a box whose policy is the default's attaches nothing. The trace adds the agent's
`pipe2` entries: a create's first pipe follows its device program, so the program's parts (11 to 13: the FIFO and
what follows it, the load, the attach) are cut between the FIFO's `mknod` and that pipe, a part with no call empty,
and the seed's fork is the first after them.

**A binary from before a change beside it** (from the same step): with `BEFORE_HEX` and `BEFORE_SEAM` set,
`create_breakdown.sh` stages them too, and every round runs their seam lifecycle (`before`) in its shuffle with the
others, so the change's ratio, the seam's create p50 over the before binary's, is each run's own and its rounds meet
the same host. `summarize` prints it by round and pooled.

**The record written once** (goal 5 extended, step 5, from cp/t7-ext-s5 on): a create writes the box's record once,
when the box is whole, and the oci runtime hands the node the state it wrote, so the seed path loses its first record
write and the state's read back. `ops_for` takes that path when the agent renames nothing into place before the box
cgroup's `mkdir`, and cuts it into 25 parts: the box's record looked for in both of its places (the agent's first two
`openat`s, each of `<id>.json`); the cgroup looked for, a watch slot reserved and the box root counted; the seed path's
parts from the cgroup's `mkdir` to the state's rename; then the record's one write: encoded, up to its temp file's
open; the temp file made; written, closed and renamed into place; and, for a record that must outlive the host, its
fsyncs. Each open's entry is marked by a kprobe on the `openat` and `open` system calls' entry points that records
nothing but where it fired (`openat_in` and `open_in`, in a group of the run's own, `hexcb_<pid>`, defined for the run
and removed at its end), so the temp file's open is cut from its entry, and the open's path is fetched only in the
detailed trace, as before. `summarize` adds the step's measures: the node's bookkeeping, the state's write and the
record's (their pooled p50s summed), against run 4's 0.292 ms (its parts 2, 3 and 23 to 26); and a record write's
filesystem work, each create's temp file made, written, closed and renamed (and synced), p50 and p99. With a binary
from before the change staged (`BEFORE_HEX`, `BEFORE_SEAM`, as before/hex and before/seam_oci, the names the tracer
keeps events of), its seam rounds are traced too (`traced-before`), cut as their path is, so the bookkeeping is also
set against the before binary's in the same rounds: its record's parts and the state written and read back, as run 4's
were summed.

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
- **Binaries:** run 1's. hex sha256 596969cb3a612644; seam_oci rebuilt at 9ff1ddb (7d9afff on cp/t7-seed) came out
  byte for byte run 1's (0656cae0a8464d57).
- **What it shows beside run 1:** the parts summed to 102.3% of the p50 of the creates between the traced ones, so the
  step's criterion was met again. The host ran held awake by the builds beside it: a burst took 0.095 and 0.107 ms at
  rest on a P-core (host_clock.sh before and after), against run 1's 0.238 and 0.223. Its tables aren't quoted here;
  they are in its run directory on fedora, `~/.cache/hex-bench/out/create-breakdown-20261030T004140Z`.

## Run 3 (2026-10-30, fedora): after plan step 2, /run a tmpfs, not of record

- **Why:** the lead's interim look at a seam create once its record left the path (step 2), before the seed (step 4),
  for step 5's cgroup-pool decision and goal 5's bar. Step 5 decides on a run after step 4, with the seed's cuts.
- **What changed in the probe:** the bench container's `/run` is a tmpfs, as host.sh's OCI pass has it since 6f7ba39
  (285421a), so the node's records there aren't synced (decision 1). A record write with no fsync is cut from its temp
  file to its rename's return, and its three later parts read zero (3b69e95); the container's state directory is the
  node's runtime directory's `<node key>-<box id>` (`/run/hex-node/2413ff8f85d7f9d9-l000001`). Parts 4 to 6 and 32 to 34
  are zero, and parts 3 and 31 hold the close and the rename.
- **Host and load:** as run 1's. 2026-10-30T01:24:33Z to about 01:26:15Z. The 1-minute load was 10.65 at the start and
  16.17 at the end; some task was stalled on CPU 0.01% and 0.06% of the last 10 s, on I/O 1.30% and 0.72%. The host's
  clock (`probes/host_clock.sh`): a burst took 0.115 and 0.123 ms at rest on a P-core, against 0.104 and 0.122 held
  awake, so the host was held awake by its load.
- **What else ran:** 34 of Focal's `s5-focal*` fleet test containers, up 2 minutes at the start, and the four
  long-lived containers of run 1; at the end a kiln CI job's container as well. No other bench of Hex's ran: the run's
  check found no hexbench container up.
- **Binaries:** `hex 0.1.0` of 285421a, sha256 f6ba399a68131377, static-pie, built in the Fedora 44 pass image.
  `seam_oci` of 285421a, sha256 f4b118b10cb95af8. libcrun 1.30.1 (079ff6a7) in process, driver sha256 5c356f2139adc232.
- **Run:** `~/.cache/hex-bench/out/create-breakdown-20261030T012433Z` on fedora, the harness at 3b69e95. 3 rounds of
  1,000 timed lifecycles after 100 untimed; 1,500 traced creates cut into their 35 parts, no trace event lost, no line
  unread; the probe's 10 detailed creates held every positional cut to its file.

### Create p50 (ms)

| | r1 | r2 | r3 | best | pooled p50 | pooled p99 |
|---|---|---|---|---|---|---|
| libcrun | 4.243 | 4.549 | 3.407 | 3.407 | 4.004 | 7.046 |
| seam | 2.595 | 4.233 | 3.369 | 2.595 | 3.290 | 6.402 |
| traced: recording | 3.825 | 4.668 | 2.718 | 2.718 | 3.745 | 7.662 |
| traced: between | 3.738 | 4.533 | 2.630 | 2.630 | 3.630 | 7.464 |

Each round's tail, its p99.9 and max (ms): seam 4.359 and 12.817, 19.465 and 20.927, 6.884 and 23.900; libcrun 16.673
and 19.306, 21.779 and 22.634, 6.408 and 7.563.

The parts' p50s sum to 3.491 ms pooled: 96.2% of the creates between the traced ones (step 1's criterion met again).

### What's left once the shim's spawn and exec are gone (pooled part p50s)

The shim's spawn and exec (parts 15 to 18), which the seed replaces, took 0.508 ms. What's left sums to 2.983 ms:

| what's left | parts | ms | share |
|---|---|---|---|
| the init's root: mounted, then pivot_root | 23, 24 | 1.250 | 41.9% |
| the init's other set-up: the shim's config read, the namespaces joined and made, the init's clone, the process become | 19, 20, 21, 22, 25 | 0.404 | 13.5% |
| the device program, loaded and attached | 12, 13 | 0.356 | 11.9% |
| the record's temp files, writes and renames | 2, 3, 5, 30, 31, 33 | 0.293 | 9.8% |
| the cgroup, made and bounded | 7, 8, 9 | 0.282 | 9.5% |
| the node's and the runtime's other work | 1, 10, 11, 14, 35 | 0.224 | 7.5% |
| the container's state written (state.json) and read back | 28, 29 | 0.088 | 2.9% |
| the handshake | 26, 27 | 0.086 | 2.9% |

- **Step 5's measure:** the cgroup, made and bounded, is 0.095 of what's left, its making (parts 7 and 8) 0.075: under
  the tenth by this run, as by run 1 (0.092 and 0.072), though by less.
- **Goal 5's bar:** a quarter of libcrun's pooled create p50 (4.004 ms) is 1.001 ms; what's left is 2.980x it. Take
  away as well the init's root, the device program, the record's and the state's writes and the cgroup's making, each
  whole, and 0.771 ms is left, 0.770 of the bar, where run 1's same sum was 1.145 of it. The init's root alone is 1.250
  ms, more than the bar: pivot_root's 0.765 ms is its walk over the host's threads (3,669 at the start), as in run 1.
- **The seed's serial work** (parts 19, 21 and 22, what a seed does per create up to the init's first run) is 0.306 ms
  at p50, against run 1's 0.527.

## Run 4 (2026-10-31, fedora): the seed path, after plan step 4, not of record

- **Why:** the lead's baseline for goal 5's added steps (Ada's call, 2026-10-31): the device program loaded once per
  policy, the container's root without pivot_root's walk, and fewer `cgroup_mutex` holds per lifecycle. It is the
  first run of the seed path's cuts (88556f8).
- **Host and load:** as run 1's. 2026-10-31T06:18:41Z to about 06:20:05Z. The 1-minute load was 10.46 at the start and
  10.62 at the end; some task was stalled on CPU 0.10% of the last 10 s at the start, on I/O 2.26%. The host's clock: a
  burst took 0.095 and 0.130 ms at rest on a P-core, against 0.098 and 0.102 held awake.
- **What else ran:** Focal's `s5-focal*` fleet test containers, up about a minute at the start, and the long-lived
  ones of run 1. No other bench of Hex's ran: the run's check found no hexbench container up.
- **Binaries:** `hex 0.1.0` of 4bc8c11, sha256 6ff8dcbfa41a3ab4, static-pie, built in the Fedora 44 pass image;
  `seam_oci` of 4bc8c11, sha256 7f8f42dee4413ccd. libcrun 1.30.1 (079ff6a7) in process, driver sha256 5c356f2139adc232.
- **Run:** `~/.cache/hex-bench/out/create-breakdown-20261031T061841Z` on fedora, the harness at 88556f8. 3 rounds of
  1,000 timed lifecycles after 100 untimed; 1,500 traced creates cut into the seed path's 30 parts, no trace event
  lost, no line unread; the probe's 10 detailed creates held every positional cut to its file.

### Create p50 (ms)

| | r1 | r2 | r3 | best | pooled p50 | pooled p99 |
|---|---|---|---|---|---|---|
| libcrun | 2.838 | 2.638 | 2.730 | 2.638 | 2.750 | 3.854 |
| seam | 2.084 | 2.120 | 1.794 | 1.794 | 2.020 | 3.143 |
| traced: recording | 2.257 | 2.259 | 1.967 | 1.967 | 2.189 | 3.299 |
| traced: between | 2.195 | 2.166 | 1.896 | 1.896 | 2.110 | 3.394 |

The parts' p50s sum to 2.134 ms pooled: 101.1% of the creates between the traced ones (step 1's criterion met).

### Create through the seam, in parts (1,500 traced creates)

| # | part | group | pooled p50 | pooled p99 |
|---|---|---|---|---|
| 1 | the call reaches the node; the spec checked; the box's record looked for | own | 0.011 | 0.020 |
| 2 | its cgroup looked for; a watch slot reserved; the record encoded, its temp file made | record | 0.047 | 0.080 |
| 3 | the record written | record | 0.032 | 0.060 |
| 4-6 | the record's fsync, its rename, the directory's fsync | record | 0.000 | 0.000 |
| 7 | the box root listed, its boxes counted | cgroup | 0.009 | 0.019 |
| 8 | the cgroup made (kernel) | cgroup | 0.130 | 0.732 |
| 9 | its bounds written: memory.max, memory.swap.max, pids.max | cgroup | 0.045 | 0.091 |
| 10 | the container's state directory made; config.json read and parsed; its device policy planned | own | 0.041 | 0.072 |
| 11 | the exec FIFO made; the device program's instructions made | own | 0.012 | 0.021 |
| 12 | the device program loaded (kernel: the verifier) | own | 0.235 | 0.333 |
| 13 | its identity read; the hints read, the program they name looked for, the hints written; attached | own | 0.041 | 0.705 |
| 14 | the pipes made; a namespace made ahead taken; the request sent to a seed, which wakes and opens the cgroup | seed | 0.053 | 0.163 |
| 15 | the seed's clone prepared | seed | 0.003 | 0.009 |
| 16 | the init's clone into its cgroup and new namespaces: pid, mount, ipc, uts (kernel), until it runs | init | 0.174 | 0.840 |
| 17 | the init's first steps, to its join of the namespace made ahead | init | 0.024 | 0.080 |
| 18 | the namespace made ahead joined (setns, kernel) | init | 0.005 | 0.014 |
| 19 | the init's root mounted: the rootfs bound, /usr, /proc and /dev, its device nodes made | init | 0.359 | 1.012 |
| 20 | pivot_root (kernel) | init | 0.535 | 0.672 |
| 21 | the old root unmounted; the hostname set; the process become (capabilities, user); ready sent | init | 0.065 | 0.660 |
| 22 | the agent wakes to the ready signal and reads it; the init's stat opened | handshake | 0.027 | 0.105 |
| 23 | the init's start time read; the container's state written (state.json, renamed into place) | own | 0.051 | 0.087 |
| 24 | the state read back (state.json); the init's stat opened | own | 0.027 | 0.040 |
| 25 | the init checked; the record encoded, its temp file made | record | 0.066 | 0.102 |
| 26 | the record written | record | 0.069 | 0.114 |
| 27-29 | the record's fsync, its rename, the directory's fsync | record | 0.000 | 0.000 |
| 30 | the init watched (its pidfd, its stat checked, the epoll set); the box's info made; the call returns | own | 0.073 | 0.113 |

| group, its parts' p50s summed | pooled | share of the sum |
|---|---|---|
| the box's record | 0.214 | 10.0% |
| the cgroup, made and bounded | 0.184 | 8.6% |
| the seed: the request and its clone's preparing | 0.056 | 2.6% |
| the init's set-up, to the exec FIFO | 1.162 | 54.5% |
| the handshake | 0.027 | 1.3% |
| the node's and the runtime's own work | 0.491 | 23.0% |

### What goal 5's added steps take (pooled part p50s, against the creates between, p50 2.110 ms)

| lever | its parts | ms | share of create |
|---|---|---|---|
| the device program once per policy | the program loaded and attached (12, 13) | 0.276 | 13.1% |
| the container's root without pivot_root's walk | the root mounted, and pivot_root (19, 20) | 0.894 | 42.4% |
| | of it, pivot_root (20) | 0.535 | 25.4% |
| fewer `cgroup_mutex` holds | the cgroup, made and bounded (7-9) | 0.184 | 8.7% |
| | of it, the making: the box root counted and the mkdir (7, 8) | 0.139 | 6.6% |

- **Goal 5's bar:** a quarter of libcrun's pooled create p50 (2.750 ms) is 0.688 ms; the create's parts (2.134 ms) are
  3.104x it. Take away the device program's load and attach, the root's mounts and pivot_root, and the cgroup's making,
  each whole, and 0.825 ms is left: 1.200x the bar. The rest is the record's two writes (0.214), the init's clone into
  its new namespaces (0.174), the state written and read back (0.078), the init watched (0.073), the bounds (0.045)
  and the seed's request (0.056), so the bar needs a lever among those as well.
- **The seed** costs 0.056 ms of request and wake-up on the critical path where the shim's spawn and exec cost 0.508
  in run 3; the namespaces the shim made by `unshare` are now made by the init's clone (0.174, against run 3's 0.115
  for the `unshare` and 0.088 for the clone).

## Run 5 (2026-10-31, fedora): the device program held (goal 5 extended, step 1), against the binary before it

- **Why:** goal 5 extended, step 1's second criterion: Hex's create p50 through the seam, against run 4's 2.020 ms,
  falls by at least 10%. The node holds each policy's program, and the default policy's sits on the oci boxes' root,
  so the bench's default-policy creates make no `bpf` call: parts 12 and 13 are empty, cut at the create's first pipe.
- **Host and load:** as run 1's. Five runs of the probe from 2026-10-31T21:06:54Z to 21:33:50Z, each 3 rounds of 1,000
  timed lifecycles after 100 untimed, step 1's binary and the one before it in turn. The 1-minute load at each run's
  start was 21.18, 15.15, 22.38, 19.14 and 15.67, from kiln's CI and the long-lived containers each run's env.txt
  names; `LOAD_GATE=0`, by Ada's rule. The host's clock before each: a burst took 0.104 to 0.110 ms at rest on a
  P-core (run 4: 0.095 and 0.130).
- **Binaries:** step 1's, of 4e518c0: `hex 0.1.0` sha256 2a475c14f822c00d, static-pie, built in the Fedora 44 pass
  image, and `seam_oci` 8a83b136fdcb4b8c (4e518c0 is 32db274's code with a CLAUDE.md line, a commit the branch has
  since dropped, kept as cp/t7-g5x-s1-measured). The base, of 876136e, the commit before step 1 (run 4's code with two fixes
  off the root node's path): `hex` 76d3e5d95ac0724f and `seam_oci` bd1cde4cf8b1b0f8, built the same way. libcrun
  1.30.1 in process, driver sha256 5c356f2139adc232.
- **Runs:** `~/.cache/hex-bench/out/create-breakdown-<run>` on fedora, the harness at 9669133 (its env.txt names
  7953067: the same commit before the branch dropped a CLAUDE.md change, kept as cp/t7-g5x-s1-measured). The base's
  second run is a rerun: the first try at it was killed with its launcher, when kiln's connection dropped, before it
  ran a create.

### Create p50 (ms), pooled over each run's 3 rounds

| run | binary | load at start | Hex through the seam | libcrun | Hex over libcrun | parts 12 and 13 |
|---|---|---|---|---|---|---|
| 20261031T210654Z | step 1 | 21.18 | 1.825 | 2.980 | 0.612 | 0.000 |
| 20261031T211054Z | base | 15.15 | 2.217 | 3.073 | 0.721 | 0.304 |
| 20261031T211237Z | step 1 | 22.38 | 1.955 | 3.046 | 0.642 | 0.000 |
| 20261031T211410Z | step 1 | 19.14 | 1.787 | 3.084 | 0.579 | 0.000 |
| 20261031T213222Z | base | 15.67 | 2.171 | 2.903 | 0.748 | 0.311 |

- **Against run 4** (2.020 ms): step 1's best of three, 1.787 ms (20261031T211410Z), is 11.5% lower; its three runs are
  9.7%, 3.2% and 11.5% lower.
- **Against run 4, each over libcrun's create in its own run** (run 4: 2.020 over 2.750, 0.735): the base's runs give
  0.721 and 0.748, run 4's within 2%, so libcrun beside it carries the host's state; step 1's give 0.612, 0.642 and
  0.579, 16.6%, 12.6% and 21.1% under run 4's.
- **Against the base on this host:** step 1's create is 15.4% lower mean to mean (1.856 against 2.194 ms) and 17.7%
  best to best (1.787 against 2.171), though the base's runs met the lighter loads (15.15 and 15.67 against 19.14 to
  22.38). Over libcrun's in the same run, step 1's is 0.579 to 0.642 and the base's 0.721 and 0.748 (run 4: 0.735).
- **What it took off:** the base's device program, loaded and attached, 0.304 and 0.311 ms (part 12, the load, 0.259
  and 0.264; part 13, the hints and the attach, 0.045 and 0.047); step 1's, nothing. Its part 11, from the FIFO's
  `mknod` to the create's first pipe, is 0.015 to 0.017 ms against the base's 0.013 and 0.014: the policy's
  instructions compared with the root's, where the base loaded them.
- **The host ran slower than run 4's:** the base's creates were 7.5% and 9.8% slower than run 4's 2.020 ms, and
  libcrun's 5.6% to 12.1% slower than its 2.750, so step 1's figures against run 4 carry that.
- Every run's parts sum to within 3% of its creates (97.0% to 100.3% of the creates between the traced ones), no trace
  event lost, and each probe step's detailed creates held every positional cut to its file.

## Run 6 (2026-10-31, fedora): step 1's binary and the one before it in the same rounds

- **Why:** the lead's call for goal 5 extended step 1's second criterion, on Ada's rule that a loaded host is never a
  block because the same run's A/B ratio cancels its noise: each run times step 1's binary (`seam`) and the one before
  it (`before`, staged by `BEFORE_HEX` and `BEFORE_SEAM`) in every shuffled round, libcrun beside them, and "10% under"
  is judged by each run's own ratio.
- **Host and load:** as run 1's. Three runs from 2026-10-31T22:07:37Z to 22:13:40Z, the 1-minute load 26.97, 34.96 and
  32.56 at their starts and 23.86 at the end, on the host as it was; the host's clock before each: a burst took 0.103
  to 0.119 ms at rest on a P-core.
- **Binaries:** run 5's: step 1's of 4e518c0 (`hex` 2a475c14f822c00d, `seam_oci` 8a83b136fdcb4b8c) and the base's of
  876136e (`hex` 76d3e5d95ac0724f, `seam_oci` bd1cde4cf8b1b0f8).
- **Runs:** `~/.cache/hex-bench/out/create-breakdown-<run>` on fedora, the harness at 67ba315 (its env.txt names
  e9c6115: the same commit before the branch dropped a CLAUDE.md change, kept as cp/t7-g5x-s1-measured).

| run | load at start | Hex through the seam | before | libcrun | seam over before, pooled | by round |
|---|---|---|---|---|---|---|
| 20261031T220737Z | 26.97 | 1.995 | 2.619 | 3.293 | 0.762 | 0.914, 0.471, 0.792 |
| 20261031T220941Z | 34.96 | 2.263 | 2.715 | 3.358 | 0.833 | 0.882, 0.757, 0.834 |
| 20261031T221154Z | 32.56 | 1.559 | 2.288 | 2.826 | 0.681 | 0.870, 0.752, 0.415 |

- **Each run's own ratio is under 0.9:** step 1's create p50 is 23.8%, 16.7% and 31.9% under the base's in the same
  rounds. Of the nine rounds, one is under by less than 10% (0.914).
- **The noise:** the base's rounds ran 1.891 to 4.796 ms, so a round's ratio swings with which binary met a busy
  moment; pooled over its rounds each run is under. The device program's parts 12 and 13 were 0.000 in every run.
- **The probe's own check failed in run 20261031T220737Z:** its parts summed to 81.3% of the creates between the
  traced ones, outside the 10% the probe holds them to, because that run's base rounds were noisy (1.922, 4.796 and
  2.553 ms). Its creates and their ratio stand; its parts don't account for its creates. The other two runs' parts
  summed to 99.4% and 97.3%.

## Run 7 (2026-10-31, fedora): three more of run 6's, step 1's binary and the one before it in the same rounds

- **Why:** run 6's measure again, on the lead's word that "10% under" is judged by each run's own ratio, the base in
  every round, the host as it is.
- **Host and load:** as run 1's. From 2026-10-31T22:23:00Z, the 1-minute load 18.08, 17.97 and 18.02 at the three
  runs' starts. The third run's launcher was killed in its second round, its load then 75.23 from other work on the
  host, so it left no summary and gives no figure; its directory holds what it ran.
- **Binaries:** run 5's (4e518c0's and 876136e's). **Runs:** `~/.cache/hex-bench/out/create-breakdown-<run>` on
  fedora, the harness at 15e60aa's probes (its env.txt names 9fde51f: the same files before the branch dropped a
  CLAUDE.md change, kept as cp/t7-g5x-s1-measured).

| run | load at start | Hex through the seam | before | libcrun | seam over before, pooled | by round |
|---|---|---|---|---|---|---|
| 20261031T222300Z | 18.08 | 1.703 | 2.159 | 2.875 | 0.789 | 0.826, 0.727, 0.812 |
| 20261031T222443Z | 17.97 | 1.512 | 1.904 | 2.701 | 0.794 | 0.751, 0.831, 0.825 |
| 20261031T222621Z | 18.02 | killed in round 2 | | | | |

- **Both runs' own ratios are under 0.9**, and every round's: step 1's create p50 is 21.1% and 20.6% under the base's
  in the same rounds. The parts summed to 100.2% and 100.4% of the creates between the traced ones; the device
  program's parts 12 and 13 were 0.000.
- **The second met the host as run 4 did:** libcrun's 2.701 ms against run 4's 2.750, and the base's 1.904 against
  run 4's 2.020. There step 1's create p50, 1.512 ms, is 25.1% under run 4's 2.020, as the plan's criterion reads it.

## The root switch at concurrency (goal 5 plan step 2, AC-e)

The pruned-seed root (decision 5's extension) ends not with `pivot_root` but with `chroot` (step 2's init moved the
root onto `/` by `MS_MOVE` before it; from step 3 the rootfs is attached on `/` directly), so the
switch holds no host-wide lock: `pivot_root` calls `chroot_fs_refs`, which takes `tasklist_lock` and walks every
task on the host (`fs/namespace.c`, `fs/fs_struct.c`, quoted under run 1), while `chroot` swaps only the caller's
own `fs->root` under its `fs->lock`. AC-e covers the root's parts under concurrency — eight creates in flight with
at least 8,000 idle threads on the host, which a `pivot_root` would walk: the **switch** (part 20) at or below a
0.5 ms p99, and the init's **root mounts** beside it (part 19, run 4's). The mounts still take the mount namespace's
lock, which the classic path takes too. Step 2 reports that cost; step 3 additionally requires the
mounts' p99 at or below 0.5 ms, selected with `AC_E_MOUNTS=1`.

`create_breakdown.py switch <trace> <stats> [seam json] [switch|mounts]` reads both parts. The harness runs it under
`AC_E=<lifecycles>`: `seam_oci`'s throughput phase pinned to eight in flight (`HEXBENCH_THROUGHPUT_CONC=8`) with the
root's events on (`sys_enter`/`sys_exit` of `setns`, `chroot` and `pivot_root`, plus `sys_exit_clone3`, filtered to the runtime's `seam_oci`
and `hex`), recording for the whole phase — no `HEXBENCH_TRACE`, so `seam_oci` never toggles `tracing_on`. The
**switch** (part 20) is each thread's `chroot` entry to its exit; the **mounts** (part 19) run from the net-namespace
join (`setns` with `CLONE_NEWNET`) that opens them to the `chroot`, spanning the init's `move_mount`/`fsmount` of the
rootfs and the spec's mounts under the kernel's mount lock. The host holds `IDLE_THREADS` idle threads
(`idlethreads.c`), outside the bench container, in the kernel's global task list a `pivot_root` walks though not in
the container's pid namespace. Two kinds of `pivot_root` can appear and are told apart by their caller: a pruned
**seed** prunes its own mount namespace once at start with a `pivot_root`, by a pid that never `chroot`s; an
**init** on this path `chroot`s and never `pivot_root`s. So an init `pivot_root` is one by a pid that also `chroot`s
(AC-a: none), and the reader checks every create switched via `chroot` (chroots == creates), a create that fell back
to the classic path `pivot_root`ing in place of `chroot` dropping the count.

A network-pool miss has no network `setns`: the init is born with `CLONE_NEWNET`. Its part 19 starts
on that init's `clone3` return zero, including initial setup and bringing loopback up. A successful
network `setns` exit remains the preferred cut. The mounts criterion requires every create to have
a mount span and whole-create sample, with the fallback count exactly matching `netns.made_on_path`
less `netns_reuse.made_on_path`, and only concurrency eight in the seam JSON. The second count is step 6's
reuse: an owner's box that finds the pool dry is given a namespace the node makes on its path
(`hex_node::netns`, `make_on_path`), which its init joins by `setns` as it joins a pooled one, while the pool
counts the take among those made on the path all the same. Missing JSON, lost events, failed/unpaired switches,
classic fallback and incomplete coverage refuse the run. No dry creates are omitted. Old switch-only
records retain their original coverage rule for historical reading.

For the prepared `/dev` comparison, `HEXBENCH_DEVFS=0` and `1` use the same release and concurrency;
JSON records whole-create `create_ms` and fixed pool reasons outside each timed phase. The fixed
before binary bf77298 uses a timing-only backport in an archived benchmark tree: only `seam_oci.rs`
throughput sampling and its timed helper change, with production sources untouched. The instrumented
bench source SHA256 is `c0e6e2e1352fb8c769f67f1fda5c6805a8f9b03b124cefe0fb3f0f549cc47c79`.
Report every run's whole-create p50/p99 beside part 19, including failed coverage or latency results.

`python3 -B -I bench/competitive/probes/create_breakdown_switch_test.py` exercises real parsers with
in-memory ftrace and seam fixtures: pooled, dry and mixed boundaries; 499/500/501/600 us and CLI scope;
missing/failed starts or switches; counter/sample/concurrency errors; lost events and init pivots.
The fixtures fail the pre-fix reader (10 assertions and one new-field error) and pass all five tests
with the complete-coverage reader. These are reader regressions, not performance measurements. A sixth,
namespaces the node made on the path for owners, fails the reader before the reuse count (both
reuse cases refused, rc 1, and the new field missing) and passes with it.

### Run 8 (2026-11-01, fedora): AC-e, not of record

- **Host:** the fedora bench host (the `kiln-a094a12d` runner, named `fedora-2` when run 1 ran; `create_breakdown.md`
  run 1), Intel Core i9-14900HX (32 CPUs, the container pinned to 4-15), 62 GiB, kernel 6.19.10-300.fc44.x86_64 — the
  same machine and kernel as every run above.
- **When and load:** 2026-11-01T00:21:24Z. `LOAD_GATE=0`, so not of record by CLAUDE.md §7, and the host was busy:
  the 1-minute load was 29.49 at the start, from kiln CI beside it. The switch is a per-call latency insensitive to
  the host's load (`chroot` neither sleeps nor takes a shared lock); the mounts' p99 does rise with concurrency, the
  mount lock being contended, which is the point of measuring them at eight in flight.
- **The host's idle threads:** `IDLE_THREADS=8000`; 8,001 came up, for 11,485 threads on the host — the threads a
  `pivot_root` would walk.
- **Binaries:** `hex` and `seam_oci` of a34657a, built on the host, dynamically linked against its `/usr` (the
  container bind-mounts it): `hex` sha256 4a0c0da58d5ac39f, `seam_oci` sha256 ef2583e8df9a1431. a34657a is
  cp/t7-ext-s2 before its rebase onto step 1's tip; the switch and mount code is unchanged at the rebased tip, which
  adds only a debug-only test seam and these records.
- **Run:** `~/.cache/hex-bench/out/create-breakdown-20261101T002124Z` on the fedora host, read by
  `create_breakdown.py switch` at the rebased tip, `AC_E=2000 HEXBENCH_THROUGHPUT_CONC=8 CPUSET=4-15`. 2,000 creates
  at eight in flight; no trace event lost, no `chroot` entry left unpaired, 1,998 of the 2,000 with a net-namespace
  join traced before the switch (the other two made their namespace on the create's own path, netns pool near dry).

The root's parts over the 2,000 creates:

- **The switch (`chroot`, part 20): p50 0.002 ms, p99 0.008 ms, max 0.162 ms** — the p99 is ~60x under the 0.5 ms
  bar, at a host load of 29, so the switch does not serialise under concurrency. **AC-e's switch met.** (Run
  20261031T233802Z, the earlier switch-only run at load 50, gave the same: p99 0.009 ms.)
- **The mounts (part 19): p50 0.273 ms, p99 2.574 ms, max 6.411 ms** — above the 0.5 ms bar and reported, not held to
  it: the init's `move_mount`/`fsmount` take the mount namespace's lock (`namespace_sem`), which the classic path
  takes too, so this is that lock's contention at eight in flight, not the `pivot_root` walk the switch replaces. The
  switch removed the walk; the mount lock is a separate, kernel-wide cost both paths pay.

Every one of the 2,000 creates switched via `chroot` (chroots == creates == 2,000): none fell back to the classic
`pivot_root` path. The only eight `pivot_root`s in the trace are by eight distinct pids that never `chroot` — the
pruned seeds' one-time prune (the seam result's `seeds.started` is 8). The reader tells them apart by the caller: 0
init `pivot_root`s (a `pivot_root` by a pid that also `chroot`s), 8 seed prunes (by pids that never `chroot`), the
two sets disjoint. **AC-a's "no init pivot_root" evidenced directly over the raw trace**, beside the cutter's
per-create assertion in the runs above (part 20, "the root moved onto / and chrooted into", one `chroot` and no init
`pivot_root`).

### Run 9 (2026-11-01, fedora): AC-e at step 2's tip

- **Host:** run 8's, rebooted at 22:43:51Z after it ran out of memory (run 10 says what took it).
- **When and load:** 2026-11-01T22:47:37Z, `LOAD_GATE=0` by Ada's rule. The 1-minute load was 15.09 at the start;
  some task was stalled on memory 7.45% of the last 10 s and on I/O 16.49% (`/proc/pressure`). What else ran: run
  10's.
- **The host's idle threads:** `IDLE_THREADS=8000`; 8,001 came up, for 10,751 threads on the host.
- **Binaries:** `hex` and `seam_oci` of aed6da3, step 2's code, built on the host and dynamically linked as run 8's:
  `hex` sha256 951bab10dc326f0c, `seam_oci` 630e5987e4e15d99. The step's tip adds to aed6da3 only a test, comments,
  docs and the probe's `IDLE_AB`.
- **Run:** `~/.cache/hex-bench/out/create-breakdown-20261101T224737Z`, `AC_E=2000 CPUSET=4-15`, the harness at
  aed6da3. 2,000 creates at eight in flight; no trace event lost, no `chroot` left unpaired, 1,985 of the 2,000 with a
  net-namespace join traced before the switch.

- **The switch (`chroot`, part 20): p50 0.002 ms, p99 0.006 ms, max 0.068 ms**, the p99 83x under the 0.5 ms bar.
  **AC-e met.**
- **The mounts (part 19): p50 0.209 ms, p99 1.553 ms, max 2.701 ms**, reported: the init's mounts at eight in flight
  are step 3's criterion, which takes run 8's 2.574 ms p99 as its baseline.
- chroots == creates == 2,000, and the trace's only `pivot_root`s are the 8 seeds' prunes, by pids that never
  `chroot`: 0 init `pivot_root`s (AC-a).

## Run 10 (2026-11-01, fedora): the pruned-seed root at step 2's tip, with and without 24,000 idle host threads

- **Why:** goal 5 extended step 2's AC-a (the switch's pooled p50 at most 0.02 ms, and no init `pivot_root` in the
  trace), AC-b (Hex's create p50 through the seam at most 0.58x libcrun's in the same run) and AC-c (Hex's create p50
  with 24,000 extra idle host threads within 10% of it without them).
- **Host and load:** run 8's, `LOAD_GATE=0` by Ada's rule. That evening the host ran out of memory twice and was
  rebooted, at 22:15:59Z and 22:43:51Z. Each time the same four containers, none of them Hex's, had started minutes
  before and came back with the boot: `issue556oss-control-plane` (kindest/node v1.35.0),
  `buildx_buildkit_kagent-builder-v0.23.00`, `kind-registry` and `kiln-t10-minio`. They were up beside every run here,
  with kiln CI's builds of the kiln crates (rustc at 1-6 GB each), each run's env.txt naming the containers and its
  load. Goal 5 extended step 5's agent ran its own units on the host beside the last of them, side by side by the
  lead's rule: `hex-g5x-s5-fedora-20261101T225558Z` from 22:55:58Z (beside 20261101T225526Z's later rounds) and
  `hex-g5x-s5-fedora-v3-20261101T231100Z` from 23:11:00Z (the end of run 12's second). From 22:35Z a run started only
  with MemAvailable at least 24 GB (the lead's rule). The first boot's runs after 20261101T221833Z met MemAvailable
  falling to 10-11 GB as rustc ran beside them, and aren't used.
- **Binaries:** run 9's. libcrun 1.30.1 in process (079ff6a7), driver sha256 5c356f2139adc232.
- **Runs:** `~/.cache/hex-bench/out/create-breakdown-<run>`, each 3 rounds of 1,000 timed lifecycles after 100
  untimed, `CPUSET=4-15`, the harness at aed6da3: the first in the first boot, the rest in the second, back to back
  (runs 9 and 11 between them). 1,500 traced creates each, every one cut; no trace event lost.

| run | idle host threads (threads on the host) | load at start | Hex through the seam | libcrun | Hex over libcrun | the switch (part 20), p50 / p99 | the parts over the creates between |
|---|---|---|---|---|---|---|---|
| 20261101T221833Z | 0 | 18.34 | 0.930 | 2.381 | 0.391 | 0.001 / 0.004 | 102.7% |
| 20261101T224621Z | 24,001 (26,522) | 22.03 | 0.714 | 4.365 | 0.164 | 0.001 / 0.003 | 99.6% |
| 20261101T224740Z | 0 | 15.09 | 0.789 | 2.186 | 0.361 | 0.001 / 0.003 | 99.0% |
| 20261101T224849Z | 24,001 (26,792) | 12.86 | 1.201 | 5.754 | 0.209 | 0.001 / 0.004 | 98.1% |
| 20261101T225414Z | 0 | 33.09 | 0.857 | 2.728 | 0.314 | 0.001 / 0.003 | 99.7% |
| 20261101T225526Z | 24,001 (27,008) | 20.25 | 0.912 | 5.257 | 0.173 | 0.001 / 0.003 | 96.2% |

- **AC-a met:** the switch's pooled p50 is 0.001 ms in every run, against run 4's `pivot_root` at 0.535 and the bar's
  0.02. Each run's raw traces (`traced.r1.txt` to `traced.r3.txt`) hold no `pivot_root` event: the seeds pruned
  before tracing began, and no init made one.
- **AC-b met:** Hex's create p50 over libcrun's in the same run is 0.391 (20261101T221833Z), 0.361 (20261101T224740Z)
  and 0.314 (20261101T225414Z) without the idle threads, against the 0.58 bar and run 4's 0.735.
- **AC-c across runs:** each run with 24,000 idle threads against the run without them beside it in the same boot:
  0.714 over 0.789 is 0.905, 0.912 over 0.857 is 1.064, both within 10%, and 1.201 over 0.789 is 1.522. That run
  (20261101T224849Z) met a build beside it in its rounds 2 and 3 (the load from 12.79 to 19.69, rustc at 223%), and
  every part of its creates grew, the init's clone (part 16) 0.176 against 0.116 and its mounts (part 19) 0.228
  against 0.169, while the switch, where a walk of the host's threads would show, stayed at 0.001. libcrun's create,
  whose `pivot_root` walks them, grew 2.0x, 2.6x and 1.9x with them (4.365 and 5.754 over 2.186, 5.257 over 2.728). A
  comparison across runs carries whatever else ran beside each, so these three are listed for the record and AC-c's
  figure of record is run 12's, measured within one run.

## Run 11 (2026-11-01, fedora): 4,000 extra mounts in the agent's namespace, not of record

- **Run:** `~/.cache/hex-bench/out/create-breakdown-20261101T225023Z`, run 10's binaries and harness, `EXTRA_MOUNTS=4000`
  (4,016 mounts in the agent's namespace), 2026-11-01T22:50:23Z, load 20.48 at the start, between run 10's
  20261101T224849Z and 20261101T225414Z, so in the same build's surge.
- **Not a clean read:** its parts summed to 72.7% of the creates between, outside the 10% the probe holds them to, so
  the parts don't account for its creates. Hex's create p50 was 1.719 ms (rounds 1.415, 2.520 and 1.591), the init's
  clone (part 16) 0.261 and its mounts (part 19) 0.380; libcrun's 20.178 ms, its init copying the agent's 4,016-mount
  table. AC-d (flat in the agent's mounts) is reported only, not a criterion of step 2's (records-plan R4); a clean read
  needs a within-run A/B as run 12's.

## Run 12 (2026-11-01, fedora): AC-c within one run, the idle threads beside their absence in every round

- **Why:** AC-c's comparison within one run. `IDLE_AB=24000` (14f9d13) adds two items to every shuffled round:
  `seam_idle` and `libcrun_idle`, the seam's and libcrun's lifecycle phases with 24,000 idle threads up inside the
  bench container. Threads in the container are the host's like any other, so `pivot_root`'s walk
  (`chroot_fs_refs`'s `for_each_process_thread`) visits them. Each runtime's ratio to its own items without them in the
  same rounds is the run's own, as step 1's `BEFORE_HEX` A/B is. libcrun's ratio is the control that they are walked.
- **Host and load:** run 10's, its second boot, `LOAD_GATE=0`; MemAvailable 33,116, 44,243 and 45,072 MiB at the
  runs' starts.
- **Binaries:** run 9's (aed6da3's). **Harness:** 14f9d13 for the first two runs; 2823023 for the third, which records
  each idle item's threads in `idle_threads.txt`: the helper's, counted at the item's start and at its end, and the
  host's then (`/proc/loadavg`'s count of the kernel's threads, which no pid namespace narrows).

| run | rounds | load at start | Hex: with / without, pooled p50 | its ratio, by round | its ratio, pooled | libcrun: with / without | its ratio, pooled |
|---|---|---|---|---|---|---|---|
| 20261101T230322Z | 3 | 21.08 | 1.010 / 1.043 | 1.041, 1.356, 0.770 | 0.969 | 5.898 / 2.975 | 1.982 |
| 20261101T230639Z | 5 | 16.61 | 1.185 / 1.205 | 0.987, 0.696, 0.936, 1.071, 1.013 | 0.983 | 6.330 / 3.316 | 1.909 |
| 20261101T232039Z | 5 | 12.41 | 0.806 / 0.806 | 0.920, 1.041, 0.893, 0.964, 1.128 | 0.999 | 4.669 / 2.389 | 1.954 |

- **AC-c met, and this is its figure of record:** with the 24,000 idle threads Hex's create p50 is 0.969, 0.983 and
  0.999 of its own without them in the same rounds (pooled over 3,000, 5,000 and 5,000 creates each), within 10%,
  while libcrun's grows 1.982x, 1.909x and 1.954x, in every round 1.40x to 2.70x: the threads are walked by a
  `pivot_root`, and Hex's create makes none. It is the figure of record because its two sides meet the same host:
  every round runs both kinds of item in a shuffled order, so what ran beside the run (kiln CI's builds, step 5's
  units, the containers filling the host's memory) falls on both alike and the ratio cancels it, as Ada's rule for a
  noisy host asks and as step 1's `BEFORE_HEX` A/B does. Run 10's pairs compare two runs that met different hosts.
  Hex's round-by-round ratios straddle 1 (0.696 to 1.356) as the host's load moved under them; libcrun's never fall
  under 1.4. The second run ended at a 1-minute load of 218, from builds beside it.
- **The threads, item by item (20261101T232039Z):** every idle item, the probe's and each round's `seam_idle` and
  `libcrun_idle`, had all 24,001 of the helper's tasks (its 24,000 threads and itself) up at its start and at its end;
  the host's threads were 26,898 to 27,745 across them. The first two runs' harness refused an item whose threads
  didn't all come up but didn't record the count; none was refused.
- The runs' other figures: Hex's create p50 0.350x, 0.363x and 0.337x libcrun's (1.043 over 2.975, 1.205 over 3.316,
  0.806 over 2.389; AC-b); the switch's pooled p50 0.001 to 0.002 ms (AC-a); the parts summed to 99.8%, 101.9% and
  98.3% of the creates between; no `pivot_root` in their traces.

## The root lab beside these runs (AC-f, AC-g, the step's tests)

`bench/oci-validation/docker_lab.sh <commit> oci_spec_box oci_box`, as root in a privileged container on the same host
and kernel (6.19.10-300.fc44), test binaries built in its Fedora 44 image:

- **At 541fe3a** (aed6da3 with the AC-f test printing what it read; run chain-20261101T221818Z): oci_box passed, the
  step's tests among it (root escapes E0-E3, a bind and a volume, the classic path's reasons, an init refusing or
  dying mid-build, `rootfsPropagation`). oci_spec_box failed AC-f's "exactly two" mounts: the seed's mountinfo listed
  one, `4324 4190 0:1371 / / ro,relatime - tmpfs tmpfs rw,seclabel,size=64k,mode=755,inode64`. The namespace's root,
  mount 4190, lies outside the seed's root, and v6.19's `show_mountinfo` lists no mount it can't reach from the task's
  root (fs/proc_namespace.c:150-151, fs/seq_file.c:517-519, fs/d_path.c:211-223).
- **At f387457** (the AC-f test as the kernel lists the namespace, 1c6436b; run chain-20261101T224533Z): 2 of 2
  passed. AC-f's line: "the pruned create seed (pid 18) lists one mount, its read-only root tmpfs: 4346 4297 0:1373 /
  / ro,relatime - tmpfs tmpfs rw,seclabel,size=64k,mode=755,inode64".

## The init's mounts, call by call (goal 5 plan step 3)

`MOUNTS=<lifecycles>` (with `MOUNTS_CONC`, `1 8` by default, and `MOUNTS_PATH`, `pruned` by default or `classic`)
runs, in place of the rounds, one `seam_oci` throughput phase at each concurrency with every system call of the `hex`
processes traced, and `create_breakdown.py mounts` cuts part 19 of each create out of the trace, call by call
(`docs/research/runtime/goal5-mounts.md` §1 says why and what it found). The events are `raw_syscalls`' entries and
exits, which record a call's number and arguments and fetch no path (the `syscalls` subsystem's events fetch each
one, about 4 us a time, which would charge the very calls this times); `sched_switch`, so the time off the CPU is
told preempted or blocked; `page_fault_user`; and `sched_process_fork`, so a classic span opens only in a process a
traced `hex` process forked or cloned (a seed, which makes its mounts private when it prunes, is the node agent's
child). A span runs from the init's join of the network namespace made ahead (`setns` with `CLONE_NEWNET`) to its
`chroot` or `pivot_root`, as `switch` cuts part 19; with `MOUNTS_PATH=classic` the throughput phase's bundle has a
createRuntime hook, so each create takes the classic path through the shim, and a span opens at the init's first
mount, making its mounts private. The report gives, per create, each call's count, time and time off the CPU, the
time between calls and the page faults; each mount's share (the calls up to its attach); and the slowest 1% the same
way. The trace adds two events to every call, so its spans are longer than the rounds' part 19: they place the time
and count the calls, and part 19's figures of record are the rounds'.

Each init's holds of the kernel's one mount lock are counted too (goal 5 extended step 3). Two kprobes, on `down_write`
and `down_read`, are filtered to `namespace_sem`'s address in `/proc/kallsyms` and to the `hex` processes. Every write
hold is `namespace_lock`'s `down_write`, every shared one a `down_read` (v6.19 `fs/namespace.c:1723-1726`, `:97`,
`:1541`). The report gives the holds per create for write and shared, each by the call the init was in.
`create_breakdown_mounts_test.py` holds the count to a synthetic trace; the reader before it fails that test (no
count).

## Run 13 (2026-11-03, fedora): step 6's binary on boxes of no owner, and the one before it, in the same rounds

Runs 8 to 12 are goal 5 extended steps 2 and 3's, on their branches. This run's container was pinned to CPUs 4-31, P-
and E-cores, as the harness runs by default. Its differences are within its own rounds, so they stand as measured
(the lead, 2026-11-03T00:48Z). Its absolute times aren't comparable with the records on 4-15, the P-core threads,
from run 4 on. Run 14 repeats it on 4-15, with enough rounds for the window.

- **Why:** goal 5 extended, step 6's third criterion: Hex's create p50 with reuse off, against the binary before this
  step in the same A/B run, within 0.02 ms of 0 ms. The probe's boxes name no owner, so none is handed a cgroup: each
  is given a fresh one, made and bounded on create's path as before the step, and its delete ends it through its
  init's pidfd.
- **Host and load:** as run 1's. Three runs from 2026-11-03T01:21:49Z to 01:32Z, the 1-minute load 21.85, 13.85 and
  9.82 at their starts and 11.50 at the end; `LOAD_GATE=0`.
- **Binaries:** step 6's, built on fedora from the step's tree (the code of `a6f32f1`, but for a doc comment): `hex`
  1404d8835bb80926, `seam_oci` 833f7025d1d9b056 (`create_serial.md`'s run 7's). The before, of 50bd7a2 (goal 5
  extended step 5's tip, the step's base), built the same way: `hex` abff48746032d99b, `seam_oci` 4d1f37f19887a0ee.
  libcrun 1.30.1 in process, driver sha256 5c356f2139adc232.
- **Runs:** `~/.cache/hex-t7/g5x-s6/hex-bench/out/create-breakdown-<run>` on fedora, the harness at a6f32f1 (the third
  run's env.txt's uncommitted changes are records, no script), 1,000 timed lifecycles a round after 100 untimed.

| run | rounds | load at start | Hex through the seam | before | libcrun | seam less before, pooled (ms) | by round |
|---|---|---|---|---|---|---|---|
| 20261103T012149Z | 3 | 21.85 | 1.331 | 1.396 | 2.733 | -0.064 | -0.060, -0.114, -0.025 |
| 20261103T012324Z | 3 | 13.85 | 1.402 | 1.374 | 2.738 | +0.028 | -0.016, +0.050, +0.033 |
| 20261103T012712Z | 10 | 9.82 | 1.365 | 1.439 | 2.853 | -0.074 | -0.054, -0.114, -0.137, +0.217, -0.093, -0.058, -0.053, -0.055, -0.314, -0.042 |

- **No run is within 0.02 ms of 0:** pooled, step 6's create p50 is 0.064 and 0.074 ms under the before's in two runs
  and 0.028 over it in one. Of the 16 rounds, one is within 0.02 ms (-0.016); 12 are under by more and 3 over.
- **The rounds spread wider than the window:** the before's own round p50s ran 1.310 to 1.621 ms across the runs. In
  the third run the seam's less the before's has a standard deviation of 0.129 ms over its 10 rounds, so its pooled
  difference carries a standard error of about 0.041 ms, twice the window's half-width.
- **The traced parts are the same but for `pivot_root`:** their p50s summed 1.436 against 1.476 ms, 1.432 against 1.445
  and 1.435 against 1.465 (step 6's against the before's, over 1,500, 1,500 and 5,000 traced creates). The one part
  that differed by 0.008 ms or more in any run is the init's `pivot_root`, which this step doesn't change: 0.017 and
  0.018 ms quicker in step 6's (0.298 against 0.315 in the first run, 0.283 against 0.301 in the third). The parts
  summed to 102.5%, 100.3% and 99.4% of the creates between the traced ones.

## Run 14 (2026-11-03, fedora): run 13 on CPUs 4-15, five chunks of 20 rounds pooled

- **Why:** run 13's three runs left step 6's third criterion unresolved: their rounds spread wider than its window.
  This repeats it on CPUs 4-15, the P-core threads, with the rounds the window needs: every round's paired difference
  pooled across runs, its mean's standard error by a bootstrap over the rounds, until that is at most 0.01 ms (the
  lead, 2026-11-03T02:36Z). In chunks of 20 rounds, each a whole A/B run with its own run id, so a host reset costs one
  chunk (the lead, 03:14Z). `create_breakdown_pool.py` pools them.
- **Binaries:** step 6's at 610bba9, the step with the lead's `cpu.max` ruling, built on fedora: `hex`
  765e809d32ae1a07, `seam_oci` 93bd371b2e2b5499 (`reuse_pass.md`'s run 4's). The before, ea891c7: the step's base, on
  the delete flake's fix, built the same way: `hex` 2bd6a0fb78b3229e, `seam_oci` e84746c9fccee592. libcrun 1.30.1 in
  process, driver sha256 5c356f2139adc232.
- **Host and load:** kiln-a094a12d (fedora, renamed by its reset at 02:59Z), Linux 6.19.10-300.fc44.x86_64, the
  probe's container pinned to 4-15 and capped (`--memory 8g`, `--oom-score-adj 1000`), `LOAD_GATE=0`; other tenants'
  builds and kiln CI jobs beside it. The fedora units `hex-g5x-s6-verify3b-20261103T030643Z` (the first chunk),
  `hex-g5x-s6-verify3c-20261103T044132Z` (three) and `hex-g5x-s6-verify3d-20261103T052454Z` (the last).
- **Runs:** `~/.cache/hex-t7/g5x-s6/hex-bench/out/create-breakdown-<run>`, 1,000 timed lifecycles a round after 100
  untimed, the harness's scripts as at 610bba9. The second chunk's first try, at 04:43:35Z, refused: its trace instance
  was there already, left by a chunk of a stopped unit. The harness names it `hexcb-$$`, and `$$` is 1 in every probe
  container, so a killed run's instance blocks the next. The refused run's own exit removed it, the next three chunks
  ran, and the second was run again last.
- **Since aa67b35 each run's tracer is its own:** the instance and the kprobe group are named by a tag the host
  script gives the run, its UTC stamp and its pid on the host; on any end of the host script, a stopped unit's TERM
  among them, the inside script's teardown runs again for that tag in a short container; and a breakdown started
  while another is up is refused by type, since each tracer keeps every process named as the runtimes are. Tried on
  fedora on step 6's tip at fea2f9c (unit `hex-g5x-s6-tracetag-20261103T094126Z`): a probe container's shell is pid
  1, twice; a second breakdown started while `create-breakdown-20261103T094127Z` ran refused, exit 3, and that run
  came through whole (its probes' 2 of 2 and 10 of 10 creates cut); `create-breakdown-20261103T094140Z`, killed by
  SIGKILL with its instance and its kprobes both up, left neither. fea2f9c lets the cutter take a box's cgroup made
  through the box root's held directory (step 6's 3ea3ffa), which the probes on the tip would otherwise refuse.

| run | rounds | load at start | Hex through the seam | before | libcrun | seam less before, pooled (ms) | rounds' mean |
|---|---|---|---|---|---|---|---|
| 20261103T033346Z | 20 | 169.52 | 1.366 | 1.684 | 3.671 | -0.318 | -0.289 |
| 20261103T044336Z | 20 | 23.65 | 0.882 | 0.940 | 1.993 | -0.058 | -0.055 |
| 20261103T045220Z | 20 | 8.42 | 0.795 | 0.871 | 1.730 | -0.077 | -0.083 |
| 20261103T050032Z | 20 | 8.15 | 1.134 | 1.131 | 2.454 | +0.003 | +0.004 |
| 20261103T052455Z | 20 | 21.93 | 0.908 | 0.967 | 2.210 | -0.059 | +0.008 |

- **Pooled, the 100 rounds:** step 6's create p50 less the before's in the same round has a mean of -0.083 ms, its
  bootstrap standard error 0.028 ms (2,000 resamples), median -0.060, standard deviation 0.289. 10 rounds are within
  0.02 ms, 74 under and 16 over.
- **The criterion isn't resolved at its precision:** the standard error is 0.028 ms, not 0.01. At this spread 0.01 takes
  about 800 rounds. At the precision reached, step 6 is faster, not slower: the mean's 95% interval, about -0.139 to
  -0.027 ms, lies under the window. It goes to Ada as measured (the lead, 03:14Z).
- **Without the first chunk:** at load 114 to 195 it set the spread. The other 80 rounds pool to -0.031 ms, standard
  error 0.018, median -0.048, standard deviation 0.165.
- **An earlier try:** `create-breakdown-20261103T023845Z`, of 100 rounds at f7c4065 (`hex` 729934ce2c45a292, `seam_oci`
  92b8a36ae1c6c74e) against the same before, lost 69 to the host's reset at 02:59Z. Its 31 whole rounds pool to -0.157
  ms, standard error 0.143, at load 32.6. It is kept apart: another binary.

## The init's mounts at eight in flight: part 19's p99 (goal 5 extended plan step 3)

AC-e's harness (`AC_E=2000 AC_E_MOUNTS=1`): fedora (`kiln-a094a12d`, i9-14900HX, Linux 6.19.10), the bench
container on CPUs 4-15, 8,000 idle host threads, 2,000 creates at eight in flight, `HEXBENCH_DEVFS=1`. Each figure
is one run's part 19 p99 in ms, from its `switch.result` in `~/.cache/hex-bench/out/create-breakdown-<run>`; the
whole create's p50/p99 is beside it, because a lever that only moves work across part 19's cut doesn't count. The
bar is 0.5 ms. Each unit's directory is `~/.cache/hex-t7/g5x-s3/<unit's dir>` (its `body.sh`, `binaries.sha256`,
`context.txt` with the load and the co-tenants). `docs/research/runtime/goal5-mounts.md` §8 says what each tree
changed.

**Step 3's tree alone, no reuse: not met.**

| tree | unit | runs: part 19 p99 | whole create p50 / p99 |
|---|---|---|---|
| 16e52a9 | hex-lead-s3-trace2-20261103T064732Z, load 11 | 064741Z 1.099 | 1.355 / 6.670 |
| 16e52a9 (s0) | hex-lead-s3-setsid-20261103T062234Z, load 9-10 | 062335Z 1.023, 062348Z 1.568, 062352Z 0.972 | 1.333-1.621 / 6.570-7.918 |
| `setsid` first (s1, cbe5673's order) | the same unit, rounds interleaved | 062337Z 0.806, 062343Z 0.883, 062354Z 0.729 | 1.535-1.664 / 6.281-6.978 |
| d6ea10b with `HEXBENCH_REUSE=0` | hex-lead-s3-reuseab2-20261103T163227Z, load 1-2 | 163249Z 1.420, 163251Z 1.246, 163257Z 1.224, 163300Z 1.250 | 1.179-1.252 / 7.880-11.149 |

The tail is the init preempted while its CPU runs the kernel's teardown of earlier boxes' cgroups (§8 of the note).

**The merged trees, step 6's reuse on: met with no other tenant's load.** `HEXBENCH_OWNER=bench-owner`, so the
bundles are an owner's (Docker's default capabilities, `/proc/sys` read-only):

| tree | unit | runs: part 19 p99 | whole create p50 / p99 |
|---|---|---|---|
| d6ea10b (step 6 e9f58a0 + step 3 16e52a9; hex f469b70f304691c9, seam_oci d70453f125b209c5) | hex-lead-s3-reuseab2-20261103T163227Z, load 1-2 | 163246Z 0.432, 163254Z 0.322, 163255Z 0.329, 163302Z 0.316 | 0.508-0.528 / 1.111-1.490 |
| mr12: x-s3 ef1a7a7 with step 6's levers (hex ff5cef383a844f85, seam_oci a89ecea7de4053f4) | hex-lead-s3-mr12clean-20261103T184941Z, alone on fedora, load 5-6 | 184943Z 0.338, 185008Z 0.295, 185012Z 0.303 | 0.492-0.513 / 1.136-1.219 |

Seven runs of seven are under the bar. So are the three runs of x-s3 itself, without step 6's later levers, in step
3's L1 A/B (hex-s3try2-timed-20261103T194825Z, load 3.5-8.2): d3189f7, x-s3 ef1a7a7 with the reader's fix (hex
2e1647a89ae4a566, seam_oci 6cd995f3a10dbbbf), read 0.280, 0.331 and 0.381 (create-breakdown-20261103T194827Z, 194900Z,
194923Z), whole create p99 1.527-2.307, each `"pass": true`, the reader's fallback count matched.
`docs/research/runtime/goal5-mounts.md` §10 has the rest of that A/B (L1, falsified).

**Under another tenant's load: not met.** L busy loops are pinned to 4-15 at nice 0, started and killed by the
job's recorded PIDs; this is Ada's "this fast even under load", met by doing less, never by priority:

| tree, unit | L = 0 | L = 6 | L = 12 | L = 24 |
|---|---|---|---|---|
| d6ea10b, hex-lead-s3-underload-20261103T163610Z | 163612Z 0.321, 163643Z 0.324 | 163616Z 0.769, 163639Z 0.705 | 163620Z 0.819, 163634Z 0.941 | 163625Z 1.182, 163629Z 1.620 |
| d6ea10b (s0), hex-lead-s3-setsidload-20261103T164725Z | 164743Z 0.330, 164755Z 0.335 | 164759Z 0.934, 164812Z 0.975 | 164816Z 0.974, 164830Z 0.890 | |
| `setsid` first (s1, hex d2c75d29b7b6aebd), the same unit | 164747Z 0.291, 164751Z 0.252 | 164804Z 0.547, 164808Z 0.539 | 164821Z 0.693, 164826Z 0.608 | |
| mr12, hex-lead-s3-mr12clean-20261103T184941Z | (above) | 184948Z 0.498, 185003Z 0.580, 185016Z 0.454 | 184952Z 0.702, 184957Z 1.068, 185021Z 0.743 | |

At L = 6 mr12's whole create was 0.806-0.916 / 2.726-13.691, and at L = 12 1.785-1.966 / 4.352-20.890. A run of
mr12 beside step 6's gates, hex-lead-s3-mr12load-20261103T182625Z (182627Z to 182653Z), had an uncontrolled load;
it isn't counted.

**The pass bit.** Runs 184943Z, 185008Z and the reuse-on runs of hex-lead-s3-reuseab2 print `"pass": false` with
part 19 under the bar. The fallback check expected 3-5 clone-started spans and found none: those were namespaces the
node made on the path for an owner's box, which its init joins by `setns`. The reader on
`cp/t7-ext-s3-xs3-reader` (ef04f50), for x-s3, subtracts the node's own count (`netns_reuse.made_on_path`); with it
every one of the A/B's 36 runs matched its count.

## Part 19 at step 3's final tree (goal 5 extended plan step 3, criterion 1)

The create breakdown's rounds (`N=1000 WARMUP=100 ROUNDS=3`, no idle threads, one create at a time), fedora
(`kiln-a094a12d`, Linux 6.19.10), CPUs 4-15, `HEXBENCH_DEVFS=1`, 2026-11-03, unit hex-s3try2-final-20261103T195758Z.
The binaries are cp/t7-ext-s3 4315e75's (hex e45fb83f1c7da66e, seam_oci acec8e2e42fba4f3), whose code is cbe5673's.
Part 19 is the init's root mounted, its bar 0.16 ms pooled p50 (run 4: 0.359):

- **The pruned path** (create-breakdown-20261103T200203Z, load 2.77 at its start): part 19 0.053 / 0.053 / 0.053 ms by
  round, pooled p50 0.053 (pooled p99 1.373). Met. The seam's create was 0.713 ms pooled p50 against libcrun's 2.381
  in the same rounds.
- **The classic path beside it** (`CLASSIC=1`, every create through the shim by a createRuntime hook;
  create-breakdown-20261103T200313Z, load 4.54): its part 23, the classic init's root mounted, 0.181 / 0.172 / 0.177,
  pooled p50 0.177. Its parts 21 and 22 were 0.051 and 0.049, the pace at which `goal5-mounts.md` §5's runs put
  d236217's part 23 at 0.150-0.171. It is 13% over the 1.5x mark (0.156 ms). §5 names what's left in it beside the
  probe's own calls: the init's code between its calls, with its copy-on-write page faults as a fork-like clone of
  the shim, and `setsid`. That is over the bar, and run 4's part 19 was the classic init's.
- **The classic path at the final code, cp/t7-ext-s3 933d56f** (`CLASSIC=1`, unit hex-s3try2-fs-20261103T203937Z, hex
  2c47054d9da44a0f; `goal5-mounts.md` §12): the prepared `/dev` handed through the shim (e1b85f4), and the classic
  init's filesystems made by `make_fs` (933d56f).
  - 933d56f's runs: part 23 0.314 / 0.112 / 0.126, pooled **0.133** (create-breakdown-20261103T204604Z), and 0.135 /
    0.145 / 0.120, pooled **0.132** (204824Z). Met.
  - 4315e75's binaries alternated beside them: 0.191 (204327Z) and 0.168 (205038Z), both over.
  - The pruned path's part 19 at 933d56f is 0.044 pooled (205252Z). Validation: 26 pass, 32 fail, 0 differ from
    bf77298.

## The masked directories' tmpfs made ahead, an A/B, not landed (goal 5 extended plan step 3)

x-s3 with the stock, `cp/t7-ext-s3-xs3-masks` 3dad265 (hex 355cda675683302a, seam_oci f5423d9770841202), `HEXBENCH_MASKS` 0 against
1 on that one binary, fedora (`kiln-a094a12d`, i9-14900HX, Linux 6.19.10), CPUs 4-15, 2026-11-03, load 12-31 with about 70 other
containers up and step 6's gates on CPUs 16-31. AC-e, 8 in flight, 8,000 idle threads, owner bundles, reuse on, 2,000 creates a run,
at 0, 6 and 12 busy loops, with and without Docker's paths, in interleaved pairs: units hex-s3try2-masks2-20261103T214604Z and
hex-s3try2-masks3-20261103T215640Z. Each run's record is in its `ace-*.log` under `~/.cache/hex-t7/g5x-s3/try2-masks{2,3}-*`.

With Docker's paths, medians over 17 pairs, off → on:
- Part 19's p50: 0.240 → 0.204, 0.258 → 0.219 and 0.251 → 0.215 ms at 0, 6 and 12 loops, lower in all 17 pairs of each.
- Part 19's p99: 0.828 → 0.716, 1.982 → 1.943 and 2.192 → 2.020, within the host's noise.
- The whole create's p50: 0.871 → 0.845, 1.373 → 1.387 and 2.329 → 2.336; its p99: 2.079 → 2.317, 6.847 → 6.885 and 14.658 → 15.059.
  No gain.

So it stays on `cp/t7-ext-s3-maskpool`. Every cell, the pairs, the trace and the reasons are in `docs/research/runtime/goal5-mounts.md` §13.

## The ipc and uts namespaces made ahead: the pool on against drained (goal 5 extended plan step 4)

`IPCUTS_AB=1` adds, to every round, the seam and traced items again on a node whose ipc and uts pool is drained
(`seam_drained`, `traced_drained`: seam_oci's `HEXBENCH_IPCUTS=0`), and `pairs`, seam_oci's `ipc_uts_pairs` phase: the
lifecycle on a node with the pool and one with it drained, interleaved one by one, the first of each pair alternating.
All on fedora, Linux 6.19.10, `CPUSET=4-15`, N=1000, WARMUP=100, `LOAD_GATE=0` with Focal's and others' containers up
beside it; every round's counts confirm which it was: each pooled create took a pair, each drained one made its own.

**Run A**, create-breakdown-20261103T230157Z (unit hex-s4-job3-20261103T225951Z, tree 53330b0, 6 rounds, 23:01:57Z to
23:05:25Z, load 74.10 at the start, 38.45 at the end). Step 6's profile ran beside it: bpftrace on every context switch
and futex call, with perf, on CPUs 4-31 from 23:00:29Z to 23:03:15Z (unit hex-g5x-s6-fdh13-20261103T225919Z), then its
interleaved pairs pinned to 4-15 to about 23:06:30Z (hex-g5x-s6-v24-20261103T225919Z). Part 16, the init's clone into
its cgroup and namespaces, pooled p50 0.105 ms on against 0.123 drained, 0.854, short of the bar; part 17, which holds
the pair's two joins, 0.053 against 0.036; the whole create 0.818 against 0.831, 0.985, its rounds 0.913 to 1.045.

**Run B**, create-breakdown-20261103T231406Z (unit hex-s4-job4-20261103T231047Z, the tree of step 4's next commit, hex
e924de4229967190, seam_oci 6b751ac622d96411, 8 rounds, load 77.67 at the start, 38.36 at the end), with the refill thread
woken only once the pool is half empty and the pair left for the init's exec to close, as the pipes are. No timed part
of step 6's overlapped it; only its untimed gates and root lab ran, on 16-31 (hex-g5x-s6-fdh23-20261103T231544Z):

| | on p50 (ms) | drained p50 | on over drained, p50 | on p99 | drained p99 | on over drained, p99 |
|---|---|---|---|---|---|---|
| create | 0.816 | 0.978 | 0.834 | 2.763 | 3.561 | 0.776 |
| part 16 | 0.104 | 0.143 | 0.727 | 1.171 | 1.345 | 0.871 |
| part 17 | 0.052 | 0.041 | 1.268 | 0.204 | 0.168 | 1.214 |
| parts 16+17 | 0.157 | 0.186 | 0.844 | 1.365 | 1.425 | 0.958 |

Part 16 by round, on over drained: 0.708, 0.639, 0.561, 0.543, 0.777, 0.992, 0.844, 0.692. Interleaved, by round, the
median of the pairs' create differences, on minus drained, and the share of pairs the pool won: -0.0293 ms (0.58),
-0.0220 (0.57), -0.0275 (0.59), -0.0208 (0.57), -0.0343 (0.52), -0.0373 (0.63), -0.0171 (0.53), -0.0318 (0.55). Start's,
exec's and delete's medians stayed within -0.0025 to +0.0095 ms of zero, so nothing moved to them. So the clone sheds
about 39 us of the pair's making, the joins add about 11 back, and a create is 17 to 37 us faster in every round.

The two runs side by side, pooled p50 (ms):

| | run A on | run A drained | run A ratio | run B on | run B drained | run B ratio |
|---|---|---|---|---|---|---|
| part 16 | 0.105 | 0.123 | 0.854 | 0.104 | 0.143 | 0.727 |
| part 17 | 0.053 | 0.036 | 1.472 | 0.052 | 0.041 | 1.268 |
| create | 0.818 | 0.831 | 0.985 | 0.816 | 0.978 | 0.834 |

The pool's side was the same in both runs, part for part; what moved is the drained side: its clone took 0.123 ms in run
A and 0.143 in run B, at like loads at each run's start and end. So the ratio's gain from run A to run B came from the
drained creates, not from the pooled path getting faster, and the two changes between the runs left the pooled parts
where they were. That the drained clone's cost moves with the host is what its work predicts: making an ipc namespace
mounts its mqueue filesystem, and every new mount takes the global mount lock (Linux v6.19 `fs/namespace.c:1154-1166`,
`setup_mnt`), as it registers two sysctl tables under the sysctl lock, so it waits on whatever else mounts or registers
then. The criterion compares creates within one run, which run B meets and run A misses by 0.004.

The sustained phase at concurrency 32 through host.sh (`PHASES=oci RUNTIMES=libcrun HEXBENCH_PHASES=sustained`, 10,000
lifecycles a round, in process, `CPUSET=4-15`): the pool was dry for 0, 0 and 30 creates in run A's tree
(out/20261103T225951Z) and 0, 25 and 0 in run B's (out/20261103T231112Z), at most 0.30% of creates. The socket driver's
node runs the same pool, whose counts the socket doesn't carry (as the prepared `/dev` pool's).

## The reader on the integrated tree, reuse on and off (goal 5 extended, the tree step 7 measures; 2026-11-04)

Step 7 measures on `cp/t7-g5x-integrate`, so its reader must cut that tree's creates. At fcdad98 it cut none. e2c130d
(cp/t7-ext-s3 into step 6) had taken step 3's breakdown files whole, losing step 5's record-once cuts and the
opens' kprobes, and the reader took an owner's create's first mkdir for its cgroup. Two probe runs on fedora showed
it, each refusing every traced create:
- reuse off: create-breakdown-20261104T143904Z, "lacks record write 2's file's open entry";
- reuse on (`HEXBENCH_OWNER`): create-breakdown-20261104T143859Z, "the first mkdir is the box's cgroup (it made
  '/run/hex-node/<key>-l000001')".

With step 5's three commits re-applied (cherry-picks -x of f99b755, 933cfa0, 50bd7a2), the kprobes enabled again
(fd11626) and the chosen cgroup cut (2c72226), both kinds of run cut every traced create whole. Fedora, CPUs 4-15,
N=1000, WARMUP=100, 3 rounds, the tree's release binaries (hex sha256 633cb8d3658e908e, dynamically linked, so not of
record; seam_oci 87def3de2a860cf1). Load 45-57 and Docker's co-tenants as each env.txt lists. The figures are a
check of the reader, not a measure of the tree.

| run | reuse | path the reader took | probes cut whole | rounds' traced creates cut whole | cgroups |
|---|---|---|---|---|---|
| create-breakdown-20261104T144934Z | on | pruned, record once, chosen cgroup | 2 of 2, 10 of 10 | 1,500 of 1,500 | every one handed on |
| create-breakdown-20261104T145145Z | off | pruned, record once, the cgroup's mkdir | 2 of 2, 10 of 10 | 1,500 of 1,500 | made, each its own |

In the reuse-on run every traced create was handed its owner's idle cgroup. Only the warm-up's first box makes a fresh
slot, and it isn't traced, so a fresh slot's cut rests on the selftest (`create_breakdown.py test`).

Since 5bc0a2c a pruned seed reads the spec the runtime checked with a pread of its sealed memfd, and opens no
config.json. The reader then found no cut there and refused every create on the tree's own binaries (the hooks
agent's create-breakdown-20261104T160938Z at 891245f). At 0b2e18b the rounds trace pread64's exit and the seed's part
ends at its last pread before the clone. Two runs on that tree's binaries (hex sha256 8ae605e02c4e63b2, dynamically
linked; seam_oci dff934f4187caac9; CPUs 4-15, N=1000, 3 rounds, load 47-60), again a check of the reader, not of
record:

| run | reuse | path the reader took | probes cut whole | rounds' traced creates cut whole | seed reads cut at a pread |
|---|---|---|---|---|---|
| create-breakdown-20261104T162204Z | on | pruned, record once, chosen cgroup (handed on) | 2 of 2, 10 of 10 | 1,500 of 1,500 | every one |
| create-breakdown-20261104T162704Z | off | pruned, record once, the cgroup's mkdir | 2 of 2, 10 of 10 | 1,500 of 1,500 | every one |

## Goal 5 extended step 7: goal 5's create measured against its bar (2026-11-04, the integrated tree 1714f72)

Fedora (kiln-a094a12d, i9-14900HX, Linux 6.19.10), the integrated tree at 1714f72 (steps 1-6, main's fd fix, the turns,
the hooks, the checked spec), static-glibc hex sha256 5c2fc732d92a41ee (static-pie linked), seam_oci decb8a6e69dc67e4,
libcrun 1.30.1 in process. Every timed part ran on CPUs 4-15 under the shared lock, --tmpfs /run as host.sh has it,
containers at 8 GB and oom-score-adj 1000, noisy (LOAD_GATE=0), one after another in unit
hex-integrate-step7-20261104T165634Z. Step 6's reuse on means HEXBENCH_OWNER set, every box of one owner; off, no owner.
The host's clock (host_clock.sh, a 100,000-step burst, p50): before, P-cores 0.112 ms at rest and 0.110 held, E-cores
0.201 and 0.199, load 31.64; after, P-cores 0.098 and 0.101, E-cores 0.187 and 0.189, load 12.72.

### host.sh's OCI pass: lifecycle and start_true, 2 rounds of 10,000, best of 2

| run | reuse | load at start, end | Hex create p50 / libcrun | ratio | Hex create p99.9 / libcrun | ratio | Hex create+start of `true` p50 / libcrun | ratio | Hex create+start of `true` p99.9 / libcrun | ratio |
|---|---|---|---|---|---|---|---|---|---|---|
| out/20261104T170413Z | on | 106.50, 20.28 | 0.50 / 3.34 ms | 0.150x | 0.88 / 24.36 ms | 0.036x | 0.53 / 2.42 ms | 0.219x | 1.25 / 4.69 ms | 0.267x |
| out/20261104T172313Z | off | 20.28, 12.72 | 0.83 / 2.33 ms | 0.356x | 3.94 / 4.20 ms | 0.938x | 0.66 / 2.50 ms | 0.264x | 4.23 / 5.08 ms | 0.833x |

The ratios are Hex's figure over libcrun's as table.md prints them (ms to two places). Both passes recorded 0 errors
in every round's driver (libcrun, the seam in process and over the socket). The reuse-on pass handed on 10,098 of
10,100 cgroups a round (2 fresh); the reuse-off pass took 10,100 and 10,099 network namespaces from the pool, 1 made on
the path. Over the socket (hex-seam-socket), reuse on: create p50 0.83, p99.9 2.41 ms; create+start of `true` p50 0.83,
p99.9 2.12. Reuse off: create p50 0.73, p99.9 3.53; create+start of `true` p50 0.76, p99.9 4.53.

### The create breakdown, seed path (pruned), N=1000, 3 rounds

| run | reuse | load at start, end | Hex create p50 (seam, pooled) | libcrun's | ratio | parts' p50s summed | over the creates between (p50) | events lost |
|---|---|---|---|---|---|---|---|---|
| create-breakdown-20261104T165844Z | on | 31.59, 47.75 | 1.924 | 14.241 | 0.135x | 1.571 | 0.808 (1.945) | 0 |
| create-breakdown-20261104T170123Z | off | 47.75, 106.50 | 1.989 | 14.551 | 0.137x | 1.731 | 0.868 (1.994) | 0 |

The parts' p50s summed are 80.8% and 86.8% of create's p50 in the same run: outside step 7's 10%, its criterion not
met at this load. Their means sum to the creates' mean span exactly (2.381 and 2.257 ms), so every part is cut; what
the p50s miss is skew. The init's clone (part 11) has a mean of 0.840 and 0.631 against p50s of 0.555 and 0.527, and the
handshake (part 17) 0.356 against 0.138 with reuse on. A sum of medians undercounts a sum of skewed parts. Both
breakdowns ran at loads of 31 to 106, where Hex's create through the breakdown's seam was about 2 ms and libcrun's
about 14, against 0.50 to 0.83 and 2.33 to 3.34 in the passes at 12 to 28 after them.

The parts by group, pooled p50 (ms), reuse on / off: the box's record 0.046 / 0.033; the cgroup, made and bounded (on:
an idle one read clean and handed on; off: mkdir and bounds) 0.264 / 0.174; the seed, the request and its clone's
preparing 0.110 / 0.094; the init's set-up to the exec FIFO 0.804 / 0.720, of it the init's clone into its cgroup and
namespaces 0.555 / 0.527 and its root's mounts 0.127 / 0.108; the handshake 0.138 / 0.511; the node's and the runtime's
own work 0.209 / 0.199.

### The parts criterion, best of N (the lead's call, 2026-11-04)

Two more breakdowns per reuse mode, alternating, at the same binaries (hex 5c2fc732d92a41ee, seam_oci decb8a6e69dc67e4),
each under the lock on CPUs 4-15, noisy, in unit hex-integrate-step7more-20261104T173958Z. The host's clock before:
P-cores 0.101 ms at rest and 0.107 held, E-cores 0.182 and 0.201, load 16.06; after: P-cores 0.106 and 0.101, E-cores
0.196 and 0.182, load 30.76. Every run is here, the two above among them. "Parts" is the pooled part p50s summed, "%" is
that over the creates between the traced ones (p50), and "means" is the part means summed beside the creates' mean
span, marker to marker.

| run | reuse | load at start, end | Hex create p50 (seam) / libcrun's | ratio | parts | creates between p50 | % | means / mean span | events lost | within 10% |
|---|---|---|---|---|---|---|---|---|---|---|
| create-breakdown-20261104T165844Z | on | 31.59, 47.75 | 1.924 / 14.241 | 0.135x | 1.571 | 1.945 | 80.8% | 2.381 / 2.381 | 0 | no |
| create-breakdown-20261104T170123Z | off | 47.75, 106.50 | 1.989 / 14.551 | 0.137x | 1.731 | 1.994 | 86.8% | 2.257 / 2.257 | 0 | no |
| create-breakdown-20261104T174001Z | on | 15.98, 20.95 | 0.656 / 2.635 | 0.249x | 0.780 | 0.742 | 105.1% | 0.928 / 0.928 | 0 | yes |
| create-breakdown-20261104T174116Z | off | 20.79, 23.86 | 0.639 / 2.175 | 0.294x | 0.692 | 0.675 | 102.5% | 0.855 / 0.855 | 0 | yes |
| create-breakdown-20261104T174221Z | on | 23.86, 22.38 | 0.419 / 1.858 | 0.226x | 0.475 | 0.456 | 104.2% | 0.519 / 0.519 | 0 | yes |
| create-breakdown-20261104T174319Z | off | 22.38, 31.95 | 0.631 / 3.158 | 0.200x | 0.617 | 0.597 | 103.3% | 0.960 / 0.960 | 0 | yes |

Step 7's criterion, the parts summed within 10% of create's p50 in the same run, is met: four of the six runs land
inside it (102.5% to 105.1%), in both reuse modes, at loads of 16 to 32. The two outside it ran at loads of 31 to 106,
where the parts' p50s sum short of the p50 (80.8%, 86.8%) while their means sum to the mean span exactly. The tracer
cuts every create whole at any load; the sum of medians undercounts once a few parts' tails grow (the init's clone,
the handshake).

The parts by group in the run closest to 100% in each mode, pooled p50 (ms): reuse on, create-breakdown-20261104T174221Z:
the box's record 0.024, the cgroup (an idle one read clean and handed on) 0.106, the seed's request 0.045, the init's
set-up 0.185, of it the init's clone 0.056 and its root's mounts 0.065, the handshake 0.012, the node's and the
runtime's own work 0.103. Reuse off, create-breakdown-20261104T174116Z: the record 0.031, the cgroup (mkdir 0.123,
bounds 0.035) 0.168, the seed's request 0.060, the init's set-up 0.249, of it the clone 0.096 and the mounts 0.085, the
handshake 0.018, the node's and the runtime's own work 0.166.
