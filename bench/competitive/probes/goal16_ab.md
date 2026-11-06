# Goal 16: the levers on the no-owner p99.9, measured on fedora's host (AC-T7.1.3)

Goal 5's bar as written (host.sh `PHASES=oci`, no owner, best of 2) is over at p99.9. Goal 16 asked which of the levers
in `docs/research/runtime/goal16-teardown.md` move it, and by how much (the lead, 2026-11-06). Runs: fedora, Linux
6.19.10, 2026-11-06, noisy host, timed parts under `fedora-timed.lock` on CPUs 4-15, no priority or affinity change
for Hex. host.sh passes are 2 rounds of 10,000 after 100, lifecycle and start_true, libcrun beside. Bodies, logs and
status: `~/.cache/hex-t7/g5tail/g16run-20261106T054154Z` (levers 1 and 2, the teardown CPU), `g16l4-20261106T065248Z`
and `g16l4b-20261106T075250Z` (lever 4), and `g16sb-20261106T065347Z` (the 7.2 guest at K superblocks). Each pass's
`out` directory is named below.

**The runs' code:** builds of cp/t7-g16-memslot (3cf5ad6, 989c245, bbbba2e), the record tree's code with lever 4's
memory slots added, off unless a run turns them on. Gate 6 there at 989c245: unit
hex-g5tail-g16gates2-20261106T060645Z, gates 7/7, the root lab and the 7.2 guest (oci_box, oci_reuse) exit 0, one
unit, its own target dir. That code stays there as a recorded negative (the lead, 2026-11-06T09:17Z); this tree takes
the record and the harness options alone.

## Lever 1: parked procs on, so no superblock is made on a no-owner create's path

One static-glibc build of cp/t7-g16-memslot 3cf5ad6 (hex sha256 2a43da90b1169543, seam_oci 4a276a890c133429), parked
procs off (A, `HEXBENCH_PARKED_PROCS=0`) against on (B, `=1`), in the order A B B A.

| pass | arm | out | create p99.9 r1 / r2 (ms) | create+start of `true` p99.9 r1 / r2 (ms) | libcrun create p99.9 (ms) |
|---|---|---|---|---|---|
| 1 | A | 20261106T055316Z | 2.07 / 2.04 | 2.81 / 2.48 | 40.94 |
| 2 | B | 20261106T060408Z | 1.99 / 1.82 | 2.69 / 2.47 | 53.21 |
| 3 | B | 20261106T061457Z | 2.23 / 2.03 | 2.54 / 2.32 | 5.54 |
| 4 | A | 20261106T063905Z | 1.88 / 1.81 | 2.18 / 2.29 | 16.10 |

- Best of 2, Hex's create p99.9: A 2.04 and 1.81, B 1.82 and 2.03, a mean of 1.93 ms each. create+start: A 2.48 and
  2.18, B 2.47 and 2.32. The lever doesn't move it.
- It is engaged: the cgroups made ahead were dry once a phase (its first create), the /dev, proc, ipc and uts pools
  never on the path after it, the network namespace pool on it once in all sixteen phases; B's parked procs hit 10,099 of
  10,099 lifecycle creates, 0 dry.
- libcrun's own p99.9 swings from 5.5 to 53 ms with the host's noise, so the ratio does too. Against its quiet 5.54,
  Hex's 2.03 is 0.37x; the bar's 0.25x would need about 1.4 ms.

**What the slowest creates wait on** (create_tail, one round of 10,000, every other create traced, 5,000 each):
- A (create-tail-20261106T065112Z), create p99.9 2.39 ms: one of the ten slowest (2.01 ms) is blocked in
  `__list_lru_init < alloc_super < sget_fc < get_tree_nodev`, woken by a kworker running `css_killed_work_fn`, 0.40 ms
  of the slowest 0.1%'s mean.
- B (create-tail-20261106T065038Z), create p99.9 1.98 ms: no create blocked in `__list_lru_init` or a shrinker call.
- In both, the slowest 0.1% are runnable behind `css_killed_work_fn`, run by a kworker, ksoftirqd or another tenant's
  thread: 1.0 to 1.7 ms of their mean, single waits to 2.3 ms. So lever 1 takes the superblock's mutex wait off the
  path, as `goal16-teardown.md` §1 says, and the p99.9 is held by the earlier boxes' cgroup offline on the create's
  CPU.

## Lever 2: Linux 7.x's offline

`css_cost` on the same binaries, 2,000 boxes each, with kprobes on `memcg_reparent_list_lrus` and
`reparent_shrinker_deferred` (ms per box):

