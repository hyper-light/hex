# Where creates wait under concurrency

Goal 5 plan step 4's criterion 4 (decision 5 of `docs/design/runtime-architecture.md`; kiln plan
`01a1877e05c7748ca8865548aab76445`) asks the seam's sustained lifecycles a second at concurrency 32 to be at least 8x
its throughput at concurrency 1 in the same run (`seam_oci`'s throughput and sustained phases, through host.sh). The OCI
pass at ca45e1a read 4.93x in process and 6.31x over the socket (below, "The pass"). This probe finds what the
lifecycles wait on at concurrency 32 that they don't at 1, beside libcrun's at 32, and what holds it.

## How it runs

`create_serial.sh <hex> <seam_oci>` runs `create_serial_inside.sh` in a privileged container like the OCI pass's (a
private cgroup namespace, `/run` a tmpfs, pinned to CPUs 4-31), with network for dnf (bpftrace isn't on the host), on
the given binaries and host.sh's libcrun driver, each copied into the run's directory. A bpftrace program runs over one
workload at a time, its per-process probes filtered by name to the runtime's own processes: `seam_oci` and `hex` for
Hex's seam (the node agent in process, its seeds, each init until it execs), `libcrun-driver` for libcrun. The
workloads share one lifecycle, the throughput phases' (create, start and delete of `true`):

- **hex.c1**: `seam_oci`'s start_true, one lifecycle at a time, 520 of them;
- **hex.c32**: `seam_oci`'s sustained phase, concurrency 32, 5,000;
- **crun.c1** and **crun.c32**: `oci_bench.py`'s start_true and sustained phases on libcrun in process, the same counts;
- **hex.c32nonet** (run 4 on): Hex's sustained phase on a `true` bundle without a network namespace of its own.

Two programs (`MODE`):

- **offcpu**: off-CPU time by kernel stack (`finish_task_switch`), an on-CPU profile at 999 Hz, and each directory
  operation (`vfs_mkdir`, `vfs_rmdir`, `vfs_unlink`, `vfs_mknod`) by the filesystem of the directory it changes. The
  reader sorts the off-CPU time into the locks a create can wait on and the waits for work.
- **locks**:
  - each `vfs_mkdir` and `vfs_rmdir` timed by its directory, whose `i_rwsem` is held exclusively across it
    (`filename_create` and `do_rmdir` through `__start_dirop`, Linux v6.19 `fs/namei.c:2873`);
  - each wait in an `rw_semaphore`'s slow path timed by the directory whose `i_rwsem` it is (an rwsem that isn't an
    inode's decodes to no filesystem; a decode whose garbage holds a comma or bracket drops out, so those totals are
    lower bounds);
  - each lookup that missed the dcache (`lookup_slow`) by directory and name;
  - for every process, `cgroup_mutex`'s takers timed whole, their wait for the lock and their hold together
    (`cgroup_mkdir`, `cgroup_rmdir`, `cgroup_kill_write`, `cgroup_bpf_prog_attach`, and the work a removed cgroup
    queues to kworkers: `css_killed_work_fn`, `css_release_work_fn`, `cgroup_bpf_release`), and a fork from
    `cgroup_can_fork` to `cgroup_post_fork`'s return, which with `CLONE_INTO_CGROUP` waits for and holds
    `cgroup_mutex` throughout;
  - from run 4, each wait for `cgroup_mutex` timed alone, from its `lock:contention_begin` tracepoint to its
    `contention_end`;
  - the state of `cgroup_mutex` and of the runtime's cgroup directory's `i_rwsem`, sampled on every CPU at 999 Hz. From
    run 4 the lock words are decoded with their flags: a mutex is held while its owner word names a task, and handed
    off while only its waiter and hand-off flags are set, free until a woken waiter runs; an `i_rwsem` is write-held,
    read-held (readers counted), handed off (only flags), or free. Run 3 took any nonzero word for held, so it counted
    a lock handed off as held. The samples are read over the 100 ms buckets in which the runtime made or removed at
    least half as many cgroups as the median bucket.

