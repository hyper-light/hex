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

## Run 5 (2026-10-31, fedora): locks, the device program held (goal 5 extended, step 1), at 9669133

- **Why:** goal 5 extended, step 1's third criterion: `cgroup_mutex` holds per default-policy lifecycle at most 16. The
  node holds each policy's program and the default policy's sits on the oci boxes' root, attached by the node's start,
  so a default-policy create attaches nothing.
- **Host and load:** as run 1's. 2026-10-31T21:34:36Z; the 1-minute load 12.56 at the start and 62.29 at the end, a
  burst of kiln CI during the concurrency-32 phases. Under the probe and that load Hex's sustained phase ran 532/s,
  without a network namespace 986/s, and libcrun's 571/s, so the shares of the time below aren't run 4's to compare.
- **Binaries:** `hex 0.1.0` of 4e518c0, sha256 2a475c14f822c00d, static-pie, built in the Fedora 44 pass image;
  `seam_oci` of 4e518c0, sha256 8a83b136fdcb4b8c. 4e518c0 is 32db274's code with a CLAUDE.md line, a commit the
  branch has since dropped, kept as cp/t7-g5x-s1-measured.
- **Run:** `~/.cache/hex-bench/out/create-serial-locks-20261031T213436Z`, the harness at 9669133 (its env.txt names
  7953067: the same commit before the branch dropped a CLAUDE.md change, kept as cp/t7-g5x-s1-measured); read with
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

## Run 6 (2026-10-31, fedora): locks, the holds for the boxes' own cgroups counted alone, at fb007d9

