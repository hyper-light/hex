# The container's root without pivot_root's walk: what each way of making it costs, and what it leaves open

Source notes for goal 5's added step (B), the container's root set up without `pivot_root`'s walk over every thread on
the host (Ada's call, 2026-10-31; kiln plan `01a190c51f07757ab54b09f1aada633c`, research
`01a190c51f137042a9697949c8fe2c7a`, where each finding is graded from these sources). The baseline is run 4 of the
create breakdown (`bench/competitive/probes/create_breakdown.md`, create-breakdown-20261031T061841Z, fedora 6.19, about
3,870 threads on the host): the container's root is 0.894 ms of a 2.110 ms create p50 (42.4%), of it the init's
mounts 0.359 and `pivot_root` 0.535; the init's clone into its new namespaces, the mount namespace's copy inside it,
0.174 (p99 0.840); the old root's detach, the hostname and the process become 0.065.

Sources were read at fixed revisions, downloaded into `~/.cache/hex-t7/goal5-root/` on adacachy (2026-10-31):
Linux v6.19, v7.0 and v7.1 from `raw.githubusercontent.com/torvalds/linux/<tag>/`; runc v1.5.2
(`raw.githubusercontent.com/opencontainers/runc/v1.5.2/`); crun 1.30.1
(`raw.githubusercontent.com/containers/crun/1.30.1/`); man pages from man7.org. Each quote below is one line of its
source, copied exactly; `path:line` is at that revision.

## 1. What pivot_root costs at Linux v6.19, and why

### The walk over the host's threads

- `SYSCALL_DEFINE2(pivot_root, const char __user *, new_root,` (`fs/namespace.c:4526`) ends its work with
  `chroot_fs_refs(&root, &new);` (`:4606`).
- `chroot_fs_refs` (`fs/fs_struct.c:54`) takes `read_lock(&tasklist_lock);` (`:60`) and walks
  `for_each_process_thread(g, p) {` (`:61`). For each thread it takes `task_lock(p);` (`:62`) and, if the thread has
  an `fs_struct`, `write_seqlock(&fs->seq);` (`:66`), to replace its root and working directory when they are the old
  root (`replace_path`, `:46-52`, `:67-68`). Every thread on the host is visited and locked, whatever namespace it is
  in; threads of one process share one `fs_struct`, so its seqlock is written once per thread.
- So the walk's cost follows the host's thread count, not the container's (run 4: 0.535 ms p50 at about 3,870
  threads; runs 1 and 3: 0.765-0.873 ms).

### The global locks pivot_root holds

- **namespace_sem, for write, across the walk.** There is one, `static DECLARE_RWSEM(namespace_sem);`
  (`fs/namespace.c:89`). `pivot_root` takes it through `LOCK_MOUNT(old_mp, &old);` (`:4554`): the macro declares
  `struct pinned_mountpoint mp __cleanup(unlock_mount) = {};` (`:2800`) and calls `do_lock_mount`, which takes the
  mountpoint's inode lock and `namespace_lock();` (`:2746-2747`), that is `down_write(&namespace_sem);` (`:1725`).
  The lock is let go by `unlock_mount` when `old_mp` leaves scope (`__unlock_mount`, `:2784-2791`), which is on return,
  after `chroot_fs_refs` at `:4606`. So (inferred from the code) while one `pivot_root` walks the host's threads, every
  other mount, umount, `unshare(CLONE_NEWNS)`, clone with `CLONE_NEWNS` and `pivot_root` on the host waits for it.
- **mount_lock, for write, around the swap only.** `lock_mount_hash();` (`:4589`) is `write_seqlock(&mount_lock);`
  (`:196`), held while the new root and the old are detached and attached, and let go by `unlock_mount_hash();`
  (`:4603`) before the walk. `mount_lock` is one seqlock for the whole kernel,
  `__cacheline_aligned_in_smp DEFINE_SEQLOCK(mount_lock);` (`:133`).
- **tasklist_lock, for read, across the walk** (above). Readers don't exclude each other, but a writer (every fork,
  exit and `setpgid` on the host takes it for write) waits for the walk to finish, and the walk waits for writers.

### The copy of the mount table

- A new mount namespace, made by `unshare(CLONE_NEWNS)` or a clone with `CLONE_NEWNS`, is made by `copy_mnt_ns`
  (`fs/namespace.c:4093`). It takes namespace_sem for write, `guard(namespace_excl)();` (`:4117`; the guard is
  `DEFINE_LOCK_GUARD_0(namespace_excl, namespace_lock(), namespace_unlock())`, `:96`), copies every mount of the
  caller's namespace, `new = copy_tree(old, old->mnt.mnt_root, copy_flags);` (`:4122`), and walks both trees again to
  add each copy to the new namespace (`while (p) {`, `:4140`). Its cost follows the number of mounts in the namespace
  being copied: the seed's, which today is the node agent's (the host's, or on the bench the bench container's).
- When the copy crosses into a user namespace other than the copied namespace's owner, the copies are made slaves and
  locked: `copy_flags |= CL_SLAVE;` (`:4121`) and `lock_mnt_tree(new);` (`:4129`). A locked mount can't be unmounted
  on its own: `do_umount` refuses it, `if (mnt->mnt.mnt_flags & MNT_LOCKED)` then `goto out` with `-EINVAL`
  (`:1937-1938`).

### Detaching the old root after the pivot