`create_serial.py` reads them, per lifecycle. Not a timed pass: the load is recorded, not gated, and the probes cost
each lifecycle some (a kprobe on each mkdir, rmdir, slow lookup and lock slow path; from run 4 the lock contention
tracepoints, which fire for every contended lock on the host), so the rates under them are below the pass's. A
taker's time includes its wait for the lock at concurrency 1 too: the kworkers' work for one lifecycle's removed cgroup
overlaps the next lifecycle's, and they wait on each other.

## The pass (2026-10-30, fedora): 20261030T041910Z, at ca45e1a, not of record

- **Host and load:** fedora-2 (Intel Core i9-14900HX, 32 threads, 62 GiB; Linux 6.19.10-300.fc44.x86_64), pinned to
  CPUs 4-31. 2026-10-30T04:19:10Z. Run with LOAD_GATE=0, so not of record by CLAUDE.md §7: the 1-minute load was 21.81
  at the start and 21.02 at the end; a burst took 0.106 and 0.105 ms at rest on a P-core (`probes/host_clock.sh`,
  before and after).
- **Binaries:** `hex 0.1.0` of ca45e1a (on cp/t7-g5-s4), sha256 19ce3ac5229fe2f7, static-pie; `seam_oci` of ca45e1a,
  sha256 476faa01049a8797; libcrun 1.30.1 in process, driver sha256 5c356f2139adc232.
- **Run:** `~/.cache/hex-bench/out/20261030T041910Z` on fedora: `host.sh` with PHASES=oci RUNTIMES=libcrun ROUNDS=2
  N=1000 WARMUP=100 LIFECYCLES=2000 HEXBENCH_SUSTAINED=10000 and HEXBENCH_PHASES "lifecycle throughput sustained
  start_true"; `/run` a tmpfs. Best of the two rounds, as its `table.md` gives them:

| | libcrun | Hex in process | Hex over the socket |
|---|---|---|---|
| throughput at concurrency 1 (lifecycles/s) | 73 | 394 | 316 |
| throughput at concurrency 8 | 627 | 1,539 | 1,556 |
| throughput at concurrency 32 | 1,562 | 1,952 | 1,954 |
| sustained at concurrency 32 (10,000 lifecycles) | 1,593 | 1,942 | 1,994 |
| sustained over concurrency 1 | 21.8x | 4.93x | 6.31x |
| create p50 (ms) | 2.97 | 1.50 | 1.62 |
| create+start of `true` p50 (ms) | 2.86 | 1.44 | 1.81 |
| delete p50 (ms) | 10.34 | 0.76 | 0.82 |

The sustained phases found the network-namespace pool dry for 65 and 71 of their 10,000 creates in process and 137 and
30 over the socket. libcrun's ratio is high because its lifecycle alone is slow: its delete polls for the container's
end.

## Run 0 (2026-10-30, fedora): offcpu's first form, at d12c3fe

`~/.cache/hex-bench/out/create-serial-20261030T031750Z`: hex fe92ee109290392c and seam_oci 57c313c0ca84994e of d12c3fe
(on cp/t7-g5-s4), the harness at f91ce70; 2026-10-30T03:17:50Z, the 1-minute load 28.73 at the start. At concurrency
32 Hex's lifecycles waited 18.976 ms each on directories' `i_rwsem`s, 5.710 of it in `getdents64` listing the box root
(each create listed it to count its boxes) behind the `mkdir`s and `rmdir`s holding it. ca45e1a counts the boxes by the
box root's link count instead (`BoxRoot::count`), and run 1 has no such wait.

## Run 1 (2026-10-30, fedora): offcpu, at ca45e1a

- **Host and load:** as the pass's. 2026-10-30T04:26:44Z; the 1-minute load 12.16 at the start and 15.00 at the end.
  Up beside it: four long-lived containers (`issue556oss-control-plane`, `buildx_buildkit_kagent-builder-v0.23.00`,
  `kind-registry`, `kiln-t10-minio`).
