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


## 6. Fresh `/dev` mounts made ahead (2026-11-02)

The bounded pool addresses the p99 records in §5. Keep `setsid` where it is; measure the whole
create as well as part 19, so moving work between the cuts cannot count as an improvement.

A worker makes a fresh detached tmpfs with exactly the spec's source, flags and data, its six default
device nodes and five default links, before a create takes it. `fsmount(2)` returns an `O_PATH` dirfd
usable by the `*at` calls, including making files before attachment, and an unconsumed mount dies with
its last descriptor ([Linux man-pages, fsmount](https://man7.org/linux/man-pages/man2/fsmount.2.html),
description and example; Linux v6.19 `fs/namespace.c`, `fsmount`). The rootless/classic paths continue
to make their own. No filesystem is shared between containers: a take moves ownership out, and no
used mount ever goes back. This takes tmpfs's superblock creation, six mknods, five symlinks and the
umask changes off the create's critical path. It does not remove this work from the machine.

The pool key is the exact decoded mount operation (including source and data, so mode, size, uid/gid
and labels never bleed between specs). Tmpfs gives each instance its own limits; `size` and
`nr_inodes` are limits, not allocations ([kernel tmpfs documentation](https://docs.kernel.org/filesystems/tmpfs.html)).
Keep four keys, LRU-evicting the least recently requested, eight mounts per key (the acceptance test's
eight creates in flight): at most 32 fresh mounts and one refill worker. A key longer than 2 KiB or a
spec with its own devices, duplicate `/dev`, a read-only `/dev`, an explicit `nr_inodes` quota, or no
writable tmpfs `/dev` uses ordinary setup. Early defaults could otherwise change which operation
exhausts the inode quota. These cases and descriptor capacity are counted by fixed eligibility reason,
without retaining a pool key. A cold/dry pool makes the mount on the path and wakes refill. Failed preparation is counted and
stops refill for that key until another request, as decision 4's network pool does. Shutdown closes the
held mounts and joins the one worker. Use explicit chmod for prepared node modes; never change the
multi-threaded agent's umask. Preparation allocates metadata only, no device data, and keeps no host
mountpoint or temporary path. Pool metadata belongs to the agent's cgroup; workload allocations belong
to the container's cgroup as before.

Mount ordering needs a guard: the ordinary init creates defaults after the spec's mounts, and a later
mount can name one of those defaults or reach it through an image symlink. The init only attaches a
prepared mount at a `/dev` reached without symlinks. Before each later mount, a lexical default-name
check and `openat2(RESOLVE_IN_ROOT | RESOLVE_NO_MAGICLINKS | RESOLVE_NO_SYMLINKS)` check whether its target
could depend on the early defaults. Missing ordinary components are safe: the ordinary mount creates
them. If the target could depend on a default, remove the eleven pristine defaults through the owned
`/dev` fd before doing that mount, and finish with today's `make_dev`. Previous guarded mounts touched
none of those entries. The fallback keeps each refusal at its original operation and name. Custom
devices use the original path from the outset, preserving their order before default links. Tests
must compare pooled and ordinary success/refusal behavior on those hostile layouts.

The seed's version-2 CREATE bytes stay byte for byte. A new kind appends the prepared mount's index and
one fd after all existing descriptors; old readers refuse the unknown kind. The total stays within
SCM_MAX_FD (253); at that limit the optional prepared mount is omitted. No new unsafe is needed.

Falsifiers: a mount option/source or device mode/ownership differs; one container can see another's
`/dev` mutation; a hostile target's refusal changes; an evicted/in-flight mount is kept or reused; a
failed maker spins; shutdown leaks a descriptor; or fedora's whole-create p99 does not improve alongside
part 19 at eight in flight with 8,000 idle host threads. Record every result, including misses and
refill failures, and rerun mount kinds, cgroupfs, escape, root-side Rootless=0, validation, HEX-RT and
the seven gates at the final tree. No p99 success is claimed from a harness whose pass bit only checks
the root switch.


## 7. Complete coverage and the setup scheduling experiment (2026-11-02)

The first pool on/off/before sequence (`hex-lead-s3-devfs-measure-20261102T0726Z`, fedora,
`lead-devfs-20261102T0719Z`, eight creates in flight, 2,000 creates, 8,000 idle threads, CPUs 4-15)
refused all three runs: the setns-only reader missed 50/103/46 network-pool misses respectively.
Raw observations, **incomplete coverage, not acceptance evidence**:

Run identifiers are `create-breakdown-20261102T072230Z` (off), `072235Z` (on), `072238Z`
(before). The draft release hashes (first 16 SHA256 digits) are hex `6256a152352b145b`, seam
`7b2a3c5cbbc1111a`; before hex `dd2016abe247a439`, seam `5ccc3c8a3792813d`. The before runtime
is bf77298, with only patch 11's throughput timing backported to its bench. Focal's `s5-focal*`
campaign was a concurrent host tenant; CPU affinity remained 4-15.

| configuration | part 19 p50 / p99, ms | whole create p50 / p99, ms |
|---|---|---|
| draft, pool off | 0.148 / 1.842 | 1.627 / 8.175 |
| same draft, pool on | 0.098 / 1.173 | 1.773 / 8.596 |
| bf77298, timing-only bench backport | 0.226 / 1.906 | 1.770 / 7.726 |

Pool on took 1,997 prepared mounts, made three on path, and had zero preparation failures. It reduced
the reported part but did not improve whole-create p99. No latency criterion is met by these records.
The corrected light reader adds each child's `clone3` return zero as the dry-network boundary, with
successful network setns preferred. It includes every create and on-path loopback setup, requires
whole samples and exact agreement with the network miss counter, and keeps the 0.5 ms bar.

### Complete-coverage records, before propagation repair

With reader 13 and fixed reasons 14, the tracked source diff SHA256 was `cdfbefaca0371ab3`;
release hex `413a34ee2e5be497`, seam `fbb6d7d00e4d6884` (first 16 SHA256 digits). Before binaries
are as above. Same fedora 6.19.10, concurrency eight, 2,000 creates and 8,000 idle host threads,
CPUs 4-15; Focal's campaign remained beside each run. Run names below all start
`create-breakdown-20261102T` and end `Z`.

| run | configuration | part 19 p50 / p99, ms | whole create p50 / p99, ms | setns / clone / expected dry |
|---|---|---|---|---|
| 075838 | off 1 | 0.155 / 1.546 | 1.886 / 14.237 | 1995 / 5 / 5 |
| 075842 | on 1 | 0.108 / 1.269 | 2.133 / 15.152 | 1999 / 1 / 1 |
| 075847 | before 1 | 0.244 / 2.425 | 2.504 / 8.753 | 1998 / 2 / 2 |
| 075851 | off 2 | 0.168 / 1.883 | 2.324 / 9.552 | 1998 / 2 / 2 |
| 075855 | on 2 | 0.096 / 1.369 | 1.797 / 7.443 | 1974 / 26 / 26 |
| 075900 | before 2 | 0.236 / 1.769 | 2.124 / 8.048 | 1982 / 18 / 18 |
| 075915 | off 3 | 0.148 / 1.380 | 1.817 / 6.431 | 1983 / 17 / 17 |
| 075940 | on 3 | 0.089 / 1.166 | 1.576 / 6.236 | 1970 / 30 / 30 |
| 080003 | before 3 | 0.204 / 1.496 | 1.634 / 6.375 | 1975 / 25 / 25 |

Each has 2,000/2,000 mount spans and whole-create samples, exact fallback-counter agreement,
zero lost events and zero failed switches. On took 1,996 / 1,996 / 1,993 prepared mounts, with
4 / 4 / 7 on-path constructions. Mount p99 exceeds 0.5 ms in all nine runs. Whole p99 is worse
with pooling in round 1 and better in rounds 2 and 3; no consistent whole-create tail gain or
acceptance is established. Percentiles above are each run's, not averages or a pooled percentile.
The propagation repair adds required kernel work; these runs do not validate its final performance.

The independent shared-source regression then failed the same production draft as required:
`hex-lead-s3-propred-20261102T080845Z`, `lead-propred-20261102T080845Z/lab.log`, fedora root lab,
build exit 0, test exit 101. Both preconditions held (root source shared:3072, bind source shared:3073).
The plain pruned create added 17 mounts in the agent's namespace, including proc, devices,
submounts, file masks and readonly binds, and mounts at the bind source's own path. The oracle
failed on `plain: agent gained mounts during create`. This is a correctness failure, explained
and repaired before further scheduling experiments (`goal5-root.md` §9).

**Candidate C: a shorter fair-class setup slice.** Linux v6.19 `__setparam_fair` accepts a nonzero
`sched_attr.sched_runtime`, clamps it to 100 us through 100 ms, and marks it custom. A zero request
clears that mark and restores `sysctl_sched_base_slice`
([fair.c](https://github.com/torvalds/linux/blob/v6.19/kernel/sched/fair.c), `__setparam_fair`).
The candidate is the kernel's minimum 100 us for init setup, retaining policy, nice and affinity;
that constant is a cited API bound, not a measured optimum. The kernel's
[EEVDF description](https://docs.kernel.org/scheduler/sched-eevdf.html) explains earlier virtual
deadlines for shorter requests. `check_preempt_wakeup_fair` can override slice protection for an
eligible shorter-slice task; cgroup scheduling and eligibility still apply. This supports an experiment,
not a wall-time guarantee. Preserve all scheduling attributes, avoid `SCHED_FLAG_KEEP_PARAMS` when
changing runtime (it replaces supplied values), and clear the custom runtime before capabilities,
seccomp, the user change and workload exec. `sched_getattr` returns the effective fair slice
([syscalls.c](https://github.com/torvalds/linux/blob/v6.19/kernel/sched/syscalls.c), `get_params`,
`sys_sched_setattr`). Detect unsupported behavior by the API, not a version string. Any raw wrapper
belongs in sys.rs with SAFETY comments and its own §12 review commit held for Ada.

Before enabling a setup hint, tests must show a custom setup slice and a default slice in the
workload, including an exec inside a box; unsupported or refused hints retain ordinary scheduling
and are counted by a fixed reason. Compare whole-create and part-19 p50/p99 with the same complete
coverage and host load. A worse whole p99, user-visible scheduling change, or unchanged runnable
wait falsifies the candidate. No scheduling hint has been implemented or measured in this note.

The root propagation oracle also needs a stated input: MS_SLAVE on a private mount leaves it private;
a singleton shared peer group also becomes private. A master requires another shared peer
([mount(2)](https://man7.org/linux/man-pages/man2/mount.2.html), propagation types;
[kernel Shared Subtrees §5a](https://docs.kernel.org/filesystems/sharedsubtree.html)). The failed
root run reported a private btrfs input and no master, before the pool oracle. Establish its baseline
and a controlled shared/private source before changing an assertion. Detached open_tree clones resume
propagation when attached ([open_tree(2), Notes](https://man7.org/linux/man-pages/man2/open_tree.2.html));
a shared fixture must also verify that filling the container root cannot propagate into its source.

## 8. The tail's cause, reuse, and `setsid` first (2026-11-03)

All runs: fedora (`kiln-a094a12d`, i9-14900HX, Linux 6.19.10), the AC-e harness at 8 in flight with 8,000 idle host
threads, CPUs 4-15, 2,000 creates a run. Each run's directory is `~/.cache/hex-t7/g5x-s3/<unit's dir>`, and its
records are `~/.cache/hex-bench/out/create-breakdown-<run>`.

- **The cause.** A clean per-call trace at 16e52a9 (unit hex-lead-s3-trace2-20261103T064732Z, load 11, no other unit)
  puts about half of the slowest 1% (a mean span of about 1,965 us) in the init runnable while its CPU ran the
  kernel's teardown of earlier boxes. Per slow create that was `css_killed_work_fn` 389-532 us,
  `css_release_work_fn` 348-437, `cgroup_bpf_release` 44 and the RCU softirq 45-64: cgroup destruction, the work a
  box's delete queues on the CPUs its creates run on.
- **Step 6's reuse takes it.** The test merge is `exp/s6-x-s3-acE` d6ea10b: step 6 e9f58a0 with 16e52a9. Its
  release binaries are hex f469b70f304691c9, seam_oci d70453f125b209c5. AC-e's bundles name the owner bundles of
  `oci_inside.sh`: Docker's default capabilities and a read-only `/proc/sys`. Run hex-lead-s3-reuseab2-20261103T163227Z
  had HEXBENCH_OWNER set and interleaved reuse on and HEXBENCH_REUSE=0 over 4 rounds, at fedora load 1-2.
  - Reuse on handed on 1,992 of 2,000 cgroups and 1,983-1,984 network namespaces. Part 19's p99 was
    0.316/0.322/0.329/0.432 ms, and the create's p50/p99 0.51-0.53/1.11-1.49 ms.
  - Reuse off: part 19 1.22-1.42 ms; create 1.18-1.25/7.9-11.1 ms.
  - The reuse-on trace's slowest 1% holds no cgroup teardown.
  - A first try, hex-lead-s3-reuseab-20261103T162841Z, measured no reuse. Its bundles named no capabilities, so the
    node refused every box (`privileged`, `could change its namespace`).
- **Under another tenant's load it rose again.** Run hex-lead-s3-underload-20261103T163610Z used the same binaries
  with reuse on, beside L busy loops pinned to 4-15 at nice 0 and started and killed by PID. Part 19's p99:

  | L | 0 | 6 | 12 | 24 |
  |---|---|---|---|---|
  | p99, ms | 0.321, 0.324 | 0.769, 0.705 | 0.819, 0.941 | 1.182, 1.620 |

  Its trace (hex-lead-s3-loadtrace-20261103T164213Z) puts the slowest 1% in `setsid`. At 6 loops that was
  1,183 us of a 2,003 us mean span, 1,099 of it preempted, the CPU going first to the busy loop (1,024 us). At 12
  loops it was 896 us of 1,623, with `move_mount` blocked on the mount lock for 386.
- **Why `setsid` gives the CPU away.** At v6.19 every `setsid` moves the caller to a new autogroup:
  - `sched_autogroup_create_attach` calls `autogroup_move_group`, which calls `sched_move_task` for every thread
    ("We can't avoid sched_move_task() after we changed signal->autogroup", `kernel/sched/autogroup.c:177-188`);
  - `sched_move_task` reschedules a running task whatever its group (`kernel/sched/core.c:9123-9128`);
  - `noautogroup` only clears the flag (`autogroup.c:225`) that `autogroup_task_group` alone reads
    (`kernel/sched/autogroup.h:33-36`), so no setting avoids it.

  Its allocations are `GFP_KERNEL` without `__GFP_ACCOUNT` (`autogroup.c:89`; `fair.c`'s `alloc_fair_sched_group`,
  `core.c`'s `sched_create_group`), so none is charged to a box's memory cgroup.
- **`setsid` first.** Run hex-lead-s3-setsidload-20261103T164725Z, reuse on, interleaved, compared s0 (d6ea10b)
  against s1: d6ea10b with `setsid` at the start of the pruned clone child (runc's order). s1's binaries are hex
  d2c75d29b7b6aebd and seam_oci 3e5e6977d9939582. Part 19's p99 and the create's p99, in ms, against L:

  | L | part 19, s0 | part 19, s1 | create, s0 | create, s1 |
  |---|---|---|---|---|
  | 0 | 0.330, 0.335 | 0.291, 0.252 | 1.283, 1.332 | 1.491, 1.303 |
  | 6 | 0.934, 0.975 | 0.547, 0.539 | 2.980, 3.561 | 2.912, 2.764 |
  | 12 | 0.974, 0.890 | 0.693, 0.608 | 7.079, 4.776 | 4.320, 4.520 |

  The create's p50 is unchanged (within 0.02 ms at each L).
  - On a quiet host, moving `setsid` only moved work across part 19's cut: hex-lead-s3-setsid-20261103T062234Z,
    without reuse, left the create's p50 and p99 unchanged, so it wasn't taken then.
  - Under load the forced pass is cheaper taken first. The init has just been placed by its clone and has run
    little of its share, so the scheduler gives it the CPU back. The create's p99 falls with part 19's.
  - So the init leads its session first, on every path (`init.rs`, `lead_session`).
- **What's left, and what would falsify it.**
  - Under 6-12 loops part 19's p99 is still 0.54-0.69 ms, over the bar.
  - The next levers are the remaining passes through the scheduler: `move_mount` and `mount_setattr` waiting on
    `namespace_sem`, and the proc superblock's allocation locks.
  - A repeat of the load runs in which `setsid` first doesn't lower part 19's p99, or raises the create's, falsifies
    the change. So would a box whose init doesn't lead its own session and process group (`oci_box.rs`,
    `created_through_a_seed_in_its_cgroup_and_namespaces`).
  - The AC-e reader's fallback check can false-fail on the merge: it reads a namespace the node made on path for an
    owner (joined by `setns`) as the clone's. The integration merge must read step 6's counters.
- **The merged tree with step 6's levers, alone on fedora** (unit hex-lead-s3-mr12clean-20261103T184941Z, load 5-6,
  reuse on, owner bundles). mr12 is x-s3 ef1a7a7 (step 6 + step 3 + the FIFO fix + `setsid` first) with step 6's later
  levers (the checker's CPU, the kept socket, the record shards); its release binaries are hex ff5cef383a844f85 and
  seam_oci a89ecea7de4053f4. Part 19's p99 in ms, three runs a level:

  | L | 0 | 6 | 12 |
  |---|---|---|---|
  | p99, ms | 0.338, 0.295, 0.303 | 0.498, 0.580, 0.454 | 0.702, 1.068, 0.743 |

  The create's p50/p99 was 0.49-0.51/1.14-1.22 ms at L = 0, 0.81-0.92/2.7-13.7 at 6 and 1.79-1.97/4.4-20.9 at 12.
- **Where criterion 3 stands** (`bench/competitive/probes/create_breakdown.md`, the last section, has every run):
  - As written (8 in flight, 8,000 idle threads, no other tenant's load), met on the merged trees: 10 runs of 10
    under 0.5 ms, 0.280-0.432 (d6ea10b, mr12, and x-s3 with the reader's fix, d3189f7, in §10's A/B, whose runs
    pass outright).
  - Not met on step 3's tree alone: 11 runs of 11 over, 0.729-1.568 (16e52a9, `setsid` first, reuse off). Its tail is
    the cgroup teardown that step 6's reuse removes, a cause outside this step's code.
  - Under another tenant's load, not met: at 6 busy loops 2 runs of 3 under (0.454-0.580), at 12 none (0.702-1.068).
    What's left is the mount lock's convoys and the proc superblock.


## 10. Masks and read-only paths bound through the init's own proc: tried, and falsified (2026-11-03)

§9 is step 6's, on its branch (`cp/t7-ext-s3-devfs-reuse`: a box's `/dev` handed on within one owner, measured and
rejected).

- **The lever, L1.** For the bench's owner spec the pruned init took `namespace_sem` for write 7 times a create:
  - `move_mount` ×5: the rootfs on `/`, `/usr`, `/proc`, `/dev` and `/proc/sys`;
  - `open_tree` ×1, `/proc/sys`'s clone;
  - `/usr`'s propagation, a `mount_setattr` with a propagation (`fs/namespace.c:4770-4773`).

  A `mount(2)` bind takes the lock once for its clone and its graft. In `do_loopback` (`:2972-3006`) `LOCK_MOUNT`
  (`:2987`, `:2799-2802`) takes it in `do_lock_mount` (`:2747`) and holds it to the function's end, with the clone
  (`:2994`) and the graft (`:2998`) under it. A detached clone takes it in `get_detached_copy` (`:3017`), and its
  `move_mount` again (`:3517`).

  The pruned namespace has no proc until the switch, but by its masks the init has made the spec's. L1 kept that
  proc's `fsmount` descriptor and entered it. It then bound each read-only path and each file's mask with one
  `mount(2)` through `self/fd/N`. A spec with no proc, or a `nosymfollow` one, kept the clone and the move.
  - The bench's spec went from 7 holds to 6.
  - Docker's default paths (moby docker-v29.9.2 `daemon/pkg/oci/defaults.go:110-116`, `:194-207`) went from 23 to 13
    for the masks and read-only paths alone. On fedora 5 of those masks are files, 3 are directories and 2 are absent.
  - Branch `cp/t7-ext-s3-l1-falsified` (bbf9870) holds it, and `cp/t7-ext-s3-xs3-l1` (072ecfc) on x-s3.
- **The A/B** (unit hex-s3try2-timed-20261103T194825Z, fedora, the 1-minute load 3.5 to 12.6, other tenants'
  containers beside it, every run's list in its `context.txt`).
  - Binaries: A is x-s3 with the reader fix and Docker's paths, d3189f7 (hex 2e1647a89ae4a566, seam_oci
    6cd995f3a10dbbbf). B is A with L1, 072ecfc (hex 16db168576b814df, seam_oci bec241455d7bfe44). Both come from unit
    hex-s3try2-build-20261103T194300Z.
  - Method: AC-e at 8 in flight with 8,000 idle threads on CPUs 4-15, owner bundles with reuse on, A and B
    interleaved over 3 rounds, the order alternating, at 0, 6 and 12 busy loops; then the same with Docker's paths.
  - Each cell gives part 19's p99 by round (its p50 range), then the whole create's p99 by round, in ms. The runs are
    `create-breakdown-20261103T194827Z` to `195112Z`, in the unit's order (its `exits`).

  | spec, L | A: part 19 p99 (p50) | B: part 19 p99 (p50) | A: create p99 | B: create p99 |
  |---|---|---|---|---|
  | owner, 0 | 0.280, 0.331, 0.381 (0.071-0.076) | 0.329, 0.316, 0.336 (0.081-0.085) | 1.527, 1.960, 2.307 | 1.708, 1.551, 2.146 |
  | owner, 6 | 0.544, 0.477, 0.521 (0.075-0.082) | 0.503, 0.530, 0.681 (0.088-0.094) | 10.181, 3.381, 11.207 | 4.896, 4.777, 4.076 |
  | owner, 12 | 1.737, 1.099, 1.233 (0.080-0.085) | 1.988, 0.744, 0.948 (0.094-0.095) | 12.223, 13.893, 18.716 | 21.259, 5.930, 21.864 |
  | Docker's, 0 | 0.842, 0.669, 0.662 (0.213-0.221) | 0.787, 0.625, 0.747 (0.236-0.238) | 1.846, 1.582, 1.675 | 1.800, 1.869, 1.800 |
  | Docker's, 6 | 1.253, 1.081, 1.835 (0.231-0.237) | 1.306, 2.107, 2.026 (0.271-0.272) | 11.349, 5.888, 4.670 | 7.774, 4.187, 4.261 |
  | Docker's, 12 | 1.519, 2.313, 2.106 (0.228-0.234) | 2.072, 1.606, 1.766 (0.258-0.265) | 12.590, 20.978, 5.871 | 7.105, 26.071, 5.681 |

  The reader matched its fallback count in all 36 runs (the node-made namespaces counted apart, ef04f50). The 6 runs
  at 0 loops on the owner spec passed outright, the merged tree's criterion 3 as written.
- **Read: falsified.**
  - Part 19's p50 was higher with L1 in every round: 0.081-0.095 ms against 0.071-0.085 on the owner spec, and
    0.236-0.272 against 0.213-0.237 with Docker's paths.
  - Its p99 under load wasn't lower. At 6 loops B read 0.503-0.681 against A's 0.477-0.544, and with Docker's paths
    1.306-2.107 against 1.081-1.835. At 12 loops the rounds cross.
  - The whole create's p99 is ruled by multi-millisecond outliers in both, crossing too.
  - That meets the falsifier L1's own note stated (bbf9870): part 19's p99 under load not lower in an interleaved
    A/B. L1 isn't taken, and `cp/t7-ext-s3` goes on from 4315e75 without it.
- **The holds, counted from the trace.** The mounts trace's `namespace_sem` probes (fe03840, on x-s3's reader branch)
  ran in unit hex-s3try2-final-20261103T195758Z: 1,000 creates at 8 in flight, owner bundles, reuse on, A's and B's
  binaries, none lost.

  | spec | A: write holds per create, by call | B (L1) |
  |---|---|---|
  | owner (create-breakdown-20261103T200508Z, 200511Z) | 7.0: move_mount 5.0, mount_setattr 1.0, open_tree 1.0 | 6.0: move_mount 4.0, mount_setattr 1.0, mount (bind) 1.0 |
  | Docker's (200513Z, 200517Z) | 28.0: move_mount 17.0, open_tree 10.0, mount_setattr 1.0 | 18.0: mount (bind) 10.0, move_mount 7.0, mount_setattr 1.0 |

  These are the trace's means per create, to one decimal; no create held it shared. So the drops L1 claimed, 7 to 6
  and, for Docker's masks and read-only paths, 23 to 13, are real, and still part 19 got no faster. Two causes:
  - What each bind cost rose more than the hold it saved. Each `self/fd/N` resolves in the box's fresh proc, whose
    `/proc/<pid>`, its `fd` directory and each `fd/N` entry are made on their first lookup, an inode each
    (`proc_pid_lookup`, `fs/proc/base.c:3501`, `proc_pid_instantiate` `:3481`; `proc_lookupfd_common`,
    `fs/proc/fd.c:226`, `proc_fd_instantiate` `:202`; both through `proc_pid_make_inode`, `base.c:1944`), and `self`
    allocates its link's text each time (`fs/proc/self.c:11-25`).
  - At these loads one hold more or less isn't what the tail waits on. In the owner spec's slowest 1% the init was
    first preempted by the node's network-namespace checker thread (`hex-netns-check`, 127 us per create), the
    kworkers and the RCU softirq (200508Z's report).
- **Docker's default paths cost more than the rest of part 19.** In A's trace with them (200513Z, every create, mean)
  part 19 made 119 calls, against 27 for the owner spec, and its traced span's p50 was 251 us against 85:
  - 22 `openat2`, the targets' lookups in the fresh proc, 53.2 us;
  - 4 superblocks made (`fsconfig` create), proc's and the 3 masked directories' empty tmpfs, 44.1 us;
  - 17 `move_mount`, 42.6 us, and 10 `open_tree` clones, 36.0 us.

  Their p99 with no other load, 0.662-0.842 ms in the A/B, is over AC-e's bar. A lever named for them, not built:
  the masked directories' empty read-only tmpfs made ahead, as the `/dev` pool makes `/dev` (§6). That takes 3
  superblocks per create off the path. Each target's lookup in the box's own proc, and a clone and an attach per mask
  and read-only path, are per box.
- **What's left of part 19's holds, and why each stays on 6.19.** Each of the 7 write holds per create is the attach of
  a mount the spec asks for, or the propagation change that must precede anything mounted beneath a cloned bind (the
  leak fixed in e2dd76c).
  - L2, the agent attaching the binds onto the detached rootfs before handing it over (v6.19 `do_move_mount` takes an
    anonymous namespace's mount as a target where `may_use_mount` lets it, `:3487-3501`, `:3549`), saves no hold. The
    agent would have to make each bind's clone private before attaching anything beneath it, as the init does now,
    so the holds would only move into the agent's part of the same create.
  - `open_tree_attr` (Linux 6.15) clones and sets attributes in one call but takes the lock twice
    (`vfs_open_tree` at `:5017`, then `do_mount_setattr` at `:5033`).
  - Skipping `/usr`'s propagation change where its source is already private would need the source's propagation
    first. `statmount` reads it under the lock, shared.
  - The structural fix is goal 6's: a mount namespace made from a detached tree in one call (`OPEN_TREE_NAMESPACE`,
    Linux 7.0; `CLONE_EMPTY_MNTNS`, 7.1; `goal5-root.md` f83, f84). Fedora runs 6.19.

## 11. What's left of part 19 under load, hold by hold and superblock by superblock (2026-11-03)

**Ada's ruling (2026-11-03).** Step 3 closes on its written bars. Her under-load bar is now a written bar of runtime
design goal 6, the mount tree built off the lock on Linux 7.x: part 19's p99 at most 0.5 ms at 8 in flight with 8,000 idle
threads, with 6 and 12 busy loops on the same cores and Docker's paths, the whole create's p99 improving with it, and no
priority, nice, RT or slice change. Today's figures beside it, on x-s3 (owner bundles, reuse on), none of them under the bar:

| part 19's p99, ms | 6 loops | 12 loops | run |
|---|---|---|---|
| Docker's paths, the bar's condition | 1.081-1.835 | 1.519-2.313 | §10's A column, unit hex-s3try2-timed-20261103T194825Z |
| Docker's paths, a busier host | 1.233-3.822, median 1.982 | 1.226-10.383, median 2.192 | §13's stock-off arm |
| The owner spec, no Docker's paths | 0.454-0.580 | 0.702-1.068 | §8, mr12 |

What follows is why 6.19 leaves them there.

For the bench's owner spec (Docker's capabilities, `/proc/sys` read-only) on the merged tree, with step 6's reuse
on. The trace with the namespace_sem probes (create-breakdown-20261103T200508Z, unit hex-s3try2-final-20261103T195758Z,
1,000 creates at 8 in flight, A = x-s3 d3189f7) gives the init's part 19 per create, every create: 27 calls, a mean
span of 110 us, of which 66 is in calls and 44 between them.

**The mount lock, write holds per create.** The init takes 7:

| # | call | what it does | can 6.19 avoid it? |
|---|---|---|---|
| 1 | `move_mount` | the rootfs's tree attached on `/` | No. A mount namespace's root is attached by one move; making a namespace from a tree in one call is `OPEN_TREE_NAMESPACE` (Linux 7.0) or `CLONE_EMPTY_MNTNS` (7.1), goal 6's. |
| 2 | `mount_setattr`, a propagation | `/usr`'s clone made private before anything can mount beneath it (e2dd76c's leak) | Only where the source is already private, which only `statmount` can tell, under the same lock (shared), at a create. |
| 3 | `move_mount` | `/usr` attached | No: it is the spec's mount. |
| 4 | `move_mount` | `/proc` attached | No. |
| 5 | `move_mount` | `/dev` attached, the pool's (§6) | No. Attaching it in the agent would only move the hold, and step 6's `/dev` handed on within one owner (§9 on its branch) was rejected because its clone adds one. |
| 6 | `open_tree`, a clone | `/proc/sys`'s clone, the read-only path | Only by `mount(2)` binding it through the box's own proc, one hold for clone and graft: L1, falsified (§10), since each `self/fd/N` walk costs more than the hold it saves. |
| 7 | `move_mount` | `/proc/sys`'s clone attached | As 6. |

Around the init's span the same lifecycle takes the lock 5 more times for write, by the code (the trace's probes count
the init's span alone). They lie outside part 19 but contend with it at 8 in flight:
- the agent's `open_tree` of the rootfs and of `/usr`, and the rootfs's propagation change (`run.rs`,
  `pruned_create`; `fs/namespace.c:3017`, `:4770-4773`);
- the seed's `clone3` copying its namespace (`copy_mnt_ns`, its guard at `:4117`);
- the box's namespace torn down at delete (`put_mnt_ns`, `:6031-6039`).

That is 12 a lifecycle. L2 and the hand-on only move them, and none of the 12 is a check or a read that could be
dropped.

**Superblocks made per create on the path.** One: the box's proc (`fsconfig` create, 18.1 us per create in 200508Z,
the largest single call). It is made for the init's own pid namespace: `proc_init_fs_context` takes the caller's active
pid namespace (v6.19 `fs/proc/root.c:338`), and `proc_get_tree` makes a superblock per mount (`get_tree_nodev`,
`:310-312`).
  - It can't be made ahead: the namespace it serves is born in the init's clone.
  - At v6.19 proc takes a `pidns` parameter (`:48`, checked at `:114-162`: `CAP_SYS_ADMIN` over the target's user
    namespace, and a descendant pid namespace). So the seed could make it beside the init once its clone returns.
    That overlaps the work, which a loaded host's CPUs don't give; it doesn't remove it. `/dev`'s tmpfs is the pool's, made ahead (§6). The init's `CLONE_NEWIPC` makes the
box's mqueue superblock in the seed's clone, before part 19 (plan step 4's ipc/uts pool takes it off the path).

**What the tail waits on under load.** At 12 busy loops the slowest 1% spent 790 us in `move_mount` blocked on the
lock, behind a holder the loops preempted (hex-lead-s3-loadtrace-s1-20261103T174130Z). With reuse on, the inits' CPU
went first to the node's own threads, the network-namespace checker most (127 us per create, 200508Z's slowest 1%),
then the kworkers and the RCU softirq. On 6.19 the levers left are the node's own CPU per create (step 6's checker
work) and, for specs with masks, the masked directories' tmpfs made ahead (§10). The structural fix for the lock is
goal 6's namespace from a tree.

## 12. The classic init's root at the bar: the `/dev` made ahead through the shim, and its filesystems made as the pruned init makes them (2026-11-03)

Run 4's part 19 was the classic init's, and the classic path measured beside the pruned one through the shim
(`CLASSIC=1`, part 23) read 0.177 ms pooled p50 at 4315e75 (create-breakdown-20261103T200313Z), over the 0.16 bar.
- **Where it went.** Its trace at 4315e75 (create-breakdown-20261103T202046Z, one create at a time, from the init's
  first run to its `pivot_root`; 38 calls and 26.9 page faults per create, every create) put, per create:
  - the two filesystems' `mount(2)`, 103.1 us: proc's and `/dev`'s superblocks, each named by its `/proc/self/fd` link;
  - `setsid`, 31.8 us;
  - the 6 default nodes and 5 links, 37.0 us;
  - the 2 binds, 24.8 us, and the cgroupfs check of the read-only `/usr` bind (`statx`, `statmount`, `listmount`),
    10.0 us;
  - the init's own code between its calls, 115.8 us, its copy-on-write page faults as the shim's fork-like child among
    it.

  The mount lock: 5 write holds (the binds 2, the filesystems 2, the propagation 1) and 2 shared (`statmount`,
  `listmount`).
- **Two cuts.**
  - e1b85f4: a create the node sends through the shim takes the pool's prepared `/dev` as the pruned path does (§6),
    by the same eligibility, each miss counted by reason (a user namespace of its own under the new
    `Unpooled::UserNamespace`). It is named to the shim by its descriptor's `/proc` path, and attached under the
    pruned path's guards. That takes `/dev`'s superblock, its 11 nodes and links and its umask changes off the path.
  - 933d56f: each filesystem the classic init makes is made by `make_fs` and attached to the target's descriptor by
    `move_mount`: the one hold `mount(2)` takes, and no `/proc/self/fd` walk (§10's cost). Binds keep `mount(2)`.
- **e1b85f4 measured** (unit hex-s3try2-shimdev-20261103T202418Z). `CLASSIC=1` breakdowns alternated 4315e75's binaries
  (hex e45fb83f1c7da66e) and e1b85f4's (hex 2582690c74ead2a9), each with the other beside it as `BEFORE_*`, CPUs 4-15.
  Parts 21 and 22, the same code in both, give each run's pace; fedora at rest runs short bursts slowly (memory notes,
  `probes/host_clock.sh`).

  | run | binary | part 21 pooled | part 23 by round | pooled |
  |---|---|---|---|---|
  | create-breakdown-20261103T202521Z | 4315e75 | 0.095 | 0.429 / 0.226 / 0.306 | 0.318 |
  | 202804Z | e1b85f4 | 0.063 | 0.161 / 0.258 / 0.279 | 0.199 |
  | 203045Z | e1b85f4 | 0.043 | 0.188 / 0.116 / 0.138 | 0.133 |
  | 203307Z | 4315e75 | 0.043 | 0.148 / 0.169 / 0.165 | 0.162 |

  At the host's full pace (part 21 at 0.043 in both) the classic root mounted fell from 0.162 to 0.133, 18%. Against
  part 21 round by round it is 2.9-3.2 times it with the prepared `/dev`, and 3.4-3.8 times it without. In the slow
  runs both binaries are over the bar.
- **The trace after** (create-breakdown-20261103T203623Z's mounts, e1b85f4, one create at a time): span p50 147 us,
  against 243. The calls fell from 237 to 90 us per create, and the init's code between them from 116 to 76.
  - One filesystem is made (proc, 29.1 us); `/dev`'s attach is a `move_mount` (1.7).
  - No node or link is made.
  - The lock is held 5 times for write (the binds 2, proc 1, the propagation 1, `/dev`'s attach 1).
- **The pruned path at e1b85f4** (create-breakdown-20261103T203517Z): part 19 0.058 / 0.061 / 0.044, pooled 0.055, as
  before. Validation, bf77298 against e1b85f4: 26 pass, 32 fail, 0 differ. 7 gates (241 passed) and the root lab 6/6,
  the prepared-`/dev` oracle among it on both paths.
- **933d56f measured, the final code** (unit hex-s3try2-fs-20261103T203937Z, the same method, 933d56f's hex
  2c47054d9da44a0f). Gates 7/7 (241 passed) and the root lab 6/6 at 4e4ca1b, whose code is 933d56f's. Validation,
  bf77298 against 933d56f: 26 pass, 32 fail, 0 differ.

  | run | binary | load | part 21 pooled | part 23 by round | pooled |
  |---|---|---|---|---|---|
  | create-breakdown-20261103T204327Z | 4315e75 | 44.1 | 0.059 | 0.186 / 0.199 / 0.193 | 0.191 |
  | 204604Z | 933d56f | 13.0 | 0.042 | 0.314 / 0.112 / 0.126 | 0.133 |
  | 204824Z | 933d56f | 6.8 | 0.042 | 0.135 / 0.145 / 0.120 | 0.132 |
  | 205038Z | 4315e75 | 8.8 | 0.045 | 0.161 / 0.189 / 0.159 | 0.168 |

  - Both of 933d56f's runs are under 0.16, and both of 4315e75's beside them are over it.
  - Its trace (205353Z) puts the span's p50 at 142 us, with 31 calls. The lock is held 5 times for write (the binds 2,
    proc's and `/dev`'s attaches 2, the propagation 1) and 2 shared.
  - Proc made by `make_fs` adds little beyond the prepared `/dev`: rounds against part 21 read 2.9-3.1 times it, as
    e1b85f4's did. It stands as the classic path's version of the pruned one's making, with no walk.
  - The pruned path's part 19 at 933d56f (205252Z) is 0.043 / 0.052 / 0.041, pooled 0.044.

## 13. The masked directories' tmpfs made ahead: built, measured, not landed (2026-11-03)

The lever §10 named, on the lead's order of 21:24Z: it lands if the A/B shows it helps, otherwise it stays on a side
branch with its numbers here. It stays on the side branch: `cp/t7-ext-s3-maskpool` 2ecace9, off d01556c. The tree measured
is x-s3 with it, `cp/t7-ext-s3-xs3-masks` 3dad265.

**What was built.** Each pruning create seed keeps up to 8 empty read-only tmpfs mounts queued on a unix stream socket, one
byte and one mount a message. It fills the queue between creates, one mount at a time and only while no request waits. Its
inits take one per masked directory and make their own if it's empty.
- A queue rather than a list handed over at the clone. The seed can't know how many directories a create masks (they lie in
  a proc and sysfs mounted after the clone, and on the kernel's config), so a list leaves some unused. An unused detached
  mount takes the lock to dissolve at its last close (v6.19 `dissolve_on_fput`, `fs/namespace.c:2288`, the lock at `:2307`),
  and a used one doesn't. `fsmount` takes no lock (`:4269` on), so the fill adds no holds. Queued, each message goes to one
  taker and what isn't taken waits for the next create.
- Each cloned init closes the fill end first, or one container's init could queue a mount for the next container's.
- The init's ready signal carries `taken,made`, so a dry or disabled stock shows, with no write of its own on the path.
- Fail first: `masked_directories_come_from_the_seeds_stock` failed at d539fad with counts alone (no mask of 20 boxes came
  from the seed's stock, 40 made; unit hex-s3try2-lab-20261103T213144Z), and passed at c76d445, which added the stock.

**The A/B.** One binary, 3dad265's (hex 355cda675683302a, seam_oci f5423d9770841202), the stock off (`HEXBENCH_MASKS=0`) and on,
one setting apart. Fedora (i9-14900HX, Linux 6.19.10), CPUs 4-15, AC-e's 8 in flight with 8,000 idle threads, owner bundles,
reuse on, 2,000 creates a run. Each pair ran within seconds, each round's order the last's reversed: units
hex-s3try2-masks2-20261103T214604Z and hex-s3try2-masks3-20261103T215640Z, whose last round didn't run (it was stopped at
22:14Z, its guard waiting on another agent's lab). The host was busy, load 12-31: about 70 other containers up, and step
6's gates on CPUs 16-31 beside most runs. So each cell gives medians off → on, then the median of on minus off over its
pairs, with how many of them read lower:

| spec, loops (pairs) | part 19 p50, ms | part 19 p99 | whole create p50 | whole create p99 |
|---|---|---|---|---|
| Owner's, 0 (22) | 0.085 → 0.082 (-0.003, 16) | 0.370 → 0.367 (-0.007, 11) | 0.692 → 0.707 (+0.015, 7) | 1.979 → 2.036 (+0.219, 9) |
| Owner's, 6 (6) | 0.093 → 0.089 (-0.003, 6) | 0.595 → 0.764 (+0.084, 2) | 1.132 → 1.129 (+0.005, 2) | 4.195 → 4.717 (+0.740, 3) |
| Owner's, 12 (6) | 0.095 → 0.091 (-0.005, 6) | 1.667 → 1.243 (-0.138, 4) | 2.246 → 2.228 (+0.039, 2) | 15.406 → 15.151 (-0.746, 3) |
| Docker's, 0 (17) | 0.240 → 0.204 (-0.035, 17) | 0.828 → 0.716 (-0.035, 11) | 0.871 → 0.845 (-0.013, 10) | 2.079 → 2.317 (+0.137, 7) |
| Docker's, 6 (17) | 0.258 → 0.219 (-0.040, 17) | 1.982 → 1.943 (+0.126, 6) | 1.373 → 1.387 (+0.032, 5) | 6.847 → 6.885 (+0.003, 8) |
| Docker's, 12 (17) | 0.251 → 0.215 (-0.036, 17) | 2.192 → 2.020 (-0.165, 11) | 2.329 → 2.336 (-0.003, 9) | 14.658 → 15.059 (-0.682, 10) |

With Docker's paths the stock served 99.9%, 99.5% and 99.1% of the masks at 0, 6 and 12 loops (101,928 taken and 72 made;
101,528 and 472; 101,037 and 963; off, 102,000 made in each). The owner spec masks nothing, so its queue sits full, and those
rows are the control.

**The trace** (1,000 creates at 8 in flight with Docker's paths, no idle threads, none lost; create-breakdown-20261103T215231Z
off, 215235Z on), every create's mean:

| | superblocks made in the init (`fsconfig` create) | calls | span p50 / p99, us | mount lock write holds |
|---|---|---|---|---|
| Off | 4.0, 49.3 us | 119.1 | 270 / 1,283 | 28.0: `move_mount` 17, `open_tree` 10, `mount_setattr` 1 |
| On | 1.0, 19.1 us | 104.1 | 232 / 945 | 28.0: the same |

**Read: it doesn't help the create, so it isn't landed.**
- Part 19's p50 falls about 15% with Docker's paths, lower in 17 pairs of 17 at every load. That is the 3 superblocks gone from
  the init's span, as named.
- Part 19's p99 doesn't fall past the host's noise: 11, 6 and 11 pairs of 17 read lower, the pair median changing sign. Its
  ranges overlap whole. The holds are unchanged, and §11 has the tail waiting on them.
- The whole create doesn't gain. Of the 0.035 ms saved at no load its p50 shows 0.013 in the pair median, at 6 loops it reads
  0.032 higher (lower in 5 pairs of 17), and at 12 no change. Its p99 is on neither side. The superblocks are still made, by the
  seed, on the same CPUs, so this is work moved across the cut, which §6's rule doesn't count.
- The control's first rounds (try2-masks2) read 2-3x over with the stock on and no masked directory, which no code on that
  path explains. With 22 pairs it's gone, 0.370 against 0.367, and no seed died or restarted in any run read. Part 19's p50
  there is 3-5 us lower with the stock on, in an arm doing slightly more; that has no explanation here, and nothing rests on it.

What would falsify this: a quiet host's A/B with the whole create's p50 and p99 lower by more than its pairs' noise. At no load
the saving can't pass 0.035 ms, 4% of that create. A spec masking many more directories would save more per create: a node's
bundles with `/sys` mounted mask 5 and one per CPU (`bundle::MASKED_PATHS`, `Place::masked`). That is unmeasured, and it would
need a higher bound than 8.