- `umount2(".", MNT_DETACH)` of the old root takes `namespace_lock();` and `lock_mount_hash();` (`:1929-1930`) and
  calls `umount_tree(mnt, UMOUNT_PROPAGATE);` (`:1945`), which visits every mount under the old root,
  `for (p = mnt; p; p = next_mnt(p, mnt)) {` (`:1776`): every mount of the copied table. Each disconnected mount goes
  on the `unmounted` list, `hlist_add_head(&p->mnt_umount, &unmounted);` (`:1819`), and when that list isn't empty
  `namespace_unlock` waits for an expedited RCU grace period, `synchronize_rcu_expedited();` (`:1715`), before it
  drops the mounts. So the old root's detach also follows the copied table's size, and adds one expedited grace
  period (an interrupt to every CPU) per create.
- The absolute root of a namespace can't be unmounted: `if (!mnt_has_parent(mnt)) /* not the absolute root */`
  (`:1940`).

## 2. What runc and crun do

### Both pivot, as Hex does

- runc v1.5.2: `if err := unix.PivotRoot(".", "."); err != nil {` (`libcontainer/rootfs_linux.go:1164`), then makes
  the old root a recursive slave and detaches it, `if err := unmount(".", unix.MNT_DETACH); err != nil {` (`:1186`).
- crun 1.30.1: `ret = pivot_root (".", ".");` (`src/libcrun/linux.c:2567`), then `ret = umount2 (".", MNT_DETACH);`
  (`:2579`), repeated until `EINVAL` (`:2583-2590`).

### Their no-pivot mode: MS_MOVE and chroot

