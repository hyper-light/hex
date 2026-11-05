# What the seam's slowest creates and starts wait on

Goal 5's bar (decision 5 of `docs/design/runtime-architecture.md`) holds Hex's create and create+start of `true`
through the seam to a quarter of libcrun's in process at p99.9 as at p50. Step 7's passes on the integrated tree
(`create_breakdown.md`, "Goal 5 extended step 7") met it at p50 in both reuse modes and missed it at p99.9: reuse on,
create+start p99.9 0.267x; reuse off, create p99.9 0.938x and create+start 0.833x. The breakdown cuts a create into
parts, and its sum of medians says where a typical create spends its time. It doesn't say what one create in a
thousand waits on. This probe does.

## How it runs

`create_tail.sh <hex> <seam_oci>` runs `create_tail_inside.sh` in a container with host.sh's OCI-pass flags
(privileged, a private cgroup namespace, no network, `/run` a tmpfs, the host's `/usr`, CPUSET, 4-15 by default), on
the OCI pass's bundles (`HEXBENCH_OWNER` set: an owner's, so step 6 hands a box's cgroup and network namespace to the
owner's next box). Inside, the cgroup root is vacated to a leaf and its controllers handed down, as `oci_inside.sh`
leaves it. One ftrace instance of the run's own records, for the runtime's processes (`seam_oci`, its threads `hex-*`,
the seeds and inits `hex`):

- every switch in and out (`sched_switch`, filtered to either side being the runtime's), and the kernel stack of each
  switch out that blocks (a `stacktrace` trigger on `prev_state & 3`);
- every wake-up of one of them (`sched_waking`, `sched_wakeup_new`), with the task and CPU that woke it, and every fork
  (`sched_process_fork`);
- every CPU's interrupt work and kworkers' work items, unfiltered (`irq/softirq_*`, `irq/irq_handler_*`,
  `workqueue/workqueue_execute_*`).

`seam_oci`'s lifecycle phase runs N lifecycles (10,000) after a warm-up (100), with the breakdown's markers
(`HEXBENCH_TRACE`) around every other timed create and start. Its directory's `trace_marker` is the instance's and its
`tracing_on` a plain file, so the markers land and the instance records throughout. A second container on CPUs the run
doesn't use (READER_CPUS, 16-31) streams the instance's `trace_pipe` to `trace.txt`, so the reading takes no CPU from
the run.

`create_tail.py read <run>` walks each marked call's critical path back from its end marker. This is the wait-graph walk
of Giraldeau and Dagenais ("Wait analysis of distributed systems using kernel tracing", IEEE TPDS 27(8), 2016, §3).
Starting from the calling thread at the call's end, each interval is charged by what the task on the path was doing:

- **run: <task>**: the task ran. Any interrupt work on its CPU within that interval is charged apart, as `softirq <action>` or `irq <name>`.
- **runq: preempted by <task> (<work>)**: the task sat runnable, preempted. The charge names what the CPU did most of meanwhile: a work item's function, or a softirq.
- **runq: woken, not yet run**: the task was woken and hadn't yet run.
- **blocked**: the task was blocked.
  - If its waker is one of the runtime's traced tasks, the walk follows into the waker, back from the wake-up.
  - Otherwise the interval is charged to the blocking stack's first frames (scheduler frames skipped) and its waker, a kworker named by its work item.
- **fork**: a new task's wait before its first run is charged as such, and the walk follows into its parent.

Every interval of a call is charged once: the reader checks that each call's charges sum to its span. It then sets the
mean charge by kind of the slowest 0.1% and 1% of calls against the middle band (p45-p55) and lists the slowest ten
calls in full. `create_tail.py test` is its selftest: a synthetic create whose path crosses a preemption, a fork, a
wake-up chain through a seed and an init, a kworker's wake-up and a softirq, every charge known.

Not a timed pass. The tracer costs each switch and wake-up some, so the spans are the traced calls', not the pass's.
What it reads is where the slow calls wait.

## Step 1: the passes reproduced (2026-11-05, fedora)

host.sh's OCI pass, lifecycle and start_true, 2 rounds of 10,000 after 100, best of 2. Setup:
- Binaries: step 7's (60b2e38's code; hex sha256 5c2fc732d92a41ee, static-pie; seam_oci decb8a6e69dc67e4; libcrun 1.30.1 in process).
- Host: fedora (i9-14900HX, Linux 6.19.10), on CPUs 4-15 under the shared lock, noisy (LOAD_GATE=0), containers at 8 GB and oom-score-adj 1000.
- Unit hex-g5tail-repro-20261105T024612Z.
- Co-tenants up: a kind cluster, three buildkit builders, a registry, minio, and kiln's lab (servers and agents in systemd units).

| run | reuse | load at start, end | create p99.9, Hex / libcrun | ratio | create+start of `true` p99.9, Hex / libcrun | ratio |
|---|---|---|---|---|---|---|
| out/20261105T024612Z | on | 23.72, 34.60 | 14.83 / 16.59 ms | 0.894x | 12.91 / 17.50 ms | 0.738x |
| out/20261105T025733Z | off | 34.60, 55.28 | 2.91 / 12.38 ms | 0.235x | 3.28 / 74.44 ms | 0.044x |

The p50s: reuse on, create 0.45 / 2.33 ms and create+start 0.43 / 2.68; reuse off, 0.53 / 2.32 and 0.50 / 2.60.

Neither repeats step 7's tails. The raw series of both runs, and of step 7's, show a distinct mode: 0.05% to 0.26% of a
round's creates take 9 to 23 ms against a p50 of about 0.5. The two runs differ in how often it happens:
- Reuse on, this run's round 2: 26 creates over 8 ms in 10,000 in the lifecycle phase, and 22 in start_true. When the mode passes 0.1% of a round, it is that round's p99.9.
- Reuse off, this run: the mode stayed under 0.1%, so p99.9 was the body's tail.
- Step 7's reuse-on round 2: also under 0.1%.

libcrun's p99.9 moves with the host as much (4.7 to 74 ms over these runs), so a ratio read from one pass at p99.9 is
mostly the luck of two tails.

## Run 1 (2026-11-05, fedora): reuse on, create-tail-20261105T032219Z, not of record

Same binaries. CPUs 4-15 under the shared lock, reader on 16-31. N=10,000, 1 round, load 28.61 at start, 30.39 at end.
3,208,594 trace lines, none dropped. 5,000 traced creates and 5,000 starts, each one's charges summing to its span.

Creates: p50 0.536, p90 0.642, p99 0.800, p99.9 1.753, max 11.091 ms.

| kind | p45-p55 mean (ms) | top 1% mean | top 0.1% mean |
|---|---|---|---|
| blocked: `__mutex_lock` < `cgroup_css_set_fork` < `cgroup_can_fork` < `copy_process` < `kernel_clone` (`clone3`), woken by a kiln lab thread | 0.000 | 0.413 | 4.126 |
| runq: woken, not yet run | 0.008 | 0.111 | 0.615 |
| runq: new task (the init), before its first run | 0.005 | 0.031 | 0.187 |
| run: hex (seed and init) | 0.099 | 0.130 | 0.169 |
| run: seam_oci (the node, in process) | 0.282 | 0.384 | 0.346 |

The two slowest creates, 11.091 and 10.788 ms, spent 10.419 and 10.213 ms in the init's `clone3`, waiting for
`cgroup_mutex`. `cgroup_css_set_fork` takes `cgroup_mutex` for a `CLONE_INTO_CGROUP` clone (Linux v6.19
`kernel/cgroup/cgroup.c:6694-6695`). The thread that released it was a co-tenant's: a thread named `hyper-rt-blocki` (tids
1559176 and 1547520, on CPUs 9 and 10), the name kiln's binaries on fedora give their blocking threads. Both threads
were gone when looked up after the run, so which of kiln's processes held the lock isn't known. Below these two, the slow creates wait in the
run queue (0.6 to 1.9 ms, woken and not yet run, or a new init before its first run).

