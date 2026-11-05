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
| C, 0 loops | 62 | 506 | 5.8 / 12.2 | 6.4 / 301 |
| C, 12 loops | 73 | 3,046 | 5.1 / 8.6 | 31.9 / 1,959 |
| A, 12 loops | 206 | 1,921 | 29.0 / 29.0 | 7.7 / 530 |

Read:
- The slowest 1% of creates spend most of their span asleep waiting for `namespace_sem` for write. With procs made
  ahead at 12 loops, that's 1,959 of a mean 4,708 us (2,347 blocked in all). Each wait is in `mount_setattr` or `move_mount`, and each sleeper
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
  - a prepared proc's masked files: each is a clone of the host's `/dev/null` made private, then attached
    (`init::prepare_proc`), 3 holds a file. A clone of a mount that's already private joins no peer group, so a
    node that keeps one private detached clone of `/dev/null` (as it keeps the empty tmpfs, where the kernel
    clones a detached mount) makes each masked file in 2 holds. The init's own masks already take 2, from the
    container's `/dev/null`;
  - fewer `mount_setattr` holds in the init (2 a create).
  Batching the refills removes nothing: every attach to a detached tree is a hold of its own, however many are
  made in a row.
  `open_tree_attr` doesn't merge a clone and its attributes into one hold (`goal5-mounts.md`, step 3's notes).
- A guest's tail isn't the host's (§9). A reading of record waits on a 7.x bench host.

The pools' own counts in both guest runs (each run's `phases.throughput.dev_mounts` and `.proc_mounts`; 2,000 creates
a cell; the copy cell makes no proc ahead):

| run | cell | proc pool dry | `/dev` pool dry |
|---|---|---|---|
| hex-g6-mttimed-20261105T031100Z | C, 0 loops | 198 | 7 |
| | C, 12 loops | 18 | 5 |
| | A, 12 loops | (disabled) | 0 |
| hex-g6-mttimed2-20261105T084016Z | C, 0 loops | 64 | 0 |
| | C, 12 loops | 5 | 1 |
| | A, 12 loops | (disabled) | 1 |

- At 0 loops the proc pool runs dry for 3-10% of creates, enough to fill the slowest 1% (20 creates). This matches
  their extra `open_tree` holds.
- At 12 loops, the cells the bar is about, it runs dry 5-18 times, fewer than or about the 20 in the slowest 1%. So
  running dry doesn't explain the tail under load. Most of the tail's lock wait there is behind other creates' inits
  (their command is still `hex` before `execve`), then the pools' maker (`hex-devfs`), then the agent (`seam_oci`,
  the bench's node).

### The prepared procs' private `/dev/null`, A/B in one guest run: indicative, NOT of record (2026-11-05)

A is a4be9dc. B is 6faee83, which masks each file of a prepared proc with a clone of the node's one private detached
`/dev/null`, 2 holds where A takes 3. The harness is a4be9dc's, which counts the lock's write holds outside the
inits' spans by holder. Both arms ran in the same Linux 7.2.15 guest as above, with the tree and procs made ahead,
at 0, 6 and 12 loops. The two rounds were interleaved, A then B in the first and B then A in the second. Unit
hex-g6-abtimed-20261105T095058Z, timed 09:51:19-09:58:09Z under the lock, fedora load 33-63; binaries built by
hex-g6-abbuild-20261105T093048Z, whose `oci_box` passed 3/3 as root in the 7.2 guest at B.

| L | arm | part 19 p50, us (r1, r2) | p99, us | init's holds | maker's holds a create | proc pool dry |
|---|---|---|---|---|---|---|
| 0 | A | 71, 82 | 622, 1,261 | 5.8, 5.5 | 28.1, 28.6 | 68, 37 |
| 0 | B | 80, 87 | 1,119, 1,952 | 5.7, 5.5 | 23.4, 23.6 | 53, 41 |
| 6 | A | 81, 76 | 1,492, 1,355 | 5.6, 5.7 | 28.4, 28.2 | 51, 59 |
| 6 | B | 83, 75 | 1,661, 1,813 | 5.5, 5.6 | 23.6, 23.5 | 37, 50 |
| 12 | A | 80, 78 | 3,078, 1,010 | 5.4, 5.1 | 28.6, 29.0 | 32, 8 |
| 12 | B | 86, 79 | 3,376, 2,584 | 5.1, 5.1 | 24.0, 24.0 | 8, 8 |

