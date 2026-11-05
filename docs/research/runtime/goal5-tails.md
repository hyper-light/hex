# Goal 5's p99.9 tails: what a create one in a thousand waits on

Source notes for the cut of goal 5's p99.9 tails (decision 5 of `docs/design/runtime-architecture.md`; step 7 met the
bar at p50 and missed it at p99.9). This note records what the measurements found, why the kernel makes it so, and the
cuts that follow. It decides nothing: the decision goes in decision 5 with these sources (CLAUDE.md §5).

**Conventions.** Kernel files were read at the Linux v6.19 tag from
`https://raw.githubusercontent.com/torvalds/linux/v6.19/<path>`, retrieved 2026-11-05. fedora runs 6.19.10, and
`path:line` is a line at that tag. The measurements are `bench/competitive/probes/create_tail.md`, runs 1 and 2 (fedora,
2026-11-05, not of record), and step 1's reproduced passes there.

## What was measured

- **A distinct 10-20 ms mode.** In host.sh's passes, 0.05% to 0.26% of a round's creates take 9 to 23 ms against a p50
  of about 0.5 ms, in both reuse modes, in process and over the socket. Once the mode passes 0.1% of a round, it is that
  round's p99.9: reuse on, create+start p99.9 12.91 ms against libcrun's 17.50 (out/20261105T024612Z). (MEASURED, ours)
- **Reuse on: the init's clone waits for `cgroup_mutex`.** The two slowest of 5,000 traced creates (11.1 and 10.8 ms)
  spent 10.4 and 10.2 ms blocked in `__mutex_lock` < `cgroup_css_set_fork` < `cgroup_can_fork` < `copy_process` <
  `kernel_clone` (`clone3`). In each case the wake-up came from a co-tenant's thread. (create-tail-20261105T032219Z)
- **Reuse off: the cgroup's `mkdir` waits for it.** The slowest (17.8 ms) spent 16.3 ms blocked in
  `cgroup_kn_lock_live` < `cgroup_mkdir`, released by a co-tenant's `runc`. Others in the slowest 1% waited for
  kworkers tearing down Hex's own earlier cgroups. (create-tail-20261105T035431Z)
- **Reuse off: teardown on the same CPUs.** The slowest 1% of creates were preempted 0.2-1.8 ms at a time by the
  kernel's teardown of earlier boxes:
  - `css_killed_work_fn`, `css_release_work_fn`;
  - RCU callbacks;
  - `fqdir_work_fn`, a network namespace's fragment queues freed at its exit.

  One waited 1.4 ms on `list_lrus_mutex` in a new superblock's `__list_lru_init`, against a memory cgroup's offline.

## Why the kernel makes it so (DESIGN)

- **A `CLONE_INTO_CGROUP` clone takes `cgroup_mutex`.** `cgroup_css_set_fork` does
  `if (kargs->flags & CLONE_INTO_CGROUP) cgroup_lock();` (`kernel/cgroup/cgroup.c:6694-6695`). A plain fork takes only
  the read side of `cgroup_threadgroup_rwsem` (`:6697`).
- **A cgroup's `mkdir` takes it.** `cgroup_mkdir` locks its parent live (`cgroup_kn_lock_live`, `cgroup.c:5995`, the
  helper at `:1710`), and that takes `cgroup_mutex`.
- **A task migration holds it across an RCU grace period.** A `cgroup.procs` write runs `cgroup_procs_write_start`
  with `cgroup_mutex` held (`cgroup.c:3048` on). It then calls `cgroup_attach_lock`, which write-locks the global
  percpu `cgroup_threadgroup_rwsem` (`:2536-2546`). `percpu_down_write` first calls `rcu_sync_enter`
  (`kernel/locking/percpu-rwsem.c:227-235`), and that waits for a grace period (`synchronize_rcu()`,
  `kernel/rcu/sync.c:133`) unless another writer passed one just before (`:100-105`).
  - Hex measured that wait on this host at 9.7 to 12.0 ms p50 after a quiet spell of 20 ms or more
    (`docs/design/runtime.md`, "Born in its cgroup", `join_cost.py`). So a migration anywhere on the host stalls every
    create that needs `cgroup_mutex` for about a grace period.
