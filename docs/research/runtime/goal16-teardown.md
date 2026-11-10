# Goal 16: making less of a box's teardown, for goal 5's no-owner p99.9

Source notes for goal 16 (the lead, 2026-11-06T01:52Z and its correction after it). Goal 5's bar as written, for boxes of no
owner (host.sh `PHASES=oci`, best of 2), is still over at p99.9: 0.406x to 0.463x of libcrun, against a bar of 0.25x.
`goal5-tails.md` puts what's left on the kernel's work for earlier boxes' cgroup teardown. That work preempts the
create, and it holds host-wide mutexes a create may wait on. This note decides nothing; decisions go in decision 8 with
these sources (CLAUDE.md §5).

**Conventions.** Kernel files are read at the Linux v6.19 and v7.2 tags from
`https://raw.githubusercontent.com/torvalds/linux/<tag>/<path>`, retrieved 2026-11-06. `path:line` is a line at the
tag named, v6.19 unless it says otherwise. Commits are read from `https://github.com/torvalds/linux/commit/<id>.patch`.
fedora runs 6.19.10.

## Moving it by affinity is ruled out

The first lever (2026-11-06T01:52Z) kept one CPU of the node's set for the teardown:
- the node's remover rmdirs there, so each css's teardown queues there;
- its seeds keep off that CPU, and the programs they start get it back before exec.

The kernel does put the chain on the rmdir's CPU:
- `percpu_ref_kill_and_confirm` queues `call_rcu_hurry` on the calling CPU (`lib/percpu-refcount.c:233`).
- The confirmation queues the offline work (`kernel/cgroup/cgroup.c:6062-6069`) on a `WQ_PERCPU` workqueue
  (`:6457-6463`), which takes the current CPU (`kernel/workqueue.c:2258-2262`).
- The release and free chain the same way from the css's last put (`cgroup.c:5689-5695`,
  `workqueue.c:2598-2629`).
- fedora boots with no `rcu_nocbs` (checked 2026-11-06).

But Ada's rule rules out moving this work by priority, nice or affinity (`goal5-tails.md`, "Reuse off's teardown: where it
runs"). Hex can only make less of it. The lever was built (`cp/t7-g16-teardown-cpu`) and is recorded as ruled out,
not merged. It wouldn't have moved the mutex holds below in any case.

## What a create can wait on, and where it is taken (v6.19)

**Two host-wide mutexes are held by a memory cgroup's offline, by every new superblock, and by every superblock's
end:**
- `list_lrus_mutex` (`mm/list_lru.c:20`):
  - taken by `list_lru_register` (`:27-35`) and `list_lru_unregister` (`:37-45`), for every memcg-aware list_lru;
  - held by `memcg_reparent_list_lrus` (`:467-503`) across a walk of every such list_lru on the host.
- `shrinker_mutex` (`mm/shrinker.c:11`):
  - taken by `shrinker_memcg_alloc` (`:216-239`) and `shrinker_register` (`:745-749`) when a memcg-aware shrinker is
    made, and by `shrinker_free` (`:789-804`) when it goes;
  - taken by `alloc_shrinker_info` (`:77-101`) when a memory cgroup comes online;
  - held by `reparent_shrinker_deferred` (`:285-311`) across every node and every shrinker unit of the dying memcg.
- **Every superblock takes them several times** (`fs/super.c`). `alloc_super`:
  - makes a memcg-aware shrinker, `shrinker_alloc` (`:378`);
  - inits its dentry and inode list_lrus, `list_lru_init_memcg` (`:388`, `:390`), which register them.
  Its end destroys both list_lrus and frees the shrinker (`:300-302`, `:473-484`).
- So a create that makes a superblock (a proc, a tmpfs, a devpts, an mqueue) waits on those mutexes for whatever
  holds them: another box's memcg offline, walking every list_lru on the host, or another superblock's making or
  end. Run 2 of `create_tail` caught one create waiting 1.364 ms in `__list_lru_init` (`goal5-tails.md`). That
  wait is on a mutex, not a CPU, so no placement of the teardown removes it.
- A create also waits on `cgroup_mutex` wherever it takes it: the cgroup's mkdir, a `CLONE_INTO_CGROUP` clone
  (`goal5-tails.md`). The offline and release work take it too (`cgroup.c:6049`, `:5629`).

## The three ways to make less of it

### 1. No superblock and no `cgroup_mutex` on a no-owner create's path

goal 5's host.sh bundles (`bench/competitive/oci_inside.sh:42-44`) mount `/usr` (a bind), `/proc` and `/dev` (a tmpfs).
A no-owner create's superblocks and locks, as built:
- **cgroup:** a slot made ahead and an init parked in it (decision 8). Neither mkdir nor clone is on the path, unless
  the ahead queue runs dry.