The node (`seam_oci`) held the lock 2.0 times a create in every run.

Read:
- B does the work it was built to do. The pools' maker takes 4.6-5.0 fewer write holds a create, 16-18%, which is
  one per masked file under `/proc` with Docker's paths. The init's own holds and the dry counts don't change.
- It doesn't make part 19 faster. Its p99 is higher than A's in all 6 pairs (by 0.17-1.57 ms), and its p50 in 5 of
  6. In a guest this noisy, that's no proof B is slower, but it does falsify B as a lever on the tail. The cause isn't
  known.
- Whatever the arm, the maker holds the mount lock about 5 times as often as an init does: 23-29 holds a create
  against 5-6. Nearly all of them are the masks and read-only paths it puts on each proc.
- **Falsified and reverted** (dc6ec80, on the lead's word, 2026-11-05): fewer holds with a higher p99 in 6 of 6
  pairs is no lever. 6faee83 stays reachable in the branch's history, its A/B above.

## 11. Part 19's tail on fedora's host (6.19), under the bar's conditions: what it is made of (2026-11-05)

One run, the lead's 10:47Z ask. Unit hex-g6-holdtime-20261105T104949Z, timed 11:59:05-11:59:19Z under the lock, fedora
load 34-38 with kiln CI beside it. Release binaries and harness of 7cc2238, which times every write hold of the mount
lock from its grant to its `up_write`, by holder. Conditions: the copy path (6.19 makes no namespace from a tree and no
proc ahead), 2,000 creates at 8 in flight, 8,000 idle threads (12,030 host threads), 12 busy loops on CPUs 4-15 at
nice 0, owner bundles with reuse on, Docker's paths, the `/dev` pool on. Record:
`~/.cache/hex-bench/out/create-breakdown-20261105T115907Z/` on fedora. Part 19 (net join to chroot): p50 278 us,
p99 709, max 5,462.

The trace was read in two passes. The reader (`mounts.c8.md`) gives the per-call table and the holds by holder. The
splits below came from passes over `mounts.c8.txt`, the same events.
- **The holds are short.** 72,071 write holds, 1.6 us on average. An init's average 1.3 us, with 0.1 us preempted
  and 0.0 us blocked inside each.
  - 51 holds lasted 50 us or more.
  - 9 of them are the create seeds' `pivot_root` at start, 1.2 ms each and all on the CPU: `pivot_root` calls
    `chroot_fs_refs` (`fs/fs_struct.c:54-80`), which walks every thread on the host, while the lock is held
    (v6.19 `fs/namespace.c:4554` takes it, `:4606` the walk).
  - The other 40, 6.4 ms in all, came after start. 4.6 ms of that was the holder preempted mid-hold, in `open_tree`
    and `move_mount`.
- **The startup holds only explain the first creates.** The first 5 creates of the run waited behind the seeds'
  `pivot_root` holds (2.5-5.5 ms). From 30 ms on (1,956 creates) the p99 is 694 us.
- **The steady tail is mostly preemption.** The slowest 1% of the steady creates average 1,102 us:
  - preempted, 538 us (49%);
  - waiting for the mount lock, 119 us (11%): 26 spinning, 93 asleep or runnable after wake;
  - the init's own work on its CPU, the remaining ~445 us.
- **Even unpreempted creates sit at the bar.** 12.4% of steady creates are preempted during part 19 at all, 3.2% for
  over 200 us, and 0.5% for over 400 us. The 1,714 creates never preempted have a p99 of 498 us.
- **Where part 19's CPU time goes**, per create: 22 `openat2` 68.7 us, 17 `move_mount` 58.7, 13 `open_tree` 53.4, proc's
  `fsconfig` 22.7, 33 `close` 10.8, the init's own code 80.6 with 18 page faults. That's 314 us in all.

Read:
- On 6.19 the mount lock is not what part 19's loaded tail waits on. A lever on hold time or hold count can reach at
  most the 11%.
- The tail is the init's own length: its unpreempted p99 is already 0.5 ms, and its chance of being preempted by a
  busy loop at equal weight grows with that length. To meet the bar here, part 19's CPU time has to fall well below
  ~300 us, roughly halving.
- The 7.2 guest's numbers are a third of that (procs made ahead and the tree, §9-§10: p50 56-80 us), so goal 6's 7.x
  path is the lever that size.
- Separately, a seed starting on a host with many threads holds the global mount lock ~1.2 ms in its `pivot_root`.
  That stalls every mount on the host, co-tenants included.

## 12. The seed without a `pivot_root` per start: what isolates it on 6.19 (2026-11-05)

§11 found each create seed's `prune` (`crates/hex-runtime/src/linux/oci/seed.rs:142-170`) holding the global mount lock
~1.2 ms on a host with ~12,000 threads. `pivot_root` calls `chroot_fs_refs` (`fs/fs_struct.c:54-80`), which walks
every thread on the host, before it gives the lock back (v6.19 `fs/namespace.c:4554`, `:4606`). The lead asked whether
a chroot into the pruned tmpfs, after detaching the old root's mounts, isolates the seed as `pivot_root` does. From
v6.19's source, it doesn't:
- **The root mount can't be detached.** The submounts of the old root can be detached, but the namespace's own
  root, the mount with no parent, is refused even with `MNT_DETACH` (`do_umount`, `fs/namespace.c:1940-1941`:
  `if (!mnt_has_parent(mnt)) goto out;` with `-EINVAL`). So after a chroot the seed's namespace still has the host's
  root filesystem as its root mount, reachable from any task whose root is moved back above the chroot.
- **The inits can get back to it.** Every init copies the seed's namespace (`clone3` with `CLONE_NEWNS`). On 6.19's
  copy path an init then `chroot`s into the container's root, and Docker's default capabilities include
  `CAP_SYS_CHROOT`. A chroot doesn't confine a holder of `CAP_SYS_CHROOT`: chroot(2) says it "is not intended to be
  used for any kind of security purpose" and that it can be escaped. After `pivot_root` the escape lands on the
  pruned, empty, read-only tmpfs. After a chroot-only prune it would land on the host's root filesystem.
- **Mounting the tmpfs beneath the root is refused on 6.19.** `MOVE_MOUNT_BENEATH` (Linux 6.5) would let the tmpfs
  replace the root with no `pivot_root`, but `can_move_mount_beneath` refuses a target that is the caller's root or a
  child of the namespace's root (`fs/namespace.c:3402-3426`: "Mounting beneath the rootfs only makes sense when the
  semantics of pivot_root(".", ".") are used").

What does isolate it without a `pivot_root` per seed: one `pivot_root` per node. Every later seed joins that pruned
namespace with `setns`.
- `setns(CLONE_NEWNS)` (`mntns_install`, `fs/namespace.c:6218-6258`) sets only the caller's root and working directory
  to the namespace's root. It takes no mount lock and walks no other task.
- It asks `CAP_SYS_ADMIN` over the namespace's owner and `CAP_SYS_CHROOT`, refuses an anonymous namespace, and needs
  the caller's `fs_struct` unshared (`fs->users == 1`, as the single-threaded seed's is).