- **`favordynmods` takes the grace period out of a migration.** With it, `cgroup_attach_lock` takes the thread
  group's own rwsem instead (`CGRP_ATTACH_LOCK_PER_THREADGROUP`, `cgroup.c:2547-2549`; `include/linux/cgroup-defs.h:156-157`).
  The documentation: it reduces "the latencies of dynamic cgroup modifications such as task migrations ... at the cost of
  making hot path operations such as forks and exits more expensive", and "the static usage pattern of creating a
  cgroup, enabling controllers, and then seeding it with CLONE_INTO_CGROUP is not affected by this option"
  (`Documentation/admin-guide/cgroup-v2.rst:194-200`).
  - The 2025 patch that made the per-thread-group lock reported "the long-tail latency of cgroup migration ... from
    milliseconds to microseconds" under fork and exec interference ([OCI-20] in `oci-runtimes-and-fast-start.md`).
  - It is a mount option of the whole hierarchy, so it changes the host for every tenant.
- **What `favordynmods` costs every fork and exit (DESIGN; the kernel gives no number).** The kernel says it "makes
  hot path operations such as forks and exits into the slow path and more expensive"
  (`include/linux/cgroup-defs.h:88-105`). Turning it on calls `rcu_sync_enter` on `cgroup_threadgroup_rwsem` and leaves
  it entered (`cgroup.c:1347-1351`), so every reader takes the slow path from then on:
  - Each `percpu_down_read` (every fork, exit and thread-group change, through `cgroup_threadgroup_change_begin`,
    `cgroup-defs.h:876-881`) goes to `__percpu_down_read` instead of a per-CPU increment. That is a full memory
    barrier (`smp_mb`, `kernel/locking/percpu-rwsem.c:48-67`; `include/linux/percpu-rwsem.h:64-67`).
  - Each `percpu_up_read` adds a barrier and an `rcuwait_wake_up` (`percpu-rwsem.h:118-132`).
  - On top of both, `down_read` and `up_read` of the thread group's own rwsem (`cgroup-defs.h:879-880`, `:891-892`).
    That is an atomic on a cache line the group's threads share.
- **It can't be fully undone without a reboot.** Once on, the per-thread-group rwsem "can't be disabled": a remount
  without `favordynmods` exits the `rcu_sync`, but `cgroup_enable_per_threadgroup_rwsem` stays set, with a warning
  (`cgroup.c:1341-1356`). So even a one-off A/B leaves every fork on the host taking the thread group's rwsem until the
  host reboots.
- **Teardown runs where it was queued.** The cgroup offline, release and free work items go on per-CPU workqueues
  (`alloc_workqueue("cgroup_offline", WQ_PERCPU, 1)` and its two siblings, `cgroup.c:6457-6463`). So a box's teardown
  runs on the CPU that removed it, beside the next box's create.
- **A memory cgroup's offline holds `list_lrus_mutex` over every list_lru.** That is `memcg_reparent_list_lrus`
  (`mm/list_lru.c:467-472`). A new superblock registers its list_lrus under the same mutex (`:20`, `:32-44`). So the
  proc, tmpfs and mqueue mounts of a create can wait on an earlier box's memory cgroup going offline.

## Prior work on the same lock

- AFaaS (Chai et al., OSDI 2025, [OCI-35] p.511-512) keeps a pool of pre-made cgroups. Even so, it warns that a pool can
  jitter "when a co-located workload holds the global cgroup lock".
- RunD ([OCI-31] p.57, p.60) and a 2021 RFC ([OCI-21]) found cgroup creation's long tail in `cgroup_mutex` under
  concurrency.

Hex's measurement adds that on a shared host the holder is usually another tenant's migration, and that it costs a
grace period.

## The cuts that follow (for decision 5)

