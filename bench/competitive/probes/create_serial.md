# Where creates wait under concurrency

Goal 5 plan step 4's criterion 4 (decision 5 of `docs/design/runtime-architecture.md`; kiln plan
`01a1877e05c7748ca8865548aab76445`) asks the seam's sustained lifecycles a second at concurrency 32 to be at least 8x
its throughput at concurrency 1 in the same run (`seam_oci`'s throughput and sustained phases, through host.sh). The
OCI pass at ca45e1a (`~/.cache/hex-bench/out/20261030T041910Z` on fedora, ROUNDS=2 N=1000, the 1-minute load 21.81 at
the start, not of record) read 1,942 against 394 in process (4.93x) and 1,994 against 316 over the socket (6.31x);
libcrun beside it ran 1,593 sustained against 73 at concurrency 1 (its delete p50 was 10.34 ms). This probe finds what
the lifecycles wait on at concurrency 32 that they don't at 1, beside libcrun's at 32, and what holds it.

## How it runs

`create_serial.sh <hex> <seam_oci>` runs `create_serial_inside.sh` in a privileged container like the OCI pass's (a
private cgroup namespace, `/run` a tmpfs, pinned to CPUs 4-31), with network for dnf (bpftrace isn't on the host), on
the given binaries and host.sh's libcrun driver, each copied into the run's directory. A bpftrace program runs over one
workload at a time, its per-process probes filtered by name to the runtime's own processes: `seam_oci` and `hex` for
Hex's seam (the node agent in process, its seeds, each init until it execs), `libcrun-driver` for libcrun. The
workloads share one lifecycle, the throughput phases' (create, start and delete of `true`):

- **hex.c1**: `seam_oci`'s start_true, one lifecycle at a time, 520 of them;
- **hex.c32**: `seam_oci`'s sustained phase, concurrency 32, 5,000;
- **crun.c1** and **crun.c32**: `oci_bench.py`'s start_true and sustained phases on libcrun in process, the same counts.

Two programs (`MODE`):

- **offcpu**: off-CPU time by kernel stack (`finish_task_switch`), an on-CPU profile at 999 Hz, and each directory
  operation (`vfs_mkdir`, `vfs_rmdir`, `vfs_unlink`, `vfs_mknod`) by the filesystem of the directory it changes. The
  reader sorts the off-CPU time into the locks a create can wait on and the waits for work.
- **locks**: each `vfs_mkdir` and `vfs_rmdir` timed by its directory (the parent's `i_rwsem` is held exclusively across
  it: `filename_create` and `do_rmdir`, Linux v6.19 `fs/namei.c`); each wait in an `rw_semaphore`'s slow path timed by
  the directory whose `i_rwsem` it is; each lookup that missed the dcache (`lookup_slow`) by directory and name; for
  every process, `cgroup_mutex`'s holders timed whole (`cgroup_mkdir`, `cgroup_rmdir`, `cgroup_kill_write`,
  `cgroup_bpf_prog_attach`, and the work a removed cgroup queues to kworkers: `css_killed_work_fn`,
  `css_release_work_fn`, `cgroup_bpf_release`), and a fork from `cgroup_can_fork` to `cgroup_post_fork`'s return,
  which with `CLONE_INTO_CGROUP` holds `cgroup_mutex` throughout; and the share of the time `cgroup_mutex` is owned and
  the runtime's cgroup parent's `i_rwsem` held, sampled on every CPU at 999 Hz over the 100 ms buckets in which the
  runtime made or removed cgroups.

`create_serial.py` reads them, per lifecycle. Not a timed pass: the load is recorded, not gated, and the probes cost
each lifecycle some (a kprobe on each mkdir, rmdir, slow lookup and lock slow path), so the rates under them are below
the pass's. A holder's time at concurrency 1 is close to its hold; at 32 it includes its wait for the lock.

## Run 1 (2026-10-30, fedora): offcpu, at ca45e1a

- **Host and load:** fedora-2 (Intel Core i9-14900HX, 32 threads, 62 GiB; Linux 6.19.10-300.fc44.x86_64), pinned to
  CPUs 4-31. 2026-10-30T04:26:44Z; the 1-minute load 12.16 at the start and 15.00 at the end. Up beside it: four
  long-lived containers (`issue556oss-control-plane`, `buildx_buildkit_kagent-builder-v0.23.00`, `kind-registry`,
  `kiln-t10-minio`).
- **Binaries:** `hex 0.1.0` of ca45e1a (on cp/t7-g5-s4), sha256 19ce3ac5229fe2f7, static-pie; `seam_oci` of ca45e1a,
  sha256 476faa01049a8797; libcrun 1.30.1 in process, driver sha256 5c356f2139adc232.
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
| a sleep (the runtime's own polling) | 0.000 | 0.000 | 4.069 |

The directory waits at concurrency 32 are Hex's `rmdir` (2.675 ms), `mkdir` (2.233) and slow lookups (1.626 and
1.518). The listing of the box root that each create made to count its boxes is gone: the same probe's first form on
d12c3fe's binaries (`create-serial-20261030T031750Z`, hex fe92ee109290392c) put 5.710 ms per lifecycle on its
`getdents64`, of 18.976 on directories; ca45e1a counts the boxes by the box root's link count (`BoxRoot::count`).