Starts: p50 0.030, p99 0.076, p99.9 0.388, max 1.739 ms. The slowest start waited in the run queue, woken and not yet run.

## Run 2 (2026-11-05, fedora): reuse off, create-tail-20261105T035431Z, not of record

As run 1 with no owner. Load 36.00 at start, 36.90 at end. 4,784,275 trace lines, none dropped. Every call's charges
summed to its span.

Creates: p50 0.664, p90 1.086, p99 1.982, p99.9 2.908, max 17.848 ms.

| kind | p45-p55 mean (ms) | top 1% mean | top 0.1% mean |
|---|---|---|---|
| blocked: `cgroup_kn_lock_live` < `cgroup_mkdir` < `kernfs_iop_mkdir` < `vfs_mkdir`, woken by a co-tenant's `runc` | 0.000 | 0.326 | 3.257 |
| runq: woken, not yet run | 0.011 | 0.227 | 0.626 |
| runq: preempted by a kworker (`btrfs_work_helper`) | 0.000 | 0.034 | 0.343 |
| runq: preempted by `rcu_preempt` (`fqdir_work_fn`) | 0.000 | 0.033 | 0.335 |
| runq: preempted by ksoftirqd (`css_release_work_fn`) | 0.000 | 0.067 | 0.258 |
| runq: preempted by a kworker (`css_killed_work_fn`) | 0.000 | 0.114 | 0.238 |
| run: seam_oci | 0.381 | 0.534 | 0.590 |
| blocked: `cgroup_kn_lock_live` < `cgroup_mkdir`, woken by a kworker | 0.000 | 0.038 | 0.201 |
| run: hex-seeds (the node's seed thread) | 0.137 | 0.290 | 0.328 |
| runq: preempted by a kworker (softirq RCU) | 0.000 | 0.019 | 0.191 |

The slowest create, 17.848 ms, waited 16.286 ms for `cgroup_mutex` in its cgroup's `mkdir`, released by a co-tenant's
`runc`. Other creates in the slowest 1% waited for the same lock on the kernel's teardown of earlier boxes' cgroups.
Most were preempted on their own CPUs by teardown work:
- the cgroup destroy work items (`css_killed_work_fn`, `css_release_work_fn`) and RCU callbacks;
- `fqdir_work_fn`, a network namespace's fragment queues freed at its exit.

One (2.730 ms) waited 1.364 ms in `__list_lru_init` < `alloc_super`, a new superblock's list_lru registration, on a
kworker running `css_killed_work_fn`.

Starts: p50 0.037, p99 0.180, p99.9 0.914, max 1.219 ms. The slowest starts waited in `ppoll` for the init until its
exec (the init, `sleep` after it, is not traced, so the waker is unnamed), or were preempted by `css_killed_work_fn` and
`css_release_work_fn` (0.6 to 0.9 ms).

## What follows from it

Both modes' rare 10-20 ms creates wait on the host-wide `cgroup_mutex`. The create takes it once:
- reuse on: the init's `CLONE_INTO_CGROUP` clone;
- reuse off: the cgroup's `mkdir`.

A task migration anywhere on the host holds that lock while it waits out an RCU grace period:
- the write takes `cgroup_mutex` (`cgroup_procs_write_start`, `cgroup.c:3048` on);
- it then write-locks `cgroup_threadgroup_rwsem` (`cgroup_attach_lock`, `:2536-2552`);
- whose writer enters through `rcu_sync_enter`, which calls `synchronize_rcu()` (`kernel/rcu/sync.c:133`). It does so unless the hierarchy is mounted `favordynmods`, when the write lock is the thread group's own (`:2547`).

On fedora the holders seen were a co-tenant's `runc` and threads of kiln's (whose work wasn't traced, so that they were migrating is the reading of the code, not yet seen). `docs/design/runtime.md` ("Born in its
cgroup") measured that grace period at 9.7 to 12 ms p50 on this host. The rarer such a migration is, the more it costs:
one after a quiet spell pays a whole grace period.

With reuse off, the slowest 1%'s body is the kernel's teardown of Hex's own earlier boxes on the CPUs their successors
run on: their cgroups' destroy work and their network namespaces' exit.

## Runs 3 and 4 (2026-11-05, fedora): the holder named, not of record

Same binaries, CPUs and lock as runs 1 and 2, N=10,000, 1 round each, no event dropped.
- Run 3 (tail2): the probe at fe078ed adds every task's `cgroup_attach_task`, `cgroup_mkdir`, `cgroup_rmdir` and
  `cgroup_release`.
- Run 4 (tail3): it adds those events' kernel stacks, and kprobes on `rcu_sync_enter`'s entry and return. That is the
  grace-period wait `percpu_down_write` makes (`kernel/locking/percpu-rwsem.c:235`, Linux v6.19).

| run | reuse | create p50 / p99 / p99.9 / max (ms) | the slowest creates' wait on `cgroup_mutex` | released by, after its | its `rcu_sync_enter` |
|---|---|---|---|---|---|
| create-tail-20261105T045112Z | on | 0.460 / 0.742 / 1.187 / 18.968 | 18.345 and 8.340 ms, in `cgroup_css_set_fork` (the init's clone) | a co-tenant's `bash` and `podman`, each after its `cgroup_attach_task` | not traced in run 3 |
| create-tail-20261105T045933Z | on | 0.559 / 0.819 / 1.127 / 10.532 | 9.794 ms, in `cgroup_css_set_fork` | `runc`, `cgroup_attach_task` < `__cgroup_procs_write` < `cgroup_procs_write` < `kernfs_fop_write_iter` < `vfs_write` | 10.238 ms |
| create-tail-20261105T050316Z | off | 1.866 / 4.477 / 12.142 / 32.297 | 17.840, 11.613 and 7.808 ms, in the cgroup's `mkdir` | `runc`, `podman`, `runc`, each in `__cgroup_procs_write` | 18.005, 11.609 and 8.088 ms |

Each holder wrote a task's pid to a `cgroup.procs` file: a migration. Its `rcu_sync_enter`, inside `cgroup_mutex`,
waited out a grace period as long as the create's wait, to within half a millisecond. So the mode is a migration
anywhere on the host, holding the lock that a create's `CLONE_INTO_CGROUP` clone, or its `mkdir`, needs.

With that mode aside, run 4's reuse-on creates had a p99.9 of 1.127 ms. In run 4's reuse-off run (load 28 to 33), the
slowest create, 32.3 ms, waited 29.8 ms as a new init before its first run, and the next, 17.5 ms, waited 17.5 ms
woken and not yet run: the run queue of a loaded host.

## The parked init, A/B against 60b2e38 (2026-11-05, fedora), not of record

host.sh's OCI pass, lifecycle and start_true, 2 rounds of 10,000 after 100, best of 2, interleaved in unit
hex-g5tail-abf-20261105T093459Z:
- A is 60b2e38's binaries (hex sha256 5c2fc732d92a41ee, seam_oci decb8a6e69dc67e4).
- B is this branch's working tree (hex sha256 87376c7d72da8737, static-pie, seam_oci 763bbad09e2ddd15): the init made
  ahead in the handed-on idle slot (reuse on) and in up to 8 cgroups made ahead for boxes of no owner (reuse off,
  `HEXBENCH_AHEAD` unset).
- Setup: CPUs 4-15 under the shared lock, noisy, containers at 8 GB and oom-score-adj 1000. Loads were 13 to 59, with
  kiln's CI burst pools and another agent's VM lab up beside them.

Figures are Hex through the seam in process, against libcrun in process in the same pass, in ms.

| run | side, reuse | load start, end | create p50 / p99.9 / max | create+start p50 / p99.9 / max | libcrun create / create+start p99.9 | p99.9 ratios | delete p50 / p99.9 / max |
|---|---|---|---|---|---|---|---|
| out/20261105T100043Z | A, on | 58.32, 41.09 | 0.41 / 0.88 / 15.35 | 0.40 / 0.97 / 19.54 | 6.84 / 7.59 | 0.129x, 0.128x | 0.17 / 1.28 / 1.99 |
| out/20261105T101419Z | B, on | 53.66, 36.23 | 0.41 / 1.56 / 3.68 | 0.37 / 0.85 / 1.79 | 7.60 / 11.32 | 0.205x, 0.075x | 0.34 / 9.56 / 16.66 |
| out/20261105T102802Z | A, off | 58.95, 39.56 | 0.57 / 17.75 / 52.81 | 0.56 / 4.94 / 21.99 | 7.43 / 8.47 | 2.389x, 0.583x | 0.24 / 12.64 / 112.01 |
| out/20261105T104307Z | B, off | 49.53, 22.80 | 0.39 / 2.71 / 3.81 | 0.38 / 3.75 / 5.52 | 8.03 / 11.65 | 0.337x, 0.322x | 0.23 / 3.08 / 21.24 |
| out/20261105T105705Z | B, on | 46.47, 23.18 | 0.34 / 1.40 / 2.84 | 0.34 / 1.12 / 3.87 | 9.98 / 7.56 | 0.140x, 0.148x | 0.33 / 3.62 / 85.34 |
| out/20261105T111043Z | A, on | 45.06, 13.08 | 0.40 / 0.92 / 59.89 | 0.37 / 1.08 / 76.20 | 6.84 / 7.62 | 0.135x, 0.142x | 0.18 / 1.36 / 2.05 |

The counts B wrote in every round:
- Reuse on: 10,100 inits parked and 10,099 claimed in the lifecycle phase, 10,100 of 10,100 in start_true; none
  failed or ended unclaimed.
- Reuse off: 10,107 made ahead and 10,099 taken; 1 create found none ready (the first); 10,096 to 10,099 claimed.
- What a park charged its cgroup, summed over a round: reuse on, 235-270 MB for 10,100, about 24.7 KB an init; reuse
  off, 2.64 GB for 10,100 to 10,107, about 262 KB an init in a fresh cgroup.

What it shows:
- **The 10-20 ms mode is off create's path in process.** B's create max in process is 2.84-3.81 ms in every pass;
  A's is 15.35-59.89. Over the socket, B's max is 3.27-4.68 ms in three passes and 52.54 in the reuse-off one, which
  this run doesn't cut; A's is 22.50-126.18. The socket's node (`hex node`) keeps no cgroups ahead (the bound is 0
  unless set), so its reuse-off column is the old path. The stall moved to the delete, which now clones the next init. B's delete p50 is 0.33
  to 0.34 against A's 0.17 to 0.18, and its max reached 85.34 ms.
- **Reuse on meets the bar in all four passes, A's as well.** The mode stayed under 0.1% of A's rounds this time, so it
  set A's max, not its p99.9. B's create p99.9 (1.40 and 1.56) is above A's (0.88 and 0.92): see run 5.
- **Reuse off is a large gain that still misses.** Create p99.9 17.75 → 2.71 ms and p50 0.57 → 0.39. Against libcrun,
  0.337x for create and 0.322x for create+start, above the 0.25x bar.

## Run 5 (2026-11-05, fedora): B, reuse on, create-tail-20261105T112335Z, not of record

B's binaries, N=10,000, load 35.16 at start, no event dropped. Creates: p50 0.386, p90 0.483, p99 0.638, p99.9 1.228,
max 3.816 ms. Every create claimed its parked init, and no create waited on `cgroup_mutex`: no `cgroup_css_set_fork`
and no `cgroup_mkdir` is on any create's path.

What the slowest 0.1% add over the middle band:
- RCU callbacks run in softirq on the create's CPU, +0.866 ms;
- longer runs of the init (+0.535) and the node (+0.315);
- a preemption by an IO completion kworker (+0.135).

That is finding 2's class: the kernel's work for earlier boxes, now including the delete's park clone, on the CPUs the
next create runs on.

## Reuse off after the cut: what the slowest 1% wait on, and what each controller's teardown costs (2026-11-05, not of record)

Two probes, both on B's binaries (hex 87376c7d72da8737, seam_oci 763bbad09e2ddd15), boxes of no owner, CPUs 4-15
under the lock.
- **create_tail, run 6** (create-tail-20261105T132045Z, load 19.48 at start, 22.92 at end): creates p50 0.390, p99
  1.557, p99.9 2.361, max 2.829 ms. Charged by kind of work (`create_tail_attr.py`'s grouping over `create_tail.py`'s walk), the
  slowest 1% add over the middle band:
  - preemption by the cgroup teardown work items (`css_*_work_fn`), +0.938 ms;
  - other waits, +0.253;
  - the run queue, +0.140;
  - the runtime's own runs, +0.108;
  - RCU softirq, +0.032;
  - `fqdir_work_fn` and `cleanup_net`, none.

  The three runs on A's binaries (runs 2-4's reuse-off traces) show the same: css work +0.73 to +1.53 ms, fqdir +0.01
  to +0.03, `cleanup_net` none. The network namespace's teardown is not the share.
- **css_cost** (`css_cost.sh`: css-cost-20261105T132631Z, N=2000, load 30.20 at start): kprobes on each controller's
  css callbacks, and every work item by function. Per box made and deleted, the cgroup teardown's work items took
  0.795 ms: `css_killed_work_fn` 0.504 (5.05 per box), `css_release_work_fn` 0.240 (6.03), `css_free_rwork_fn`
  0.051 (6.00). Of that, each controller's own callbacks, per box:
  - memory: offline 0.388 ms (mean 0.384, max 1.805), free 0.010, released 0.001;
  - cpu: free 0.0057, released 0.0005, offline 0.0004;
  - io: free 0.0013, offline 0.0006;
  - pids: free 0.0003.

  So memory's offline is about half the teardown, and the rest is the work items' own cost per css, about 0.08 ms for
  each css a box's cgroup carries.

**The lever proposed (a box's cgroup with the controllers its bounds use) is not built.** Leaving io out would spare a
box about one css's work, about 0.08 ms of 0.795 (about 10%). On run 6's slowest 1% that is about 0.1 ms, so p99.9
goes from about 2.36 to about 2.26 ms, about 0.32x of the A/B's libcrun: still over the 0.25x bar. Memory's offline,
the largest single part, can't be left out: every box's memory usage is reported, and most specs bound it. The
falsifier needed no build: the css count can't fall by enough to move p99.9 under the bar.
(The work-item list also holds the host's own: `btrfs_work_helper` and the write-back items are other tenants' IO, not
Hex's.)