1. **No `cgroup_mutex` on create's path.** Clone each box's init into its cgroup ahead of its create:
   - reuse on: into the idle cgroup handed to the owner's next box, at the owner's delete;
   - reuse off: into a cgroup of a pool made ahead.

   The `mkdir` and the `CLONE_INTO_CGROUP` clone then wait off the path, and a create hands the spec to an init already
   in place. It would falsify this if a create with the init made ahead still blocked on `cgroup_mutex` in create_tail
   (some other call on the path takes it), or if the made-ahead init's hand-over cost more at p50 than the clone it
   saves.
2. **`favordynmods` on the node's cgroup2 mount.** A host setting: Ada's call, not Hex's. It shortens other tenants'
   holds; it doesn't take Hex's create off the lock.
3. **Reuse off's teardown.** It is Hex's own work, landing on the next create's CPUs. This note records it. It isn't
   cut by (1), and it is the larger share of reuse off's p99 body.

## Reuse off's teardown: where it runs (for the lever after the parked init)

With the parked init, a reuse-off create is 0.337x of libcrun at p99.9 (`create_tail.md`, the A/B). Its slowest 1%
are preempted, on their own CPUs, by the kernel's teardown of earlier boxes (run 2): cgroup work items
(`css_killed_work_fn`, `css_release_work_fn`), `fqdir_work_fn`, and RCU callbacks. Where each runs, Linux v6.19,
retrieved 2026-11-05:
- **A cgroup's teardown is per-CPU work, queued where the box was removed.** The offline, release and free work goes
  on `cgroup_offline`, `cgroup_release` and `cgroup_free`, each `alloc_workqueue(..., WQ_PERCPU, 1)`
  (`kernel/cgroup/cgroup.c:6457-6463`). It is queued once per css: each controller's state, and the cgroup's own
  (`:6069` offline, `:5695` release, `:5686`/`:5829` free after a grace period). A box's cgroup has one css per
  controller its parent hands down: cpu, io, memory, pids (`hex_core::cgroup::CONTROLLERS`). A memory cgroup's
  offline also holds `list_lrus_mutex` over every list_lru (`mm/list_lru.c:467-472`).
- **A network namespace's fragment queues go the same way.** `fqdir_exit` queues `fqdir_work_fn` on `inet_frag_wq`
  (`net/ipv4/inet_fragment.c:256-260`), a per-CPU workqueue (`create_workqueue`, `:213`). That frees the fragment
  hash table, then queues the free of the fqdir itself on `system_percpu_wq` a second later (`:179-187`). Every
  namespace's cleanup (`cleanup_net`) runs on `netns`, one single-threaded workqueue (`net/core/net_namespace.c:1264`,
  queued at `:744`), on whatever CPU it takes.
- **RCU callbacks run on the CPU that queued them**, in softirq or that CPU's rcuc kthread, unless the kernel is
  booted with `rcu_nocbs=` to offload them (`Documentation/admin-guide/kernel-parameters.txt:5532-5540`).

So a box's delete on a create CPU leaves that CPU work in proportion to what the box made: its cgroup's css (four
controllers and its own) and its network namespace (fragment queues, cleanup, the RCU callbacks of all of them).
Ada's rule rules out moving it by priority, nice or affinity. Hex can only make less of it.

**The lever proposed, not built: make each box's cgroup with the controllers its bounds use, so its teardown queues
no work for one it doesn't.**
- The io controller is enabled for every box, but a box's io state is written only when its bounds set io
  (`hex-runtime/src/linux/cgroup.rs:223-225`). Each controller left out of a box spares that box one offline, one
  release and one RCU-delayed free.
- Boxes would sit under a parent per controller set, since `cgroup.subtree_control` hands the same controllers to
  every child.
- It would falsify the lever if the A/B's reuse-off create p99.9 doesn't fall with the css count, or if
  `create_tail`'s top-1% preemption by `css_*_work_fn` doesn't fall in proportion.
- The network namespace's share (fragment queues, `cleanup_net`) isn't cut by it. A namespace of a box of no owner is
  never handed on, since what it keeps can't be reset (`docs/research/runtime/goal5-netns-reuse.md`).