- **Binaries:** the pass's.
- **Run:** `~/.cache/hex-bench/out/create-serial-20261030T042644Z`, the harness at ca45e1a with this probe uncommitted
  (its files are `bt.hex.c1.txt`, `bt.hex.c32.txt` and `bt.crun.c32.txt`; the mode's prefix came later). Under it Hex's
  sustained phase ran 1,936/s and libcrun's 1,469/s. The two concurrency-32 profiles filled bpftrace's default map
  (4,096 keys; the program now asks 262,144), so stacks first seen after it filled went uncounted: the classes are
  lower bounds, the top sites were seen early.

Off-CPU ms per lifecycle:

| What the lifecycle waits on | hex.c1 | hex.c32 | crun.c32 |
|---|---|---|---|
| a directory's `i_rwsem` | 0.000 | 8.459 | 4.375 |
| `cgroup_mutex` | 0.027 | 1.332 | 0.954 |
| the mount namespace's lock | 0.001 | 1.345 | 0.886 |
| an RCU grace period (umount) | 0.024 | 0.175 | 0.792 |
| a futex (the runtime's own locks) | 0.000 | 0.601 | 0.000 |
| work: a socket | 5.866 | 7.248 | 6.126 |
| work: a pipe or FIFO | 1.199 | 3.110 | 0.308 |
| work: a poll | 0.813 | 2.084 | 0.000 |
| work: a child | 0.000 | 0.013 | 0.004 |
| a sleep (the runtime's own polling) | 0.000 | 0.000 | 4.069 |
| other | 0.093 | 0.398 | 3.806 |

The directory waits at concurrency 32 are Hex's `rmdir` (2.675 ms), `mkdir` (2.233) and slow lookups (1.626 and
1.518).

## Run 2 (2026-10-30, fedora): locks, at ca45e1a

- **Host and load:** as run 1's. 2026-10-30T04:41:51Z; the 1-minute load 13.56 at the start and 23.16 at the end; the
  same four containers up.
- **Binaries:** the pass's.
- **Run:** `~/.cache/hex-bench/out/create-serial-locks-20261030T044151Z`, the harness at ca45e1a with this probe
  uncommitted, before the lock sampling. Under it Hex's sustained phase ran 1,850/s (5,000 lifecycles in 2.702 s) and
  libcrun's 1,475/s (in 3.390 s).

Each runtime makes every container's cgroup in one directory: Hex in its box root, `/sys/fs/cgroup/hex-oci-seam`;
libcrun in the bench container's cgroup root. That directory, per lifecycle:

| | hex.c1 | hex.c32 | crun.c1 | crun.c32 |
|---|---|---|---|---|
| `mkdir`s in it, and their time (ms) | 1.00, 0.168 | 1.00, 0.282 | 1.00, 0.134 | 1.00, 0.222 |
| `rmdir`s in it, and their time (ms) | 1.00, 0.104 | 1.00, 0.205 | 2.99, 0.055 | 2.17, 0.210 |
| waits on its `i_rwsem` to write (ms) | 0.000 | 4.714 | 0.000 | 2.916 |
| waits on its `i_rwsem` to read (ms) | 0.000 | 3.042 | 0.000 | 1.430 |
| lookups in it that missed the dcache, of the new cgroup's directory | 2 | 2 | 2 | 2 |

At concurrency 32 Hex's `mkdir`s and `rmdir`s held the box root exclusively 0.487 ms a lifecycle, 90% of the run's
time at 1,850/s, and `cgroup_mkdir` and `cgroup_rmdir` inside them, each waiting for and holding `cgroup_mutex`, took
0.472 ms of it (87%).

`cgroup_mutex`'s takers, each one's time in us, wait and hold together (and how many a lifecycle), concurrency 1 then
32:

| Taker | Hex | libcrun |
|---|---|---|
| `cgroup_mkdir`, the runtime's | 161.1, 276.4 (1 a lifecycle) | 129.2, 217.1 (1) |
| `cgroup_rmdir`, the runtime's | 93.1, 196.0 (1) | 14.0, 92.3 (2.99, 2.17) |
| `cgroup_kill_write`, the runtime's | 58.8, 546.9 (1) | 38.0, 358.8 (1.99, 1.54) |
| `cgroup_bpf_prog_attach`, the runtime's | 46.1, 369.9 (1) | none seen |
| a fork into the cgroup (`CLONE_INTO_CGROUP`) | 20.6, 545.0 (1) | 49.1, 486.0 (1) |
| `css_killed_work_fn`, a kworker's | 125.2, 353.0 (5.02, 5.00) | 75.8, 196.6 (8.01, 8.00) |
| `css_release_work_fn`, a kworker's | 24.5, 286.3 (6.02, 6.00) | 27.2, 162.0 (9.00, 8.99) |
| `cgroup_bpf_release`, a kworker's | 12.0, 587.7 (1.01, 1.00) | 107.0, 622.3 (1.00, 1.00) |

Beside the cgroup directory, Hex's waits at concurrency 32 are on the bench's one bundle's `rootfs`, the mountpoint of
every container's root (1.133 ms to write; libcrun 0.436), and its mountpoints `proc`, `usr` and `dev` (0.325), on
rwsems that aren't a directory's (1.285 to write, 0.035 to read), among them the mount namespace's `namespace_sem`, and
on the node's record directory on the bench container's overlay (0.601 to write, 0.070 to read; the same 0.601 as run
1's futexes, by chance).

## Run 3 (2026-10-30, fedora): locks with the first form of the lock sampling, at e302ac8

- **Host and load:** as run 1's. 2026-10-30T04:56:17Z, beside Focal's fleet test (34 `s5-focal*` containers) and a
  burst of kiln CI jobs: the 1-minute load 74.79 at the start and 36.23 at the end. Under the probe and that load Hex's
  sustained phase ran 1,077/s and libcrun's 867/s.
- **Binaries:** `hex` as the pass's (e302ac8 changes no line it's built from; the build's sha256 is the same,
  19ce3ac5229fe2f7); `seam_oci` of e302ac8, sha256 cf0fa3001bbf1d87, which also reads the create seeds' counts.
- **Run:** `~/.cache/hex-bench/out/create-serial-locks-20261030T045617Z`, the harness at e302ac8.

Its sampler took any nonzero lock word for a held lock, so each share counts a lock handed off as held: each is an
upper bound on held, and its "read" share is mostly hand-offs (two slow lookups a lifecycle would need about 53 us of
read hold each to fill it).

| Share of the busy time, nonzero lock word | hex.c1 | hex.c32 | crun.c1 | crun.c32 |
|---|---|---|---|---|
| `cgroup_mutex` | 21.7% | 90.6% | 10.9% | 76.4% |
| the cgroup directory's `i_rwsem`, writer bit set | 7.4% | 78.8% | 1.9% | 56.2% |
| the cgroup directory's `i_rwsem`, any other nonzero count | 0.2% | 11.5% | 0.1% | 4.6% |

The takers and the directory's waits are run 2's, longer under the load: at concurrency 32 the box root's `mkdir`
and `rmdir` took 0.417 and 0.303 ms a lifecycle and its waits 6.670 ms to write and 3.885 to read, and a fork into the
cgroup took 1.014 ms.

## Run 4 (2026-10-30, fedora): locks, the lock words decoded and the waits timed, at 72797aa

- **Host and load:** as run 1's. 2026-10-30T06:04:24Z; the 1-minute load 7.32 at the start and 25.53 at the end; the
  four long-lived containers up.
- **Binaries:** `hex 0.1.0` of 87f4aff (on cp/t7-g5-s4), sha256 32ef384ba04ff6ea, static-pie, built in the pass image
  (`hexbench/pass:44`, its glibc-static); `seam_oci` of 72797aa, sha256 cb3a2089e909cc7e.
- **Run:** `~/.cache/hex-bench/out/create-serial-locks-20261030T060424Z`, the harness at 72797aa. Under it Hex's
  sustained phase ran 1,359/s, without a network namespace 1,340/s, and libcrun's 1,136/s: the contention tracepoints
  cost every contended lock on the host, so the rates sit lower than run 2's.

| Share of the busy time | hex.c1 | hex.c32 | hex.c32nonet | crun.c1 | crun.c32 |
|---|---|---|---|---|---|
| `cgroup_mutex` held | 24.0% | 83.4% | 83.0% | 8.2% | 74.6% |
| `cgroup_mutex` handed off | 0.3% | 6.8% | 6.2% | 0.0% | 5.1% |
| the cgroup directory's `i_rwsem` write-held | 8.6% | 78.2% | 74.6% | 1.8% | 62.0% |
| the cgroup directory's `i_rwsem` read-held | 0.2% | 15.4% | 17.7% | 0.1% | 5.5% |
| the cgroup directory's `i_rwsem` handed off | 0.0% | 4.8% | 7.4% | 0.0% | 4.3% |

Waits for `cgroup_mutex` alone, per lifecycle (how many, each one's mean in us, ms in all):

| Waiter | hex.c1 | hex.c32 | hex.c32nonet | crun.c1 | crun.c32 |
|---|---|---|---|---|---|
| the runtime's | 0.49, 459.3, 0.224 | 3.76, 476.3, 1.789 | 3.50, 503.3, 1.763 | 0.26, 598.2, 0.153 | 4.11, 404.7, 1.662 |
| other processes' (the kworkers) | 2.24, 321.2, 0.719 | 11.09, 370.0, 4.104 | 10.97, 348.9, 3.827 | 0.83, 287.6, 0.240 | 15.25, 232.7, 3.549 |

The seeds over Hex's sustained phases: 15 started and 15 waited for of 5,000 creates, 15 busy at once at most; without
the network namespace 16 started, 23 waited, 15 at most.

## Run 5 (2026-10-31, fedora): locks, the device program held (goal 5 extended, step 1), at 7953067

- **Why:** goal 5 extended, step 1's third criterion: `cgroup_mutex` holds per default-policy lifecycle at most 16. The
  node holds each policy's program and the default policy's sits on the oci boxes' root, attached by the node's start,
  so a default-policy create attaches nothing.
- **Host and load:** as run 1's. 2026-10-31T21:34:36Z; the 1-minute load 12.56 at the start and 62.29 at the end, a
  burst of kiln CI during the concurrency-32 phases. Under the probe and that load Hex's sustained phase ran 532/s,
  without a network namespace 986/s, and libcrun's 571/s, so the shares of the time below aren't run 4's to compare.
- **Binaries:** `hex 0.1.0` of 4e518c0, sha256 2a475c14f822c00d, static-pie, built in the Fedora 44 pass image;
  `seam_oci` of 4e518c0, sha256 8a83b136fdcb4b8c.
- **Run:** `~/.cache/hex-bench/out/create-serial-locks-20261031T213436Z`, the harness at 7953067; read with
  `create_serial.py` over 520 lifecycles at concurrency 1 (500 timed, 20 untimed) and 5,000 at 32.

`cgroup_mutex`'s takers, each one's time in us, wait and hold together, and how many a lifecycle, concurrency 1 then
32 (run 4's counts beside them):

| Taker | Hex | run 4's counts |
|---|---|---|
| `cgroup_mkdir`, the runtime's | 160.0, 434.0 (1.00, 0.89) | 1 |
| `cgroup_rmdir`, the runtime's | 86.5, 348.6 (1.00, 0.90) | 1 |
| `cgroup_kill_write`, the runtime's | 50.5, 1462.8 (1.00, 0.90) | 1 |
| `cgroup_bpf_prog_attach`, the runtime's | 5.3, none seen (0.00 at concurrency 1) | 1 |
| a fork into the cgroup (`CLONE_INTO_CGROUP`) | 27.8, 1315.6 (1.00, 0.90) | 1 |
| `css_killed_work_fn`, a kworker's | 107.0, 322.4 (5.03, 4.58) | 5.02, 5.00 |
| `css_release_work_fn`, a kworker's | 34.8, 255.3 (6.02, 5.50) | 6.02, 6.00 |
| `cgroup_bpf_release`, a kworker's | 54.3, 710.6 (1.01, 0.92) | 1.01, 1.00 |

- **The attach is gone.** At concurrency 1 the takers sum to 16.06 a lifecycle against run 4's 17.05: the device
  program's attach is 0.00 a lifecycle where it was 1 (the one attach left is the node's start's, at the oci boxes'
  root), and the rest are as they were. These takers count every kworker's work on the host: `cgroup_bpf_release`,
  which runs once for each cgroup removed, ran 1.01 times a lifecycle, so the 0.06 above 16, as run 4's 0.05 above
  17, is the kernel's work for cgroups besides the run's 520 boxes. Run 6 counts the boxes' own holds alone. At
  concurrency 32 no attach was seen at all. The counts there
  sit under 1 a lifecycle: the trace saw 4,462 `cgroup_mkdir`s of the phase's 5,000 lifecycles, with no event
  reported lost, so they're read against `cgroup_mkdir`'s, which each lifecycle takes once.
- **The shares of the time** the lock was held: 19.8% at concurrency 1, 59.0% at 32 (handed off 9.9% more), 71.6%
  without a network namespace (7.5%); libcrun's 7.6% and 51.9% (8.9%). Under this run's load the rates were well
  under run 4's, so these shares don't measure the attach's absence; step 6's measure does, with its holds.

## Run 6 (2026-10-31, fedora): locks, the holds for the boxes' own cgroups counted alone, at c84fb64

- **Why:** run 5's takers count the kernel's work for every cgroup removed on the host, and the node's start's own,
  as the lifecycles'. The probe now also counts each taker, the attach and the fork into a cgroup for the boxes' own
  cgroups alone (`@box`: those directly under seam_oci's `/sys/fs/cgroup/hex-oci-seam/.oci`, found from the work's or
  the call's cgroup), and `create_serial.py` reports them per box made.
- **Host and load:** as run 1's. 2026-10-31T21:55:58Z; the 1-minute load 39.91 at the start and 23.62 at the end.
  Hex's sustained phase ran 1,429/s, without a network namespace 1,520/s, and libcrun's 1,268/s.
- **Binaries:** run 5's, of 4e518c0 (`hex` 2a475c14f822c00d, `seam_oci` 8a83b136fdcb4b8c).
- **Run:** `~/.cache/hex-bench/out/create-serial-locks-20261031T215558Z`, the harness at c84fb64.

| `cgroup_mutex` holds for the box's own cgroup, per box made | hex.c1 (520 boxes) | hex.c32 (5,000) | hex.c32nonet (5,000) |
|---|---|---|---|
| `cgroup_mkdir` | 1.000 | 1.000 | 1.000 |
| `cgroup_rmdir` | 1.000 | 1.000 | 1.000 |
| `cgroup_kill_write` | 1.000 | 1.000 | 1.000 |
| `cgroup_bpf_prog_attach` | 0 | 0 | 0 |
| a fork into the cgroup (`CLONE_INTO_CGROUP`) | 1.000 | 1.000 | 1.000 |
| `css_killed_work_fn`, a kworker's | 5.000 | 5.000 | 5.000 |
| `css_release_work_fn`, a kworker's | 5.996 | 5.999 | 5.999 |
| `cgroup_bpf_release`, a kworker's | 1.000 | 1.000 | 1.000 |
| in all | 15.996 | 15.999 | 15.999 |

- **A default-policy box's lifecycle takes the lock 16 times, where run 4's took 17:** its cgroup's `mkdir`, `rmdir`
  and `cgroup.kill`, its init's clone into it, and the kernel's work for it once removed (five csses offlined, six
  released, its BPF state released), and no attach. Each count is a whole number a box but the six releases, of which
  a box's last ones hadn't run when the trace ended (3,118 of 3,120 at concurrency 1, 29,994 of 30,000 at 32).
- **Host-wide**, the run's takers sum to 16.00 a lifecycle at concurrency 1 (`css_killed_work_fn` 5.00,
  `css_release_work_fn` 6.00, `cgroup_bpf_release` 1.00, and the runtime's four), the host making fewer other
  cgroups than in run 5, and the node's start's one attach is 0.00 a lifecycle.

## What the runs find

- **At concurrency 32 Hex's rate is what `cgroup_mutex` admits, one lock for the whole host.** In run 2, under the
  lighter probe, the box root's exclusive holds filled 90% of the run's time (0.487 ms a lifecycle at 1,850/s), and
  `cgroup_mkdir` and `cgroup_rmdir` inside them, each waiting for and holding `cgroup_mutex`, 87%. In run 4, with the
  lock words decoded, `cgroup_mutex` was held 83.4% of the time and handed off 6.8% more: it was free for less than a
  tenth of the run. The runtime's own takers waited for it 1.789 ms a lifecycle, and the kworkers' work for the
  removed cgroup 4.104 ms more.
- **It is the lock, not the network namespaces.** Without a network namespace of its own Hex's sustained phase ran
  1,340/s against 1,359/s with one, the lock held 83.0% of the time against 83.4% (run 4).
- **Hex's lifecycle takes the lock 17 times.** The runtime's `mkdir` and `rmdir` of the container's cgroup, its
  `cgroup.kill`, its device program's attach and its clone into the cgroup; and the kernel's own work for the removed
  cgroup in kworkers: five csses offlined, six released, and its BPF state released (`cgroup_bpf_release` runs for
  every removed cgroup, a program attached or not; Linux v6.19 `kernel/bpf/cgroup.c:314`). Inside the lock
  `cgroup_mkdir` and `cgroup_rmdir` also take the hierarchy's `kernfs_rwsem` to write, for each interface file they
  make and once to remove them (`fs/kernfs/dir.c:796`, `1554`), and every lookup in the cgroup filesystem takes it to
  read as it revalidates (`1145-1146`, `1167`, `1185`).
- **The cgroup directory's `i_rwsem` is held as long as the lock inside it.** `mkdir` and `rmdir` hold it exclusively
  while `cgroup_mkdir` and `cgroup_rmdir` wait for and hold `cgroup_mutex` (`cgroup_kn_lock_live`), so the box root
  was write-held 78.2% of the time (run 4), and each lifecycle's two slow lookups of its new cgroup's directory wait
  behind those holds: the 3.042 ms to read in run 2.
- **libcrun contends on the same lock.** Its lifecycle takes `cgroup_mutex` more often (more csses, two kills, three
  rmdirs at concurrency 1), and at its lower rate the lock was held 74.6% of its busy time (run 4). It isn't bound by
  it as Hex is: it leaves the lock free a fifth of the time, and spends 4.069 ms of each lifecycle in its own polling
  sleeps (run 1).
- **Hex's own waits are not the ceiling.** Its futexes (0.601 ms per lifecycle, run 1, a lower bound) and its record
  directory (0.671, run 2) are each under a tenth of the cgroup directory's (7.756: 4.714 to write and 3.042 to read,
  run 2).
- **The ceiling moves with the host's load.** A lock handed off stays unusable until its woken waiter runs, and that
  wait grows with the host's other work, as the hand-offs' share does; so the rate at concurrency 32, and the ratio
  with it, depend on the load in a way the rate at concurrency 1 doesn't. The pass ran at a load of 21.81.
- **So the 8x isn't in reach of this step.** The rate at concurrency 32 is what `cgroup_mutex` admits, however fast a
  lifecycle runs alone, and the faster one runs alone the further 8x of it sits above that. Fewer holds of the lock per
  lifecycle would raise the ceiling:
  - a cgroup reused from one box to the next rather than made and removed for each, which takes the `mkdir`, the
    `rmdir` and the kworkers' 12 holds off it;
  - fewer controllers enabled for a box: each costs an offline and a release, and its interface files;
  - one device program attached at the box root for boxes that share a policy, in place of one attach per box;
  - a kill that doesn't go through `cgroup.kill`.
- **What wouldn't.** A pool that makes cgroups ahead and removes them after use takes the lock as often: under a
  sustained rate it moves no hold. Under a burst it can move up to 14 of the 17 out of the burst, if the making comes
  before it and the removal after. Spreading the boxes' cgroups over several parent directories would part their
  `i_rwsem`s but not `cgroup_mutex`: with the lock free under a tenth of the time (run 4) it would add about a tenth at
  most, short of 8x.
