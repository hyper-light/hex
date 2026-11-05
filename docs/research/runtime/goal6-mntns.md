# Goal 6: a mount namespace without a copy, read from the kernel at v7.0 and v7.1 (2026-11-04)

Runtime design goal 6 (decision 6, `docs/design/runtime-architecture.md`) asks that create make the container's mount
namespace with `open_tree(OPEN_TREE_NAMESPACE)` on Linux 7.0 and with `CLONE_EMPTY_MNTNS` on 7.1, detected at runtime,
the faster one the default, and that Ada's under-load bar for the init's root (part 19) hold. This note reads both calls
from the kernel's own source, counts the holds of the mount lock (`namespace_sem`) each takes, states their limits and
failures, and sets that against what the pruned seed's path (goal 5, `goal5-root.md` §7, `goal5-mounts.md`) already
does on 6.19.

Sources: Linux at tags v6.19, v7.0 and v7.1, read from `raw.githubusercontent.com/torvalds/linux/<tag>/<path>`
(`fs/namespace.c`, `fs/nullfs.c`, `kernel/fork.c`, `kernel/nsproxy.c`, `kernel/pid_namespace.c`, `fs/proc/root.c`,
`include/uapi/linux/mount.h`, `include/uapi/linux/sched.h`); crun 1.30.1's release tarball, `src/libcrun/linux.c`. Line
numbers below are the tag's. Without a tag they are v7.1's `fs/namespace.c`.

## 1. `open_tree(OPEN_TREE_NAMESPACE)` (Linux 7.0)

- **The flag.** `#define OPEN_TREE_NAMESPACE (1 << 1) /* Clone the target tree into a new mount namespace */`
  (`include/uapi/linux/mount.h:65`, v7.0 and v7.1). v6.19's `vfs_open_tree` doesn't know it, so it refuses with
  `EINVAL` at its flag check (v6.19 `fs/namespace.c:3073-3076`).
- **What it checks.** The flag set (`:3197-3200`); `AT_RECURSIVE` only with a clone or a namespace (`:3202-3204`); not
  with `OPEN_TREE_CLONE` (`:3206-3207`); `CAP_SYS_ADMIN` over the caller's own user namespace, `EPERM` otherwise
  (`:3219-3221`), where `OPEN_TREE_CLONE` asks `may_mount()` in the caller's mount namespace (`:3223-3224`). Then the
  path's lookup (`:3226-3229`), and only then the namespace (`:3231-3232`). v7.1 adds `ENOTDIR` for a source that isn't
  a directory (`:3106-3107`); v7.0 has no such check (v7.0 `:3090-3165`).
- **What it makes** (`create_new_namespace`, `:3094-3176`). A namespace owned by the caller's user namespace
  (`alloc_mnt_ns(user_ns, false)`, `:3112`; not anonymous, so `setns` takes it). It clones the caller's namespace's root
  mount alone, under the lock (`LOCK_MOUNT_EXACT_COPY`, `:3122`, which is `lock_mount_exact` with a copy, `:3843-3874`).
  At 7.x that root is always the immutable, empty nullfs (`fs/nullfs.c:29-34`; the boot namespace is nullfs with the
  rootfs on it, `:6197-6203`, and every namespace since is a copy of it), which the code asserts (`:3120`). It copies
  the given tree, recursively with `AT_RECURSIVE` (`__do_loopback`, `:3149`), attaches it on that root (`:3163`),
  locks the tree where the caller's user namespace isn't the source namespace's owner (`CL_SLAVE` and
  `lock_mnt_tree`, `:3109-3110`, `:3164-3165`), and adds every mount to the namespace (`:3168-3171`). It returns an
  nsfs file (`open_new_namespace`, `:3178-3186`).
- **Joining it.** `setns(fd, CLONE_NEWNS)` (`mntns_install`, `:6458-6498`) wants `CAP_SYS_ADMIN` over the namespace's
  owner and `CAP_SYS_CHROOT` and `CAP_SYS_ADMIN` over the caller's (`:6467-6470`), refuses an anonymous namespace and a
  caller whose `fs_struct` is shared (`fs->users != 1`, `:6472-6476`), and sets the caller's root and working
  directory to the topmost mount on the namespace's root (`LOOKUP_DOWN`, `:6482-6496`): the copied tree. It takes no
  mount lock. So a process that joins it is already at the container's root, with nothing beneath it but nullfs: no
  `pivot_root`, no `chroot`, and `current_chrooted` is false, so nested user namespaces stay allowed (`goal5-root.md`
  §3).