| kernel | superblocks added | `css_killed_work_fn` | memory offline | `memcg_reparent_list_lrus` (max) | `reparent_shrinker_deferred` |
|---|---|---|---|---|---|
| 6.19.10, fedora (css-cost-20261106T065151Z) | 0 | 0.203 | 0.153 | 0.127 (1.83) | 0.020 |
| 7.2.15, guest (k0) | 0 | 0.035 | 0.022 | 0.010 (0.16) | 0.001 |
| 7.2.15, guest (k2000) | 2,000 | 0.702 | 0.566 | 0.491 (2.57) | 0.020 |
| 7.2.15, guest (k8000) | 8,000 | 1.362 | 1.085 | 1.019 (2.72) | 0.052 |

- On fedora the memory offline is 75% of `css_killed_work_fn`, and the walk of every list_lru is most of it. Every
  other controller's callbacks are under 0.01 ms a box.
- The guest's bench namespace holds 27 mounts; fedora runs dozens of other tenants' containers and their mounts (its
  count of superblocks isn't read here), so 6.19 against the plain guest compares hosts, not kernels. With K more tmpfs
  superblocks (two memcg-aware list_lrus each; `SUPERBLOCKS=K`, refused on a kernel the run doesn't have alone), 7.2's
  walk grows by 60 to 120 ns a list_lru, though it has 98733f3f0bec's lockless skip (v7.2 `mm/list_lru.c:525-563`):
  with 2,000 more it is already 4x fedora's 6.19 figure.
- So 7.x doesn't take the walk away. It grows with every superblock on the host, other tenants' included. The guest's
  shrinker count isn't shown (no debugfs view in the bench's namespace).

## The teardown CPU (ruled out, a measurement)

Ruled out by Ada's rule against moving this work by priority, nice or affinity (`goal5-tails.md`, "Reuse off's
teardown: where it runs"). Recorded, not merged. One build of cp/t7-g16-teardown-cpu ae6b94d (unit
hex-g5tail-g16ab-20261106T020058Z's binaries), off (A) against `HEXBENCH_TEARDOWN_CPU=1` (B), A B B A:

| pass | arm | out | create p99.9 (ms) | create+start of `true` p99.9 (ms) |
|---|---|---|---|---|
| 1 | A | 20261106T065212Z | 1.87 | 2.42 |
| 2 | B | 20261106T071411Z | 1.50 | 2.44 |
| 3 | B | 20261106T073838Z | 1.63 | 2.43 |
| 4 | A | 20261106T075805Z | 1.80 | 2.25 |

Create p99.9 fell from a mean of 1.84 to 1.57 ms with the teardown kept on one CPU; create+start didn't move. The
passes' JSON carry no count of the teardown CPU's use (seam_oci at ae6b94d reads none); `env.txt` shows the variable
reached host.sh.

## Lever 4: memory slots

`Node::with_memory_slots` (`goal16-teardown.md` §4), off by default, on cp/t7-g16-memslot only: a recorded negative,
not merged (the lead, 2026-11-06T09:17Z).

**First A/B, invalid as a measure of the lever** (unit hex-g5tail-g16l4-20261106T065248Z, build of 989c245, hex
23d54f30725afce9): seam_oci bounds every box to 256 MiB of memory, and the slots are scoped to boxes with no memory
bound. So in the on arm (20261106T072612Z, 074821Z) all 10,100 creates a phase were counted `not_for_memory_bound`,
each made its own cgroup and lost the cgroup made ahead: create p99.9 2.39 / 2.36 against the off arm's 1.74 / 1.70
(20261106T070246Z). That is what the ahead path is worth, not the lever. The lever can't move the bar as written while
it is scoped so.

**Second A/B: boxes without their memory bound** (`HEXBENCH_BOX_MEMORY=none`, so not the bar's figure; unit
hex-g5tail-g16l4b-20261106T075250Z, build of bbbba2e, hex 0155c4a83888504b, seam_oci a82441436101fc2f). Off (M0)
against on (M1), M0 M1 M1 M0, no owner, parked procs on and Docker's default capabilities (`HEXBENCH_DOCKER_CAPS=1`) in
both arms:

| pass | arm | out | create p99.9 r1 / r2 (ms, seam JSON) | best of 2 (table.md) | create+start of `true` p99.9 (ms) | libcrun create p99.9 (ms) |
|---|---|---|---|---|---|---|
| 1 | M0 | 20261106T080746Z | 2.09 / 1.86 | 1.84 | 2.40 | 3.17 |
| 2 | M1 | 20261106T082807Z | 1.67 / 1.60 | 1.59 | 2.43 | 3.26 |
| 3 | M1 | 20261106T083818Z | 1.72 / 1.73 | 1.71 | 2.13 | 4.73 |
| 4 | M0 | 20261106T084834Z | 1.60 / 1.64 | 1.59 | 2.27 | 4.25 |

- Engaged in every M1 phase: 6 to 13 slots made, about 10,090 taken again, every one given back, none refused.
- Create p99.9: a mean of 1.72 ms off, 1.65 on, about 4%, within the passes' spread; create+start 2.34 against 2.28.
- The reclaim at hand-back costs 4.4 to 5.3 s of the remover's time a phase, about 0.5 ms a box for about 158 KB
  reclaimed each: more CPU than the memory offline it spares (0.153 ms a box, lever 2's table), on a thread the node
  doesn't place.
- create_tail (create-tail-20261106T090050Z on, 090135Z off): create p99.9 1.65 against 1.68 ms. The slowest 0.1% still
  wait behind `css_killed_work_fn`, 0.67 ms of their mean on against 0.85 off: the leaf's own controllers' offline and
  the host's other cgroups' are still there.

## Which moves the no-owner p99.9, and by how much

None reaches the bar. Lever 1 removes the superblock wait and doesn't move the p99.9 (1.93 against 1.93 ms). Lever 2,
7.x, doesn't take the walk away; it grows with the host's superblocks. The teardown CPU (ruled out) took 15% off
create, not create+start. Lever 4, on boxes without a memory bound, takes about 4%, within the spread, and it doesn't
engage on the bar's own boxes as scoped. Hex's create p99.9 stays at 1.6 to 2.0 ms. Against libcrun's quiet p99.9 of
3 to 5.5 ms in these passes, the bar's 0.25x would need 0.8 to 1.4 ms.

## Whose work the no-owner tail waits on (the lead's follow-up 3, 2026-11-06)

create_tail at the bar's conditions: no owner, Hex with the defaults of 3cf5ad6's build (parked procs off, as the
record tree had them before 5a2a8d6 turned them on for a root node; memory slots off; the 256 MiB bound). Lever 1's
create_tail above is the one with parked procs on. The build of 3cf5ad6, 3 rounds of 10,000 after 100, 15,000 traced creates, CPUs 4-15 under the timed lock. The harness
(this tree's create_tail, first at cp/t7-g16-memslot a081cf4) adds an fprobe on each controller's offline and
release callback (the cgroup id its css is of), joined to
`cgroup_rmdir`'s and `cgroup_release`'s paths, so each teardown work item is named by whose cgroup it took apart (this
run's container id in the path is this run's boxes). It also traces the bundles' programs (`sleep`, `true`), so a
create's wait on its container's process is followed into it.

**The host** (both runs, start and end): 106 to 109 mount namespaces, 4,559 to 4,787 mounts, 1,873 to 1,883 distinct
superblocks mounted; load 31 to 40. The superblocks are a floor: one detached from every mount isn't counted.

**Run 2** (create-tail-20261106T092236Z, the programs traced): create p99.9 2.21 ms. Its slowest 0.1%, 15 creates, mean
ms a create by where it went:

| where | mean ms | whose |
|---|---|---|
| a lock wait: the create's lookup of a cgroup file (`kernfs_dop_revalidate`, `kernfs_iop_lookup`) queued behind the refill's `mkdir` (`hex-ahead`, `kernfs_add_one < css_populate_dir`) | 1.377 | Hex's own refill, itself waiting about 3 ms for a kworker running `kernfs_notify_workfn` on CPU 18 |
| run: seam_oci and hex | 0.612 | Hex |
| preempted by network namespace teardown (`fqdir_work_fn`, unbound kworkers) | 0.347 | not named (no probe on a namespace's owner) |
| preempted by cgroup teardown (`css_killed_work_fn`) | 0.300 | this run's Hex boxes, all of it |
| woken, not yet run | 0.189 | — |
| RCU softirq on the create's CPU | 0.182 | not named |
| a lock wait in `memfd_fcntl`'s LRU drain | 0.064 | — |
| preempted by cgroup teardown whose callbacks weren't probed | 0.008 | — |
| preempted by another tenant's task (`systemd-journal`) | 0.003 | another tenant |

- The three slowest creates (3.69, 3.69, 3.86 ms) are the first row's chain, read oldest first:
  1. The create takes its cgroup made ahead, which wakes the refill.
  2. The refill's `mkdir` waits about 3 ms for the cgroup filesystem's lock, held by `kernfs_notify_workfn`: a single
     host-wide work item that delivers every cgroup file's change notifications (`cgroup.events` and the like), the
     host's tenants' and Hex's alike, so whose it was serving isn't named here.
  3. The create's own lookup of a file in its cgroup then queues behind that waiting `mkdir`.
- The fourth slowest (3.58 ms) is the create's init preempted 2.68 ms by `fqdir_work_fn`.
- libcrun's teardown doesn't overlap Hex's calls here: create_tail runs Hex alone. In host.sh, `oci_inside.sh` runs each
  runtime's round whole, one after another in a shuffled order, so only a hand-over's spill could overlap.

**Run 1** (create-tail-20261106T091721Z, the programs not traced), read again with release work items named: create
p99.9 1.91 ms. Of its slowest 0.1%, 0.853 ms was preempted by teardown of this run's Hex boxes, 0.188 by work items
whose callbacks weren't probed, none by another tenant's cgroups. Its largest single charge, 0.660 ms blocked in
`ppoll` woken by its container's process, is what run 2 follows into the chain above.

**Read together:** no other tenant's cgroup teardown preempts the slowest creates. What they wait on is Hex's own work:
- its boxes' cgroup teardown, 0.3 to 0.85 ms;
- probably its boxes' network namespace teardown, 0.35 ms (Hex removes the most namespaces on the host by far, but no
  owner is named);
- and, largest, its own refill's `mkdir` holding up the create's cgroup file lookups, 1.38 ms, while that `mkdir` waits
  on the host-wide notification work.

## No cgroupfs lookup on a no-owner create's path (cp/t7-g16-prefetch, the lead, 2026-11-06T09:47Z)

The biggest wait above, 1.38 ms of the slowest 0.1%, was the create's own lookups of its cgroup's files, queued behind
the refill's `mkdir`. bpftrace on `kernfs_iop_lookup` found 14 such lookups a create (7 bounds, 4 counters, `memory.peak`,
2 of the box's id), and oci_reuse's red counted 20 on the creating thread (2842286). The fix (1190a2c), on by default:
- the refill opens those files when it makes the cgroup ahead;
- a box root keeps the id-named cgroups it may hold, so the id's absence needs no lookup;
- the refill is woken after the create's file work.
The count is 0. Gate 6 at 1190a2c: unit hex-g5tail-g16pfgates-20261106T094401Z, gates 7/7, the root lab and the 7.2
guest exit 0, one unit, its own target dir.

**A/B at the bar's conditions** (unit hex-g5tail-g16pfab-20261106T094401Z): A cp/t7-g16-record 13214b6 (hex
619b6bf5d6aa0973, seam_oci 6af462e4248fc994), B cp/t7-g16-prefetch 1190a2c (hex 0307ce5541d10960, seam_oci
4656f14dd6731c34), each as it ships (parked procs on for the root node). host.sh PHASES=oci, no owner, 2 rounds of 10,000
after 100, A B B A, CPUs 4-15 under the lock, noisy:

| pass | side | out | Hex create p99.9 (ms) | Hex create+start of `true` p99.9 (ms) | libcrun create p99.9 (ms) |
|---|---|---|---|---|---|
| 1 | A | 20261106T094720Z | 1.72 | 2.59 | 17.86 |
| 2 | B | 20261106T095745Z | 1.60 | 2.46 | 4.58 |
| 3 | B | 20261106T101757Z | 1.81 | 2.44 | 4.50 |
| 4 | A | 20261106T102834Z | 1.75 | 2.49 | 7.19 |

Create p99.9: a mean of 1.735 ms (A) against 1.705 (B), 2%. create+start: 2.54 against 2.45, 3.5%. Both are within the
passes' spread. Every pool was full; parked procs hit 10,098 to 10,100 a phase in both arms. Against libcrun's 4.50 and
4.58 in B's passes, B's create is 0.36x and 0.40x, not 0.25x.

**create_tail** (3 rounds, 15,000 traced creates each):
- B (create-tail-20261106T103911Z): create p99.9 1.92 ms. The slowest 0.1% have no cgroupfs lock wait. 1.40 ms of their
  mean is preemption by `css_killed_work_fn` tearing down this run's own boxes; the rest is Hex running (0.53) and RCU
  softirqs.
- A (create-tail-20261106T104104Z): create p99.9 2.20 ms. In the slowest 0.1%:
  - 1.26 ms is this run's boxes' teardown;
  - the lookup wait is still there, but only 0.17 ms this time (1.38 in the earlier trace: it varies with what else holds
    the hierarchy's lock);
  - 0.26 ms is a btrfs directory lookup, the state directory's, the bench container's overlay on the host's btrfs.
- Host: 126 to 132 mount namespaces, 5,780 to 6,013 mounts, 1,947 to 1,989 superblocks mounted.

So the lock wait is gone. What's left in the no-owner tail is the kernel's teardown of Hex's own earlier boxes,
preempting the next create on its CPU, which this rule leaves Hex to make less of, not to move.