## Run 2 (2026-10-30, fedora): locks, at ca45e1a

- **Host and load:** as run 1's. 2026-10-30T04:41:51Z; the 1-minute load 13.56 at the start and 23.16 at the end; the
  same four containers up.
- **Binaries:** as run 1's.
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

`cgroup_mutex`'s holders, each one's time in us (runs a lifecycle at that rate), concurrency 1 then 32:

| Holder | Hex | libcrun |
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
on the node's record directory on the bench container's overlay (0.601 and 0.070).

## Run 3 (2026-10-30, fedora): locks with the lock sampling, at e302ac8

- **Host and load:** as run 1's. 2026-10-30T04:56:17Z, beside Focal's fleet test (34 `s5-focal*` containers) and a
  burst of kiln CI jobs: the 1-minute load 74.79 at the start and 36.23 at the end. Under the probe and that load Hex's
  sustained phase ran 1,077/s and libcrun's 867/s.
- **Binaries:** `hex` as run 1's (e302ac8 changes no line it's built from; the build's sha256 is the same,
  19ce3ac5229fe2f7); `seam_oci` of e302ac8, sha256 cf0fa3001bbf1d87, which also reads the create seeds' counts.
- **Run:** `~/.cache/hex-bench/out/create-serial-locks-20261030T045617Z`, the harness at e302ac8.

| Share of the busy time | hex.c1 | hex.c32 | crun.c1 | crun.c32 |
|---|---|---|---|---|
| `cgroup_mutex` owned | 21.7% | 90.6% | 10.9% | 76.4% |
| the cgroup directory's `i_rwsem` held to write | 7.4% | 78.8% | 1.9% | 56.2% |
| the cgroup directory's `i_rwsem` held to read | 0.2% | 11.5% | 0.1% | 4.6% |

The holders and the directory's waits are run 2's, longer under the load: at concurrency 32 the box root's `mkdir`
and `rmdir` took 0.417 and 0.303 ms a lifecycle and its waits 6.670 ms to write and 3.885 to read, and a fork into the
cgroup took 1.014 ms.

## What the runs find

- **The ceiling at concurrency 32 is `cgroup_mutex`, one lock for the whole host.** It was owned 90.6% of Hex's busy
  time at concurrency 32 in run 3, under a load of 37 to 75; in run 2, under less, the box root's exclusive holds alone
  filled 90% of the run's time (0.487 ms a lifecycle at 1,850/s). Hex's lifecycle takes the lock 17 times: the
  runtime's `mkdir` and `rmdir` of the container's cgroup, its `cgroup.kill`, its device program's attach and its
  clone into the cgroup, and the kernel's own work for the removed cgroup in kworkers, five csses offlined, six
  released and its device program released. Each holder's time grows from concurrency 1 to 32 by its wait for the
  lock (run 2's table).
- **The cgroup directory's `i_rwsem` is held as long as the lock inside it.** `mkdir` and `rmdir` hold it exclusively
  while `cgroup_mkdir` and `cgroup_rmdir` wait for and hold `cgroup_mutex` (`cgroup_kn_lock_live`), so the box root
  was held 90.3% of the time (run 3), and each lifecycle's two slow lookups of its new cgroup's directory wait behind
  those holds: the 3.042 ms to read in run 2.
- **libcrun is bound by the same.** Its lifecycle takes `cgroup_mutex` more often (more csses, two kills, three
  rmdirs at concurrency 1), and at its lower rate the lock was owned 76.4% of its busy time; its cgroup directory's
  waits are 2.916 and 1.430 ms a lifecycle (run 2).
- **Hex's own waits are not the ceiling.** Its futexes (0.601 ms per lifecycle, run 1) and its record directory
  (0.671, run 2) are each under a tenth of the cgroup directory's (7.756: 4.714 to write and 3.042 to read, run 2).
- **So the 8x isn't in reach of this step.** The rate at concurrency 32 is what `cgroup_mutex` admits, however fast a
  lifecycle runs alone, and the faster one runs alone the further 8x of it sits above that. What would raise the
  ceiling is fewer holds of the lock per lifecycle: a cgroup reused from one box to the next rather than made and
  removed for each, which takes the `mkdir`, the `rmdir` and the kworkers' css and device-program work off it, or a
  kill that doesn't go through `cgroup.kill`. A pool that makes cgroups ahead and removes them after use takes the lock
  as often, only at other moments. Spreading the boxes' cgroups over several parent directories would part their
  `i_rwsem`s but not `cgroup_mutex`.