- **Its propagation.** The copy is made with `copy_flags` 0 for a caller in the source's user namespace (`:3103`,
  `:3109-3110`), so `clone_mnt` keeps a shared source's peer group: the new namespace's root is the host source's peer
  until something changes it. This is the leak `goal5-root.md` §9 fixed for the pruned path; on this path the change
  has to come after the namespace is made. crun does it in the container's process after its `setns`: a bind remount
  of `/`, `MS_REC | MS_PRIVATE`, then the spec's rootfs propagation (crun 1.30.1 `src/libcrun/linux.c:4162-4174`).
- **Which sources it takes.** `may_copy_tree` (`:2909-2971`): a mount of the caller's own namespace, or an anonymous
  (detached) tree whose origin is the caller's namespace (`check_anonymous_mnt`, `:951-960`: `seq_origin` 0 or equal to
  the caller's namespace's id). An `open_tree(OPEN_TREE_CLONE)` copy records the namespace it was cloned in
  (`get_detached_copy`, `:3044-3055`), so a tree the agent cloned can be made a namespace only by a process in the
  agent's namespace: not by a pruned seed. An `fsmount` mount's anonymous namespace records none (`:4521-4528`), so
  any namespace may. Unbindable sources are refused (`:2977-2978`).

## 2. `CLONE_EMPTY_MNTNS` (Linux 7.1)

- **The flag.** `#define CLONE_EMPTY_MNTNS (1ULL << 37) /* Create an empty mount namespace. */` (v7.1
  `include/uapi/linux/sched.h:42`), with `UNSHARE_EMPTY_MNTNS 0x00100000` for `unshare` (`:54`; it aliases
  `CLONE_PARENT_SETTID` and is translated, `kernel/nsproxy.c:226-232`). It is `clone3`'s only: v7.1's
  `clone3_args_valid` lists it among the known flags (`kernel/fork.c:2964-2969`), v7.0's doesn't (v7.0
  `kernel/fork.c:2897-2900`), so v7.0 and earlier refuse it with `EINVAL` before anything is made (`:3017-3022`).
- **What it makes.** It implies `CLONE_NEWNS` (`kernel/fork.c:2682-2685`), so the `CLONE_NEWNS | CLONE_FS` refusal
  holds (`:1986`), and the capability is `CLONE_NEWNS`'s, `CAP_SYS_ADMIN` over the user namespace (`kernel/nsproxy.c:181`).
  `copy_mnt_ns` then clones the parent namespace's root mount alone, `clone_mnt(old, old->mnt.mnt_root, copy_flags)`,
  under the lock (`:4252`, `:4261-4262`), and sets the child's root and working directory to it (`:4275-4290`), where
  a plain `CLONE_NEWNS` copies every mount (`copy_tree`, `:4264`) and walks both trees (`:4291-4320`). The root it
  clones is nullfs (§1), so the child starts on an empty, immutable directory: the container's root is attached on it
  by `move_mount`, as the pruned init attaches it on the seed's tmpfs today (`init.rs`, `setup_rootfs_pruned`).
- **No copy at all, but one attach.** The child's namespace holds one mount, where the pruned seed's copy holds two
  (`goal5-root.md` §7). The rootfs is still a tree made in the agent's namespace and attached in the child, as on the
  pruned path.

## 3. `fsmount(FSMOUNT_NAMESPACE)` (Linux 7.1)

`FSMOUNT_NAMESPACE 0x00000002` (v7.1 `include/uapi/linux/mount.h:113`) makes a new filesystem's mount the root tree of
a new namespace (`SYSCALL_DEFINE3(fsmount)`, `:4431-4533`: `CAP_SYS_ADMIN` over the caller's user namespace,
`:4445-4447`; `open_new_namespace(&new_path, MOUNT_COPY_NEW)`, `:4517-4519`, which clones the one new mount,
`:3146-3147`). The mount the context made is never in a namespace, so dropping it takes no lock (`dissolve_on_fput`
returns at its RCU check, `:2304-2307`). A fresh superblock's mount has no peer group. So a layered root's overlay,
which the agent makes with `fsmount` today (`run.rs`, `pruned_create`), becomes a namespace in one call and one hold,
with no propagation to change.

## 4. The mount lock, hold by hold

`namespace_sem` is one rwsem for every mount namespace on the host (`namespace_lock`, `:1727-1730`). Its write holds
on the paths in question:

| call | holds for write | where |
|---|---|---|
| `open_tree(OPEN_TREE_CLONE)` | 1 | `get_detached_copy`, `guard(namespace_excl)` `:3042` |
| `open_tree(OPEN_TREE_NAMESPACE)` | 1 (the root's clone, the tree's copy and attach under it) | `lock_mount_exact` `:3852`, released at `create_new_namespace`'s end |
| `fsmount(FSMOUNT_NAMESPACE)` | 1 | as above, through `open_new_namespace` |
| `clone3(CLONE_NEWNS)` or `CLONE_EMPTY_MNTNS` | 1 | `copy_mnt_ns`, `guard(namespace_excl)` `:4252` |
| `move_mount` | 1 | `do_move_mount`, `LOCK_MOUNT_MAYBE_BENEATH` `:3642` |
| `mount(2)` bind | 1 (clone and graft under it) | `do_loopback`, `LOCK_MOUNT` `:3012` |
| `mount_setattr` with a propagation | 1 | `do_mount_setattr` `:4937-4943`; flags alone take only the mount hash (`:4953`) |
| `setns(CLONE_NEWNS)` | 0 | `mntns_install` `:6458-6498` |
| a detached tree's last close, never attached | 1, then an expedited RCU grace period | `dissolve_on_fput` `:2309-2318`; `namespace_unlock` `:1684-1724` |
| a namespace's last reference | 1, then an expedited RCU grace period | `put_mnt_ns` `:6271-6279` |

Per create, for the container's root (the rootfs's tree, not the spec's other mounts):

| path | agent | seed's clone | init (part 19) | delete | lifecycle |
|---|---|---|---|---|---|
| pruned seed (6.19 and later, today) | clone 1, propagation 1 | copy of 2 mounts, 1 | the rootfs's attach, 1 | 1 | 5 |
| `CLONE_EMPTY_MNTNS` (7.1) | clone 1, propagation 1 | the root's clone, 1 | the rootfs's attach, 1 | 1 | 5 |
| `OPEN_TREE_NAMESPACE` from the rootfs's path (7.0) | 1 | 0 (no `CLONE_NEWNS`) | the propagation, 1 (§1) | 1 | 3 |
| `OPEN_TREE_NAMESPACE` from the agent's private clone | clone 1, propagation 1, namespace 1, the clone's dissolve 1 + grace period | 0 | 0 | 1 | 5 |
| `FSMOUNT_NAMESPACE`, a layered root (7.1) | 1 | 0 | 0 | 1 | 2 |