- The namespace it joins is the one `prune` made: the empty read-only tmpfs as its root, the old root detached. So
  each seed, and each init that copies it, sees what it sees today.
- The node keeps that namespace by a descriptor for its life. At most one `pivot_root` happens per node start, before
  any create, in place of one per seed start.

## 13. Sizing the 6.19 levers on part 19's CPU time before building them (2026-11-05)

- **(1) Targets resolved beneath one opened `/proc`.** `bench/competitive/probes/openat2_micro.c` times `openat2`
  (`O_PATH`) of Docker's 14 `/proc` targets two ways: in the root (`RESOLVE_IN_ROOT` from `/`), and beneath one opened
  `/proc` (`RESOLVE_BENEATH`, that one open counted). It ran on fedora (6.19), CPU 8, under the timed lock, load 22,
  as unit hex-g6-micro-20261105T121049Z, 3 passes of 20,000 rounds each.
  - In the root: 905, 806 and 663 ns a target.
  - Beneath `/proc`: 638, 888 and 596 ns.
  - The difference is under 0.3 us a target and changes sign between passes. With ~15 such targets a create, that's
    at most ~4 us of part 19's 314, inside the noise. Not worth building.
- **(3) Fewer page faults in the init.** §11's trace has 36,333 user faults inside part 19, 18 a create:
  - 56% are instruction fetches of pages not present (`error_code` 0x14). `fork` doesn't copy the page-table
    entries of file-backed code, so the init faults in each code page it runs.
  - 34% are copy-on-write writes (0x7), and 10% reads (0x4).
  - At ~0.5-1 us a minor fault, all 18 come to ≤ ~18 us a create. The code faults go only if the init runs less code
    or doesn't fork. The writes go with fewer pages written after the clone.