- **network, ipc and uts namespaces:** from the pools made ahead. A fresh ipc namespace mounts its mqueue superblock
  when it's made, so a pool that's dry puts one on the path.
- **`/dev`:** the prepared tmpfs pool (goal 5 extended step 3), unless dry.
- **`/proc`:** made by the init on the path (part 19), unless parked procs are on. With them on (decision 8), the proc
  is made on the parking thread, here the `hex-ahead` refill.

So with parked procs on and no pool dry, a no-owner create makes no superblock and takes no `cgroup_mutex`. Its waits
on `list_lrus_mutex` and `shrinker_mutex` go with them. The test is the lead's A/B: no-owner p99.9 with
`HEXBENCH_PARKED_PROCS` 0 against 1, with `create_tail`'s trace showing no create blocked in `__list_lru_init` or the
shrinker's calls, and every pool's dry count beside it. What it doesn't remove: CPU preemption by the teardown work
itself, which is 2 and 3's.

### 2. Linux 7.x's offline

- **98733f3f0bec**, "mm/list_lru: drain before clearing xarray entry on reparent" (2026-06-01). It is a race fix, not
  a speed-up: it reparents each list before clearing the dying memcg's xarray entry, to stop a concurrent
  `list_lru_del` corrupting a list. In passing, `memcg_reparent_list_lrus` now skips a list_lru the dying memcg never
  used with a lockless `xa_load` (v7.2 `mm/list_lru.c:525-563`), where 6.19 took the xarray's IRQ lock and stored for
  every one. The walk of every list_lru on the host under `list_lrus_mutex` stays; only its per-list cost falls.
- **f1cf8d2f36dc**, "mm: memcontrol: eliminate the problem of dying memory cgroup for LRU folios" (dated 2026-03-05; the lead places it in
  7.1, not checked here). A folio's
  `memcg_data` now points to an object cgroup, and a cgroup's death reparents them. The dying memcg is freed promptly
  instead of lingering, at the cost of reparenting work at death.
- Whether 7.x's offline is cheaper per box is a measurement: `css_cost` (`bench/competitive/probes/css_cost.*`), with
  kprobes on `memcg_reparent_list_lrus` and `reparent_shrinker_deferred`, on 6.19 against the 7.2 guest.

### 3. Less teardown per box

- **Fewer controllers:** each css a box doesn't have spares one offline, one release and one RCU-delayed free, about
  0.08 ms of kworker time each (`create_tail.md`, "Reuse off after the cut"). io is the only one a box of no bounds
  never writes, and it saves about 0.1 ms off p99.9 (`goal5-tails.md`).
- **No memcg for a box that sets no memory limit.** Memory is the costly css: 0.388 ms of a box's 0.795 ms of css
  teardown, from the two walks above. A cgroup's `subtree_control` decides which controllers its children have
  (`Documentation/admin-guide/cgroup-v2.rst:472-491`: "disabling 'memory' from B would remove the 'memory.' prefixed
  controller interface files from C and D").
  - So no-owner boxes whose spec sets no memory limit can live under one parent whose `subtree_control` leaves memory
    out, beside the parent the other boxes live under. Each such box then has no memcg to offline.
  - Its memory is charged to that parent's memcg, which is made once and never offlined (`cgroup-v2.rst:1964-1965`:
    memory stays charged to the cgroup that instantiated it).
  - **What it loses:**
    - the box's own `memory_bytes`, `memory_peak_bytes` and `oom_kills` in `Usage` (`hex-api/src/runtime.rs:138-149`).
      Hex reads them from the box's `memory.current`, `memory.peak` and `memory.events`
      (`hex-runtime/src/linux/cgroup.rs:564-622`), which such a box doesn't have.
    - per-box OOM confinement: an OOM in the shared parent's memcg can kill a task of any box under it.
  - `Usage` has no way to say "not measured", so CLAUDE.md §3 asks for an API change (an optional field, or a typed
    refusal for those fields) rather than zeros.
  - cpu, io and pids reporting are untouched.
- Not built. It needs Ada's or the lead's word on losing per-box memory reporting for such boxes.

### 4. Sketch, if 1 doesn't reach the bar: a memory slot made once, under a fresh leaf without memory

The next lever if 1 left the no-owner p99.9 over, with 3 held (the lead, 2026-11-06). It was built off by default,
scoped to boxes of no owner with no memory bound that couldn't change their cgroup, and measured
(`bench/competitive/probes/goal16_ab.md`, "Lever 4"): about 4% where it applies, and it doesn't apply to the bar's
memory-bounded boxes. A recorded negative: the code stays on cp/t7-g16-memslot, not merged (the lead,
2026-11-06T09:17Z). It keeps what 3 gives up, per-box memory reporting and per-box OOM confinement, and takes away the
memory cgroup's offline, the costly css in a box's teardown.