So:
- `CLONE_EMPTY_MNTNS` takes as many holds as the pruned seed: it copies one mount, not two (about 10-15 us against
  the host's 25-4,000 us for a full copy, `goal5-root.md` f76), and needs no pruned seed.
- `OPEN_TREE_NAMESPACE` from the rootfs's own path takes two holds fewer a lifecycle, and moves the init's one from the
  attach to the propagation change: part 19 keeps one hold for the root.
- Made from a clone the agent made private first, it takes the init's hold off part 19 but adds a dissolve and a grace
  period to the agent's part: worse for the create, as §13 of `goal5-mounts.md` found for unused detached mounts.
- A layered root on 7.1, through `FSMOUNT_NAMESPACE`, takes one hold a create, none in part 19.

## 5. Limits and failures

- **Namespaces per user.** Every mount namespace, these included, is charged to `UCOUNT_MNT_NAMESPACES` of the owning
  user namespace (`inc_mnt_namespaces`, `:4173-4176`); past `user.max_mnt_namespaces` the call fails `ENOSPC`
  (`alloc_mnt_ns`, `:4197-4199`). The agent holds an `OPEN_TREE_NAMESPACE` namespace only from its making until the
  seed answers, so its count is bounded by the creates in flight.
- **Mounts per namespace.** `sysctl fs.mount-max` (default 100,000, `:42`) bounds attaches (`count_mounts`,
  `:2466-2487`, `ENOSPC`); `create_new_namespace` attaches without it (`:3163`), so a rootfs tree is not refused by it.
- **What each refuses, and with what.** `OPEN_TREE_NAMESPACE`: `EINVAL` below 7.0 or with a bad flag set, an
  unbindable source or one another namespace cloned (§1); `EPERM` without `CAP_SYS_ADMIN` in the caller's user
  namespace; `ENOTDIR` (7.1) for a file; `ENOENT` for a source being unmounted; `ENOSPC`, `ENOMEM`, `EMFILE`.
  `CLONE_EMPTY_MNTNS`: `EINVAL` below 7.1 or with `CLONE_FS`; `EPERM` without `CAP_SYS_ADMIN`; `ENOSPC`, `ENOMEM`.
  `setns`: `EPERM` without the capabilities, `EINVAL` for a shared `fs_struct`.
- **No partial state.** Each fails before it publishes a namespace: `create_new_namespace` unwinds its root under the
  lock (`:3123-3125`, `:3151-3154`), `copy_mnt_ns` frees the namespace (`:4265-4268`).

## 6. Detecting them at runtime, without making anything

- `OPEN_TREE_NAMESPACE`: `open_tree(-1, "", AT_EMPTY_PATH | OPEN_TREE_NAMESPACE | OPEN_TREE_CLOEXEC)`. Below 7.0 the
  flag check refuses `EINVAL` (v6.19 `:3073-3076`). At 7.0 and later the capability is checked (`EPERM`), then the
  lookup of an empty path at descriptor -1 fails `EBADF` (`:3226-3229`), before any namespace (`:3231`). So `EBADF`
  means the call is there and this process may make one; it takes no lock and makes nothing.
- `CLONE_EMPTY_MNTNS`: `clone3` checks flags after copying its arguments and before making anything
  (`kernel/fork.c:3017-3022`), and every refusal on that way is `EINVAL`, as is a clone the flag is unknown to. There is
  no refusal that tells them apart without a process made, so the seed learns it from its first create's clone: an
  `EINVAL` refusal of the clone with the flag, whose other flags the same seed's clones use otherwise, is the kernel
  without it. The flag's absence costs one refused `clone3` per seed's life.

## 7. Under load: what part 19 is made of, and what 7.x can take

`goal5-mounts.md` §10-§13 measured part 19 on 6.19 with Docker's default masked and read-only paths: 119 calls a
create, 22 target lookups in the box's fresh proc (53 us), 4 superblocks (44 us), 17 `move_mount` (43 us), 10
`open_tree` clones (36 us), 28 write holds; p50 0.21-0.25 ms, p99 1.08-2.31 ms at 6 and 12 busy loops (§11). It also
found that holds alone aren't the tail: binding the masks with `mount(2)` cut the holds from 28 to 18 and part 19 got
no faster (L1, §10), and a stock of the masked directories' tmpfs took 3 superblocks off part 19 but moved them onto
the same CPUs during the create (§13). The tail waits on the CPU: a holder preempted, the init descheduled behind a
busy loop each time it blocks.

What the calls above take from part 19 is the root's one hold at most (§4): 1 of 28. They don't reach the bar by
themselves. What they make possible is a namespace built whole before the init runs, joined with no lock:
- `setns` into a namespace takes no lock (§1). So everything that can be in the namespace before the init's clone can
  leave part 19.
- What has to wait for the clone is what depends on namespaces born in it. Proc depends on the pid namespace, sysfs on
  the network namespace, mqueue on the ipc namespace. The masks and read-only paths depend on proc and sysfs.
- A pid namespace can be made before the process that will be its init: `unshare(CLONE_NEWPID)` sets only the caller's
  children's namespace (`copy_pid_ns`, v7.1 `kernel/pid_namespace.c:175-183`), the seed already joins a pid namespace
  for its children with `setns(CLONE_NEWPID)` (`seed.rs`), which takes a descendant of its own
  (`pidns_install`, `:392-416`), and the first process cloned into an empty one becomes its pid 1. And proc can be
  mounted for a pid namespace other than the caller's: `pidns=` (v7.1 `fs/proc/root.c:48`, `:115-160`, with
  `pidns_install`'s permission model, `:151-159`).
- **But only from Linux 7.1 can the namespace be held before its init.** To mount proc for it or hand it to the seed,
  the maker needs it as a file: `/proc/thread-self/ns/pid_for_children`. Through v7.0 `pidns_for_children_get`
  returns nothing for a namespace with no process yet, `if (!ns->child_reaper)` (v6.19 `kernel/pid_namespace.c:372-379`,
  v7.0 `:374`), so the open fails `ENOENT`. v7.1 dropped the check (`:361-373`). Measured: on fedora's 6.19.10 the
  maker's open failed `ENOENT` (unit hex-g6-rootunit-20261104T091410Z). In the 7.2.15 guest the node made procs
  ahead and its creates took them (hex-g6-lab-vm7-20261104T091252Z, where a separate seed bug then failed them).
- The network namespace is already made ahead (decision 4), and the ipc and uts namespaces (goal 5 extended step 4).

So on 7.x a create can be handed a mount namespace built whole, ahead: the rootfs, the spec's binds, `/dev` from the
pool (`goal5-mounts.md` §6), proc for a pid namespace made ahead with the masks and read-only paths on it, sysfs for the
pooled network namespace with its masks, and the rest. The init joins it, and its pid namespace, with `setns`. What
stays in part 19 is what only the init can do in its own context. Whether that is cheap enough to reach the bar, and
whether the work moved ahead costs the whole create more than it saves, is for the measurement (decision 6's update
says what would falsify it).

## 8. Masks without a superblock each: what crun and youki do

- **crun 1.30.1** masks every directory with one empty directory, bound: it opens a shared empty directory under its
  state root once per container (`get_shared_empty_directory_path`, `src/libcrun/linux.c:1194`; cached, `:1181-1210`)
  and attaches a bind of it per masked directory (`fs_move_mount_to`, `:1266`). A bind makes no superblock.
- **youki v0.7.0** mounts a fresh read-only tmpfs per masked directory
  (`crates/libcontainer/src/process/init/process.rs:588-596`), binds `/dev/null` per masked file (`:603-608`), and
  binds and remounts each read-only path (`:522-551`). It has no `OPEN_TREE_NAMESPACE` path.
- **Hex.** The pruned init made a read-only tmpfs per masked directory, as youki does: 3 superblocks a create with
  Docker's paths on fedora, more with `/sys`. Each superblock takes `list_lrus_mutex` and `shrinker_mutex`, host-wide
  (step 6, `create_serial.md` runs 23-24). A proc made ahead (decision 6's update, part 2) now masks each directory
  with a clone of one empty read-only tmpfs the node keeps for its life (`init::ProcSources`). In the 7.2.15 guest,
  two procs made ahead held 4 filesystems for their masked directories before (hex-g6-rootunit-vm7-20261104T092752Z)
  and 1 after (hex-g6-rootunit-vm7-20261104T092845Z).
- **On every kernel, every init.** The node's one empty tmpfs now masks every masked directory on the pruned path
  and through the shim, too. A test-only scratch tree (1d44e83) failed first on fedora 6.19.10: four boxes' masked
  directories sat on 4 filesystems (hex-g6-lab-seam-20261104T142809Z). With the change, 12 masked directories of
  four boxes are one filesystem on 6.19 (hex-g6-lab-seam-20261104T143343Z), and 8 in the 7.2.15 guest
  (hex-g6-lab-vm7-20261104T143443Z). The trade is a clone's one hold of the mount lock in place of a superblock.
- **What reuse can't reach.** A mask sits on a dentry of one proc superblock, and a proc superblock shows one pid
  namespace, so no mask tree outlives its container. The sources do: the empty tmpfs and `/dev/null`, one per node.

## 9. Both parts in a 7.2 guest under load: indicative, NOT of record (2026-11-04)

A KVM guest on Fedora's kernel 7.2.15-200.fc44 (12 vCPUs, 6 GB, its QEMU on fedora CPUs 4-15 under the timed lock),
release binaries of 92c03ac, AC-e (8 in flight, 8,000 idle threads in the guest), owner bundles with reuse on,
Docker's paths, the `/dev` pool on. Arms: A copy and no procs made ahead (today's), B from its tree, C from its tree
with procs made ahead, D copy with procs made ahead. Interleaved, 3 rounds, 2,000 creates a run, at 0, 6 and 12
busy loops in the guest. Unit hex-g6-bdtimed-20261104T093255Z; records under
`~/.cache/hex-t7/g6/bd/hex-g6-bdtimed-20261104T093255Z/` on fedora. Fedora's load was 7-35, about 100 containers up.
Every run's counts showed its arm (36 of 36). Part 19 here is "root in place": from its start to the init's
`chdir("/")`, since a namespace made from its tree makes no `chroot` (539781f).

| L | arm | part 19 p50, ms (3 rounds) | part 19 p99 | create p99 |
|---|---|---|---|---|
| 0 | A | .173 .212 .191 | .718 1.304 1.183 | 2.134 3.112 2.688 |
| 0 | C | .057 .064 .056 | .585 .689 .791 | 3.441 3.919 2.321 |
| 6 | A | .207 .195 .195 | 2.306 2.041 2.699 | 5.925 6.121 7.474 |
| 6 | C | .060 .063 .058 | 1.347 4.153 1.041 | 6.146 10.468 4.312 |
| 12 | A | .187 .186 .222 | 1.201 2.043 1.342 | 6.999 9.292 8.822 |
| 12 | C | .063 .056 .081 | .703 .966 2.989 | 10.015 5.192 14.142 |

B sits within A's ranges; D within C's (its 12-loop p99 .497 .483 .560, the only cell near the bar, below its own
6-loop figures). Read:
- The procs made ahead take part 19's p50 down about 3x (0.17-0.22 to 0.056-0.081 ms) in all 18 runs of C and D.
- Part 1 alone changes nothing measurable, as §4's hold count predicts.
- Part 19's p99 under 6 and 12 loops stays over 0.5 ms with both parts (C: 1.04-4.15 and 0.70-2.99), and the whole
  create's p99 doesn't fall. In a guest whose vCPUs are themselves preempted by a host at load 7-35, that tail can't
  be told from the VM's; this falsifies nothing and confirms nothing about the bar. The reading of record needs a 7.x
  bench host.

## 10. Part 19's tail in a 7.2 guest, by its mount-lock waits: indicative, NOT of record (2026-11-05)

The mounts trace of part 19 (root in place) in the same guest as §9, with the mount lock's holds and the slowpaths of
its waits probed (kprobes on `down_write`/`down_read` filtered to `namespace_sem`, and on
`rwsem_down_write_slowpath`/`rwsem_down_read_slowpath`). Release binaries of 4ac134b, harness 7c8c150; 2,000
creates at 8 in flight, 8,000 idle guest threads, owner bundles with reuse on, Docker's paths, the `/dev` pool on.
Cells: C (from its tree, procs made ahead) at 0 and 12 loops, A (copy, no procs made ahead) at 12. Unit
hex-g6-mttimed2-20261105T084016Z, timed 09:00:44-09:01:36Z under the lock, fedora load 27-43; records under
`~/.cache/hex-t7/g6/mt/hex-g6-mttimed2-20261105T084016Z/` on fedora. An earlier run of the same cells
(hex-g6-mttimed-20261105T031100Z) found the slowest creates asleep in `mount_setattr` and `move_mount`, but its
probes never attached: the guest has no `awk`, which looked up `namespace_sem`'s address (fixed at 7c8c150).

| cell | span p50, us | p99 | write holds per create: all / the slowest 1% | blocked on the lock per create: all / the slowest 1%, us |
|---|---|---|---|---|
| C, 0 loops | 62 | 506 | 5.8 / 12.2 | 6.7 / 327 |
| C, 12 loops | 73 | 3,046 | 5.1 / 8.6 | 31.9 / 2,346 |
| A, 12 loops | 206 | 1,921 | 29.0 / 29.0 | 7.7 / 530 |

Read:
- The slowest 1% of creates spend most of their span asleep waiting for `namespace_sem` for write. With procs made
  ahead at 12 loops, that's 2,346 of a mean 4,708 us. Each wait is in `mount_setattr` or `move_mount`, and each sleeper
  is woken by the holder that releases the lock. The holders named are Hex's own processes: the node agent (`hex`), the
  `/dev` pool (`hex-devfs`) and the bench driver (`seam_oci`). The 1,169 us preempted between calls is the guest's 12
  loops, which §9 can't tell apart from the VM's own preemption.
- The slow creates also take more holds than the median: 8.6-12.2 against 5.1-5.8. The extra holds are `open_tree`
  (1.8-3.6 a create), i.e. the creates whose proc or namespace was made on the path because a pool ran dry.
- The copy keeps its 29 holds a create, and its tail waits behind the same holders.
- So, with goal 6's two parts in place, what remains of part 19's tail on 7.x is contention on the one mount lock, and
  much of it comes from Hex's own make-ahead work running beside creates in flight. A lever that removes holds rather
  than trading them, to try next (indicative first):
  - keep the pools from running dry under 8 in flight, so no create makes on the path;
  - batch the pools' refills, so each refill takes the lock once rather than once per proc or `/dev`;
  - fewer `mount_setattr` holds in the init (2 a create).
  `open_tree_attr` doesn't merge a clone and its attributes into one hold (`goal5-mounts.md`, step 3's notes).
- A guest's tail isn't the host's (§9). A reading of record waits on a 7.x bench host.