- **Why:** run 5's takers count the kernel's work for every cgroup removed on the host, and the node's start's own,
  as the lifecycles'. The probe now also counts each taker, the attach and the fork into a cgroup for the boxes' own
  cgroups alone (`@box`: those directly under seam_oci's `/sys/fs/cgroup/hex-oci-seam/.oci`, found from the work's or
  the call's cgroup), and `create_serial.py` reports them per box made.
- **Host and load:** as run 1's. 2026-10-31T21:55:58Z; the 1-minute load 39.91 at the start and 23.62 at the end.
  Hex's sustained phase ran 1,429/s, without a network namespace 1,520/s, and libcrun's 1,268/s.
- **Binaries:** run 5's, of 4e518c0 (`hex` 2a475c14f822c00d, `seam_oci` 8a83b136fdcb4b8c).
- **Run:** `~/.cache/hex-bench/out/create-serial-locks-20261031T215558Z`, the harness at fb007d9 (its env.txt names
  c84fb64: the same commit before the branch dropped a CLAUDE.md change, kept as cp/t7-g5x-s1-measured).

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

Runs 7 to 10 ran on CPUs 4-31, P- and E-cores, as the harness runs by default. Their counts and shares are each
within one run, so they stand as measured (the lead, 2026-11-03T00:48Z). Their absolute rates and times aren't
comparable with the records on 4-15, the P-core threads, from create_breakdown.md's run 4 on. Run 11 repeats run 7 on
4-15, at f7c4065.

## Run 7 (2026-11-03, fedora): locks, one owner's boxes, each handed the cgroup its owner's earlier box left

- **Why:** goal 5 extended, step 6's first criterion: `cgroup_mutex` holds per lifecycle on the reuse path, one owner's
  boxes, with step 1 in, at most 3 at concurrency 32. Every box seam_oci made named the owner `bench-owner`
  (`HEXBENCH_OWNER`), its bundle's process holding Docker's default capabilities (a box that keeps all of root's never
  shares a cgroup), and its delete ended it through its init's pidfd.
- **Host and load:** fedora (Intel Core i9-14900HX, 32 threads, the container pinned to 4-31), Linux
  6.19.10-300.fc44.x86_64, 2026-11-03T01:16:43Z; the 1-minute load 38.07 at the start and 23.67 at the end, two CI jobs
  beside it. Hex's sustained phase ran 2,394/s, without a network namespace 2,334/s, under the probes.
- **Binaries:** built on fedora from the step's tree (the code of `a6f32f1`, but for a doc comment): `hex`
  1404d8835bb80926, `seam_oci` 833f7025d1d9b056.
- **Run:** `~/.cache/hex-t7/g5x-s6/hex-bench/out/create-serial-locks-20261103T011643Z`. The workload started once
  bpftrace said "Attached 38 probes" (the harness's wait since this step), so the probes saw every lifecycle: one
  clone into a cgroup each.
- **What the node did** (`seam_oci`'s counts): at concurrency 1, 519 of 520 boxes were handed an idle cgroup and 1 was
  given a fresh one; at 32, 4,968 of 5,000 handed one and 32 fresh, one for each box in flight; none refused, every
  box ended through its init's pidfd, 32 idle cgroups left at the end, removed then.

| `cgroup_mutex` holds for the boxes' own cgroups, per lifecycle | hex.c1 (520) | hex.c32 (5,000) | hex.c32nonet (5,000) |
|---|---|---|---|
| a fork into the cgroup (`CLONE_INTO_CGROUP`) | 1.000 | 1.000 | 1.000 |
| `cgroup_mkdir` | 0.002 | 0.006 | 0.007 |
| `cgroup_rmdir` | 0.002 | 0.006 | 0.007 |
| `cgroup_kill_write` | 0 | 0 | 0.000 |
| `css_killed_work_fn`, a kworker's | 0.010 | 0.032 | 0.033 |
| `css_release_work_fn`, a kworker's | 0.008 | 0.036 | 0.039 |
| `cgroup_bpf_release`, a kworker's | 0.002 | 0.006 | 0.007 |
| in all | 1.023 | 1.087 | 1.092 |

- **One owner's box takes the lock about once a lifecycle,** the clone of its init into the cgroup it was handed, where
  run 6's took it 16 times. The rest is the fresh cgroups' making and, at the end, their removal: 32 at concurrency 32,
  each a `mkdir`, an `rmdir` and the kworkers' work, over 5,000 lifecycles. Without a network namespace one box of the
  5,000 was ended by `cgroup.kill` (its init's exit was recorded between the delete's read of its record and its
  unwatching), its cgroup removed rather than kept and a 33rd made: 0.000 a lifecycle.
- **The lock is mostly free:** held 12.4% of the busiest buckets at concurrency 32 (24.2% without a network
  namespace), where run 8's boxes of no owner, on the same host minutes later, held it 87.1% (89.3%).

## Run 8 (2026-11-03, fedora): locks, boxes of no owner, ended through their init's pidfd

- **Why:** what a box of no owner takes now: a fresh cgroup each, as before, and its delete through its init's pidfd
  rather than `cgroup.kill`.
- **Host, load and binaries:** run 7's, 2026-11-03T01:18:20Z; the 1-minute load 23.67 at the start and 21.93 at the
  end. Hex's sustained phase ran 1,634/s, without a network namespace 2,010/s, under the probes.
- **Run:** `~/.cache/hex-t7/g5x-s6/hex-bench/out/create-serial-locks-20261103T011820Z`, the workload started on
  "Attached 38 probes".

| `cgroup_mutex` holds for the boxes' own cgroups, per lifecycle | hex.c1 (520) | hex.c32 (5,000) | hex.c32nonet (5,000) |
|---|---|---|---|
| `cgroup_mkdir` | 1.000 | 1.000 | 1.000 |
| `cgroup_rmdir` | 1.000 | 1.000 | 1.000 |
| `cgroup_kill_write` | 0 | 0 | 0 |
| a fork into the cgroup (`CLONE_INTO_CGROUP`) | 1.000 | 1.000 | 1.000 |
| `css_killed_work_fn`, a kworker's | 5.000 | 5.000 | 5.000 |
| `css_release_work_fn`, a kworker's | 5.996 | 6.000 | 6.000 |
| `cgroup_bpf_release`, a kworker's | 1.000 | 1.000 | 1.000 |
| in all | 14.996 | 15.000 | 15.000 |

- **A box of no owner takes it 15 times, where run 6's took 16:** every box ended through its init's pidfd, none by
  `cgroup.kill`.
- **Not of record: the first runs of this step** (`create-serial-locks-20261103T005157Z` and `T005423Z`, under fedora's
  load of 52 to 447) started their workloads 5 s after bpftrace, before it had attached every probe, and their traces
  missed about the first half of each run's lifecycles: 0.57 clones into a cgroup a lifecycle where every lifecycle
  makes one. The harness has waited for bpftrace's own "Attached N probes" notice since.

## Run 9 (2026-11-03, fedora): offcpu, one owner's boxes, what holds concurrency 32 once the cgroup lock is free

- **Why:** goal 5 extended, step 6's second criterion, 8x at concurrency 32, is unmet (`reuse_pass.md`) with
  `cgroup_mutex` held 12.4% of the time (run 7). This run finds what one owner's lifecycles wait on now, every box
  `bench-owner`'s and its bundle's process holding Docker's default capabilities, as in run 7.
- **Host and load:** run 7's, 2026-11-03T01:33:07Z; the 1-minute load 10.16 at the start and 9.36 at the end. Under
  the probe Hex's sustained phase ran 2,973/s and libcrun's 1,415/s.
- **Binaries:** run 7's (`hex` 1404d8835bb80926, `seam_oci` 833f7025d1d9b056). **Run:**
  `~/.cache/hex-t7/g5x-s6/hex-bench/out/create-serial-offcpu-20261103T013307Z`, the harness at a6f32f1 (its env.txt's
  uncommitted changes are this file's runs 7 and 8), each workload started on bpftrace's "Attached 7 probes".

Off-CPU ms per lifecycle, with run 1's concurrency 32 beside it (every box's cgroup made and removed; its classes are
lower bounds, its map having filled):

| What the lifecycle waits on | hex.c1 | hex.c32 | crun.c32 | run 1's hex.c32 |
|---|---|---|---|---|
| a directory's `i_rwsem` | 0.000 | 0.027 | 6.945 | 8.459 |
| `cgroup_mutex` | 0.141 | 0.043 | 1.442 | 1.332 |
| the mount namespace's lock | 0.001 | 4.153 | 0.649 | 1.345 |
| an RCU grace period (umount) | 0.037 | 0.137 | 0.890 | 0.175 |
| a futex (the runtime's own locks) | 0.001 | 0.369 | 0.000 | 0.601 |
| work: a socket | 3.127 | 7.858 | 6.116 | 7.248 |
| work: a pipe or FIFO | 0.844 | 6.899 | 0.316 | 3.110 |
| work: a poll | 0.500 | 3.252 | 0.000 | 2.084 |
| work: a child | 0.000 | 0.002 | 0.003 | 0.013 |
| a sleep (the runtime's own polling) | 0.000 | 0.000 | 3.568 | 0.000 |
| other | 0.028 | 3.414 | 5.097 | 0.398 |

- **`cgroup_mutex` is off the path:** 0.043 ms a lifecycle at concurrency 32, against run 1's 1.332, and no cgroup
  directory is waited on (0.027 ms of all directories' `i_rwsem`s, against 8.459).
- **The mount namespace's lock is the wait now.** It is one `rw_semaphore` for the whole host, `namespace_sem` (Linux
  v6.19 `fs/namespace.c:89`). Every mount, bind, unmount and `pivot_root` takes it to write through `do_lock_mount`
  (`:2747`, `namespace_lock` at `:1723-1725`), held to the end of the caller (`:2799-2801`); a mount namespace's copy
  takes it to write too (`copy_mnt_ns`, `:4117`), and every read of a mount table takes it to read (`m_start`,
  `:1541`). The inits waited for it 5.349 ms a lifecycle at concurrency 32:
  - 4.153 to write, the class above: a bind mount 1.829 (`do_loopback`), `pivot_root` 1.279, a new mount 0.610, an
    unmount 0.148, the namespace's copy at the init's clone 0.146, and 0.141 in `path_mount` (a remount or a
    propagation change; for a bind's remount that `rw_semaphore` can be its superblock's `s_umount`, which the stack
    doesn't tell apart);
  - 1.196 to read, counted under other: the init's read of `/proc/self/mountinfo`, which the runtime makes for each
    recursive read-only bind to remount the cgroup filesystems it carries (`remount_cgroupfs_readonly`, 0cefe6c), the
    bench's `/usr` among them.
- **`pivot_root` walks every thread on the host inside that hold:** `chroot_fs_refs` (`fs/namespace.c:4606`, under
  the lock taken at `:4554`) takes each thread's lock in turn (`fs/fs_struct.c:54-77`). It is 6.64% of Hex's on-CPU
  samples at concurrency 32 and 15.40% at concurrency 1, where nothing waits on it. At concurrency 32 another 16.44% is
  spinning to take an `rw_semaphore` to write (`rwsem_spin_on_owner` 8.64%, `osq_lock` 7.80%).
- **RTNL is the next wait,** counted under other: 2.089 ms a lifecycle at concurrency 32. A network namespace the
  create makes on its path (`netns.made_on_path`: 3,751 and 4,034 of the OCI pass's two sustained phases' 10,000)
  registers its loopback device under the host's one `rtnl_mutex` (`drivers/net/loopback.c:218`, `net/core/dev.c:11519`,
  `net/core/rtnetlink.c:229-231`, `:89-91`, `:76`): 1.862 ms, and 0.227 more in `rtnl_newlink`. Another 2.86% of the
  on-CPU samples spin for it.
- **libcrun meets the same locks at its lower rate:** directories' `i_rwsem`s 6.945 ms (its cgroups' `mkdir`, `rmdir`
  and slow lookups among its top sites), the mount namespace's lock 0.649, RTNL for the loopback 2.553.
- **Where the levers are:** goal 5 extended's steps 2 and 3, on their own branches and not in this one, take the
  holds the inits wait for. On a root node the init attaches the rootfs and bind sources the agent cloned into a copy
  of the seed's two-mount namespace and ends with `chroot`, "with no `pivot_root` walk of the host's threads"
  (`cp/t7-ext-s3`'s `crates/hex-runtime/src/linux/oci/init.rs:316-320`, `:848`; the seed's one `pivot_root` is once
  in its life, `seed.rs:133`), and sets a recursive bind's flags on its whole tree with `mount_setattr` before
  attaching it, with no read of the mount table (`init.rs:887-892`). No lever of this step reaches them.

## Run 10 (2026-11-03, fedora): offcpu, one owner's boxes, an experiment with steps 2, 3 and 6 merged

- **Why:** what holds concurrency 32 with step 2's and step 3's levers in as well as this step's. Of record for no
  branch: the binaries are of the merge `reuse_pass.md`'s run 2 builds (`hex` 8268582ba6f124ea, `seam_oci`
  58e1325a8a636b42), which is no commit.
- **Host and load:** run 7's, 2026-11-03T01:44:03Z; the 1-minute load 20.72 at the start and 14.46 at the end. Under
  the probe Hex's sustained phase ran 3,524/s and libcrun's 1,523/s.
- **Run:** `~/.cache/hex-t7/g5x-s6/hex-bench/out/create-serial-offcpu-20261103T014403Z`, run 9's harness in the merge
  (its env.txt's uncommitted changes are the merge), each workload started on "Attached 7 probes".

Off-CPU ms per lifecycle:

| What the lifecycle waits on | hex.c1 | hex.c32 | crun.c32 |
|---|---|---|---|
| a directory's `i_rwsem` | 0.000 | 0.026 | 6.783 |
| `cgroup_mutex` | 0.000 | 0.037 | 1.548 |
| the mount namespace's lock | 0.000 | 0.007 | 0.702 |
| an RCU grace period (umount) | 0.012 | 0.028 | 0.857 |
| a futex (the runtime's own locks) | 0.000 | 0.328 | 0.000 |
| work: a socket | 2.205 | 3.893 | 6.014 |
| work: a pipe or FIFO | 0.351 | 3.610 | 0.324 |
| work: a poll | 0.408 | 4.892 | 0.000 |
| work: a child | 0.001 | 0.001 | 0.004 |
| a sleep (the runtime's own polling) | 0.000 | 0.000 | 3.362 |
| other | 0.023 | 6.815 | 3.767 |

- **Neither lock of runs 7 and 9 is waited on:** `cgroup_mutex` 0.037 ms a lifecycle at concurrency 32, the mount
  namespace's lock 0.007, against run 9's 0.043 and 5.349. `pivot_root`'s walk is 0.16% and 0.09% of Hex's on-CPU
  samples at concurrency 1 and 32 (the seeds', once in each one's life), against run 9's 15.40% and 6.64%.
- **RTNL is:** 6.670 ms a lifecycle at concurrency 32, nearly all of other. Each box's network namespace registers its
  loopback device under it (`register_netdev`, 3.560 ms; run 9's sources) and the init brings that loopback up by
  rtnetlink (`rtnl_newlink`, 3.110: `crates/hex-runtime/src/linux/oci/net.rs`), each under the host's one
  `rtnl_mutex`. 11.44% of Hex's on-CPU samples spin for it (`mutex_spin_on_owner` and `osq_lock` under
  `register_netdev`, 3.33% and 2.94%, and under `rtnl_newlink`, 3.23% and 1.94%).
- **And sysctl's lock:** 12.58% of the samples spin on the one spinlock the kernel's sysctl tables share (Linux v6.19
  `fs/proc/proc_sysctl.c:88`), taken as each namespace registers its tables (`__register_sysctl_table`, `:1370`,
  taking it at `:1389` and `:1398`: 6.74%; `sysctl_mkdir_p`, `:1299`: 5.84%).
- **So what holds concurrency 32 next is the making of a network namespace a box,** on the create's path or ahead of
  it: making it ahead moves its holds of those two locks off the create but not off the host, as a pool of cgroups made
  ahead would move none of `cgroup_mutex`'s.

## Run 11 (2026-11-03, fedora): locks, one owner's boxes, on CPUs 4-15, at f7c4065: of record for the first criterion

- **Why:** runs 7 to 10 ran on CPUs 4-31, P- and E-cores. The records from create_breakdown.md's run 4 on, and goal
  5 extended's steps 1, 2, 3 and 5, are on 4-15, the i9-14900HX's P-core threads (16-31 are its E-cores), so those
  runs aren't comparable with them (the lead, 2026-11-02T17:49Z). This repeats run 7 on 4-15, at f7c4065, the step
  built on the delete flake's fix (5013c4b), with the lead's caps on the container (`--memory 8g`,
  `--oom-score-adj 1000`).
- **Host and load:** fedora, Linux 6.19.10-300.fc44.x86_64, 2026-11-03T02:34:47Z, the container pinned to 4-15; the
  1-minute load 17.03 at the start and 19.80 at the end. Under the probes Hex's sustained phase ran 1,949/s, without a
  network namespace 2,140/s, and libcrun's 1,059/s.
- **Binaries:** built on fedora at f7c4065 (cp/t7-ext-s6): `hex` 729934ce2c45a292, `seam_oci` 92b8a36ae1c6c74e.
  **Run:** `~/.cache/hex-t7/g5x-s6/hex-bench/out/create-serial-locks-20261103T023447Z`, each workload started on
  "Attached 38 probes"; the fedora unit `hex-g5x-s6-verify2b-20261103T023358Z`.
- **What the node did:** at concurrency 1, 519 of 520 boxes were handed an idle cgroup and 1 a fresh one; at 32, and
  without a network namespace, 4,968 of 5,000 and 32 fresh. None was refused, and every box ended through its init's
  pidfd, none by `cgroup.kill`.

| `cgroup_mutex` holds for the boxes' own cgroups, per lifecycle | hex.c1 (520) | hex.c32 (5,000) | hex.c32nonet (5,000) |
|---|---|---|---|
| a fork into the cgroup (`CLONE_INTO_CGROUP`) | 1.000 | 1.000 | 1.000 |
| `cgroup_mkdir` | 0.002 | 0.006 | 0.006 |
| `cgroup_rmdir` | 0.002 | 0.006 | 0.006 |
| `cgroup_kill_write` | 0 | 0 | 0 |
| `css_killed_work_fn`, a kworker's | 0.010 | 0.032 | 0.032 |
| `css_release_work_fn`, a kworker's | 0.012 | 0.038 | 0.038 |
| `cgroup_bpf_release`, a kworker's | 0.002 | 0.006 | 0.006 |
| in all | 1.027 | 1.089 | 1.090 |

- **The first criterion is met on 4-15:** 1.089 holds a lifecycle at concurrency 32, against a bar of 3. The rest
  is the 32 fresh cgroups' making and, at the end, their removal, as in run 7.
- **The lock is mostly free:** held 8.5% of the busiest buckets at concurrency 32, 16.8% without a network namespace.
- **The waits at concurrency 32 are on other locks, as in run 9:** the runtime's waits on an `rw_semaphore` that isn't
  a directory's (the mount namespace's lock among them, which run 9 names, or a process's `mmap_lock`) came to 2.663
  ms a lifecycle to read and 2.065 to write, against 0.002 and 0.003 at concurrency 1.

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
- **Since goal 5 extended, step 6: the lock is off one owner's path, and the next locks hold 8x off.** One owner's box
  takes `cgroup_mutex` about once a lifecycle, its init's clone into the cgroup it was handed (1.087 at concurrency 32,
  run 7), and waits for it 0.043 ms (run 9). What holds concurrency 32 then is the mount namespace's lock, one for the
  whole host (5.349 ms a lifecycle, run 9); with steps 2 and 3 merged in as well, RTNL and sysctl's lock in making each
  box's network namespace (6.670 ms, and 12.58% of the samples spinning, run 10). The ratio stayed 4.05x to 5.41x
  (4-31, as host.sh runs it) and is 2.89x to 3.32x on 4-15 (`reuse_pass.md`, runs 1 to 3), because a lifecycle alone
  got faster with each lever too.