Built at 3c8b121 (tests fixed at 1e8908d and 43eb4ab), on the lead's word. The create pool prunes once as it opens, in
a thread of its own (`createpool.rs`, `prune_once`, `seed::prune_namespace`), and holds the namespace by a
descriptor. Each create seed joins it as its socket's first record (`seed::send_namespace`, `join_pruned`). The
node's descriptor bound counts it (`netns.rs`, `descriptor_bounds`).
- Failing first, the tests alone on scratch trees, on fedora (6.19, as root):
  - `create_seeds_share_the_nodes_one_pruned_namespace` at b3d5c3c: 5 create seeds in 5 mount namespaces
    (hex-g6-seedns-20261105T134306Z).
  - `the_seeds_pruned_namespace_is_counted_in_what_a_node_may_hold` at eae6b46: the bound without it
    (hex-g6-seedns-20261105T134054Z).
- Passing at 43eb4ab (hex-g6-seedns-20261105T134306Z): 7 create seeds in one pruned namespace, which lists 1 mount
  (the read-only tmpfs root; the namespace's root beneath it isn't listed, as `oci_spec_box`'s AC-f says), the same
  after six creates at once as after the first. `oci_box` 3/3 and `oci_spec_box` pass, and all 7 gates exit 0.

### Lever (4)'s A/B of record on fedora's host (6.19), 2026-11-05

A is 68be241 (a `pivot_root` per seed start), B is 92443a8 (one per node, every seed joining by `setns`). The harness
is 92443a8's for both. Unit hex-g6-ab4-20261105T134612Z, timed 13:55:57-13:58:55Z under the lock, load 14-20.
- Each cell is a fresh node whose seeds start in it, running the mounts trace of part 19 under the bar's
  conditions: 2,000 creates at 8 in flight, 8,000 idle threads, 6 or 12 busy loops on CPUs 4-15 at nice 0, owner
  bundles with reuse on, Docker's paths, the `/dev` pool on.
- Three rounds were interleaved, each round's order the last's reversed. All 12 cells exited 0.
- Records are under `~/.cache/hex-bench/out/create-breakdown-20261105T1355*` to `T1358*`, named in the unit's
  `exits` file.

| L | round | arm | part 19 p50 / p99, us | unpreempted p99, us | whole create p99, ms | first 20 creates' max, ms | longest hold |
|---|---|---|---|---|---|---|---|
| 6 | 1 | A | 251 / 838 | 568 | 2.229 | 10.448 | `pivot_root` 1,349 us (5 in the top 5) |
| 6 | 1 | B | 244 / 631 | 498 | 1.977 | 2.904 | the node's prune 1,088 us |
| 6 | 2 | A | 262 / 727 | 467 | 2.267 | 10.390 | `pivot_root` 1,222 us (5) |
| 6 | 2 | B | 435 / 1,379 | 1,367 | 3.196 | 2.081 | a co-tenant (`systemd-hostnamed`) 1,371 us |
| 6 | 3 | A | 238 / 659 | 513 | 2.246 | 2.345 | `pivot_root` 1,156 us (5) |
| 6 | 3 | B | 241 / 591 | 498 | 1.741 | 2.597 | the node's prune 1,071 us |
| 12 | 1 | A | 260 / 903 | 646 | 2.625 | 6.387 | `pivot_root` 1,429 us (4) |
| 12 | 1 | B | 253 / 776 | 551 | 2.429 | 2.811 | `seam_oci` 1,332 us |
| 12 | 2 | A | 239 / 643 | 530 | 2.311 | 9.804 | `pivot_root` 1,243 us (5) |
| 12 | 2 | B | 254 / 669 | 507 | 1.954 | 1.579 | the node's prune 1,085 us |
| 12 | 3 | A | 243 / 665 | 564 | 2.256 | 10.690 | `pivot_root` 1,441 us (5) |
| 12 | 3 | B | 244 / 770 | 486 | 1.737 | 1.897 | the node's prune 1,088 us |

Read:
- **Fewer long holds.** B's seeds make no `pivot_root`. The node's one prune, ~1.08 ms, is B's only `pivot_root`, against
  4-5 per cell in A's top five holds.
- **The first creates.** Their worst create falls from 2.3-10.7 ms (median ~10.4) to 1.6-2.9 ms (median ~2.1), lower
  in 5 of 6 pairs.
- **The whole create's p99** falls from a median of 2.26 ms to 1.97 ms, lower in 5 of 6 pairs. The pair it isn't is
  B's 6-loop round 2, where a co-tenant held the global mount lock 1.37 ms.
- **The unpreempted p99** falls from a median of 547 us to 503 us, lower in 5 of 6 pairs.
- **Part 19's p50 and p99 don't change** beyond the host's noise. p99 is lower in 3 of 6 pairs, at medians of 696 and
  723 us, and still above the 0.5 ms bar, as §11 predicts: the steady tail is preemption and the init's own length.

Co-tenants at bench start: the same 12 traces probe every process's write waits on the mount lock. In each cell's
first 50 ms (the seeds' start), no process outside Hex waited on it, in either arm. Co-tenants weren't mounting then,
so these runs can't show (4)'s effect on them either way. What they do show is the hold (4) removes: 4-5 holds of
~1.2-1.4 ms each in A against the node's one in B. Showing the stall a co-tenant would see needs one that mounts in a
loop during seed start, which this A/B didn't have.

## 14. Where the under-load bar stands on 6.19, in plain words (2026-11-05)

On fedora's Linux 6.19, Ada's under-load bar for part 19 (p99 at most 0.5 ms with 6 and 12 busy loops, 8 in flight,
8,000 idle threads, Docker's paths, no priority change) is approached, but not yet met.
- Goal 6's own levers on 6.19 can't reach it. §11 measured the tail as mostly the init being preempted, plus the
  init's own CPU time; the mount lock is about a tenth. §13 sized the levers on the init's CPU time at about 20% at
  best, about 580 us at p99. Lever (4) removes the seeds' startup holds and improves the whole create's p99, but
  doesn't move part 19's steady p99.
- **Parked procs come closest** (decision 8, "parked procs", cp/t7-g5-parkproc; the tail agent's design, built by it
  and reviewed here). A parked init carries a proc made for its own pid namespace, with its masks, on the parking
  thread, through proc's `pidns=` (Linux 6.18 and later). So a create that claims one with a matching key takes part
  19's proc mount and masks off its path. It's off by default.
- Its A/B of record (`bench/competitive/probes/parked_procs_ab.md` on cp/t7-g5-parkproc, "Of record at 2caebe2"): A off,
  B on, both with parked inits, 4 interleaved rounds at 6 and 12 loops, 2,000 creates at 8 in flight, on fedora's
  host. Its gates and root lab ran at that tree in one unit, hex-g5tail-ppfull-20261105T154329Z. The failing-first
  run is cp/t7-g5-parkproc-red dcde154, with only the proc's making filtered out (unit
  hex-g5tail-ppred-20261105T154153Z): oci_reuse 2b fails at its hit assertion, and oci_box's parked-proc test times
  out waiting for a parked init with its proc.
  - Part 19's p50: about 210 us → 35-38 us.
  - Part 19's p99: 1,018-2,132 us → 351-761 us. It's under 0.5 ms in 2 of the 8 B cells (475 and 351 us, both at 12
    loops) and over it in the other 6.
  - Part 19's unpreempted p99: 427-800 us → 71-438 us.
  - The init's mount-lock holds: 31 → about 5 a create. They move to the node's park (about 3 → 32-34 a box); they
    aren't removed.
  - The predicted key: 1,992 of 1,992 hits in 7 of 8 cells. The eighth had 8 misses, from the ipc/uts pair pool
    running dry.
- **The cost is on the delete.** The create's p50 falls 0.16-0.43 ms.
  - At 8 in flight (the A/B above), the delete's p50 rises 0.29-0.54 ms, and its p99 is higher in 7 of 8 pairs. At 6
    loops the lifecycle rate is 2.5-5.3% lower.
  - Serially (host.sh's lifecycle pass, cp/t7-g5-parkproc f15393c's record), the delete's in-process p50 rises
    0.03-0.12 ms, with every create a hit.
- **Where the proc is made, decided (the lead, 2026-11-05 17:00Z): on the parking thread**, as built at
  cp/t7-g5-parkproc f15393c. Both worker arms failed. A single worker couldn't keep pace (hits 409-663 of 1,992, part 19
  back at the off arm's numbers), and the workers were bound by the one shared mount lock all the procs' holds take.
- **It's opt-in:** `Node::with_parked_procs`, off by default. Whether it goes on by default is Ada's call.
- So with parked procs on, the under-load bar is approached on 6.19, but not yet met: part 19's p99 is 351-761 us
  against 0.5 ms, under it in 2 of 8 cells, with a p50 of ~37 us.

What can reach the bar is goal 6's 7.x path: the namespace made from a tree, with procs made ahead.
- In the Linux 7.2 guest, part 19's p50 is about a third of 6.19's (56-80 us against about 250, §9-§10). Both the
  unpreempted tail and the chance of being preempted shrink with it.
- A reading of record needs a bench host on Linux 7.1 or later. Whether fedora moves to 7.x is Ada's decision.

### Lever (4)'s short A/B with a co-tenant (the lead's 13:45Z ask), 2026-11-05

A is 34107da, B is 43eb4ab, the harness 19485e1's. Unit hex-g6-ab4b-20261105T141018Z, timed 14:11:24-14:13:34Z under
the lock, load 13-16. Conditions as above, at 6 and 12 loops, 2 rounds interleaved (AB, then BA); all 8 cells exited
0.
- The co-tenant is `bench/competitive/probes/mount_cotenant.c`. It's rootless, in a user and mount namespace of its
  own, on CPUs 16-31. It mounts and unmounts a tmpfs every ~200 us, from 1 s before each node starts, and times each
  pair on the trace's clock.
- "During start" runs from the trace's first event to the 20th create's part 19 end.
- Readings by `seedstart_extract.py`.

| L | round | arm | seed-start `pivot_root` holds | first 20 creates' part 19 p50 / max, us | co-tenant's worst pair during start, us | steady part 19 p50 / p99, us | whole create p99, ms |
|---|---|---|---|---|---|---|---|
| 6 | 1 | A | 8: 954-1,186 us | 322 / 6,858 | 4,431 | 234 / 563 | 2.102 |
| 6 | 1 | B | 1: 1,048 us (the node's prune) | 222 / 584 | 1,005 | 236 / 628 | 1.772 |
| 12 | 1 | A | 9: 924-1,160 us | 240 / 4,627 | 2,480 | 234 / 628 | 2.742 |
| 12 | 1 | B | 1: 1,050 us | 233 / 424 | 871 | 236 / 713 | 2.193 |
| 6 | 2 | B | 1: 1,159 us | 248 / 689 | 1,174 | 240 / 713 | 1.956 |
| 6 | 2 | A | 8: 1,164-1,272 us | 551 / 7,982 | 3,411 | 241 / 689 | 1.816 |
| 12 | 2 | B | 1: 1,121 us | 238 / 2,670 | 1,141 | 251 / 850 | 4.268 |
| 12 | 2 | A | 9: 927-1,365 us | 255 / 7,123 | 5,369 | 249 / 834 | 2.154 |

B's prune thread also took 4 short holds (0-43 us), its other mount calls.

Read:
- **At node start, lever (4) does what it was built for.** The seeds' 8-9 holds of ~0.9-1.4 ms become the node's one.
  - The first 20 creates' worst part 19 falls from 4.6-8.0 ms to 0.4-2.7 ms, lower in 4 of 4 pairs.
  - The co-tenant's worst mount during start falls from 2.5-5.4 ms to 0.9-1.2 ms, lower in 4 of 4 pairs.
- **The steady readings don't read equal.**
  - Part 19's p50 is level (234-251 us).
  - Its p99 is higher with B in all 4 pairs here, by 15-85 us, and in 7 of 10 pairs across this run and the one
    above. That's inside this host's run-to-run spread (A alone read 563-903 us across the two runs), but one-sided.
    Not explained; the cause isn't measured.
  - The whole create's p99 is mixed: B lower in 2 of 4 here, 5 of 6 above.
- **The co-tenant's worst mount after the start** is 1.6-109 ms in both arms, from causes the trace doesn't name (it
  follows only Hex's processes off their CPUs).

## 15. A parked init with its proc made ahead: goal 6's side of decision 8's proposal (2026-11-05, not built)

The design is the tail agent's: decision 8's "Proposal, not built: a parked init with its proc made ahead"
(`docs/design/runtime-architecture.md` on cp/t7-g5-tail, 12fd867 and 3355fd5). This agent reviewed it, owns the
prepared proc it uses, and owns part 19's measurement of it. Here is goal 6's side.
- **Why it reaches 6.19.** Goal 6's proc pool needs 7.1 only because it opens a pid namespace with no process in it
  yet (§7), so on 6.19 the pool disables itself at its first refusal. A parked init is pid 1 of the fresh pid
  namespace it was parked with. Proc's `pidns=` option mounts for any pid namespace given by descriptor. It's in Linux
  6.18 and later, so 6.19 has it, and v6.17 has no `pidns` (v6.19 `fs/proc/root.c:48`, `:115-160`). It asks only for
  `CAP_SYS_ADMIN` over the target namespace's user namespace and a target descended from the mounter's active pid
  namespace (`:153-159`), not an empty namespace. So the proc and its masks can be made at park, off the create's
  path, on 6.19 as on 7.x.
- **The prepared proc, split.** `init::prepare_proc` becomes two parts:
  - making the pid namespace: the 7.1 part, which the pool keeps;
  - `prepare_proc_in(pid_namespace, key, sources)`: `fsopen("proc")`, `pidns=`, the key's options and flags,
    `fsmount`, then the masks and read-only paths beneath the detached tree.
  The pool and the parked inits share the second, one code path (CLAUDE.md §8). The worker opens the parked init's
  namespace through the pidfd the node holds (`PIDFD_GET_PID_NAMESPACE`, Linux 6.11 and later), never by
  `/proc/<pid>`, so a reused pid can't hand it another process's.
- **The masks don't depend on the namespace's processes.** Docker's `/proc` paths are global entries, not per-pid.
  The symlink refusal (`openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_MAGICLINKS`) and the
  `/dev/null` and empty-tmpfs clones behave the same. The key must stay the proc op's whole option string, as
  `procmount::Key` has it, since `hidepid=` and `subset=` change what's visible.
- **Off the create's path.** The proc's ~25 mounts with Docker's paths, their holds of the mount lock and the proc
  superblock (with `list_lrus_mutex` and `shrinker_mutex`) are the refill worker's. A missed or unclaimed tree is
  dropped by the worker too. Dropping a detached tree unmounts it under the mount lock and waits an expedited RCU
  grace period (`namespace_unlock`, v6.19 `fs/namespace.c:1680-1720`), which must never be on a create's thread.
- **Part 19's measurement (goal 6's).** The mounts trace's harness on fedora's host (6.19), the setting on against
  off, both with parked inits, interleaved, under the bar's conditions: 8 in flight, 8,000 idle threads, 6 and 12
  busy loops, Docker's paths, no priority change. It reports for each:
  - part 19's p50, p99 and unpreempted p99;
  - the whole create's p99;
  - mount-lock holds a create, by holder (the init and the worker);
  - the predicted key's hits and misses.
  The 7.2 guest runs beside it, indicative only.
- **What would falsify it for part 19.** Part 19's p99 not falling with the setting on. Or the whole create's p99
  not falling. Or the worker's holds moving the tail onto the creates instead of off them, which the by-holder waits
  would show.
- Not built. It waits on the lead's go to the joint proposal.

### Settling the one-sided steady p99 (the lead's 14:15Z ask), 2026-11-05

The lead asked for a trace of the inits' namespace copy, but in both arms the copy (`copy_mnt_ns` under the mount lock)
runs in the seed's `clone3(CLONE_NEWNS)`, before the init exists, so it's outside part 19's span. What does differ
inside the span is the root's attach point:
- In B every init attaches its container's root on the one shared pruned tmpfs's root, the same dentry, from a copy
  of the same namespace. In A each seed had a tmpfs of its own.
- `lock_mount` takes the mountpoint inode's lock before the mount lock (v6.19 `fs/namespace.c:2723-2766`: `inode_lock`
  at :2746, `namespace_lock` at :2747). So B's attaches queue on one inode lock, where A's were spread across the
  seeds' own.
- A namespace copy shares the same superblock and dentries, so a per-seed `unshare(CLONE_NEWNS)` (the lead's arm C)
  wouldn't spread them.

So the trace (a646981) names an init's wait on any other rwsem apart from the mount lock's.
- The run: unit hex-g6-ab4c-20261105T141917Z, A = 34107da, B = 43eb4ab, harness a646981. 4 rounds alternating (AB, BA),
  6 and 12 loops, no co-tenant. Timed 14:20:27-14:27:24Z under the lock, load 13-26; 16 of 16 cells exited 0.
- Three cells show `pivot_root` holds beyond their arm's own, which is 8-9 for A and 5 for B (the prune thread's): A at
  6 loops round 4 (12), B at 6 loops rounds 1 and 3 (18 and 24). Those are other Hex processes on the host, and those
  cells hold the run's worst tails (part 19 p99 885-1,839 us). They're set aside.

The five pairs neither side of which was hit:

| L | round | part 19 p99 A / B, us | unpreempted p99 A / B, us | other-rwsem waits a create A / B | mount-lock waits a create A / B |
|---|---|---|---|---|---|
| 12 | 1 | 762 / 661 | 619 / 424 | 0.073 / 0.069 | 12.7 / 11.2 |
| 12 | 2 | 639 / 664 | 500 / 530 | 0.059 / 0.066 | 11.9 / 10.7 |
| 12 | 3 | 634 / 631 | 518 / 483 | 0.051 / 0.068 | 11.6 / 11.1 |
| 12 | 4 | 717 / 599 | 510 / 475 | 0.047 / 0.089 | 10.3 / 11.9 |
| 6 | 2 | 610 / 601 | 475 / 523 | 0.028 / 0.063 | 10.9 / 11.0 |

Read:
- **The one-sided p99 doesn't reproduce.** Part 19's p99 is lower with B in 4 of 5 pairs, with medians of 639 us (A)
  and 631 us (B). So the earlier +15-85 us was the host's noise.
- **The shared attach point shows, but too rarely to matter.** Waits on another rwsem are a little more frequent with
  B (0.063-0.089 a create against 0.028-0.073), which fits the shared inode. They're rare and carry no p99 cost here.
- **Lever (4) stands as built.** Arm C isn't needed.
