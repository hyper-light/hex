# The init's mounts at the system calls' cost: where part 19 goes, and what takes it

Source notes for goal 5 extended plan step 3, the init's mounts at the system calls' cost (kiln plan
`01a190c51f07757ab54b09f1aada633c` v6, research `01a190c51f137042a9697949c8fe2c7a`, f18 and f78). Run 4 of the
create breakdown (`bench/competitive/probes/create_breakdown.md`, create-breakdown-20261031T061841Z) put part 19,
the init's root mounted (the rootfs bound, `/usr`, `/proc` and `/dev`, its device nodes), at 0.359 ms p50 and
1.012 ms p99 on the classic seed path. Step 2 moved a root node's creates to the pruned seed's path, where run 10
measured part 19 at 0.169 ms p50 (0.228 with a build beside it) and run 9, at eight creates in flight with 8,000
idle host threads, at 0.209 ms p50 and 1.553 ms p99. The pruned-seed probe (`goal5-root.md` §4, `rootprobe.c`)
made the same mounts and nodes in about 0.104 ms (f78). The step's bars: part 19's pooled p50 at most 0.16 ms, and
its p99 at eight in flight with 8,000 idle host threads at most 0.5 ms, on fedora.

Sources were read at fixed revisions, downloaded on adacachy (2026-11-02) into `~/.cache/hex-t7/g5x-s3/src` and
`~/.cache/hex-t7/goal5-root`: Linux v6.19 from `raw.githubusercontent.com/torvalds/linux/v6.19/`, crun 1.30.1
(`raw.githubusercontent.com/containers/crun/1.30.1/`), runc v1.5.2
(`raw.githubusercontent.com/opencontainers/runc/v1.5.2/`), man pages from man7.org. `path:line` is at that revision;
Hex's own lines are at bf77298, the step's base.

## 1. The trace

### Method

- **The probe.** `create_breakdown.sh` with `MOUNTS=<lifecycles>` (`MOUNTS_CONC`, default `1 8`) runs, in the
  bench's privileged container, one `seam_oci` throughput phase per concurrency with every system call of the
  `hex` processes traced (`create_breakdown_inside.sh`, `mounts_trace`), and `create_breakdown.py mounts` cuts each
  create's part 19 out of it, the span the AC-e reader (`switch`) uses: from the exit of the init's join of the
  network namespace made ahead (`setns` with `CLONE_NEWNET`) to the entry of its `chroot` (the pruned path) or
  `pivot_root` (the classic one).
- **The events.** `raw_syscalls/sys_enter` and `sys_exit`, which record a call's number and its arguments and fetch
  no string: the `syscalls` subsystem's per-call events fetch each path from user memory, about 4 us a time
  (`exec_breakdown.md`), which would charge the very calls this times. With them `sched/sched_switch`, so time off
  the CPU is told apart, preempted (`prev_state` R) or blocked, and `exceptions/page_fault_user`. A call's time runs
  from its entry to its exit; the rest of the span is the init's own code between calls, its page faults and the
  trace's own two events per call among it.
- **By mount.** Each mount's share is the calls from the one after the previous attach (`mount` or `move_mount`) to
  its own attach.
- **The lab run (not of record).** adacachy, AMD Ryzen 9 9950X3D2 (32 CPUs, the container pinned to 16-27), Linux
  7.2.9-2-cachyos, the 1-minute load 50 to 63, fedora being offline; `MOUNTS=500`, 2026-11-02T00:44:38Z,
  `~/.cache/hex-t7/g5x-s3/bench/out/create-breakdown-20261102T004438Z`, the binaries of bf77298 (hex sha256
  2a94c58f01f7f6ab, seam_oci 863df01e72723582), no event lost. Its kernel is not fedora's and the trace adds two
  events per call, so its figures place the time and count the calls; the step's bars are measured on fedora.

### What part 19 is made of (bf77298, the pruned path, one create in flight, 500 creates)

Span p50 337 us, p99 1,563 (traced). Per create, by call:

| call | calls | us | us per call | off the CPU, us: preempted / blocked |
|---|---|---|---|---|
| fsconfig (create) | 2.0 | 58.3 | 29.16 | 3.4 / 1.4 |
| openat2 | 36.0 | 45.6 | 1.27 | 7.5 / 0.0 |
| setsid | 1.0 | 35.9 | 35.87 | 9.2 / 0.9 |
| close | 55.0 | 27.7 | 0.50 | 1.5 / 0.0 |
| move_mount | 4.0 | 26.5 | 6.62 | 8.5 / 6.7 |
| open | 25.0 | 18.2 | 0.73 | 1.9 / 0.0 |
| mknodat | 6.0 | 17.5 | 2.92 | 0.4 / 0.0 |
| symlinkat | 5.0 | 14.5 | 2.90 | 1.0 / 0.0 |
| fsmount | 2.0 | 8.7 | 4.37 | 0.5 / 0.0 |
| fsopen | 2.0 | 6.7 | 3.37 | 0.4 / 0.0 |
| mount (MS_MOVE) | 1.0 | 6.3 | 6.32 | 0.7 / 2.0 |
| fchmodat | 6.0 | 4.4 | 0.74 | 0.0 / 0.0 |
| fsconfig (string) | 2.0 | 3.3 | 1.66 | 0.0 / 0.0 |
| mount_setattr | 1.0 | 2.0 | 1.98 | 0.0 / 0.0 |
| fstat | 1.0 | 1.2 | 1.19 | 0.1 / 0.0 |
| chdir | 1.0 | 0.7 | 0.73 | 0.0 / 0.0 |

The mean span is 403.8 us: 277.6 in calls and 126.1 between them (17.5 of it preempted), 150 calls and 17 page
faults in user mode per create. By mount: to the rootfs's attach 53.4 us of calls (`setsid` among them), `/usr`
9.6, `/proc` 55.2, `/dev` 39.0, and the 6 nodes and 5 links to the `MS_MOVE` 120.4.

### Findings

- **M1, where the calls go.** The probe makes the same mounts in 17 calls (`rootprobe.c`'s `mounts_pruned`, its
  rootfs attach and its switch). Hex makes 150. Its `inroot::open` opens the root by path again for every path it
  resolves and resolves each prefix of that path in turn, an `openat2` and a `close` each (`inroot.rs:32-92`); and
  the devices and links are each looked for first, then their directory opened again to make them in
  (`init.rs:1238-1363`), and each node's mode set by `fchmodat` after `mknodat` (its mode is masked by the umask:
  "The file mode is modified by the process's umask in the usual way: ... (mode & ~umask)", mknod(2)). `open`,
  `openat2`, `close` and `fchmodat` are 122 of the 150 calls, 96 us of the 277.6 in calls, and most of the 126
  between them; the mounts themselves need about a dozen of them (a target's resolution, its descriptors' closes).