- **The kernel's rule it rests on.** A cgroup's effective css for a controller is "the matching css of the nearest
  ancestor including self which has @ss enabled" (`cgroup_e_css_by_mask`, `kernel/cgroup/cgroup.c:534-560`). So a
  leaf whose parent's `subtree_control` leaves memory out has no memcg of its own: its tasks are charged to the
  parent's, and its removal queues no memory offline. The parent's other controllers still reach the leaf
  (`cgroup-v2.rst:472-491`).
- **The layout.** Each box of no owner gets a slot `M`, made once and handed from one box to the next, and a fresh leaf
  `C` inside it, made ahead as today. `M` enables memory and hands the leaf cpu, io and pids, not memory. `M` holds one
  box at a time, so:
  - memory is bounded and reported per box. The box's memory bounds go on `M`; Hex reads the box's memory from `M`, as
    deltas from a baseline taken at the hand-over, with the peak through a `memory.peak` descriptor written then. That
    is what reuse within one owner already does (decision 5, "Usage", in `docs/design/runtime-architecture.md`).
  - an OOM in `M` is still one box's: the memcg that hits its bound is `M`, which holds one box. (Hex writes no
    `memory.oom.group` today, so the kernel kills one task in the memcg, as before.)
  - cpu, io and pids are the leaf's own, fresh for each box, so their counters start at zero.
- **What the box sees.** Its cgroup namespace's root is the leaf, so `M`, and the counters earlier boxes left in it,
  are outside its namespace, and the kernel contains a delegated sub-hierarchy ("processes can't be moved into or out of the
  sub-hierarchy by the delegatee", `cgroup-v2.rst:574-576`). This is the part reuse
  across owners lacks today: there, the box reads the whole reused cgroup (`goal5-cgroup-lock.md`, f34).
- **What a box's delete still tears down:** the leaf's cpu, io and pids css and its own, at about 0.08 ms each
  (`create_tail.md`). The memory offline goes: 0.388 ms of 0.795, plus its own work item, and its hold of
  `list_lrus_mutex` and `shrinker_mutex`. So that is about half to 60% of a box's css teardown time, if the 0.08 ms
  each holds.
- **What it costs:**
  - **A box sees no `memory.*` files.** Inside, the leaf has no `memory.max` or `memory.current`. A runtime that sizes
    itself from `memory.max` (a JVM's container support, for one) then sees no limit. This has to be decided, or the
    lever limited to boxes whose spec sets no memory limit, where the box would read `max` today.
  - **What the last box left stays charged to `M`.** That is page cache and reclaimable slab; anon and tmpfs memory go
    with its processes and mounts. It counts against the next box's `memory.max` until reclaim takes it (inferred, as
    for reuse within one owner, `goal5-cgroup-lock.md` f82). A `memory.reclaim` write by the remover at the hand-over
    would clear it, at a cost to be measured.
  - **A slot whose box was killed through `cgroup.kill` is never handed on** (the `kill_seq` rule, `goal5-cgroup-lock.md`). The leaf
    takes the kill, so `M` is unaffected: to be checked by the kill_seq probe.
  - **Bounds.** There is one `M` per box in flight plus those made ahead, under `reuse::PER_NODE`.
- **What the gain depends on (lever 2's probe).** In 6.19 a memcg offline locks and stores into the xarray of every
  memcg-aware list_lru on the host (`mm/list_lru.c:467-503`). It also walks every shrinker id up to the high-water mark
  (`mm/shrinker.c:285-311`). Each superblock adds two list_lrus and a shrinker (`fs/super.c:378-390`). So the 0.388 ms
  grows with the host's superblocks, other tenants' included, and lever 2's kprobes split it. If that walk is most of
  it, this lever removes it on every kernel, and 7.2's skip only shortens it.
- **What would falsify it:**
  - a box's css teardown time with the leaf doesn't fall by about memory's share;
  - no-owner p99.9 doesn't fall with it;
  - the hand-over (`memory.max` write, peak descriptor, baseline read) costs more at create p50 than the fresh slot's
    path.

## What would falsify each

- 1: no-owner p99.9 doesn't fall with parked procs on and no pool dry, or `create_tail` still shows a create blocked
  in `__list_lru_init` or a shrinker call.
- 2: `css_cost`'s memory offline in the 7.2 guest isn't below 6.19's.
- 3: a box without a memcg doesn't cut its css teardown time by about memory's share, or no-owner p99.9 doesn't fall
  with it.
- 4: as its own list above.