- runc: `NoPivotRoot will use MS_MOVE and a chroot to jail the process into the container's rootfs`
  (`libcontainer/configs/config.go:97`); `This is a common option when the container is running in ramdisk.` (`:98`).
  The flag's usage: `do not use pivot root to jail process inside rootfs.  This should be used whenever the rootfs is
  on top of a ramdisk` (`run.go:61`, `create.go:48`). Its man page: `: Do not use pivot root to jail process inside
  rootfs. This should not be used` / `except in exceptional circumstances, and may be unsafe from the security` /
  `standpoint.` (`man/runc-run.8.md:33-35`; `man/runc-create.8.md:28-30`).
- runc's `msMoveRoot` (`rootfs_linux.go:1197`) first unmounts, or covers with a tmpfs where it may not unmount, every
  full proc and sysfs mount outside the rootfs (`:1215-1251`), because `chroot(2) the *host* procfs mount is still
  reachable in the mount` / `namespace and the kernel permits procfs mounts inside --no-pivot` (`:1201-1202`); it
  calls this `but to avoid such a trivial security flaw we apply a best-effort` (`:1206`). Then
  `if err := mount(rootfs, "/", "", unix.MS_MOVE, ""); err != nil {` (`:1254`) and `chroot()` (`:1257`):
  `if err := unix.Chroot("."); err != nil {` (`:1261`).
- runc refuses private root propagation without a pivot: `return nil, errors.New("rootfsPropagation of [r]private is
  not safe without pivot_root")` (`libcontainer/specconv/spec_linux.go:445`). With the host's mounts left in the
  container's namespace, private propagation would keep a host unmount from reaching it (inferred: the mount stays
  pinned for the container's life).
- crun: `--no-pivot` is "Use `chroot(2)` instead of `pivot_root(2)` when creating the container." (`crun.1.md:149`,
  `:176`), with no security note. Its `move_root` (`src/libcrun/linux.c:3570`) unmounts or hides `/sys` and `/proc`
  only (`:3578`, `:3582`; `umount_or_hide` covers a locked one with an empty tmpfs, `:3556-3558`), then
  `ret = mount (rootfs, "/", "", MS_MOVE, "");` (`:3586`) and `ret = chroot (".");` (`:3590`).

### crun's OPEN_TREE_NAMESPACE path (Linux 7.0 and later)

- crun 1.30.1 opens every mount of the spec as a file descriptor first, "while the host file system is still
  reachable" (`linux.c:4035-4036`): binds by `get_bind_mount` (an `open_tree` clone, `:4098`), the rest by
  `open_mount_of_type` (`:4107`).
- Then, where it may, `tree_fd = syscall_open_tree (rootfs_fd, "",` with `OPEN_TREE_NAMESPACE | OPEN_TREE_CLOEXEC`
  (`:3664-3665`), and `ret = setns (tree_fd, CLONE_NEWNS);` (`:4162`): no `unshare`, no bind of the root, no
  `pivot_root`. An older kernel's `EINVAL`, `ENOSYS` or `EPERM` falls back to the classic path (`:3669-3670`).
- It may not with create-time hooks or a user namespace: `if (has_hooks || has_userns)` / `return false;`
  (`:3959-3960`); nor when crun itself runs in a user namespace (`:3970-3973`); nor when any mount couldn't be opened
  ahead (`:3977-3986`).

## 3. What a root made without pivot_root leaves open, and what closes it (Linux v6.19)

### The kernel's rules

- **chroot is O(1).** `SYSCALL_DEFINE1(chroot, const char __user *, filename)` (`fs/open.c:595`) checks
  `if (!ns_capable(current_user_ns(), CAP_SYS_CHROOT))` (`:610`) and sets only the caller's root,
  `set_fs_root(current->fs, &path);` (`:616`). No other thread is visited.
- **A chrooted process can't make a user namespace.** `current_chrooted` (`fs/namespace.c:6084`) takes the namespace's
  topmost root mount, `root = topmost_overmount(current->nsproxy->mnt_ns->root);` (`:6096`), and calls the caller
  chrooted unless its root is that mount's root: `return fs_root.mnt != &root->mnt || !path_mounted(&fs_root);`
  (`:6098`). `create_user_ns` refuses a chrooted caller: `if (current_chrooted())` (`kernel/user_namespace.c:106`,
  `-EPERM` from `:105`). A root moved onto `/` with MS_MOVE and then `chroot(".")` is that topmost mount, so its
  processes may still make user namespaces (nested containers, sandboxes such as bubblewrap's); a plain chroot into a
  subdirectory may not.
- **`..` can't climb out of a root stacked on the namespace's root.** `follow_dotdot` (`fs/namei.c:2142`), at a mount's
  root (`:2148`), asks `choose_mountpoint` (`:2151`) for the mountpoint above, and stays put if there is none
  (`goto in_root;`, `:2153`). `choose_mountpoint_rcu` climbs while each mountpoint is its parent's root and stops only
  at one that isn't: `if (mountpoint != m->mnt.mnt_root) {` (`:1440`). A root moved onto `/`, or pivoted, sits on a
  stack of root mounts down to the namespace's root, so the classic chroot escape (mkdir x; chroot x; cd .. repeatedly;
  chroot .) stays inside it.
- **proc and sysfs are allowed only where one is already fully visible, hidden ones included.** In a mount namespace
  not owned by the initial user namespace (`if (ns->user_ns == &init_user_ns)`, `:6167`), a new proc or sysfs mount is
  refused unless `mnt_already_visible` finds one (`return !mnt_already_visible(ns, sb, new_mnt_flags);`, `:6181`), and
  it looks at every mount of the namespace: `rbtree_postorder_for_each_entry_safe(mnt, n, &ns->mounts, mnt_node) {`
  (`:6109`), those hidden beneath the container's root included. This is runc's "trivial security flaw" (§2).
- **pivot_root locks the new root in a less privileged namespace; MS_MOVE doesn't.** `if (root_mnt->mnt.mnt_flags &
  MNT_LOCKED) {` (`:4591`) `new_mnt->mnt.mnt_flags |= MNT_LOCKED;` (`:4592`): in a mount namespace owned by the
  container's own user namespace, the copied mounts are locked (§1), and pivot_root passes the old root's lock to the
  new root, so the container can't unmount its root to see what's beneath. A root moved by MS_MOVE is a mount the
  container made itself, unlocked.
- **setns into a mount namespace** (`mntns_install`, `:6218`) wants `if (!ns_capable(mnt_ns->user_ns, CAP_SYS_ADMIN) ||`
  (`:6227`) and `!ns_capable(user_ns, CAP_SYS_CHROOT) ||` (`:6228`) and CAP_SYS_ADMIN in the caller's own user
  namespace; refuses an anonymous namespace, `if (is_anon_ns(mnt_ns))` (`:6232`); refuses a caller that shares its
  `fs_struct`, `if (fs->users != 1)` (`:6235`), which every multithreaded process does; and sets only the caller's
  working directory and root, `set_fs_pwd(fs, &root);` and `set_fs_root(fs, &root);` (`:6255-6256`), at the
  namespace's topmost root mount (`vfs_path_lookup` of `/` with `LOOKUP_DOWN`, `:6243-6244`). No walk.
- **Detached trees cross namespaces; paths don't.** `do_move_mount` attaches the root of an anonymous mount namespace
  into the caller's whoever made it: `if (!anon_ns_root(old))` (`:3536`) and the target in the caller's namespace,
  `if (!may_use_mount(mp.parent))` (`:3549`). Cloning a path needs it in the caller's namespace or in an anonymous one
  cloned from it: `static inline bool may_copy_tree(const struct path *path)` (`:2928`), `if (check_mnt(mnt))`
  (`:2933`), `return !seq || (seq == current->nsproxy->mnt_ns->ns.ns_id);` (`check_anonymous_mnt`, `:965`). So a
  container whose namespace holds nothing of the agent's gets its rootfs and bind sources as detached trees the agent
  clones (`open_tree(OPEN_TREE_CLONE)`) and passes by `SCM_RIGHTS`, as crun's OPEN_TREE_NAMESPACE path opens every
  mount ahead (§2).
- **MOVE_MOUNT_BENEATH can't replace the root at 6.19.** `can_move_mount_beneath` refuses
  `if (&mnt_to->mnt == current->fs->root.mnt)` (`:3419`) and `if (parent_mnt_to == current->nsproxy->mnt_ns->root)`
  (`:3421`).

### Escape attempts (the probe, §4, `escape` run)

Each mode makes the root, then a forked process of the container tries, as root in a privileged container (so with
CAP_SYS_CHROOT and CAP_SYS_ADMIN): E0 make a user namespace; E1 the chroot escape; E3 a fresh proc in a nested user,
mount and pid namespace while the container's `/proc/kcore` is masked; E2 unmount its own root, then `setns` into its
own mount namespace, which roots it at the namespace's topmost root mount. A marker file, root's alone, sits in the
agent's `/run/pb`, outside the rootfs. The `_userns` modes put the container in a user namespace of its own and run E2.

| mode | E0 user namespace | E1 chroot escape | E3 nested proc | E2 umount / then setns |
|---|---|---|---|---|
| classic (pivot_root) | allowed | stays in the rootfs | refused (EPERM) | an empty root, marker not reached |
| move (MS_MOVE + chroot in a copy) | allowed | stays in the rootfs | **ALLOWED** | **the agent's tree, marker REACHED** |
| pruned (seed, MS_MOVE + chroot) | allowed | stays in the rootfs | refused (EPERM) | the seed's empty tmpfs (`r`), not reached |
| pruned_pivot | allowed | stays in the rootfs | refused (EPERM) | an empty root, not reached |
| chroot_only (no MS_MOVE) | **refused (EPERM)** | **marker REACHED** | (no user namespace) | marker REACHED |
| classic_userns | | | | umount refused (EINVAL): the new root is locked |
| move_userns | | | | **the agent's tree, marker REACHED** |
| pruned_userns | | | | the seed's empty tmpfs, not reached |

Printed lines, as the probe wrote them (`g5r-probe-escape`):
`escape move           E2 umount / then setns       marker REACHED; / holds: .profile afs bin boot dev etc home lib lib64 media mnt opt proc root run sbin `;
`escape move           E3 nested proc mount         ALLOWED: a fully visible proc is in the namespace`;
`escape pruned         E2 umount / then setns       marker not reached; / holds: r `;
`escape chroot_only    E1 chroot escape             marker REACHED; / holds: .profile afs bin boot dev etc home lib lib64 media mnt opt proc root run sbin `;
`escape classic_userns E2 umount / then setns       umount refused: Invalid argument`;
`escape move_userns    E2 umount / then setns       marker REACHED; / holds: .profile afs bin boot dev etc home lib lib64 media mnt opt proc root run sbin `;
`escape pruned_userns  E2 umount / then setns       marker not reached; / holds: r `.

So MS_MOVE in a copy of the agent's namespace (runc's and crun's no-pivot) is unsafe as Hex's default for three
reasons the kernel's rules predict and the probe shows: the agent's proc stays visible to `mnt_already_visible` (E3),
the agent's tree lies beneath for any process that may unmount the root (E2), and for a user-namespaced container
that is the container's own root user (move_userns). The pruned seed closes all three by having nothing beneath the
root but an empty read-only tmpfs and no proc or sysfs anywhere in the namespace.

## 4. The probe: what each way costs on fedora

### Method

- **Program.** `rootprobe.c` (in `~/.cache/hex-t7/goal5-root/probe` on adacachy and fedora, its own git repository;
  sha256 `5e7f22d7ed41ee81` for the escape and scaling runs, `28c0ad36f7b37a96` with a one-mode filter for the
  concurrency run; built on fedora with gcc 16.2.1, `-O2`). Each create is a forked child that times its phases with
  `CLOCK_MONOTONIC` and writes them to a pipe. The four modes are interleaved, the order rotating every create.
- **Where.** fedora (`kiln-a094a12d`), Linux 6.19.10-300.fc44, as root in the bench's privileged container
  (`registry.fedoraproject.org/fedora:44`, `--privileged --security-opt label=disable --cgroupns=private --network=none
  --tmpfs /run:rw,exec,nosuid,size=1g -v /usr:/usr:ro --cpuset-cpus=4-31`), as `create_breakdown.sh` starts it.
  2026-10-31, 07:27Z to 07:29:29Z; the 1-minute load 7.9 to 16.0 from other work (by Ada's rule a busy host is the
  default; the comparisons are within each run). About 2,860 threads on the host.
- **The agent's namespace.** The probe first makes a private copy of the container's namespace (14 mounts) and, for
  the mounts setting, adds 1,000 or 4,000 bind mounts under `/run/pb/m`. The bundle is the bench's: a rootfs directory
  on `/run` with `usr`, `proc`, `dev` and `tmp`; its mounts `/usr` bound read-only, `/proc`, a `/dev` tmpfs and six
  device nodes.
- **Modes.** `classic`: `unshare(CLONE_NEWNS)`, `/` made rprivate, the rootfs bound, the mounts, `pivot_root(".", ".")`,
  the old root detached (Hex today). `move`: the same to the mounts, then `mount(".", "/", MS_MOVE)` and `chroot(".")`
  (runc's and crun's no-pivot). `pruned`: a seed that at start unshared, made `/` rprivate, mounted a 16 KiB tmpfs with
  an empty `r`, pivoted into it once, detached the old root and remounted the tmpfs read-only; per create the agent
  clones the rootfs and `/usr` with `open_tree(OPEN_TREE_CLONE | AT_RECURSIVE)` and sends them by `SCM_RIGHTS`, and the
  seed's child unshares (copying the seed's two mounts), attaches them with `move_mount`, makes the mounts, then
  MS_MOVE onto `/` and `chroot(".")`. `pruned_pivot`: the same with `pivot_root` and the detach in place of MS_MOVE and
  chroot.
- **Threads.** A helper process holds 8,000 or 24,000 idle threads (64 KiB stacks, blocked on a pipe); the host's
  count is `/proc/loadavg`'s, which isn't namespaced.

### Numbers (p50 in microseconds; 300 creates per mode per setting)

| setting (host threads; agent mounts) | classic copy | classic switch (pivot_root) | classic detach | classic root, summed | move root | pruned root (agent's clones in) | pruned_pivot switch |
|---|---|---|---|---|---|---|---|
| 2,859; 14 | 25.4 | 213.0 | 28.4 | 391.0 | 142.2 | 114.3 | 231.2 |
| 10,861; 14 | 27.3 | 1,152.5 | 33.8 | 1,342.6 | 148.9 | 119.9 | 1,152.1 |
| 26,861; 14 | 26.4 | 2,992.7 | 78.4 | 3,270.0 | 143.3 | 117.0 | 2,980.5 |
| 2,866; 1,014 | 900.9 | 235.7 | 482.1 | 1,801.9 | 1,048.8 | 126.5 | 271.9 |
| 2,850; 4,014 | 4,062.0 | 404.9 | 2,078.5 | 6,993.3 | 4,395.3 | 364.5 | 427.8 |
| 2,877; 14 (again) | 26.9 | 346.1 | 42.0 | 565.4 | 161.3 | 130.6 | 364.4 |

- pivot_root: about 0.11-0.12 us per host thread, the same in the pruned seed's two-mount namespace (f75). MS_MOVE plus
  chroot: 3-4 us at every thread count.
- The copy and the detach: about 0.9 and 0.5 us per mount in the namespace copied (f76). The pruned copy: 10-15 us.
- MS_MOVE in a copy moves the copied table's teardown to the container's exit: the child's exit and reaping took
  2,073 us p50 at 4,014 mounts against 128 after pivot_root and the detach (f77).
- The agent's two `open_tree` clones: 8.4 us p50 with 14 mounts, 21 with 1,000 and 207 with 4,000 sibling mounts under
  the clones' source mount (`/run`), because a clone scans the source mount's children (f79).

### Under concurrency (8 probes at once, 200 creates each, one mode each; `g5r-probe-conc`)

| host threads | mode | 1,600 creates took | per-create root, p50 (8 probes) |
|---|---|---|---|
| ~3,000 | classic | 347 ms | 1,107-1,230 us (377 alone) |
| ~3,000 | pruned | 64 ms | 72-80 us (102 alone) |
| ~3,000 | move | 86 ms | 163-181 us |
| ~11,000 | classic | 1,725 ms (about 930 a second) | 5,517-6,088 us; pivot_root's p99 5.7-7.4 ms |
| ~11,000 | pruned | 115 ms | 79-114 us |

So pivot_root's walk, under namespace_sem (f7), caps a host's roots near one per walk (f80, inferred): at 11,000
threads the classic sequence made about 930 a second however many creators ran.

## 5. Linux 7.0 and 7.1: a namespace without a copy

- **7.0, `OPEN_TREE_NAMESPACE`.** `#define OPEN_TREE_NAMESPACE	(1 << 1)	/* Clone the target tree into a new mount
  namespace */` (`include/uapi/linux/mount.h:65`). `create_new_namespace` (v7.0 `fs/namespace.c:3090`) makes the
  namespace owned by `current_user_ns()` (`:3093`), copies only the namespace's root, which in 7.0 is nullfs
  (`VFS_WARN_ON_ONCE(old_ns_root->mnt.mnt_sb->s_type != &nullfs_fs_type);`, `:3112`), clones the given tree
  (`mnt = __do_loopback(path, flags, copy_flags);`, `:3138`) and attaches it on that root (`:3152`), locking the tree
  when it crosses user namespaces (`lock_mnt_tree(new_ns_root);`, `:3154`). It wants only
  `!ns_capable(current_user_ns(), CAP_SYS_ADMIN))` (`:3207-3208`). Joining it is `setns`, which sets only the caller's
  root (f72). So no copy of the agent's table and no walk (f83). crun takes it where it may (f17); its maintainer
  measured `unshare(NEWNS)` plus `pivot_root` at 0.343 ms clean and 0.917 ms with many mounts against 0.044 ms
  (`oci-runtimes-and-fast-start.md`, S12).
- **7.1, `CLONE_EMPTY_MNTNS`.** `#define CLONE_EMPTY_MNTNS	(1ULL << 37) /* Create an empty mount namespace. */`
  (v7.1 `include/uapi/linux/sched.h:42`); it implies `CLONE_NEWNS` (`kernel/fork.c:2682-2683`). `copy_mnt_ns` then
  clones only the root, `new = clone_mnt(old, old->mnt.mnt_root, copy_flags);` (v7.1 `fs/namespace.c:4262`), and
  resets the child's root and working directory to it (`:4284-4288`), in place of `copy_tree` over every mount
  (f84).
- **7.1, MOVE_MOUNT_BENEATH at the root.** v7.1's `can_move_mount_beneath` (`:3539`) no longer has v6.19's refusals of
  the caller's root and of a mount whose parent is the namespace's root (v6.19 `:3419-3422`; f74, f84).
- **What 7.x leaves to the same plumbing.** Both start from a namespace that holds nothing of the agent's, so the
  rootfs and every bind source still come in as detached trees made in the agent's namespace (f73), and nothing in the
  init may read the agent's paths: the same work the pruned seed needs on 6.19.

## 6. The alternatives compared

Run 4 (fedora 6.19, about 3,870 host threads): create p50 2.110 ms between traced creates (2.020 through the seam);
the root 0.894 ms: mounts 0.359, pivot_root 0.535; the clone with the copy inside 0.174 (p99 0.840); the old root's
detach, the hostname and the process become 0.065.

| | removes per create (run 4 terms) | how it scales | security and correctness | rootless | 6.19 / 7.x | findings |
|---|---|---|---|---|---|---|
| **(a) MS_MOVE + chroot in a copy** (runc/crun no-pivot) | pivot_root 0.535 -> ~0.004 and the detach (~0.03); keeps the copy | walk gone; copy still ~0.9 us per agent mount; teardown moved to exit (2.07 ms at 4,014 mounts) | **Fails**: agent's tree beneath (E2 reached), nested proc allowed past masks (E3), agent mounts pinned for the container's life (runc refuses rprivate) | **Fails**: a userns container unmounts its own root and reaches the agent's tree | both | f13-f16, f66-f70, f75-f77 |
| (a') chroot alone | the same | the same | **Fails**: the classic `..` escape reaches the agent (E1), and no nested user namespace (E0 refused) | fails | both | f66, f67 |
| **(b) detached trees, 6.19** (open_tree, move_mount, fsmount) | nothing alone: the namespace is still a copy and the switch still pivot_root or MS_MOVE; MOVE_MOUNT_BENEATH can't swap the root before 7.1 | | the means to bring sources into a namespace holding nothing of the agent's | `OPEN_TREE_CLONE` wants `may_mount()` in the agent's namespace: an unprivileged agent can't | 6.19 | f73, f74 |
| (b) 7.0 `OPEN_TREE_NAMESPACE` + setns | copy, pivot_root, detach (crun: 0.343-0.917 -> 0.044 ms) | flat in threads and mounts | nullfs beneath, tree locked across user namespaces | rootfs yes (CAP_SYS_ADMIN in own userns); host-path binds no; crun keeps userns classic | 7.0+ only | f17, f72, f83 |
| (b) 7.1 `CLONE_EMPTY_MNTNS` | the copy at the clone; with MS_MOVE or MOVE_MOUNT_BENEATH, pivot_root | flat | an empty namespace by construction | as 7.0 | 7.1+ only | f84 |
| **(c) setns into a namespace made ahead** (a pool per rootfs) | the copy and the switch per create, but per-container proc (pid ns), devpts, mqueue, sysfs, writable layer and binds stay (most of 0.359) | a pool made the classic way still walks once per namespace on the host, under namespace_sem: no lift to the host-wide cap; made the pruned way it adds little over (d) | setns wants CAP_SYS_ADMIN over the target and CAP_SYS_CHROOT+CAP_SYS_ADMIN in own userns, a single-threaded caller (f72); a reused namespace needs its mounts undone (an expedited RCU grace period each, f11) | the pooled namespace must be owned by the container's own userns: the userns pooled too, the pid ns made under it | both | f7, f11, f72, f80 |
| **(d) the pruned seed** (recommended) | pivot_root 0.535 -> 0.003; the detach (~0.03); the copy 25 -> 11 us (more on a host with many mounts); adds the agent's clones ~0.01 | flat in threads (117 us at 26,861) and in agent mounts but for the clones' scan of the source mount's children (207 us at 4,000) | nothing beneath but an empty read-only tmpfs: E1 stays in the rootfs, E2 reveals only the empty tmpfs, E3 refused, nested user namespaces allowed; no agent mount pinned | a rootless seed can't own a mount namespace without a userns of its own: classic there; on a root node pruned_userns E2 also held | both | f66-f69, f73, f75, f76, f78-f80 |

## 7. Recommended: one goal-5 step, the pruned seed's root; what stays for goal 6

### The step (root nodes, Linux 6.19 and 7.x alike)

1. **The seed prunes its own mount namespace once, at start.** `unshare(CLONE_NEWNS)`, `/` made rprivate, a small
   tmpfs with an empty `r`, one `pivot_root` into it (one walk per seed's life, off create's path), the old root
   detached, the tmpfs remounted read-only. Its namespace then holds two mounts, the namespace's root and that tmpfs:
   no proc, no sysfs, nothing of the host's (E3's guard and E2's). What the seed reads by path today it opens before
   pruning or gets from the agent as descriptors: the cgroup directory (`open_cgroup`), the container's directory (the
   exec FIFO's), the spec (or its bytes).
2. **The agent clones each source.** Per create it makes the rootfs and each bind mount's source a detached tree,
   `open_tree(OPEN_TREE_CLONE | AT_RECURSIVE | OPEN_TREE_CLOEXEC)`, and sends them with the create by `SCM_RIGHTS`, as
   crun opens every mount ahead for OPEN_TREE_NAMESPACE (f17, f73). It closes its copies once the seed answers; a tree
   never attached dissolves with its last descriptor.
3. **The init builds the root on the copy of the seed's namespace.** Its clone copies two mounts (10-15 us, f76). It
   attaches the rootfs at `/r` with `move_mount`, makes the spec's mounts beneath with descriptor-based calls (each
   bind's tree by `move_mount`, read-only by `mount_setattr`, proc first by `fsopen`/`fsmount`, since there is no
   `/proc/self/fd` until the container's own proc is mounted; masks bind the container's `/dev/null`, not the
   host's), then `chdir("/r")`, `mount(".", "/", MS_MOVE)`, `chroot(".")`, `chdir("/")` and the rootfs propagation
   the spec asks for. No pivot_root, no detach, no expedited grace period (f11).
4. **The classic path stays**, counted by reason: a rootless node (its seed can't own a mount namespace without a user
   namespace of its own), a container with a user namespace of its own (pruned_userns held in the probe, but the trees
   attached into it aren't locked: a follow-up), create-time hooks (they run against host paths), a mount namespace
   joined by path, a source that can't be cloned ahead. crun gates its OPEN_TREE_NAMESPACE path the same way (f17).

On run 4's figures this takes pivot_root's 0.535 ms p50 to about 0.003 and the old root's detach (about 0.03) to
nothing, and the clone's copy from about 25 to 11 us: about 0.55-0.6 ms off a 2.110 ms create, leaving about
1.5 ms (2.2x goal 5's 0.688 ms bar; the other added levers carry the rest). It also takes the root's growth with the
host's threads away (classic 3.27 ms at 26,861 threads, pruned 0.117) and lifts the host-wide cap the walk sets
(f80). Hex's own mount code costs about three times the raw calls (run 4's mounts 0.359 ms against the probe's 0.104
for the same four mounts and six nodes): a separate lever, not this step's.

### What stays for goal 6 (decision 6)

On 7.0+, `open_tree(OPEN_TREE_NAMESPACE)` and `setns`; on 7.1+, `CLONE_EMPTY_MNTNS` (no seed pruning, no copy at
all) and MOVE_MOUNT_BENEATH at the root; measured against the pruned seed on a 7.x host, the faster made the default
where the kernel has it; and the user-namespace cases 7.x opens (OPEN_TREE_NAMESPACE wants only CAP_SYS_ADMIN in the
caller's own user namespace, f83). The goal-5 step builds what all of these need: sources as detached trees, the
init's mounts on descriptors, nothing read by host path inside the container's namespace. So goal 6 swaps how the
namespace is made, not the rest.

### Acceptance criteria (each measured on fedora, 6.19, in the same run as libcrun)

- **AC-a, no walk.** In the create breakdown (run 5, run 4's harness), the root's switch (run 4's part 20, pivot_root,
  0.535 ms) is at most 0.020 ms pooled p50 and 0.050 ms p99, and the trace of the 1,500 traced creates shows no
  `pivot_root` by an init on the pruned path.
- **AC-b, the saving.** Hex's create p50 through the seam over libcrun's in the same run is at most 0.58 (run 4: 2.020
  over 2.750, 0.735).
- **AC-c, flat in threads.** With 24,000 extra idle threads on the host (the probe's helper), Hex's create p50 through
  the seam is within 10% of its p50 without them in the same run (the classic root grows 2.8 ms, f75).
- **AC-d, flat in mounts.** With 4,000 extra mounts in the agent's namespace, not under the bundles' mount, the init's
  clone (part 16) and the root's parts are within 10% of their p50 without them; the agent's clones are reported and
  at most 0.05 ms p50.
- **AC-e, under concurrency.** At 8 creates in flight with 8,000 extra idle threads, the root's parts p99 are at most
  0.5 ms (classic: 5.5-6.1 ms p50, f80).
- **AC-f, the seed's namespace.** A started seed's namespace holds exactly two mounts, the namespace's root and a
  read-only tmpfs on it, and no proc, sysfs or cgroup2. Its mountinfo, read from the agent, lists the tmpfs alone: the
  namespace's root lies outside the seed's root, and the kernel lists no mount it can't reach from the task's root
  (v6.19 `show_mountinfo`, fs/proc_namespace.c:150-151, "mountpoints outside of chroot jail will give SEQ_SKIP on
  this"; `seq_path_root`'s SEQ_SKIP, fs/seq_file.c:517-519; `__d_path`, fs/d_path.c:211-223).
- **AC-g, the classic path counted.** Every create that takes the classic path is counted by its reason (a fixed set,
  so no high-cardinality label; the name agreed with kiln's Observe owner per CLAUDE.md §10), and each still passes the
  OCI conformance it passes today.

### Tests (do X, expect Y)

- **The root-escape test.** Create a pruned-path container whose spec keeps CAP_SYS_ADMIN and CAP_SYS_CHROOT, with a
  marker file, root's alone, in the agent's namespace outside the rootfs. Inside it: (1) `mkdir x; chroot x;` `chdir
  ..` sixteen times, `chroot .`; (2) `umount2("/", MNT_DETACH)`, then `setns` into its own mount namespace through a
  pidfd, then list `/`; (3) look for the marker by its path after each. Expect: (1) `/` is still the container's
  rootfs; (2) `/` holds only the seed's empty tmpfs (`r`); the marker unreachable after both. The same on the classic
  path expects an empty `/` after (2). This is the probe's E1 and E2 (`g5r-probe-escape`).
- **No proc past the masks.** In a pruned-path container whose `/proc` has a masked path, make a user, mount and pid
  namespace, map root, fork, mount proc: expect EPERM (E3).
- **Nested user namespaces work.** In a pruned-path container, `unshare(CLONE_NEWUSER)`: expect success (E0; a plain
  chroot would refuse it).
- **No pivot_root on the path.** Make a pruned-path container under the breakdown's trace: expect no `pivot_root` from
  the init, one MS_MOVE and one `chroot`.
- **Nothing of the host's inside.** In a pruned-path container whose spec binds `/usr` read-only and a volume: expect
  both present with the options asked, and the bundle's `config.json` and the node's state directory unreachable
  from inside, by `..` or `/proc/self/root`.
- **A failed attach is refused, not leaked.** Hand the init a tree descriptor that fails to attach: expect a typed
  refusal naming the mount and why, no container left, and the agent's clone dissolved (its namespace count back where
  it was).
- **A crash mid-root leaves nothing.** Kill the init between the attach and the MS_MOVE: expect its namespace and
  mounts gone with it and no mount left in the agent's namespace.
- **The classic path where it must.** On a rootless node, with a user namespace in the spec, with createContainer
  hooks, with a mount namespace joined by path: expect the classic path, counted under its reason, and today's tests
  passing.
- **Propagation as asked.** With `rootfsPropagation` shared, slave and private on the pruned path: expect the root's
  propagation as asked in mountinfo, and no mount made inside appearing in the agent's namespace.

### What would falsify the recommendation

- The root-escape test reaching the marker, or any path of the agent's, from a pruned-path container, on 6.19 or 7.x.
- A nested proc mount succeeding in a pruned-path container whose `/proc` is masked.
- The pruned root's switch growing more than 10% between 0 and 24,000 extra host threads.
- Create p50 through the seam not dropping by at least 0.45 ms against run 4's in the same harness on fedora (pivot_root's
  0.535 less the agent's clones and noise).
- A seed's namespace holding more than its two mounts, or a proc or sysfs, at any create.

## 8. Findings (research `01a190c51f137042a9697949c8fe2c7a`)

f6 the walk; f7 namespace_sem across it (inferred); f8 mount_lock; f9 the copy; f10 the copy locked across user
namespaces; f11 the old root's detach and its grace period; f12 runc and crun pivot; f13 runc's no-pivot and its
warning; f14 runc's proc/sysfs guard; f15 runc refuses rprivate without a pivot; f16 crun's no-pivot; f17 crun's
OPEN_TREE_NAMESPACE path and its gates; f66 chrooted processes and user namespaces; f67 `..` on a root stack; f68 proc
already visible; f69 MS_MOVE leaves the agent's tree beneath (E2); f70 pivot_root locks the new root; f71 chroot is
O(1); f72 setns's rules; f73 detached trees cross, paths don't; f74 MOVE_MOUNT_BENEATH at 6.19; f75 pivot_root per
thread; f76 the copy and detach per mount; f77 MS_MOVE's teardown at exit; f78 the pruned seed's root; f79 the agent's
clones; f80 under concurrency (inferred); f83 OPEN_TREE_NAMESPACE (7.0); f84 CLONE_EMPTY_MNTNS and MOVE_MOUNT_BENEATH
(7.1).


## 9. Shared-source propagation regression (2026-11-02)

The fedora 6.19.10 probe at `~/.cache/hex-t7/g5x-s3/leak-probe-20261102T0805Z`
(`RESULT.md`, `leak_probe.c`) reproduces a pruned-path correctness bug: cloning a shared source,
attaching the clone in a private child mount namespace, then mounting tmpfs beneath it creates a
second mount in the agent's namespace. The agent mount survives the child's exit. Making the clone
recursively private first prevents the leak. Private and slave sources also prevent it. The probe
ran inside a disposable privileged lab, with its own tmpfs source.

Why: v6.19 `fs/namespace.c:1259-1288` (`clone_mnt`) retains the original peer group/master unless
CL_PRIVATE or CL_SLAVE changes it. The detached `open_tree` copy (`:3007-3040`, `get_detached_copy`)
uses `__do_loopback` (`:2963-2966`), and `copy_tree` passes those flags to `clone_mnt` (`:2153-2166`).
After attachment, `fs/pnode.c:311-365` (`propagate_mnt`) creates secondary mounts in peer groups and
their slaves. Namespace membership alone does not break propagation
([mount_namespaces(7)](https://man7.org/linux/man-pages/man7/mount_namespaces.7.html)).

The labs hid the condition: `seam_root.sh` binds HOME rprivate; `docker_lab.sh` uses a named volume
which the fedora Docker probe showed as `master:1` (a slave). A plain bind showed no tag, while
`:rslave` showed `master:158`; fedora's host `/` and `/home` were shared. A slave root-source inherited
by a clone could satisfy the old master-tag assertion without exercising any shared peer. On a
private bind the same slave assertion fails on both bf77298 and the pool draft. Neither result
proved that shared host inputs were contained.

The correction isolates source propagation before population. On the agent's detached rootfs,
apply MS_SLAVE recursively for requested rootfsPropagation shared/slave, MS_PRIVATE recursively
otherwise. The init later applies the requested final root propagation as before. Slave retains
source master:N; shared creates a new peer group, with N remaining an incoming master, never its
own peer. Default/plain and private have neither tag. Each bind tree is also isolated recursively
before attachment: private by default; slave initially for explicit shared/slave, followed by the
requested shared/unbindable transition. This removes every host peer before a child mount can be
made. The ordinary/classic path's upfront recursive-private operation remains in place.

`mount_setattr` can target the detached tree by descriptor and combine attribute/propagation fields
([mount_setattr(2)](https://man7.org/linux/man-pages/man2/mount_setattr.2.html)). At v6.19
`fs/namespace.c:4756-4821`, `do_mount_setattr` accepts an anonymous namespace root but still takes
namespace_sem for write when propagation is nonzero. Agent-side root isolation therefore costs
one real writer hold per create; it is outside part 19, included in whole-create latency. Bind
isolation remains in the init and folds attributes and propagation into one call for recursive
binds. Nonrecursive attribute changes retain their original scope; recursively isolating a tree
must not make those attributes recursive. Additional calls in that case are a correctness cost,
not a reason to widen the spec's flags. The prepared `/dev` pool is unchanged.

The regression makes both rootfs and bind sources shared in its own lab namespace, asserts
shared:N preconditions, and checks plain/shared/slave/private on actual pruned creates. It compares
complete agent mountinfo before, during and after each box, with proc, devices, devpts, mqueue,
binds, a mount below the bind, masks and read-only paths present. Slave must show master:N exactly;
shared must show a different peer group; default/private show neither; default binds are private.
Source guards undo their temporary mounts on every outcome. The test must fail on the pre-fix
code, with that output retained, then pass with the correction. A gained agent mount, wrong master,
or altered mount option falsifies the correction. Re-run escape, mount kinds, rootless/cgroupfs,
validation, conformance, gates and the complete-coverage performance measurements afterward.