- **M2, the work itself.** The calls the probe makes too: the two superblocks made (`fsconfig` create, proc's and
  `/dev`'s tmpfs, 29 us each here), the four attaches and the move (32.8 us), `fsopen` and `fsmount` (15.4), the six
  `mknodat` (17.5). The five `symlinkat` (14.5) are the OCI spec's `/dev` links, which the probe doesn't make.
- **M3, not a mount: `setsid`.** It lies in part 19 because `container()` calls it before the root is built
  (`init.rs:349`): 35.9 us per create at one in flight, 62.1 at eight. v6.19's `ksys_setsid` takes `tasklist_lock`
  for write (`kernel/sys.c:1276`), then makes the new session an autogroup (`kernel/sys.c:1298`):
  `autogroup_create` calls `sched_create_group` (`kernel/sched/autogroup.c:95`), which allocates a `cfs_rq` and a
  scheduling entity for every possible CPU (`kernel/sched/fair.c:13623-13624`), and `sched_online_group`
  (`kernel/sched/autogroup.c:117`), which takes `task_group_lock` (`kernel/sched/core.c:9031`) and then every CPU's
  runqueue lock in turn (`kernel/sched/fair.c:13654-13657`), on every `setsid`, whether autogroup scheduling is on
  or not. crun's container init calls it (`src/libcrun/container.c:1131`), and so does runc's
  (`libcontainer/nsenter/nsexec.c:1195`): every runtime's create pays it, and the probe's 0.104 ms holds none.
- **M4, the kernel's mount lock.** At v6.19 a `move_mount` takes `namespace_sem` for write (`do_move_mount`,
  `fs/namespace.c:3517`, through `do_lock_mount`'s `namespace_lock`, `:2747`), and so does a `mount(2)` that makes a
  filesystem (`do_new_mount_fc`, `:3654`) or moves one (`MS_MOVE`: `do_move_mount_old`, `:3589`). `fsmount` doesn't:
  its mount goes into an anonymous namespace of its own (`:4351`), and `mount_too_revealing` returns at once in the
  initial user namespace (`:6167`). `mount_setattr` takes it only to change a propagation (`:4770-4773`), and closing
  an attached tree's descriptor not at all (`dissolve_on_fput` leaves at `:2303`). So part 19 holds it five times
  per create at bf77298 (the rootfs at `/r`, `/usr`, `/proc`, `/dev`, the `MS_MOVE`), as the probe does (the
  rootfs, `/usr`, proc, tmpfs, the `MS_MOVE`).
- **M5, the tail.** At eight in flight the slowest 1% (a span of 2,235 us or more) spent 2,294 us per create in
  `move_mount`, 1,489 of it off the CPU, and 611 in `setsid`, all of it on the CPU (writers spinning:
  `tasklist_lock` is a spinning lock, and so are the runqueue locks). At one in flight the slowest 1% (1,601 us or
  more) spent 945 us in `move_mount` (918 off the CPU) and 614 in `openat2` (579 off the CPU, preempted). The tail is
  waits for the mount lock and preemption, and a span of 150 calls lies open to both longer than one of 40.
- **M6, page faults.** 17 per create in user mode (16 at eight in flight). The init is a fork-like clone of its seed
  (`sys.rs`'s `clone_sibling`, no `CLONE_VM`), so its first write to each page it still shares with the seed copies
  that page; fewer allocations and a shallower stack touch fewer.
- **M7, the tail on fedora: the CPU, not the calls.** Fedora's traces (§5) charge each wait the init spends
  runnable but off the CPU to the process the CPU went to first, and to the softirq or work item that ran there
  meanwhile (1e7a2fa). At eight in flight the slowest 1% are mostly such waits: 2,405 of a 3,210 us mean span
  (20261102T025959Z, load 25), the CPU going first to ksoftirqd, kworkers and other hex processes; with 8,001 idle
  threads up and other tenants' builds on the same CPUs, 3,293 of 4,305 us (20261102T033211Z), the CPU going first
  to their rustc threads (2,538 us per create), python3, the node's network-namespace pool and other hex processes,
  the kernel's teardown of exited boxes (`css_killed_work_fn`, the RCU softirq) running there meanwhile. What is
  blocked is the superblocks' making (`fsconfig` create, 505 us). `setsid` gives the CPU away by itself: at v6.19
  `sched_autogroup_create_attach` moves every thread of the caller (`kernel/sched/autogroup.c:187-188`), and
  `sched_move_task` asks for a reschedule of a task that is running (`kernel/sched/core.c:9123-9128`) whether its
  group changed or not, so the init passes through the scheduler mid-mounts and waits behind whatever is runnable
  (1,029 us of the tail in `setsid` at 20261102T025959Z, 1,002 of it preempted). crun calls `setsid` after its
  pivot and mounts (`src/libcrun/container.c:1131`, after `:1055` and `:1060`); runc before them
  (`libcontainer/nsenter/nsexec.c:1195`, its rootfs made in Go after). A shorter part 19 is open to fewer of these
  waits; none of them is a call it makes more than the probe does, but `setsid`'s place.

## 2. What takes it

1. **One descriptor of the root per build** (`inroot::Root`). On the pruned path it is the rootfs tree the agent
   cloned, an `O_PATH` descriptor of the tree's root mount, which still names it once attached; on the classic path
   the bound rootfs opened once, and again after a spec mount at `/` itself, which goes over the bind (a lookup of the
   rootfs's path lands on the top, and the build must too). A path that's there resolves in one `openat2` with
   `RESOLVE_IN_ROOT | RESOLVE_NO_MAGICLINKS` of its normal form ("Treat the directory referred to by dirfd as the
   root directory while resolving path. Absolute symbolic links are interpreted relative to dirfd.", openat2(2)). Any
   other is walked component by component as before: what's missing is made as before, and a path refused (a link
   that leads nowhere inside the root, a component refused) is refused naming the component the walk was refused at,
   as before. A missing path asked to make nothing is the one answer taken from the single call: the kernel resolves
   components in order, so nothing before the missing one was refused.
2. **`/dev` opened once** for its defaults. Each node made with `mknodat` under a zero umask, its mode exact, the umask
   put back after, in place of a `fchmodat` each; each link with `symlinkat`. A name the kernel refuses as there
   (`EEXIST`, "linkpath already exists", symlink(2)) is then looked for, as it was looked for before each was made, so
   every outcome is the one the look before gave: a name that leads to something in the root is left as it is,
   whatever it is; a node's name that leads to nothing inside the root is refused, naming it; a name whose look is
   refused (a magic link, `ELOOP` under `RESOLVE_NO_MAGICLINKS`; a file taken for a directory, `ENOTDIR`) is refused,
   naming it. At bf77298 a default node whose name is there with another type is left as it is, not refused
   (`init.rs:1240-1242`), as runc leaves it (`createDeviceNode` returns on `EEXIST`,
   `libcontainer/rootfs_linux.go:1003-1004`); the change keeps that.
3. **A bind's source is looked at only when its target is missing** (to make the target a file or a directory), and
   a mount's propagation is set on the mount's own descriptor, not on its target opened again.
4. **The pruned path attaches the rootfs on `/` itself**, over the seed's read-only tmpfs, where step 2's `MS_MOVE`
   from `/r` put it: the same mount tree, one attach and one hold of `namespace_sem` fewer (four per create for the
   bench's spec), then `fchdir` and `chroot` (part 20). Until its `chroot` the init's own root is the tmpfs's, and it
   walks no path by name. The tree being the same, so is its containment (decision 5): `..` from a mount's root
   climbs only where it isn't mounted on its parent's own root (`fs/namei.c:1433-1446`), and the rootfs and the tmpfs
   beneath it are each mounted on their parent's root, so E1 climbs nowhere; the rootfs is the namespace's topmost
   mount over its root, so nested user namespaces stay allowed (E0; `fs/namespace.c:6095-6097`,
   `kernel/user_namespace.c:106`). The seed's tmpfs held an empty `r` for step 2's attach; with nothing attached
   there it holds nothing now (the lead's call, CLAUDE.md §13), and step 2's AC-f and E2 hold it to that.
5. **Masked files share one descriptor** of the container's `/dev/null`.
6. **`setsid` stays where it is.** OCI runtimes make the container's init a session leader (crun, runc above), and its
   cost is the kernel's, in part 19 because the init's set-up calls it there; the trace reports it among the calls.

7. **The classic path lists a recursive read-only bind's cgroup filesystems from the kernel, not the table.** On
   fedora the classic init's root mounted (the shim path's part 23, the classic part 19) took 0.201 ms pooled p50
   (§5), over the bar, and its table read was 72 us of it per create in the trace: the mount table's text made for
   `remount_cgroupfs_readonly` (two `read`s, 59.9 us), its open (7.3) and the bind's path read back through
   `/proc/self/fd` (`readlink`, 4.6). Since Linux 6.8, `listmount(2)` returns the unique id of every mount beneath a
   mount, not only its children: it walks the namespace's mounts in id order and keeps each one reachable from the
   given mount (v6.8 `fs/namespace.c:5069-5077`, `listmnt_next` at `:5045-5048`; v6.19 `:5866-5874`), in batches
   resumed from the last id, holding `namespace_sem` shared, not for write (v6.19 `:5951-5952`). `statmount(2)` gives
   one mount's filesystem type and mount point, relative to the caller's root and unescaped, as offsets into the
   strings after its fixed 512 bytes (v6.19 `include/uapi/linux/mount.h:155-189`, `STATMOUNT_MNT_POINT` and
   `STATMOUNT_FS_TYPE` at `:217-218`); both take a mount by the unique id `statx(2)`'s `STATX_MNT_ID_UNIQUE` gives
   (Linux 6.8), in the request 6.8 published (`MNT_ID_REQ_SIZE_VER0`, 24 bytes, `:207`), which every later kernel
   still takes. So the bind's top is asked its id and mount point, its submounts listed, and each asked its type and
   point: three calls for a bind that carries none, as the bench's `/usr` does, in place of five that make and read
   the whole table. Their numbers are the common table's on x86_64 and aarch64, 457 and 458 (v6.19
   `arch/x86/entry/syscalls/syscall_64.tbl:384-385`, `scripts/syscall.tbl:400-401`, which arm64's table is made
   from, `include/uapi/asm-generic/unistd.h:828-832`); the libc crate at 0.2.191 carries them for m68k alone, so
   `sys.rs` names them, and rustix wraps neither, so the two calls are new `unsafe` there (CLAUDE.md §12, its own
   commit, held for Ada). A kernel without them, before 6.8, answers `ENOSYS`, or a `statx` without the unique id;
   the init then reads the bounded table as before, the same paths found either way, as a test holds them.

For the bench's spec this makes part 19 about 41 calls, against 150, holding the mount lock four times, against five.

## 3. What would falsify it

- Part 19's pooled p50 on fedora above 0.16 ms after the change, or its calls per create not under 50 in the trace.
- `oci_box`'s `every_mount_kind_is_in_the_box_as_its_spec_asks` failing on either path after the change; or
  `escapes_are_contained` (E0-E3) failing on the root attached on `/`.
- A hostile `/dev` or mount target meeting another outcome than the look before each gave (`init.rs`'s tests: a name
  that leads out of the root, one whose look is refused, one there already of whatever type).
- The classic path's part 19 (the shim path's part 23) above 0.16 ms pooled p50 on fedora with the table read gone;
  or `listmount` and `statmount` finding other cgroup filesystems beneath a bind than the table lists for it (the
  test that runs both over the host's `/sys`), which would mean 6.8's walk isn't the table's.
- Part 19's p99 at eight in flight with 8,000 idle host threads above 0.5 ms: fedora's is (§5), and its traces put
  the tail on the CPU, not on the mount lock or the calls (M7): the init waits runnable behind the host's other work,
  so the levers are a shorter part 19 and fewer points where it gives the CPU away (`setsid`'s place).

## 4. The change in the lab (adacachy, not of record)

The change's binaries (937f54f: hex sha256 0f5a842324e22c7c, seam_oci 2374d0748a4013a0) beside the base's
(bf77298, above), each traced as in §1, 500 lifecycles at one and at eight in flight, the two interleaved twice on
adacachy, 2026-11-02T01:35:10Z to 01:38:17Z, the 1-minute load 182 at the first round's start and 66 at the second's
(`~/.cache/hex-t7/g5x-s3/bench/out/create-breakdown-<run>`). Part 19's span, traced, in microseconds:

| run | binary | one in flight: p50 / p99 | eight in flight: p50 / p99 | calls per create | page faults per create |
|---|---|---|---|---|---|
| 20261102T013510Z | base | 371 / 2,569 | 207 / 2,384 | 150 | 16.0 / 16.5 |
| 20261102T013605Z | change | 231 / 1,885 | 196 / 1,770 | 41 | 12.0 / 13.3 |
| 20261102T013648Z | base | 234 / 727 | 221 / 1,275 | 150 | 17.0 / 16.3 |
| 20261102T013737Z | change | 130 / 359 | 133 / 1,193 | 41 | 13.0 / 12.9 |

- **The calls.** 41 per create, against 150: the 4 attaches, one `openat2` per target and one for `/dev`, the 6
  nodes under one umask's change and back, the 5 links, the filesystems' `fsopen`/`fsconfig`/`fsmount`, `setsid` and
  the 9 closes of what they opened; the rootfs attached on `/` and no `MS_MOVE`. What's left at one in flight is the
  work the probe does too, and `setsid` (88.7 us of the first change round's 331.5 mean, 61.3 of it preempted at load
  182).
- **The tail.** At eight in flight the slowest 1% are still lock waits, in both binaries: `move_mount` blocked on
  `namespace_sem` (2,201 us of a 5,418 us mean span in the first change round), `setsid` spinning (1,326), and the
  superblocks' making (`fsconfig` create, 342 us blocked), whose allocations take global locks. The change takes the
  calls out, not those holds; on adacachy every container on the host shares them.
- **The classic path** (`MOUNTS_PATH=classic`, its creates through the shim, 300 at one in flight,
  20261102T014207Z and 20261102T014315Z): 154.5 calls per create to 44.7, the span's p50 426 us to 273 (traced). Of
  what's left, `remount_cgroupfs_readonly`'s read of the whole `/proc/self/mountinfo` for a recursive read-only bind
  (`/usr`), to find the cgroup filesystems it carries: an `openat`, 9 `read`s and a `readlink`, 72.8 us of the 233.1
  in calls. The read had no bound, and its buffer started small and grew; read into 64 KiB from the start and
  bounded at 16 MiB (ac153c5) it takes 2 reads, the classic path 36 calls per create (20261102T020049Z), but the
  reads' time stayed, 58.6 us: it is the kernel's making of the whole table's text, not the calls. The probe's
  classic sequence reads no table; listing the bind's own submounts (`listmount`/`statmount`, Linux 6.8) would
  take it, and rustix wraps neither, so it means new `unsafe` in `sys.rs` (CLAUDE.md §12): the lead's and Ada's
  call.
- **What the mount kinds' test found on the pruned path, before the change as after it.** A filesystem made with
  `fsopen`/`fsmount` was given no source, so the box's mountinfo read `none` where its spec named `proc`, `tmpfs` or
  `shm` (`oci_box.rs:465` at f674357 and 937f54f: `("proc", "none")` against `("proc", "proc")`), and `fsmount` took
  only read-only, `nosuid`, `nodev` and `noexec`, so `strictatime` (Docker's `/dev`, which OciSpec renders) became
  relatime and `noatime`, `nodiratime`, `nosymfollow`, `sync` and `dirsync` were lost. `mount(2)` on the classic path
  passes the source as the context's `source` parameter (v6.19 `fs/namespace.c:3706`, which every filesystem takes,
  `fs/fs_context.c:151-156`) and keeps the flags (`fs/namespace.c:3972-3992`; `sync` and `dirsync` are the context's
  flags, `fs/fs_context.c:41-46`). `make_fs` now does both (2733a0c, 5d24dae), two `fsconfig` calls more per create
  for the bench's spec.
- **Validation and the root lab.** runtime-tools bd3b0bd on adacachy against the base's `hex` and the change's: 26
  pass, 32 fail, 0 skip each, the same test by test. As root (`docker_lab.sh`): at 937f54f E0-E3 contained on the
  root attached on `/`, AC-f's seed listing its one mount, the classic reasons counted with none under `Rootless`;
  at c423bb4 (the fixes in) `oci_box`, `oci_spec_box` and `node_socket` all pass, the mount kinds as their spec asks
  on the pruned and the classic path; at the same tree with 5d24dae reverted (10ccbe7, branch
  `cp/t7-ext-s3-noflags-demo`) the test stops at the pruned box's `/strict`, listed `rw,nosuid,relatime` where its
  spec asks `strictatime`. Rootless, the 7 gates pass at c423bb4 on adacachy, `oci_box` among them with the mount
  kinds on the classic path.


## 5. Fedora, of record

fedora (`kiln-a094a12d`: i9-14900HX, 32 CPUs, Linux 6.19.10-300.fc44), the bench container pinned to CPUs 4-15
with no load gate (CLAUDE.md's noisy-host rule): the host's own tenants (a kind cluster, buildkit, a registry, minio)
up throughout, at times another agent's create breakdown on CPUs 4-31 and other tenants' Rust builds, each run's
`env.txt` naming them (`~/.cache/hex-bench/out/create-breakdown-<run>`), 2026-11-02T02:59Z to 05:15Z. Binaries:
before, bf77298 (hex sha256 ad2ebdf3a7afe7cb); the change, f3c6c09 (a999609cf3ac28d5); the classic path's listing,
d236217 (a0f704d9a2e3eb18), and its smaller buffer, 5cccb60. Part figures are the create breakdown's pooled p50 over its three rounds, in ms.

- **Part 19 on the pruned path**, against the bar of 0.16 ms (run 4: 0.359): 0.241 at f3c6c09 (20261102T031219Z,
  rounds 0.262 / 0.256 / 0.132, the 1-minute load 56 at its start with another breakdown on the same CPUs), 0.131
  (20261102T031447Z, 0.128 / 0.141 / 0.127, load 37), and 0.125 at d236217 (20261102T050253Z, 0.127 / 0.163 / 0.105,
  load 18, f3c6c09 in the same rounds: the create's p50 0.843 ms against its 0.940). Met in two runs of three; in
  the third the host's other work took it, as M7 finds.
- **Part 19 on the classic path**, the shim path's part 23 (its first run to its `pivot_root`, `CLASSIC=1`: every
  create through the shim by a createRuntime hook). Each run times one binary, so each run's parts 21 and 22, the
  same code in both, show the host's pace beside it:

  | run | binary | load | part 21 | part 22 | part 23 (r1 / r2 / r3) |
  |---|---|---|---|---|---|
  | 20261102T044432Z | f3c6c09 | 58.8 | 0.048 | 0.046 | 0.201 (0.203 / 0.194 / 0.203) |
  | 20261102T050008Z | d236217 | 12.6 | 0.078 | 0.078 | 0.243 (0.262 / 0.270 / 0.216) |
  | 20261102T050738Z | d236217 | 28.2 | 0.083 | 0.077 | 0.251 (0.276 / 0.251 / 0.225) |
  | 20261102T050945Z | f3c6c09 | 21.8 | 0.053 | 0.050 | 0.212 (0.236 / 0.209 / 0.185) |
  | 20261102T051129Z | d236217 | 10.8 | 0.052 | 0.049 | 0.172 (0.171 / 0.158 / 0.185) |
  | 20261102T051330Z | f3c6c09 | 34.5 | 0.060 | 0.058 | 0.242 (0.403 / 0.228 / 0.195) |
  | 20261102T052438Z | 5cccb60 | 28.0 | 0.076 | 0.068 | 0.234 (0.231 / 0.227 / 0.254) |
  | 20261102T052820Z | d236217 | 73.3 | 0.043 | 0.043 | 0.156 (0.160 / 0.159 / 0.150) |

  In the rounds at the same pace (part 21 at 0.043 to 0.050 ms) f3c6c09's part 23 is 0.185 to 0.209 and d236217's
  0.150 to 0.171, under the bar in 20261102T052820Z; over each run's own part 21, f3c6c09's is 4.0 to 4.2 times it
  and the listing's 3.0 to 3.6. The listing takes about 35 us, a fifth, and the classic path is at the bar: under it
  at the host's full pace, over it where the host runs short bursts slowly (an idle fedora clocks short
  bursts down: `bench/competitive/RESULT.md`, `probes/host_clock.sh`). 5cccb60's smaller `statmount` buffer took 2 of the init's page faults per create (27.1 against 29.1, and
  4.3 us against 9.3 before its `statmount`, 20261102T052422Z against 052432Z, traced). The traced
  spans, interleaved at one in flight (20261102T050528Z, 050538Z, 050546Z, 050555Z: f3c6c09, d236217, f3c6c09,
  d236217), say where: the table's `read`s, `openat` and `readlink` (51 to 55 us) became a `statx`, a `statmount`
  and a `listmount` (8 to 10 us), the span's p50 281 and 322 us to 233 and 205. What's left beside the probe's own
  work: the init's code between its calls, 86 to 115 us with 29 to 30 page faults per create (the init is a
  fork-like clone of the shim, its first write to each page it shares a copy), and `setsid`, 19 to 33 us.
- **Part 19's p99 at eight in flight with 8,001 idle host threads** (AC-e's harness), against the bar of 0.5 ms:
  2.302 at f3c6c09 (20261102T031440Z; p50 0.172, max 5.106) and 2.051 (20261102T050726Z; p50 0.180, max 4.086),
  2.433 at bf77298 (20261102T031444Z; p50 0.263). Not met, M7's cause. Three more runs refused themselves (04:42Z,
  04:46Z and 05:05Z): the node's pool of network namespaces ran dry for 89, 67 and 85 of their 2,000 creates, over
  the 1% the harness allows, the 5-minute load 166 at the first.
- **The calls, traced** (`MOUNTS`, 1,000 lifecycles each; 20261102T025948Z before, 025959Z the change): 150 calls
  per create to 43, the span's p50 251 to 173 us at one in flight and 299 to 206 at eight; the classic path's, from its
  first mount, 36 to 34 at d236217.
- **Validation**: runtime-tools bd3b0bd, 26 pass, 32 fail, 0 skip at bf77298 and at f3c6c09, the same test by test
  (`~/.cache/hex-t7/g5x-s3/fjob/valid-before`, `valid-tip`).
- **The root lab at d236217** (adacachy, as root, `docker_lab.sh`): `oci_box` (the mount kinds on both paths; E0-E3
  contained; the classic reasons counted, none under `Rootless`), `oci_spec_box` (AC-f: the seed lists its one
  read-only tmpfs) and `node_socket` pass; rootless, `oci_cgroupfs_box` and `oci_box`.
