# OCI runtimes (runc, crun, youki) and how container and process start gets fast

Research note for CP-T7.1 and AC-T7.1.3 (`docs/requirements.toml`). Written and sourced 2026-10-22; every
source below was opened and read that day (accessed 2026-10-22). This note decides nothing: decisions go
in `docs/design/` with these sources cited (CLAUDE.md §5). Hex's bar is at least 4-5x better than the best
competing implementation on throughput, latency (p50 through p99.9 and max) and resource use
(`docs/research/README.md`); here the competitor is crun, the fastest OCI runtime the sources show.

## Conventions

- **Kinds** (as in `docs/research/README.md`): `MEASURED-INDEPENDENT` (a third party measured it),
  `MEASURED-VENDOR` (the project measured itself, including kernel developers measuring their own patch),
  `CLAIMED` (no method), `DESIGN` (a fact from code or documentation). "(computed here)" marks a ratio or sum
  worked out in this note from cited numbers; it takes the kind of its inputs.
- **Citations** are `[OCI-n]` into the numbered References at the end; `p.N` is the printed page of a
  paper. Code is cited as `path:lines` inside a repository pinned to a commit (OCI-1 runc, OCI-2 crun,
  OCI-3 youki, OCI-5 Linux v7.2); the line numbers are those of the files read at that commit.
- **"Start" is not one thing.** This note's own class is *runtime invoked to workload's first
  instruction* (namespaces, cgroup, rootfs, `execve`; no image pull). Papers about VM-based sandboxes
  (Catalyzer, RunD, AFaaS) appear where their findings are about the host kernel and transfer to a
  namespace container; their absolute numbers are a different class and are never compared across.
- **Percentiles.** Every source below reports a mean with a standard deviation, a min and max, a CDF in a
  figure, or (rarely) a p99. **No source reports a p99.9.** Where a source gives a range it is quoted as
  a range, not as a percentile.
- **Kernel versions matter.** Most published primitive costs are from Linux 4.x and 5.x; the newest
  primitives below need Linux 7.0 or 7.1 while the runner Hex was last measured on runs 6.19
  ([OCI-4] `docs/design/runtime.md:30-31`).

## 1. Summary table: the numbers this note rests on

| # | Number | What it is, and the conditions | Kind | Source |
|---|---|---|---|---|
| S1 | crun 1.69 s vs runc 3.34 s (-49.4%) | 100 sequential containers running `/bin/true`; "on my machine"; hardware and versions not stated | MEASURED-VENDOR | [OCI-2] `README.md:44-53` |
| S2 | runc 6.89 s vs crun 3.86 s; max RSS 15,120 KB vs 3,752 KB; user+sys 6.76 s vs 2.40 s (computed here) | 100 sequential `run foo` of `/usr/bin/true`, GNU `time -v`; hardware not stated; 2020-08-03 | MEASURED-VENDOR | [OCI-45] |
| S3 | crun 47.3 ± 2.8 ms, youki 111.5 ± 11.6 ms, runc 224.6 ± 12.0 ms (ranges 42.4-56.2, 84.0-142.5, 190.5-255.4; the README prints youki's range with a "±" where "..." is meant); runc/crun 4.7x, youki/crun 2.4x (computed here) | hyperfine, `create && start && delete -f`, page cache dropped before every run, 10 warm-ups, >=100 runs; Ubuntu 22.04.4, Linux 6.5.0-35, 16 cores, 64 GB; crun 1.15, youki 0.3.3, runc 1.1.7 | MEASURED-VENDOR | [OCI-3] `README.md:39-80` |
| S4 | crun 153.5 ± 21.6 ms, youki 198.4 ± 52.1 ms, runc 352.3 ± 53.3 ms | same method, 2021-12-27; Ubuntu 21.04, Linux 5.11; crun 0.19, runc 1.0.0-rc93 | MEASURED-VENDOR | [OCI-46] |
| S5 | crun 0.28 s, youki 0.32 s, runc 0.34 s, runsc 0.62 s mean; crun min 0.13 / max 0.41 s; youki failed 36 of 1,000 | 1,000 serial creates of busybox `/usr/bin/true` through nerdctl + containerd 1.7.18 (so the stack is in the number); Hetzner CCX33, Debian 12, Linux 6.1.0-21; author: "NOT a scientific benchmark" | MEASURED-INDEPENDENT | [OCI-47] |
| S6 | runc 98 ms vs crun 52 ms | `docker run --rm alpine echo hello`, mean of 10; Intel N100, Debian 12, Linux 6.8 | MEASURED-INDEPENDENT (low rigor) | [OCI-48] |
| S7 | `crun run foo`: 159.2 ± 21.8 -> 28.9 ± 5.9 -> 5.6 ± 3.0 ms (last: user 1.0 ms, system 4.5 ms, range 4.2-26.8 ms) | hyperfine; Fedora 24 / Linux 4.5.5, then after libseccomp and kernel fixes, then with a seccomp-BPF cache; hardware not stated; 2022-11-16 | MEASURED-VENDOR | [OCI-44] |
| S8 | Up to 50% of runc start spent in `percpu_down_write` during cgroup PID writes; skipping cgroup assignment -27% launch time; seccomp bypass about -30 ms; reusing a netns about -50 ms; runc alone 44 ms (1-vCPU VM), over 60 ms (multi-vCPU) | runc in GCP c3 VMs, Ubuntu, Linux 6.8.0; arXiv preprint 2026-09-03 | MEASURED-INDEPENDENT | [OCI-42] p.11-12 |
| S9 | Hex today: `oci create`+`start` p50 14.4 ms, p90 15.3, p99 16.7, min 12.7, max 18.1 (N=200) | `/usr/bin/true`, fedora 32 cores NVMe, load about 4.0; Hex's oci code has no seccomp and no resource cgroup (§2.6); delete is not in the timed span | MEASURED-VENDOR | [OCI-4] `bench/hex_start/RESULT.md` |
| S10 | `unshare -n true` 47.7 ± 51.4 ms -> 1.5 ± 0.5 ms | Fedora 24 / 4.5.5 -> 5.19.15; one fix freed netfilter hooks with `call_rcu` ("-40% of the original time"); SELinux registers 6 netfilter hooks per netns | MEASURED-VENDOR | [OCI-44] |
| S11 | mqueue mount in a new mntns 16.8 -> 0.7 ms; `unshare -i true` 10.9 -> 0.1 ms; 1,000x `unshare(CLONE_NEWIPC)` 8.05 s -> 0.03 s | same series; the IPC fix moved a `synchronize_rcu()` to a work queue | MEASURED-VENDOR | [OCI-44] |
| S12 | Mount-namespace setup 0.343 ms -> 0.044 ms (7.8x); with many host mounts 0.917 ms -> 0.043 ms (21.5x) | `unshare(NEWNS)`+`pivot_root` vs `open_tree(OPEN_TREE_NAMESPACE)`; avg of 100; hardware, kernel and mount count not stated; 2026-05-07 | MEASURED-VENDOR | [OCI-23] |
| S13 | "about a 40% increase in throughput" for container creation with OPEN_TREE_NAMESPACE (secondary: 73k vs 109k "containers" per 60 s) | kernel 7.0 VFS pull request, 2026-02-06; the 73k/109k pair is Phoronix quoting a mailing-list message I could not open | MEASURED-VENDOR (CLAIMED for the pair) | [OCI-18]; [OCI-26] |
| S14 | `clone()` latency 1.45 ms -> about 418 ms worst case at 24-way concurrency, dominated by new network and IPC namespaces; sustained x24 for 1 h: 110 -> 45 functions/s | Ant Group production stack (gVisor-based), 24-core Xeon Platinum 8163, 512 GB, Linux 5.10 | MEASURED-VENDOR | [OCI-35] p.500, p.503 |
| S15 | Containers/s: about 200 (stock netns), about 400 (IPv6 off, broadcast code removed), 900 (no netns); cgroup reuse at least 2x creating; bind mount about 2x AUFS at scale | churning 5 namespaces concurrently; 8-core Xeon D-1548, Linux 4.13.0-37 | MEASURED-INDEPENDENT (Linux primitives) | [OCI-27] p.58-60 |
| S16 | cgroup pool + rename: -94% cgroup creation time (0.09 / 0.10 / 0.14 s at 1 / 50 / 200 threads); cause of the baseline: global `cgroup_mutex`, `css_set_lock`, `freezer_mutex` | 2,000 containers; 104-vCPU Xeon 8269CY, Linux 4.19.91 | MEASURED-VENDOR | [OCI-31] p.57, p.60 |
| S17 | fork 7,581.8 us vs vfork 31.9 us vs posix_spawn 44.9 us vs io_uring_spawn 28.6 us (1 GB allocated and written); with no extra memory 52.0 / 31.5 / 44.5 / 29.5 us | minimum of 2,000 runs; hardware not stated in the slides; 2022 | MEASURED-VENDOR (single author) | [OCI-34] |
| S18 | fork of a 1 GB parent 6.5 ms avg (min 5.4); 22.4 ms with 3 concurrent forkers; 50 GB 253.9 ms avg; over 0.8 ms at 128 MB | the paper's baseline Linux `fork()`, 16-core machine | MEASURED-INDEPENDENT (baseline) | [OCI-33] §2.1 |
| S19 | idmapped mount vs recursive chown, container start in seconds: BusyBox 0.135 vs 4.964; Ubuntu 0.171 vs 15.713; Fedora 0.143 vs 38.799 (computed here: 37x, 92x, 271x) | containerd PR #5890, "numbers are in seconds"; hardware not stated | MEASURED-VENDOR | [OCI-25] |
| S20 | 76 req/s (18x Docker), 130 ms mean (19x) at 10 concurrent no-op lambdas; zygotes add 3x; cold start with zygote 32 ms vs unpausing 3 ms | SOCK on OpenLambda; 8-core Xeon D-1548, 64 GB, Linux 4.13.0-37 | MEASURED-VENDOR | [OCI-27] p.65 |
| S21 | Fork boot 0.97 ms (C hello), under 2 ms (Java SPECjbb); cold / warm / fork boot 40 / 12 / 1 ms; gVisor baseline 142 ms for C hello | Catalyzer on gVisor; i7-7700, 32 GB, Linux 4.17.3 | MEASURED-VENDOR | [OCI-28] p.470-471, p.476 |
| S22 | Unikernel-context deploy cold / warm / hot 7.67 / 2.95 / 0.82 ms; 16-way creation rate per second: Docker overlay2 5.3, Linux process 45, SEUSS 128.6, Firecracker 1.3; Docker creation 541 ms alone -> 1.5 s with over 1,000 containers | SEUSS; 16-vCPU 88 GB VM, Ubuntu 18.04, Linux 4.15, Docker 18.09 | MEASURED-VENDOR | [OCI-29] p.4, p.8 |
| S23 | Production start 5.45-9.41 ms; at x24 sustained 6.97-14.55 ms vs 38.39-74.05 ms for the Catalyzer baseline; 1.80x-8.14x end to end in production; pre-allocates 1,000 veths and 600 cgroups | AFaaS (OSDI 2025), Ant Group | MEASURED-VENDOR | [OCI-35] p.499, p.510-511 |
| S24 | 88 ms single start; 200 sandboxes in 1 s; for 200 concurrent: Kata-FC 47.6 s, kata-qemu 6.85 s, kata-template 2.98 s, RunD 1 s | RunD (ATC 2022) on microVMs, not containers | MEASURED-VENDOR | [OCI-31] p.53, p.61 |
| S25 | Pre-created network endpoints: about -80% startup at 100 concurrent cold starts; network setup up to 84% of total startup | PCPM on OpenWhisk (HotCloud 2019); Particle (SoCC 2020) on three overlay networks | MEASURED-VENDOR | [OCI-36]; [OCI-37] |
| S26 | Fork server 1.5x-2x for fast targets (10x+ in deferred mode); Chromium zygote amortizes about 60 ms/GHz of relocation time and about 6 MB per process | AFL docs (CLAIMED); Chromium docs, measured on a Z620 with `LD_DEBUG=statistics` | CLAIMED; MEASURED-VENDOR | [OCI-14]; [OCI-13] |
| S27 | runc's per-start memfd copy of its own binary "adds around ~60% overhead during container startup"; the overlayfs seal is "on par" with no cloning | code comment, "basic performance testing" | CLAIMED | [OCI-1] `libcontainer/exeseal/cloned_binary_linux.go:222-228` |
| S28 | Min kernel: `clone3` 5.3, `CLONE_PIDFD` 5.2, `CLONE_INTO_CGROUP` 5.7, mount API (`open_tree`, `fsmount`, `move_mount`) 5.2, `mount_setattr` 5.12, `MOVE_MOUNT_BENEATH` 6.5, `OPEN_TREE_NAMESPACE` 7.0, `CLONE_EMPTY_MNTNS` / `UNSHARE_EMPTY_MNTNS` / `CLONE_AUTOREAP` / `CLONE_NNP` / `CLONE_PIDFD_AUTOKILL` / `FSMOUNT_NAMESPACE` 7.1 | man-pages 6.19; v7.0 and v7.1 pull requests; flags absent from v7.0 `kernel/fork.c`, present in v7.1 | DESIGN | [OCI-11]; [OCI-18]; [OCI-19]; [OCI-6] |

What the table says, in five lines:

1. The published crun-over-runc gap runs from 1.2x (through containerd, S5) to 4.7x (runtime alone, cold cache, S3); crun's best
   published figure is 5.6 ms for a whole create-run-delete of `/bin/true` (S7). Four to five times better than that is
   about 1.1-1.4 ms for the same operation (computed here) - below every published figure for a namespace container;
   the only published figures that low are forks of pre-built sandboxes (0.97 ms, S21; 0.82 ms, S22).
2. The time is in the kernel: at 5.6 ms crun spends 4.5 ms in system time and 1.0 ms in user time (S7).
3. What is expensive is a short list of kernel objects that are O(N) or take a global lock: the mount-namespace copy, network and IPC
   namespaces, cgroup create and join, and seccomp load (§4). Each has a published mitigation (§5).
4. The newest kernel primitives (7.0/7.1) target exactly those costs but have only two published measurements (S12, S13) and none on the
   hardware Hex is benchmarked on (§8).
5. Hex's measured 14.4 ms (S9) is not yet comparable to any of these: it omits seccomp and resource cgroups, and no competitor has
   been run through `bench/hex_start/` on the same host.

## 2. How runc, crun and youki implement `create` and `start`

### 2.1 Side by side (all `DESIGN`: read from code at the pinned commits)

| | runc | crun | youki |
|---|---|---|---|
| Language and re-exec | Go; a C constructor `nsexec()` runs before the Go runtime in the re-exec'd `runc init` [OCI-1] `libcontainer/nsenter/README.md:1-14` | C, usable as a library [OCI-2] `README.md:40-42` | Rust [OCI-3] `README.md:12` |
| Process creations before the workload | 3 (`runc init` stage-0, then stage-1, then stage-2) plus one `execve` of the runc binary, and two Go-runtime starts (the CLI and stage-2) | 1 (`clone3` of the container init); a second `fork` only when a PID or time namespace must be entered after a user namespace, or is joined | 2 (`clone3` intermediate, then `clone3(CLONE_PARENT)` init); no `execve` of itself |
| How children are made | Go `os/exec` = `clone(CLONE_VFORK|CLONE_VM)` then exec [OCI-9] `src/syscall/exec_linux.go:317-319`; stages 1-2 by `clone(CLONE_PARENT|SIGCHLD)` [OCI-1] `nsexec.c:314-322` | `clone3` with `CLONE_INTO_CGROUP` and `CLONE_NEW{IPC,NET,UTS,PID}` (no user ns); falls back to `clone` [OCI-2] `src/libcrun/linux.c:7175-7222` | `clone3` with flags 0 or `CLONE_PARENT`; its `cgroup` field is 0, so never `CLONE_INTO_CGROUP`; `clone` fallback on ENOSYS [OCI-3] `crates/libcontainer/src/process/fork.rs:44-126` |
| cgroup join | After the process exists: `manager.Apply(pid)` [OCI-1] `libcontainer/process_linux.go:826`; `CLONE_INTO_CGROUP` only for `runc exec` (`:371-419`, `:459-466`) | Creates the cgroup (mkdir, enable controllers, open dirfd) before the clone, then `CLONE_INTO_CGROUP` [OCI-2] `src/libcrun/cgroup-cgroupfs.c:56-103` | The intermediate process writes its own pid to the cgroup, then clones init [OCI-3] `.../container_intermediate_process.rs:85-89, 322-334` |
| Namespaces | stage-1: `setns` joins, `unshare(USER)`, stage-0 writes uid/gid maps, `unshare(rest)`; stage-2 cloned so the PID ns applies [OCI-1] `nsexec.c:1010-1159` | In the clone: net, ipc, uts, pid; mount ns in the child; cgroup ns after the cgroup join [OCI-2] `linux.c:7080-7088, 7175-7197` | user, time, pid by `unshare`/`setns` in the intermediate; the rest in init [OCI-3] `.../container_intermediate_process.rs:108-151`, `.../init/process.rs:78` |
| rootfs | `mount("/", MS_SLAVE\|MS_REC)`, bind rootfs `MS_REC`, mounts, `pivot_root(".", ".")`, rslave, `umount2(MNT_DETACH)` [OCI-1] `libcontainer/rootfs_linux.go:1099-1113, 1139-1188` | On kernel 7.0+ with no hooks and no user ns: `open_tree(OPEN_TREE_NAMESPACE)` + `setns`; otherwise `unshare` + bind + `pivot_root`; new mount API for most mounts [OCI-2] `linux.c:4263-4304, 4471-4540`; `NEWS:239-241` | bind, mounts, `pivot_root(".", ".")`, rslave, `umount2(MNT_DETACH)` [OCI-3] `crates/libcontainer/src/syscall/linux.rs:383-454` |
| Start gate | `exec.fifo`: init re-opens an O_PATH fd as O_WRONLY (blocks until a reader exists) and writes `0`; `start` polls the FIFO and a pidfd [OCI-1] `standard_init_linux.go:268-279`, `container_linux.go:237-329` | `exec.fifo`: `create` keeps a read fd; init does `select`+`read`; `start` opens the write end, `unlink`s, writes a byte [OCI-2] `container.c:1699-1720`, `status.c:835-883` | Unix socket `notify.sock`: init `accept()`s; `start` connects and writes "start container" [OCI-3] `.../notify_socket.rs:80-93, 129-158` |
| seccomp | not examined | BPF built in the parent while the child sets up (`NEWS:1128`, crun 0.11); cached on disk keyed by a BLAKE3 of the profile (`NEWS:751`, crun 1.7; `seccomp.c:89, 454-559`) [OCI-2] | not examined |

### 2.2 runc

- **The re-exec is the architecture.** `newParentProcess` builds `exec.Command(exePath, "init")` where `exePath`
  is `/proc/self/exe` or a sealed clone of the binary (`libcontainer/container_linux.go:531-565`), passing the
  init pipe, sync pipe, log pipe and the exec FIFO as an O_PATH fd through `ExtraFiles` and environment
  (`:581-654`). Sealing exists against CVE-2019-5736-style attacks; the current code prefers a read-only
  overlayfs view over copying the binary into a memfd, and says the memfd copy "adds around ~60% overhead
  during container startup" (S27, `exeseal/cloned_binary_linux.go:218-249`).
- **nsexec's stages.** The constructor reads bootstrap data, then `setjmp`/`longjmp` dispatches three roles
  (`nsexec.c:847-870`): stage-0 clones stage-1 with `CLONE_PARENT|SIGCHLD` (`:867`); stage-1 does the `setns`
  joins, `unshare(CLONE_NEWUSER)` (`:1055`), asks stage-0 over a socketpair to write the uid/gid maps
  (`:1074-1083`), `unshare`s the remaining namespaces (`:1107`) and clones stage-2 (`:1132`) because a PID
  namespace applies only to children; stages 0 and 1 `exit(0)` (`:997`, `:1159`); stage-2 returns into the Go
  runtime (`:1222-1223`), which is `runc init`. The author's own comment explains why the shape cannot be
  simpler: user-namespace mapping, SELinux and old-kernel `CLONE_PARENT|CLONE_NEWPID` bugs (`:800-845`).
- **Parent side.** `initProcess.start` runs `cmd.Start()` (`process_linux.go:780`), then `manager.Apply(pid)`
  (`:826`, a cgroup join after the fork), copies bootstrap data (`:848`), reads the child pid (`:852`), waits for
  the first child to exit (`:867`), creates network interfaces (`:887`), sends the config as JSON (`:904`), and
  serves a sync loop for mount requests, seccomp fds, `procReady` and hooks (`:909-1051`).
- **Init side.** `Init()` pins `GOMAXPROCS(1)` and the OS thread (`init_linux.go:106-108`), decodes the config
  (`:233`), then `prepareRootfs` (`standard_init_linux.go:91`, `rootfs_linux.go:170-273`), `finalizeRootfs`
  (`:116`), sysctls, readonly and masked paths, seccomp, `syncParentReady` (`:188`), closes the pipes, blocks on the
  FIFO and writes `0` (`:258-288`), closes every fd it does not pass on and calls `execve` (`:308-311`).
- **`start`.** `handleFifo` opens the FIFO `O_RDONLY|O_NONBLOCK`, `poll`s it with a pidfd, reads one byte and
  removes it (`container_linux.go:237-256, 261-293, 319-329`).

### 2.3 crun

- **One clone.** `libcrun_run_linux_container` computes the clone flags: if there is no user namespace, the
  new net, ipc, uts and pid namespaces are in the flags, while mount, time and cgroup namespaces are not
  (`linux.c:7175-7186`); it then calls `clone3` with `CLONE_INTO_CGROUP` and the pre-opened cgroup dirfd,
  falling back to `clone` on any error (`:7199-7222`). The cgroup namespace is unshared "later, once the process
  is in the correct cgroup" (`:7086-7088`).
- **Pipelined with the parent.** The parent and child talk over a `SOCK_SEQPACKET` socketpair (`:7090-7095`;
  sync points `container.c:3078-3196`). While the child builds its mounts, the parent finalizes the cgroup
  (`:3065`, `:3097`), moves network devices into the new netns (`:3073`), runs hooks, and **generates the seccomp
  BPF** (`:3128`; crun 0.11 moved generation "in the parent process, not in the container", `NEWS:1128`). The
  BPF is cached on disk under `.cache/seccomp`, keyed by a BLAKE3 over every field that affects the filter, with
  a size-bounded eviction by access time (`seccomp.c:89-102, 454-559, 616-700`; `NEWS:751` crun 1.7).
- **Mount namespace.** `setup_mount_namespace` uses `open_tree(OPEN_TREE_NAMESPACE)` + `setns` when the kernel has it,
  the container has no `createRuntime`/`createContainer`/`prestart` hooks and no user namespace, and no bind source
  holds a pinned namespace (`linux.c:4263-4304`, `:4471-4516`, `NEWS:21-25`); otherwise `unshare(CLONE_NEWNS)`,
  `MS_PRIVATE`, a recursive bind of the rootfs and a deferred `pivot_root` (`:4517-4540`, `:4597-4620`). The
  maintainer's commit message: "to replace the traditional unshare(CLONE_NEWNS) + bind mount rootfs + pivot_root
  sequence" ([OCI-24]), and the release note says it "improves container startup performance" (`NEWS:163-164`);
  the only numbers are S12.
- **Gate and start.** The FIFO is made at `create`; the read end is kept open and the container init waits on it
  (`status.c:835-854`, `container.c:1699-1720`); `start` writes one byte (`status.c:857-883`, `container.c:3593`).
- **CLI shape.** `crun create` passes options `0`, which takes the direct path: no extra `fork`, the container is
  created in the CLI process and `wait_for_process` returns at once when detached (`create.c:107-111`,
  `container.c:3489-3499`, `:2308`). The `fork()`+`detach_process()` path (`:3507-3558`) is for library users who set
  the prefork option.
- **Release notes that are performance claims without numbers (CLAIMED):** `clone3` to join the cgroup directly
  (1.7, `NEWS:758`); `CLONE_INTO_CGROUP` for `exec` (1.4, `:857`); `cgroup.kill` is faster than recursing over pids
  (1.1, `:887-890`); open_tree + mount_setattr/move_mount "avoids unnecessary syscalls and improves container
  startup performance" (1.27, `:239-241`); skip `enable_controllers` when already joined (1.27, `:249`);
  OPEN_TREE_NAMESPACE "improves container startup performance" (1.29, `:163-164`).

### 2.4 youki

- **Two clones, no re-exec.** `container_main_process` creates four unidirectional channels and clones the
  intermediate process (`container_main_process.rs:50-103`), which clones `init` as a sibling with
  `CLONE_PARENT` so the main process, not pid 1, owns it (`fork.rs:37-56`,
  `container_intermediate_process.rs:203-214`). The `clone3` wrapper passes `cgroup: 0`, so
  `CLONE_INTO_CGROUP` is not used (`fork.rs:80-126`); `clone` with a hand-allocated `mmap` stack (RLIMIT_STACK, 8 MB if
  unlimited) is the fallback (`:128-237`).
- **Order inside the intermediate:** join the cgroup (`add_task(pid)`, a `cgroup.procs` write) and apply
  resources (`container_intermediate_process.rs:85-89, 322-351`), set up the user namespace with the main process
  writing the maps (`:108-123`, `container_main_process.rs:119-126`), set rlimits, time ns, then `unshare` the
  PID namespace (`:148-151`), then clone init.
- **Init:** `setsid`, io priority, scheduler, `apply_rest_namespaces`, `prepare_rootfs`, hooks via the main
  process, `pivot_root` (`init/process.rs:43-146`), sysctls, then `init_ready` and the notify-socket `accept()`
  (`:444-472`) before `exec`. Its rootfs and pivot code is the classic sequence (`syscall/linux.rs:383-454`); the
  syscall layer already carries `open_tree`, `fsopen`, `fsmount`, `move_mount` and `mount_setattr` wrappers
  (`:664-935`).
- **Benchmark harness in-tree:** a PR comment `!github easy-benchmark` runs hyperfine on `run` + `delete -f`
  against the main branch with `drop_caches` before each run (`.github/workflows/benchmark_execution_time.yml:99-125`).

### 2.5 What the OCI lifecycle permits

The runtime-spec requires `create` to build the environment "according to the configuration" without running the
user program, then `prestart`, `createRuntime` and `createContainer` hooks, then a separate `start` (hooks:
`startContainer`, run the program, `poststart`) [OCI-10] `runtime.md:57-79`. Two consequences the sources show:
(1) any fast path that pre-builds state must still present exactly the observable `created` state, and (2) hooks
are a performance cliff - crun turns off its OPEN_TREE_NAMESPACE path when `prestart`/`createRuntime`/
`createContainer` hooks exist, because they expect the rootfs at its host path in the host mount namespace
[OCI-2] `linux.c:4263-4273`, [OCI-23].

### 2.6 Hex's `oci` and `process` classes today (for contrast; `DESIGN`, working tree of branch `cp/t7-runtime`)

- `hex oci create` spawns **the hex binary itself** (`std::process::Command::new(current_exe)`, subcommand
  `__oci-init`) [OCI-4] `crates/hex-runtime/src/linux/oci/run.rs:78-88`. With no `pre_exec` closure, uid, gid or
  chroot set, Rust's `Command` is eligible for the `posix_spawn` path, which glibc implements with
  `clone(CLONE_VM|CLONE_VFORK)` [OCI-8] `library/std/src/sys/process/unix/unix.rs:469-477` (master, fetched
  2026-10-22), [OCI-11] posix_spawn(3).
- The shim `unshare`s (user first, then the rest), then `fork()`s the container (`oci/init.rs:61-101`,
  `oci/sys.rs:19-30`); the container does `mount_change("/", PRIVATE|REC)`, a bind of the rootfs, the planned mounts,
  devices, symlinks, masked and readonly paths, `pivot_root(".", ".")`, `unmount(".", DETACH)` (`:289-344`), then opens
  the FIFO and writes `0` (`:166-168`) and `execve`s (`:171`). That is the same chain as runc stages 0-2 minus Go and
  minus the user-space cgroup/seccomp work.
- **Not present:** the grep of `oci/*.rs` finds no `seccomp`, and `cgroup` only as the cgroup-namespace flag
  (`init.rs:234, 277`) and the parsed `cgroupsPath` field (`spec.rs:177`); no resource cgroup is made for an oci
  container. S9 is therefore less work than crun's, and any comparison must add it.
- The `process` class joins its cgroup in the forked child: `Command::pre_exec` writes `0` to the box's
  `cgroup.procs` (`crates/hex-runtime/src/linux/sys.rs:27-37`, `process.rs:36-37`). A `pre_exec` closure forces the
  fork path in Rust's `Command` ([OCI-8] `unix.rs:469-477`) and the `cgroup.procs` write takes the kernel's global
  locks (§4.5).
- Measured: S9. The runner Hex benchmarked on runs Linux 6.19 [OCI-4] `docs/design/runtime.md:30-31`, so
  `OPEN_TREE_NAMESPACE` (7.0) and the 7.1 clone flags are not available there.

## 3. Published start-latency and memory comparisons

### 3.1 The comparisons, with methods

| Source | By | Method and workload | Hardware, kernel, versions | Result | Kind |
|---|---|---|---|---|---|
| crun README [OCI-2] `README.md:44-64` | crun's author | Elapsed time to run 100 containers sequentially, each `/bin/true`; also `--memory` limits through podman | "my machine"; versions and kernel not stated; undated | crun 1.69 s vs runc 3.34 s (-49.4%). `podman --memory 512k` runs with crun; the same with runc at 4M fails with a "getting pipe fds ... no such file" error (an old runc error format) | MEASURED-VENDOR |
| Red Hat blog, "An introduction to crun" [OCI-45] | Walsh, Rothberg, Scrivano (Red Hat), 2020-08-03 | `time -v sh -c 'for i in {1..100}; do $RUNTIME run foo; done'`, `/usr/bin/true` | not stated | runc 6.89 s (user 2.16, sys 4.60, max RSS 15,120 KB); crun 3.86 s (user 0.53, sys 1.87, max RSS 3,752 KB). Binary about 300 KB with `-Os` vs about 15 MB. runc cannot start with `--pids-limit 5` (Go runtime threads); crun runs with `--pids-limit 1`. The authors report having run a container with as small as a 250K memory limit | MEASURED-VENDOR |
| Red Hat Developer blog, "How I decreased the time to create and destroy an OCI container to 5 milliseconds" [OCI-44] | Scrivano (crun's author), 2022-11-16 | hyperfine `crun run foo`, plus micro-measurements of `unshare` and mounts, over 5 years of fixes | Fedora default kernels: 4.5.5 (Fedora 24) and 5.19.15; hardware not stated | 159.2 -> 5.6 ms (S7); breakdown in §4 | MEASURED-VENDOR |
| youki first-release post [OCI-46] | youki's author, 2021-12-27 | hyperfine `create && start && delete -f`, `drop_caches` before each run, 10 warm-ups, >=100 runs | Ubuntu 21.04, Linux 5.11, 32 GB; crun 0.19.1, runc 1.0.0-rc93 | crun 153.5 ± 21.6 ms (80.9-196.6), youki 198.4 ± 52.1 (97.2-296.1), runc 352.3 ± 53.3 (248.3-772.2) | MEASURED-VENDOR |
| youki README [OCI-3] `README.md:39-80` | youki team | same command, later versions | Ubuntu 22.04.4, Linux 6.5.0-35, 16 cores, 64 GB; crun 1.15, youki 0.3.3, runc 1.1.7 | S3 | MEASURED-VENDOR |
| "Benchmarking what actually drive our containers" [OCI-47] | Henrik Gerdes (practitioner), 2024-07-01 | `time` per container, 1,000 serial creations of busybox `/usr/bin/true` via `nerdctl`; includes containerd; warm-up for CPU boost; also minimum `--memory` that starts | Hetzner CCX33 dedicated, Debian 12, Linux 6.1.0-21; containerd 1.7.18, crun 1.15, runc 1.1.13, runsc 20240617, youki 0.3.3 | S5; "A 21% difference is still impressive" for crun over runc; youki 36/1,000 errors ("not acceptable in a production environment"); no median or percentiles; lowest memory limit that started busybox: crun 160 kB (text); table column lists runc 8 MB, youki 2 MB, runsc 32 MB without defining the column; writes about 700-800 IOPS and 4-4.5 MB/s for crun and runc | MEASURED-INDEPENDENT |
| gntech blog [OCI-48] | one author, 2026-06-16 | `docker run --rm alpine echo hello`, mean of 10; RSS of the container's init pid (the post does not say why that measures the runtime) | Intel N100 mini PC, Debian 12, Linux 6.8, Docker 27.x, runc 1.2+, crun 1.17+ | runc 98 ms vs crun 52 ms; "runc child ~14 MB RSS vs crun ~4 MB" (method doubtful: the init pid is the workload) | MEASURED-INDEPENDENT (low rigor) |
| arXiv 2609.04478 [OCI-42] | Niroula, Shan, Ding (NJIT, Hofstra), preprint 2026-09-03 | runc run directly, with perf/KernelShark traces, varying vCPUs, CPU quota, load; isolation relaxations | GCP c3-standard-4 and -22 VMs, a local HPE DL380 Gen11; Ubuntu, Linux 6.8.0 | S8; runc 52.4-56.5 ms per instance with 4 instances in 4 VMs sharing 4 cores | MEASURED-INDEPENDENT |
| arXiv 2602.15214 [OCI-43] | S. Khan (practitioner), preprint 2026-02-16 | `docker run` warm start, 50 iterations, plus separate micro-tests | Azure D2s_v3 2 vCPU 8 GB, Docker 28.x overlay2 | alpine warm start 568 ± 5 ms on Premium SSD; "namespace creation requires 7.94 ms (SSD, σ = 2.05)" (under 1.5% of startup); model Tstartup = Tkernel (about 8 ms) + Truntime + Tstorage. The text does not say how the 7.94 ms was isolated, and it measures Docker, not OCI runtimes | MEASURED-INDEPENDENT (low rigor) |

### 3.2 What they show, and what they do not

- **The direction is stable, the size is not.** crun is faster than runc in every source (1.2x through containerd on
  Linux 6.1, S5; 1.8-2.0x runtime-only, S1, S2; 2.3x cold-cache 2021, S4; 4.7x cold-cache 2024, S3). youki sits
  between them or at runc's level (S3-S5) and, at 0.3.3, failed 36 of 1,000 creates in the one run that counted (S5).
  The only third-party ratios are 1.2x (S5, through containerd) and 1.9x (S6, through Docker, 10 runs, computed here); every
  ratio of 2x or more comes from a project's own harness.
- **"crun" moved.** In the youki harness crun went from 153.5 ms (0.19, 2021) to 47.3 ms (1.15, 2024) on different
  machines (3.2x, computed here); in Scrivano's series it went from 159.2 to 5.6 ms through kernel, libseccomp
  and crun changes (S7). The crun measured in S3 and S5 (1.15) pre-dates the new-mount-API and OPEN_TREE_NAMESPACE work
  in 1.27-1.29 (`NEWS:163-164, 239-241`), so a baseline against crun must use the current crun on Hex's own hardware.
- **CPU and memory.** Per container, runc spent 67.6 ms of CPU and crun 24.0 ms (S2, computed here); max RSS was 15,120 KB
  vs 3,752 KB, 4.0x (computed here). By 2022 crun's per-container CPU was 5.5 ms (user 1.0 + system 4.5, S7). No source reports
  youki's memory beyond the undefined "Min Mem" column in Gerdes's table (2 MB).
- **Tail.** The youki 2021 post gives min and max (runc 248.3-772.2 ms is a 3.1x spread); Gerdes gives min and max over 1,000
  runs (crun 0.13-0.41 s); Scrivano gives a min-max range (4.2-26.8 ms around a 5.6 ms mean). **None gives a p99, p99.9 or a CDF**
  for any OCI runtime start.
- **What a Docker-level number hides.** Docker's own warm start is 568 ms with namespace creation at about 8 ms [OCI-43]; the
  gap between crun and runc through containerd is 60 ms [OCI-47]. Stack overhead dilutes runtime differences, and
  AFaaS measures the same effect in production: 18-25 ms of control path (shim, binary call, RPC) is 30-40% of the cold start
  even with Catalyzer's sub-millisecond sandbox fork [OCI-35] p.500. A runtime-level bar (`hex oci`) and a stack-level bar (kiln
  dispatching a leg) are different bars and need different benchmarks.

## 4. Where the time goes

### 4.1 Cost model: what a start creates, the kernel path behind it, and what is measured

| Step | What the kernel does (DESIGN) | Measured | Avoided by |
|---|---|---|---|
| Process creation | `fork` copies page tables; `vfork`/`CLONE_VM` shares the address space [OCI-11] vfork(2); small parent about 31-52 us, 1 GB written parent 7.6 ms | S17, S18; `posix_spawn` about 0.5 ms flat [OCI-32] | vfork-class spawn or `clone3` from a small helper (§4.7) |
| Mount namespace | `copy_mnt_ns` takes the namespace lock exclusively and `copy_tree`s the whole tree [OCI-5] `fs/namespace.c:4232-4268`; `CLONE_EMPTY_MNTNS` clones only the root (`:4258-4266`) | 0.343-0.917 ms with `pivot_root`; 0.043-0.044 ms with `OPEN_TREE_NAMESPACE` (S12); SOCK: clone rate "approaches zero" as host mounts grow [OCI-27] p.58 | `OPEN_TREE_NAMESPACE` (7.0), `CLONE_EMPTY_MNTNS` (7.1), small host mount table (§4.2) |
| rootfs mounts (bind, tmpfs, proc, sysfs, devpts, mqueue) | each pseudo-filesystem mount allocates a superblock; mqueue's was created eagerly per IPC namespace | mqueue 16.8 -> 0.7 ms (S11); bind about 2x AUFS (S15); **per-mount cost of proc, sysfs, devpts, tmpfs on 6.x: not published** | fewer pseudo-filesystems; pre-built detached mounts; bind or EROFS instead of union FS (§5.10) |
| Network namespace | `copy_net_ns` takes `pernet_ops_rwsem` (read) and `setup_net` runs every registered pernet init; teardown is deferred to a work queue ending in `rcu_barrier()` [OCI-5] `net/core/net_namespace.c:445-449, 575-581, 662-725` | 47.7 -> 1.5 ms (S10); about 200 c/s stock (S15); 2 -> 21.14 ms network prepare at 24-way [OCI-35] p.503; Docker -50 ms when reused [OCI-42] | pool, share or skip (§4.3) |
| IPC namespace | separate mqueue superblock per namespace; free via work queue | 10.9 -> 0.1 ms (S11); sustained-churn collapse 110 -> 45 functions/s (S14) | share or skip; no mqueue unless asked |
| cgroup create | `cgroup_mkdir` runs under the global `cgroup_mutex`, makes files, enables controllers [OCI-5] `kernel/cgroup/cgroup.c:6038-6089, 1695-1724` | 900 ms long tail creating v1 cgroups for 400 containers [OCI-21]; -94% with pool+rename (S16) | cgroup pool (§4.5) |
| cgroup join | a `cgroup.procs` write takes `cgroup_mutex`, `cpus_read_lock` and the global `percpu_down_write(&cgroup_threadgroup_rwsem)` unless `favordynmods` (`cgroup.c:2532-2551, 3091-3100, 5412-5416`); `CLONE_INTO_CGROUP` takes `cgroup_mutex` and the read side only (`:6793-6796`) | up to 50% of runc start, -27% when skipped (S8) | `CLONE_INTO_CGROUP` (5.7) |
| seccomp | each install builds a new filter object, JIT-compiles it and emulates it for the syscall cache [OCI-5] `kernel/seccomp.c:669-701, 921-964`; a forked child shares the parent's filter by refcount (`kernel/fork.c:1777-1789`, `seccomp.c:979-986`) | 38.9 ms of user time at 4.5.5-era libseccomp, 12.6 ms after the perfect-hash fix, about 0 cached (S7); 7.58 -> 13.01 ms under 24-way contention [OCI-35] p.503; bypass -30 ms [OCI-42] | cache or precompile; install once in a seed (§4.6) |
| Binary and shim hop | runc re-execs a 15 MB Go binary and starts two Go runtimes | binary load 18.02 ms in a Go sandbox engine [OCI-35] p.502 | no exec per start (§4.8) |
| Teardown | netns/mntns/ipc namespaces wait for RCU grace periods: "tens of milliseconds", mostly with no global lock held except for network cleanup [OCI-27] p.59; early destroy in AFaaS because shutdown "can take over 10 seconds" for JVMs [OCI-35] p.507; cgroup rmdir and `cgroup.kill` cost: **not measured in any source** | S11 for IPC | asynchronous teardown; `cgroup.kill` [OCI-5] `cgroup-v2.rst:1051-1062` |

### 4.2 Mount namespace and rootfs

- **The copy is O(mounts) under a global lock.** `copy_mnt_ns` for `CLONE_NEWNS` runs under `guard(namespace_excl)()` and copies the
  entire tree with `copy_tree`, then walks it twice (`namespace.c:4256-4268, 4303-4325`); with `CLONE_EMPTY_MNTNS` it clones one root
  mount (`:4258-4266, 4279-4295`) [OCI-5]. The author of OPEN_TREE_NAMESPACE states the motive in the cover letter: a basic
  system "copies about 30 mounts" only to "get rid of them later"; with large mount tables and "thousands or ten-thousands of namespaces"
  in parallel this "quickly becomes a bottleneck increasing contention on the semaphore" [OCI-17].
- **runc and youki then do O(mounts) work twice more:** `mount("/", MS_SLAVE|MS_REC)` over the copied tree
  (`rootfs_linux.go:1099-1113`) and `umount2(".", MNT_DETACH)` of the whole old root (`:1139-1188`; youki
  `syscall/linux.rs:428-447`). Hex's `setup_rootfs` does the same (`oci/init.rs:298-302, 329-331`).
- **Measured, crun maintainer (S12):** `unshare(NEWNS)`+`pivot_root` averages 0.343 ms (min 0.147, max 0.735) on a clean host and
  0.917 ms (0.588-2.403) with "a bunch of mounts on the host"; `open_tree(OPEN_TREE_NAMESPACE)` alone averages 0.044 ms
  (0.010-0.090) and 0.043 ms (0.027-0.085). When a user namespace forces an intermediate `unshare(CLONE_NEWNS)` the copy cost comes
  back (0.153 ms clean; 0.911 ms with many mounts) [OCI-23]. The same thread records the limits: creating `proc`/`sysfs` mounts
  from inside a user namespace needs kernel help, and hooks that expect the rootfs at its host path force the old path - "we need to
  keep the two mechanisms anyway" [OCI-23].
- **Measured, kernel (S13):** the 7.0 pull request reports "about a 40% increase in throughput" for container creation
  [OCI-18]. Phoronix quotes the underlying message as 73k vs 109k containers per 60 s and "roughly 40% faster" - I could not open
  the mailing-list message (lore.kernel.org sits behind a bot wall), so the pair is secondary [OCI-26]; note that 109/73 is 1.49x.
- **v7.1 goes further:** `CLONE_EMPTY_MNTNS`/`UNSHARE_EMPTY_MNTNS` create "a namespace containing only a single nullfs root mount with an
  immutable empty directory" so a runtime can "mount a real filesystem over the root and build the mount table from scratch,
  which avoids copying and tearing down the entire parent mount tree"; `FSMOUNT_NAMESPACE` returns a namespace fd with a new
  filesystem attached, "especially useful when mounting a real filesystem to serve as a container rootfs"; `MOVE_MOUNT_BENEATH`
  can now target the caller's rootfs and carry `MNT_LOCKED`, which "makes it possible to switch out the rootfs without pivot_root(2)"
  [OCI-19]. No performance number accompanies them.
- **Older evidence, same direction:** chroot costs under 1 us and bind mounts are about twice as fast as AUFS at scale; SOCK avoids
  mount namespaces entirely for that reason [OCI-27] p.58, p.62. The cost also depends on how many containers already exist:
  Docker's creation latency rose from 541 ms to 1.5 s on average once over 1,000 containers existed, and was proportional to the
  number of concurrent creations (8.5 s mean 16-way) [OCI-29] p.8 (whole Docker stack; the mount tree is one suspect, not isolated).

### 4.3 Network namespace

- **Why it is the worst offender.** `setup_net` runs the initializer of every registered `pernet_operations` for the new namespace
  (`net_namespace.c:445-449`); that is one reason SELinux's six netfilter hooks per namespace mattered (S10) and why cost depends on
  which networking modules are loaded (an inference from the loop; no source isolates it). Teardown is a batched work item
  that ends in `rcu_barrier()` (`:662-725`) [OCI-5]. In 2018 the global mutex let a single `synchronize_rcu()` in cleanup block every
  creation: "I observed 20-30 seconds hangs of `unshare -n` on ordinary 8-cpu laptop with preemptible RCU" [OCI-16].
- **What has been fixed (S10):** from 47.7 ms to 1.5 ms for `unshare -n true` between kernels 4.5.5 and 5.19.15, by freeing netfilter
  hooks with `call_rcu` instead of `synchronize_net()` ("creating a new network namespace takes -40% of the original time" on a
  2-core machine) [OCI-44]. So on a modern idle kernel a netns is cheap; what remains is how it scales.
- **How it scales:** creation "iterates over all existing namespaces while holding the lock" so cost grows with the count; cleanup
  holds a global lock across an RCU grace period; throughput peaked near 200 containers/s and reached 900/s with no netns [OCI-27] p.59.
  At 24-way concurrency in production, `clone()` with new net+IPC namespaces went from 1.45 ms to about 418 ms worst case and network
  preparation from 2 ms to 21.14 ms [OCI-35] p.503. AWS Lambda reports the same shape at hyperscale: "creating Linux network devices
  (tap, veth, namespaces) got slower as density increased ... the cost of creating the N+1 network grew with N", and creating thousands in
  parallel "queued behind" the RTNL lock until "what should have taken seconds stretched to minutes" (CLAIMED: an AWS blog, no method)
  [OCI-49].
- **What the fixes were:** Lambda pre-created all 4,000 networks per worker at boot (three minutes), pooled namespaces first, created
  veths inside the namespace before moving them out, and batched eBPF attachments [OCI-49]; AFaaS pre-allocates 1,000 veths and shares
  the seed's netns and IPC ns [OCI-35] p.506, p.511; PCPM pre-created pause-container network endpoints and cut startup about 80% at 100
  concurrent cold starts (network creation was the dominant stage) [OCI-36]; Particle found network setup up to 84% of startup in
  overlay networks [OCI-37]; independent measurement: reusing a running container's netns saved about 50 ms per Docker container [OCI-42].
- **The isolation price:** reuse must not leak state between tenants: AFaaS notes that PCPM-style sharing of a network stack
  "introduces security risks by reusing the TCP/IP stack from previous containers" and instead pre-creates independent veth pairs so
  "each fork initializes a clean TCP/IP stack" [OCI-35] p.504.

### 4.4 IPC namespace and mqueue

`unshare -i true` took 10.9 ms until a kernel patch moved a `synchronize_rcu()` into a work queue (1,000 `unshare(CLONE_NEWIPC)`: 8.05 s -> 0.03 s),
and mounting mqueue took 16.8 ms until the internal mount was made on demand (0.7 ms) [OCI-44]. Yet in production the IPC namespace
was still the cause of a 110 -> 45 functions/s decline over one hour at 24-way concurrency: each IPC namespace needs "a unique superblock",
the superblock cache "hit rate is relatively low", and `sb_lock` and `mount_lock` contend; the system "transitions from the fast path to the
slow path for newly created IPC namespaces" [OCI-35] p.503, p.509. This is the clearest published case of a cost that a short benchmark
cannot see and a sustained one can.

### 4.5 cgroups

- **Kernel documentation says to do it once.** "Migrating a process across cgroups is a relatively expensive operation ... A workload
  should be assigned to a cgroup according to the system's logical and resource structure once on start-up" [OCI-5] `cgroup-v2.rst:620-630`;
  and for the pattern Hex needs, "creating a cgroup, enabling controllers, and then seeding it with CLONE_INTO_CGROUP is not affected" by
  `favordynmods` (`:194-200`).
- **`CLONE_INTO_CGROUP`.** Its author: it "does not need to grab the write side of the cgroup cgroup_threadgroup_rwsem. This global lock makes
  moving tasks/threads around super expensive" [OCI-15]; the man page: spawning directly into the target cgroup "is significantly cheaper than moving
  the child process into the target cgroup after it has been created" and removes "accounting jitter" [OCI-11] clone(2) (since Linux 5.7).
  The v7.2 code still takes `cgroup_mutex` for it (`cgroup.c:6793-6796`), so it does not remove serialization on `cgroup_mutex`, only the
  threadgroup write lock and the second syscall.
- **What the write lock costs, measured (S8):** an independent study traced runc on Linux 6.8 and found contention "associated with
  WRITE|PERCPU operations during PID updates to cgroups" that "can sometimes account for up to 50% of runc startup time. During this period,
  the runc process makes no forward progress", with a `percpu_down_write` stack trace; skipping cgroup assignment cut launch time 27%
  [OCI-42] p.11-12. A 2025 kernel patch made the lock per thread group when `favordynmods` is on, reducing "the long-tail latency of cgroup
  migration ... from milliseconds to microseconds" under heavy fork/exec interference [OCI-20]; its motivation is that container creation
  and deletion are now frequent, "making cgroup process migration no longer a cold path".
- **Creation under concurrency:** a 2021 kernel RFC reported a 900 ms long-tail for creating cgroups under each subsystem when starting 400
  containers (cgroup v1), split between the `cgroup_mutex` critical section and sleep/scheduling time [OCI-21]; RunD found latency *rising* with
  more threads because of `cgroup_mutex`, `css_set_lock` and `freezer_mutex` and fixed it with pre-created, renamed cgroups (-94%) [OCI-31]
  p.57, p.60; SOCK measured reuse as at least twice as fast as create [OCI-27] p.60. AFaaS keeps a pool (600 pre-allocated) and warns that
  even a pool needs "lightweight locks" and can jitter when a co-located workload holds the global cgroup lock [OCI-35] p.511-512.
- **Teardown:** `cgroup.kill` kills a tree "deal[s] with concurrent forks appropriately" [OCI-5] `cgroup-v2.rst:1051-1062`; crun uses it because
  there is "no need to recurse over the cgroup pids" [OCI-2] `NEWS:887-890`.

### 4.6 seccomp

- **User space dominated crun's 2017-2022 time.** Of 159.2 ms, user time was 43.0 ms of which libseccomp was 38.9 ms (43.0 minus 4.1 ms
  without a profile); a perfect-hash syscall lookup cut that to 12.6 ms (3.1x, computed here), and a BPF cache brought the whole run to
  5.6 ms with user time 1.0 ms [OCI-44]. libseccomp's own commit: 1,000 name+number resolutions over the database went from 0.45 s to 0.06 s
  (quoted in [OCI-44]).
- **In-kernel cost and the inheritance escape.** A load builds a new filter, JIT-compiles it, and emulates it to find cacheable syscalls
  (`seccomp.c:669-701, 884-902, 921-964`); a `clone` shares the parent's filter object by reference count (`fork.c:1777-1789`,
  `seccomp.c:979-986`) and the documentation says children are "constrained to the same filters" (`seccomp_filter.rst:67-69`) [OCI-5] - so a
  seed that installs once makes every fork free of this step (an inference from those lines, not measured).
- **Under concurrency and churn:** install latency went from 7.58 to 13.01 ms at 24-way, and repeated create/destroy hit `bpf_jit_limit` leaks
  that made installs fail [OCI-35] p.503, p.511; bypassing seccomp in runc saved about 30 ms [OCI-42].
- **crun's design to copy or beat:** build in the parent while the child sets up (`NEWS:1128`), cache by BLAKE3 of every field that affects the
  filter, bound the cache directory by inode size and evict half by access time when over (`seccomp.c:89-102, 616-700`) [OCI-2].

### 4.7 Process creation: fork, vfork, posix_spawn, clone3, pidfd

- **Documentation.** `fork` is copy-on-write but "the only penalty ... is the time and memory required to duplicate the parent's page
  tables"; `vfork` "is used to create new processes without copying the page tables of the parent" [OCI-11] vfork(2); since glibc 2.24
  `posix_spawn` calls `clone(CLONE_VM|CLONE_VFORK)` [OCI-11] posix_spawn(3). glibc has no `clone3` wrapper [OCI-11] clone(2).
- **Measured:** S17: a 1 GB parent that has written its memory costs 7,581.8 us to fork+exec vs 31.9 us with vfork (238x, computed here);
  with no extra memory 52.0 vs 31.5 us (1.65x) [OCI-34]. S18: fork of a 1 GB parent 6.5 ms, 22.4 ms when three do it at once, 50 GB
  253.9 ms; the authors attribute the parallel slowdown to memory and cache-line contention on the `struct page` reference counts
  [OCI-33] §2.1-2.2. The HotOS paper
  measured `posix_spawn` flat at about 0.5 ms regardless of parent size (i7-6850K, Ubuntu 16.04.3) while fork+exec grew with it, and noted
  Chrome seeing "delays of up to 100 ms in fork" [OCI-32] p.3.
- **Language runtimes.** Go's `os/exec` already uses `CLONE_VFORK|CLONE_VM` and switches to `clone3` with `CLONE_INTO_CGROUP` and
  `CLONE_PIDFD` when asked ([OCI-9] `exec_linux.go:317-345`), which is how runc starts `runc init` cheaply and `runc exec` directly into
  the cgroup. Rust's `Command` falls back to `fork` as soon as there is a `pre_exec` closure, a uid/gid/groups or a chroot
  ([OCI-8] `unix.rs:469-477`) and has no `clone3` path. Hex's `process` class is therefore on the fork path, with the `cgroup.procs` write
  in the child (§2.6).
- **pidfd.** `CLONE_PIDFD` (5.2) returns a pollable pidfd from the clone itself; a pidfd can be polled and waited with `waitid` [OCI-11]
  clone(2), pidfd_open(2); runc already waits on the FIFO and a pidfd with one `poll` (`container_linux.go:261-293`).
- **Kernel 7.1 closes the reaper and supervisor gaps** with three clone3 flags: `CLONE_AUTOREAP` ("makes a child auto-reap on exit without
  becoming a zombie", per process, surviving reparenting), `CLONE_NNP` (sets `no_new_privs` on the child at creation) and
  `CLONE_PIDFD_AUTOKILL` (SIGKILL when the last reference to the clone's pidfd closes: "useful for container runtimes ... where the
  child must die if the parent crashes"; requires `CLONE_PIDFD` and `CLONE_AUTOREAP`, and `CLONE_NNP` makes it unprivileged)
  [OCI-19]; validation in `kernel/fork.c:2061-2094` [OCI-5]; they are in v7.1 and absent from v7.0 [OCI-6]. No performance measurement exists.
- **Safety, not only speed.** A multi-threaded parent may use only async-signal-safe functions between `fork` and `exec`, and "forking
  many child processes from the same parent without calling exec undermines address-space randomization" (SOCK, citing Morula)
  [OCI-32] p.3, [OCI-27] p.67 - the reason Hex re-execs a single-threaded helper today ([OCI-4] `oci/sys.rs:1-8`).

### 4.8 Control path, binary exec and runtime init

- runc re-execs a ~15 MB binary per container and starts a Go runtime in both the CLI and `runc init` [OCI-45], [OCI-1]; the memfd-copy
  variant "adds around ~60% overhead during container startup" by runc's own measurement (S27). The Go runtime also spawns threads, which
  is why `--pids-limit 5` fails for runc and `--pids-limit 1` works for crun [OCI-45].
- At the sandbox-engine level (Go, secure containers), the shim loading the runtime binary for each start cost **18.02 ms**, and the whole
  high-level-to-low-level interaction 18-25 ms, "approximately 30%-40% of the overall cold start period" [OCI-35] p.500, p.502-503. AFaaS replaced
  binary calls with a plugin interface (FRI) so only the root seed is created through the OCI-style path and later starts are plain function
  calls [OCI-35] p.505. crun is built to be linked as a library "without requiring an external process" [OCI-2] `README.md:40-42`.

### 4.9 Concurrency, sustained churn and the tail

| Effect | Evidence |
|---|---|
| Per-start latency rises with concurrency | AFaaS container-init tail ranges [5.21, 9.85] ms (x1), [5.27, 12.49] (x12), [5.53, 17.67] (x24) [OCI-35] p.500. The one independent runc study finds the opposite sign for CPU load: runc takes 44 ms on a 1-vCPU VM but over 60 ms on several vCPUs, 59 ms beside a CPU-bound neighbor on a multi-vCPU VM (about 6 ms faster than alone; on one vCPU the same neighbor made it slower, 44 -> 61 ms), and 52.4-56.5 ms with four running at once across 4 VMs on 4 cores (about 10 ms faster than alone on a 4-vCPU VM); the authors attribute it to idle wake-up and cross-CPU synchronization [OCI-42] p.11 |
| Throughput decays over time | 110 -> 45 functions/s over 1 h at x24, from IPC-namespace superblock misses [OCI-35] p.500, p.509 |
| Existing containers slow new ones | Docker creation 541 ms -> 1.5 s with over 1,000 containers [OCI-29] p.8; Lambda's N+1 network device cost [OCI-49] |
| Global locks serialize starts | `cgroup_mutex`, `css_set_lock`, `freezer_mutex` [OCI-31] p.57; RTNL [OCI-49]; `namespace_sem` (`namespace.c:4256`); `pernet_ops_rwsem` (`net_namespace.c:575`) |
| The template becomes the bottleneck | "the seed itself may become a bottleneck because only one instance can be cloned from it at a time (serially)"; mitigated by several seeds per type [OCI-35] p.511 |
| Resource leaks surface under churn | `bpf_jit_limit` exhaustion made seccomp installs fail [OCI-35] p.511 |
| Bimodal latency from page-cache state | the youki harness drops caches before every run, so its numbers include cold-cache start (S3); Hex's `bench/hex_start` does not [OCI-4] |

## 5. Fast-start techniques from research and practice

Each entry says what the system does, what was measured and where, and where it stops applying. All numbers are in the system's own
hardware and kernel; none is a measurement of a namespace container on a 6.x/7.x kernel unless stated.

### 5.1 SOCK: lean containers and generalized zygotes (USENIX ATC 2018) [OCI-27]

- **Lean containers (p.62).** Bind-mount a root from four host directories (a RAM-disk base, read-only packages, read-only handler code,
  a scratch dir), enter it with `chroot`, create the remaining namespaces with one `unshare` that leaves out **the mount and network
  namespaces**, and take a cgroup from a pre-created pool ("It is relatively expensive to create cgroups"). A Unix socket in the scratch
  dir carries control messages and file descriptors for namespaces and roots.
- **Zygote protocol (p.63).** A zygote helper that has already imported libraries is forked into a *new* container: the manager passes it fds for
  the new container's namespaces and root; the child does `fchdir`+`chroot` and `setns` into each namespace, forks again because `setns` on a PID
  namespace applies only to children, and the manager then moves the grandchild into a pooled cgroup. Three tiers of cache sit on top:
  paused handlers, a read-only install cache, and a tree of zygotes (the import cache) with a bounded-memory eviction rule (p.64).
- **Measured (S20, S15).** With zygotes off, 18x Docker's throughput and 19x lower latency at 10 concurrent no-op lambdas (76 req/s, 130 ms);
  zygote pre-initialization adds 3x throughput; cold start with a zygote is 32 ms against 3 ms to unpause a frozen container; with install
  and import caches a request that needed a >1 s package install took 20 ms (at least 45x); in an image-resizing case SOCK's cold platform latency was
  365 ms, 2.8x faster than AWS Lambda and 5.3x faster than OpenWhisk [OCI-27] p.65-67 (hardware and kernel in S15/S20).
- **Limits the authors state.** The method suits runtimes that load libraries dynamically; "it is not obvious how SOCK techniques could be applied to
  statically-linked applications (e.g., most Go programs)" (p.67). Zygotes undermine address-space randomization unless each child execs
  (their citation of Morula, p.67), and a malicious pre-imported package could subvert the protocol, so a zygote is only used for handlers
  that import a subset of what it imported (p.64).

### 5.2 Catalyzer: init-less booting and `sfork` (ASPLOS 2020) [OCI-28]

- **Diagnosis.** For gVisor, sandbox initialization alone is 22.3 ms (parse config 1.369 ms, boot I/O process 0.319 ms, boot sandbox process 0.757 ms,
  create and initialize kernel/platform 19.889 ms, Java SPECjbb, Figure 2) and a plain C hello takes 142 ms; restoring from a gVisor checkpoint
  still spends 135.9 ms recovering kernel and I/O state (p.469-471).
- **Mechanism.** Boot from a "func-image" with copy-on-write, file-mapped memory ("overlay memory"), separate deserialization from system-state recovery,
  reconnect I/O lazily, keep function-independent sandbox "Zygotes"; and `sfork` - fork a running *template sandbox*, handling the Go runtime's
  threads by briefly going single-threaded, cloning an overlay file system, and using user and PID namespaces to keep ids stable (§4, p.472-476).
- **Measured (S21).** Fork boot 0.97 ms (C hello) and under 2 ms (Java SPECjbb); cold / warm / fork boot 40 / 12 / 1 ms (Figure 7); Java runtime
  template cold boot 29.3 ms vs gVisor 659.1 ms and native 89.4 ms (Table 2); the template costs memory (a SPECjbb template is over 200 MB,
  per-function metadata 165-683 KB) [OCI-28] p.476-478. 
- **Limits.** The authors' own caveat: "caching does not help with the tail latency, which is dominated by the 'cold boot' in most cases"
  (p.469); fork boot is "more suitable for frequently invoked (hot) functions" because of memory (p.471). It is a gVisor/KVM design, so its numbers
  are a different class from a namespace container; what transfers is the structure (prepare once, clone many).

### 5.3 SEUSS: unikernel snapshots (EuroSys 2020) [OCI-29]

Snapshot stacks (a page-level diff per function on a base snapshot) and "anticipatory optimization" (run the dummy work before snapshotting) cut a
Node.js unikernel context from 7.67 ms cold to 0.82 ms hot (Figure 2); a function's cache miss fell from 500 ms to 7 ms; 54,000 instances fit where
3,000 Docker containers did; with 16-way parallel creation SEUSS built 128.6 contexts/s against 5.3 Docker containers/s, 45 processes/s and 1.3 Firecracker
microVMs/s (S22, p.8). It replaces Linux on the compute node, so it is a lower bound on what snapshotting can do rather than a design Hex can adopt;
its Docker numbers (541 ms to create one container, 1.5 s with over 1,000 present, 8.5 s mean at 16-way) are the useful transferable part (§4.9).

### 5.4 AFaaS: "Fork in the Road" (USENIX OSDI 2025) [OCI-35]

The most relevant recent source, because it is a production system whose authors measured what remains after fork-based start (Catalyzer) is in place:

- **Findings (p.499-504).** After Catalyzer's sub-millisecond sandbox fork, a cold start still took hundreds of ms to seconds in Ant Group's platform because of
  (1) control-path latency (18-25 ms, 30-40% of a cold start; binary load 18.02 ms), (2) resource contention under concurrency and sustained churn (`clone()` 1.45 -> 418 ms,
  network prepare 2 -> 21.14 ms, seccomp 7.58 -> 13.01 ms; throughput 110 -> 45 functions/s over an hour, S14), and (3) user-code initialization (238.73 of 275.53 ms for
  a Node.js function).
- **Design (p.504-507).** (1) FRI, a plugin interface that replaces per-start binary calls with function calls; (2) **pooling** of pre-allocatable objects - veth devices and
  cgroups ("1,000 veths and 600 cgroups" pre-allocated, p.511) - and **sharing** of inheritable ones - the seed and its forks share one network and one IPC namespace; (3)
  seccomp "pre-compiled during seed preparation" so a fork only installs; (4) tree-structured seeds (root, language, user-code) with multi-level fork, copy-on-write between levels
  (6.9-135 MB per user-code seed, p.510); (5) early destroy of finished instances because shutdown "can take over 10 seconds" for some runtimes.
- **Measured (S23).** Production start 5.45-9.41 ms; at x24 sustained concurrency 6.97-14.55 ms against 38.39-74.05 ms for Catalyzer alone (5.5x at the low end, 5.1x at the high end,
  computed here); end-to-end 1.80x-8.14x faster for eight production functions; for simple functions 3.76x-6.68x on mean and 6.31x-11.74x on P99, both against
  the Catalyzer baseline (p.508). Setup: 24-core Xeon Platinum 8163,
  512 GB, Linux 5.10; Kata 3.15 and gVisor 20250414 "exhibit orders-of-magnitude worse E2E performance under high concurrency" (p.509).
- **Lessons the authors list (p.511-512).** Reused state needs re-initialization after fork (random numbers, tokens); a seed is a serial bottleneck (deploy several per type); `bpf_jit_limit`
  leaks make seccomp installs fail under long churn; global cgroup locks held by neighbors still jitter a pooled design; "don't be a noisy neighbor during preparation".

### 5.5 RunD: cgroup pool and lightweight cgroups (USENIX ATC 2022) [OCI-31]

Targets Alibaba's secure containers (a container in a microVM), but its host-side finding is general: starting 2,000 containers concurrently makes cgroup creation *slower with more threads*
because of `cgroup_mutex`, `css_set_lock` and `freezer_mutex` (p.57). RunD merges the per-controller cgroups into one "lightweight cgroup" and pre-creates them in a pool that hands one out by
`rename` ("a lightweight operation without acquiring any global lock", p.59); cgroup creation took 0.09 / 0.10 / 0.14 s at 1 / 50 / 200 threads, "94% of the cgroups creation time" less (p.60).
Also found: CFS iterating over 10,000+ cgroups was 7.6% of host CPU cycles (p.57). Total: 88 ms single start, 200 sandboxes per second (S24).

### 5.6 Zygotes and fork servers: Android, Chromium, AFL

- **Android.** `ZygoteInit` "pre-initializes some classes, and then waits for commands on a UNIX domain socket. Based on these commands, forks off
  child processes that inherit the initial state of the VM" [OCI-12] `ZygoteInit.java:81-88` (DESIGN; the source gives no number). SOCK credits Android as the origin of the idea [OCI-27] p.63.
- **Chromium.** A zygote exists because "forking a process after some expensive setup has been performed can save time and share extra memory pages"; it amortizes dynamic-loader
  relocations at "~6 MB and 60 ms/GHz per process" (measured with `LD_DEBUG=statistics` on a Z620: 56,836,478 cycles of 73,899,158 in relocation) and up to about 8 MB of shared dirty pages per
  process, owns the namespace-sandbox bookkeeping, and keeps a reference to a consistent binary and library set [OCI-13] (MEASURED-VENDOR; Chrome is a very large binary).
- **AFL.** The fork server runs `execve`, dynamic linking and libc initialization once, then clones from a stopped image: "usually between 1.5x and 2x" for fast targets, "10x+" when
  placed after larger initialization ("deferred" mode) [OCI-14] `docs/technical_details.txt:446-470` (CLAIMED).
- **What transfers.** For a runtime whose workload is itself a heavy interpreter (the agent case), the seed can be the *workload's* initialized state, not only the sandbox's; that is
  SOCK's 3x and 45x and AFaaS's user-code seeds. For a trivial `/bin/true` it removes only the runtime's own exec and setup.

### 5.7 Adjacent: VM snapshots and remote fork (abstract-level only; depth is in the sibling note on substrates and sandboxes)

- Firecracker boots to application code in under 125 ms, with under 5 MB overhead, at up to 150 microVMs per second per host; pre-configured Firecracker's 99th percentile was 146 ms
  over 1,000 boots at 50-way concurrency (153 ms at 100-way) [OCI-30] p.419, p.427-428 (MEASURED-VENDOR, AWS).
- REAP: function execution from a snapshot is "95% higher, on average" than memory-resident, and record-and-prefetch "slashes the cold-start delays by 3.7x, on average" [OCI-40];
  FaaSnap: up to 3.5x faster than the state of the art and on average 3.5% slower than snapshots cached in memory [OCI-39]; SnapFaaS: "2-10x as fast as prior work for most functions" [OCI-38];
  MITOSIS: forks over 10,000 containers from one instance across machines within a second and cuts tail latency 89% under load spikes (needs RDMA) [OCI-41].

### 5.8 Kernel primitives that make the above cheaper

| Primitive | Needs | What it removes | Evidence |
|---|---|---|---|
| `clone3` | 5.3 | the 32-bit flag word and fixed arguments of `clone`: adds flags above bit 32, `cgroup`, `pidfd`, `set_tid`; glibc has no wrapper [OCI-11] clone(2) | man page |
| `CLONE_INTO_CGROUP` | 5.7 | the post-spawn `cgroup.procs` write and its global write lock | S8, §4.5 |
| `CLONE_PIDFD` | 5.2 | the extra `pidfd_open(pid)` after `fork`, race-free only under conditions listed in pidfd_open(2) | man page; a pidfd can be polled and waited on with `waitid` [OCI-11] pidfd_open(2) |
| `CLONE_CLEAR_SIGHAND` | 5.5 | resetting signal handlers in the child | [OCI-11] clone(2) |
| `open_tree`, `fsopen`/`fsmount`, `move_mount` | 5.2 | path-based `mount(2)` and its lookups; detached mounts can be assembled before entering the namespace | crun `NEWS:239-241, 163` (CLAIMED) |
| `mount_setattr` (incl. `MOUNT_ATTR_IDMAP`) | 5.12 (idmapped: ext4/xfs 5.12; erofs and overlayfs 5.19; squashfs 6.2; tmpfs 6.3) | recursive `chown`; remounts to change flags | S19; [OCI-11] mount_setattr(2) |
| `MOVE_MOUNT_BENEATH` | 6.5 (extended to the rootfs in 7.1) | `pivot_root` and the detached old root | [OCI-11] move_mount(2); [OCI-19] |
| `open_tree(OPEN_TREE_NAMESPACE)` | 7.0 | `unshare(CLONE_NEWNS)` + bind + `pivot_root` + unmount of the copied tree | S12, S13 |
| `CLONE_EMPTY_MNTNS` / `UNSHARE_EMPTY_MNTNS`, `FSMOUNT_NAMESPACE` | 7.1 | the whole parent mount-tree copy | no number; [OCI-19]; `fork.c:2708-2711` |
| `CLONE_AUTOREAP`, `CLONE_NNP`, `CLONE_PIDFD_AUTOKILL` | 7.1 | zombie reaping and `SIGCHLD`; a `prctl` in the child; `PDEATHSIG` races | no number; [OCI-19] |
| per-thread-group `cgroup.procs` lock (`favordynmods`) | present in v7.2 (introduction release not established here) | global write lock on cgroup migration, for migrating workloads only | [OCI-20]; `cgroup-v2.rst:194-200` |

(The version of each primitive's introduction comes from the man pages and, for 7.0 and 7.1, from the pull requests and from the flag's presence in `kernel/fork.c` at v7.1 and absence at v7.0 [OCI-6]. The flag values are those in the
UAPI headers as installed on this workstation (linux-api-headers 7.1-1): `CLONE_INTO_CGROUP` 1<<33, `CLONE_AUTOREAP` 1<<34, `CLONE_NNP` 1<<35, `CLONE_PIDFD_AUTOKILL` 1<<36, `CLONE_EMPTY_MNTNS` 1<<37, `UNSHARE_EMPTY_MNTNS` 0x00100000,
`OPEN_TREE_NAMESPACE` 1<<1, `FSMOUNT_NAMESPACE` 0x2 [OCI-7] `include/uapi/linux/sched.h:38-54`, `mount.h:65, 113`.)

### 5.9 Pooled or pre-created namespaces and cgroups

| Who | What is pooled or shared | Bound and refill | Result |
|---|---|---|---|
| SOCK [OCI-27] p.62 | cgroups (returned to the pool on termination) | not stated | reuse at least 2x creating (p.60) |
| RunD [OCI-31] p.59-60 | single lightweight cgroup per sandbox, renamed out of a pool; states idle/busy in a list | "pre-defined node capacity"; recycled on exit | -94% creation time |
| AFaaS [OCI-35] p.506, p.511 | veth devices and cgroups pooled; netns and IPC ns shared with the seed; seccomp precompiled | 1,000 veths, 600 cgroups pre-allocated; scheduler keeps instances within node capacity | start 5.45-9.41 ms; stable under x24 |
| PCPM [OCI-36] | "pause containers" holding a ready network endpoint, bound to a function container and salvaged afterwards | pool manager | about -80% at 100 concurrent |
| AWS Lambda [OCI-49] | 4,000 networks (namespace, tap, bridge, veth, tunnel) per worker, created at worker boot | fixed per worker; three minutes at boot because workers cycle rarely; "constant work" | Geneve tunnel setup 150 ms -> 200 us on the hot path (dummy VNI + eBPF rewrite); capacity 200 -> 4,000 per worker |
| Linux cgroup RFC [OCI-21] | a hidden kernfs tree of pre-made cgroups (v1) | pool size interface, delayed refill | not in the kernel: v7.2 `cgroup.c` has no pool; the thread's first reply was "Perhaps you shouldn't be creating that many containers all at once?" |

Two things follow from these. First, every system that pools bounds the pool and decides what happens when it runs dry (the sources that state it refill in the background or size the pool to node capacity), which is
the shape CLAUDE.md §2 requires. Second, the kernel is not going to fix cgroup creation for this workload soon, so the pool lives in user space.

### 5.10 Rootfs without copying: bind mounts, overlayfs, idmapped mounts, EROFS and composefs

- **Bind or tmpfs instead of union file systems when the base is shared:** SOCK measured bind mounts about twice as fast as AUFS at scale and avoided the mount namespace (p.58, p.62) [OCI-27];
  the 7.0/7.1 kernel work pushes toward building the tree from scratch over an empty namespace rather than copy-and-prune [OCI-19].
- **Idmapped mounts instead of `chown`** (S19): a user-namespaced container whose image must be owned by the mapped ids paid 4.964 s (BusyBox), 15.713 s (Ubuntu), 38.799 s (Fedora) for recursive chown against 0.135-0.171 s
  with an idmapped mount [OCI-25]; the kernel documentation says that for recursive ownership changes "for really large sets of files this becomes increasingly costly" [OCI-5] `idmappings.rst:598-602`. Filesystems that support them,
  by first kernel: ext4/xfs/FAT 5.12, btrfs/ntfs3 5.15, f2fs 5.18, erofs and overlayfs (ID-mapped lower and upper layers) 5.19, squashfs 6.2, tmpfs 6.3, cephfs 6.7, hugetlbfs 6.9 [OCI-11] mount_setattr(2). The mount must be a
  detached clone (`open_tree(OPEN_TREE_CLONE)`) with no writers.
- **Overlayfs.** Multiple lower layers are stacked in one mount; since v6.8 layers whose names contain colons can be given with `fsconfig(..., "lowerdir+", ...)`; `metacopy` copies up only metadata for metadata-only operations
  such as chown/chmod, and "data-only" lower layers can hold the file data it points at [OCI-5] `overlayfs.rst:350-450`. The kernel documentation gives no mount-time cost. A 2026 single-author study found overlayfs sequential-write throughput
  at 0.006-0.010x of a volume mount on Azure SSD/HDD (copy-up) but metadata operations 1.3-4.8x faster [OCI-43]; its method is weak (§3.1), so treat it as a lead.
- **EROFS and composefs.** EROFS is "designed for various use cases including immutable system images, container images, application sandbox images" and expects writes to be redirected "via overlayfs" [OCI-5]
  `erofs.rst:8-20`. In the composefs review thread one participant's `ls -lR` of a mounted image (the thread's benchmark image was 2.6 GB CentOS 9) took about 2.5x as long, and about 3x the memory, over EROFS+overlayfs as over composefs, and a
  reviewer asked for "benchmarks with images that correspond with the real life use case" instead [OCI-22]; **no source measures container start with EROFS or composefs.**
- **runc's binary seal** is itself a "rootfs without copying" case: a read-only overlayfs view of `/proc/self/exe` avoids copying a 15 MB binary into a memfd (S27) [OCI-1].
- **What Hex does today:** a recursive bind of the bundle's rootfs, spec mounts, `pivot_root` (§2.6); the rootfs itself is not copied.

## 6. Techniques worth stealing, ranked by measured speedup

"Speedup" is the best measured factor for the step or for the whole start as the source states it; the denominators differ, so the ranking orders *evidence strength per step*, not an end-to-end forecast. Kind and kernel
requirement are in the rows.

| Rank | Technique | Measured speedup and conditions | Kind | Needs | What would falsify it for Hex |
|---|---|---|---|---|---|
| 1 | Idmapped mounts instead of recursive `chown` (user-namespaced rootfs) | 37x / 92x / 271x container start (BusyBox / Ubuntu / Fedora), S19 | MEASURED-VENDOR | 5.12 + filesystem support (overlayfs, erofs 5.19; tmpfs 6.3) | Hex's bundles are already owned correctly (single-id rootless mapping) so there is no chown to avoid |
| 2 | Clone from a prepared seed instead of building a sandbox (sfork, zygote, snapshot-stack) | 0.97 ms fork boot vs 40 ms cold boot (about 40x) and 142 ms gVisor baseline (S21); 0.82 vs 7.67 ms (9.4x, computed here; S22); SOCK 3x throughput (S20); AFaaS 5.1-5.5x vs Catalyzer at x24 (S23); AFL 1.5-2x, 10x+ deferred (S26) | MEASURED-VENDOR; CLAIMED (AFL) | a long-lived seed process per node; `clone3` with namespace flags; safe re-initialization of entropy and tokens | A seed fork of Hex's actual sandbox (namespaces + mounts + cgroup + seccomp) is not under crun's 5.6 ms on Hex's host, or seed clone rate saturates under 24-way load (AFaaS: one clone at a time per seed) |
| 3 | vfork-class spawn (`CLONE_VM|CLONE_VFORK`, `posix_spawn`, `clone3`) from a small helper, never `fork` the large agent | 238x for a 1 GB written parent (7,581.8 vs 31.9 us), 1.65x for a small one (S17); 6.5 ms per GB of parent for fork (S18) | MEASURED-VENDOR (single author); MEASURED-INDEPENDENT (fork baseline) | none (all kernels); Rust has no `clone3` wrapper in std | The node agent's RSS stays small enough that `fork` is cheap, or a measured `clone3` from the agent is no faster than the helper |
| 4 | Mount namespace without the copy: `OPEN_TREE_NAMESPACE`, then `CLONE_EMPTY_MNTNS`/`FSMOUNT_NAMESPACE` | 7.8x on a clean host and 21.5x with many mounts for that step (S12); +40% creation throughput (S13) | MEASURED-VENDOR | 7.0 / 7.1 (the runner measured runs 6.19); crun's gating: no create-time hooks, no user namespace | Runner kernels stay below 7.0 or most Hex containers use hooks/userns, so the fast path is rarely taken; or the step is a small fraction of Hex's measured start |
| 5 | Cache or precompile the seccomp filter; install once in a seed; generate in parallel | 5.2x on whole `crun run` (28.9 -> 5.6 ms) and 3.1x on libseccomp time (S7); -30 ms in runc (S8); install latency 7.58 -> 13.01 ms under 24-way contention [OCI-35] p.503 | MEASURED-VENDOR; MEASURED-INDEPENDENT | a profile-keyed cache with a bound; fork inheritance by refcount (`fork.c:1777-1789`) | Hex has no seccomp today (§2.6): the saving is *avoided cost on a feature not yet built*, not a speedup over the current 14.4 ms |
| 6 | No per-start network namespace: skip, pool or share (with clean stacks) | 4.5x containers/s with no netns (200 -> 900 c/s, S15); about -80% startup at 100 concurrent (S25); -50 ms Docker (S8); clone() worst case 418 ms -> bounded (S14) | MEASURED-INDEPENDENT; MEASURED-VENDOR | a pool refill policy; isolation review (no cross-tenant reuse of live stacks) | Hex boxes need a private network by default, and pre-creation cost at 100,000 boxes per node exceeds its benefit (Lambda needed 3 minutes per 4,000 at boot) |
| 7 | `CLONE_INTO_CGROUP` and a cgroup pool | `cgroup.procs` write up to 50% of runc start, -27% when skipped (S8); pool -94% creation (S16); reuse at least 2x (S15) | MEASURED-INDEPENDENT; MEASURED-VENDOR | 5.7; pool bounded and reset between tenants | Hex measures the join as a small share on fedora, or reused cgroups leak accounting between tenants |
| 8 | No binary exec or shim hop per start; long-lived helper | binary load 18.02 ms in a Go engine; control path 18-25 ms = 30-40% of cold start [OCI-35] p.500, p.502; runc->crun 1.8-2x (S1, S2; no source attributes it to this alone) | MEASURED-VENDOR | a persistent per-node helper or in-process library | Hex's measured `__oci-init` exec is under 1 ms (static binary, no dynamic linking) |
| 9 | Bind or tmpfs rootfs instead of union file systems for shared bases | about 2x AUFS at scale (S15) | MEASURED-INDEPENDENT (primitive) | none | Hex's read-only base is already bind-mounted (§2.6) |
| 10 | Avoid per-start IPC namespace and mqueue superblocks | 110 -> 45 functions/s decline avoided under sustained churn (S14); 10.9 -> 0.1 ms and 16.8 -> 0.7 ms already fixed in current kernels (S11) | MEASURED-VENDOR | share or omit `mqueue` unless the spec asks | the decline does not reproduce on a 6.19/7.x kernel (the 5.10 result may be fixed) |

## 7. The shortlist for a 4-5x advantage over crun

**What the bar means in numbers.** If crun's 5.6 ms (S7) is the competitor, 4-5x is 1.1-1.4 ms for create, run, exit and delete of `/bin/true` (computed here); if the youki-harness crun (47.3 ms cold-cache, S3) is the competitor,
it is 9-12 ms. Hex's own 14.4 ms (S9) omits seccomp, cgroup and delete. The competitor's number on Hex's hardware is the first thing to measure (§8); this note only shows where published time goes.

**Partial accounting from cited numbers (indicative; different machines and kernels, each isolated measurement includes its own process spawn).** On a 5.19.15 kernel: network namespace create+destroy 1.5 ms, mqueue mount in a new mount
namespace 0.7 ms, IPC namespace 0.1 ms (S10, S11), and a mount-namespace setup of 0.34 ms (S12, newer kernel); these sum to about 2.6 ms, roughly 47% of crun's 5.6 ms (computed here). They are the pieces that pooling, sharing
and `OPEN_TREE_NAMESPACE` remove from the hot path. The remainder - process creation and exit, cgroup, seccomp load, FIFO gate and the workload's own exec - is where seeds and `clone3` flags act. **No source decomposes a
current crun start end to end**, so the split is a hypothesis to measure.

**The five techniques with the most evidence (they compose; none alone reaches 4-5x on a 5.6 ms baseline):**

1. **A per-node seed and `clone3` fast path** (rank 2, 3, 8, parts of 5 and 10). Prepare once: mount tree, seccomp filter installed, namespaces created, cgroup parent; then each box is one `clone3` with
   `CLONE_INTO_CGROUP | CLONE_PIDFD | CLONE_AUTOREAP | CLONE_NNP | CLONE_PIDFD_AUTOKILL` (7.1; `CLONE_INTO_CGROUP` and `CLONE_PIDFD` from 5.x) and either `CLONE_EMPTY_MNTNS` or a `setns` into a pre-built mount namespace.
   Evidence: Catalyzer's 0.97 ms fork (S21), AFaaS's 5.45-9.41 ms production start and 5x under concurrency (S23), SOCK (S20); kernel facts that a forked child shares the parent's seccomp filter by refcount and a seed removes the
   binary-exec hop. Risks the sources name: serial seed (clone one at a time), entropy and ASLR duplication, memory per seed, and that every published number is a VM-based or Python-lambda class. Mitigations are in AFaaS
   (several seeds per type, re-initialize unique state).
2. **Move kernel-object creation off the start path: pool and share.** Pre-created cgroups (bounded) entered with `CLONE_INTO_CGROUP`; pre-created or no network namespaces with clean stacks; no per-start IPC namespace or
   mqueue. Evidence: RunD -94% (S16), SOCK 4.5x with no netns (S15), PCPM -80% (S25), AFaaS tail stability (S14, S23), Lambda's pre-created 4,000 networks, and the independent 50%/27% cgroup finding (S8).
3. **Build the mount table without copying one:** `OPEN_TREE_NAMESPACE` (7.0) then `CLONE_EMPTY_MNTNS`/`FSMOUNT_NAMESPACE` (7.1), pre-built detached mounts for `proc`/`dev`/`tmpfs`, idmapped read-only base. Evidence: 7.8-21.5x on the
   step and +40% throughput (S12, S13), the O(mounts) exclusive-lock code (`namespace.c:4256-4268`), and 37-271x for idmapped rootfs ownership (S19). It needs a newer kernel than the last measured runner (6.19) and a fallback.
4. **Take seccomp off the hot path:** compile and cache the BPF keyed by profile hash (bounded, evicted by access time as crun does), and install it once in the seed so forks inherit it. Evidence: 28.9 -> 5.6 ms in crun (S7), 7.58-13.01 ms
   contention and `bpf_jit_limit` failures under churn ([OCI-35] p.503, p.511).
5. **Spawn without `fork` and without a pre_exec cgroup write:** for the `process` class too, replace `Command` + `pre_exec` + `cgroup.procs` write with a vfork-class `clone3(CLONE_INTO_CGROUP|CLONE_PIDFD|CLONE_VM|CLONE_VFORK)` (as Go's `os/exec` does for `runc exec`).
   Evidence: 238x fork vs vfork for a 1 GB parent (S17), 6.5 ms per GB (S18), the global-lock finding (S8).

**What is deliberately not on the list:** a language switch (runc -> crun's 1.8-2x is measured, but no source attributes it to the language alone; its author argues Go's fork/exec model is the cause [OCI-45], and Hex is already Rust),
union-filesystem tuning (no container-start data exists for composefs/EROFS), and VM snapshots (a different class, in the sibling note).

## 8. Open questions and gaps (what I could not verify)

1. **The competitor on our hardware.** No published runc/crun/youki figure was measured on Hex's runners (fedora, Linux 6.19; adacachy), with Hex's harness (`bench/hex_start/`), with the current crun (1.30.1 at the pinned commit; 1.15 in the youki and
   Gerdes runs). Nothing here replaces that baseline; it should also record seccomp default profile, cgroup limits, netns on and off, warm vs cold page cache (the youki harness drops caches), and concurrency x1, x12, x24 and a one-hour sustained run
   (AFaaS shows decay only over time).
2. **No p99, p99.9 or CDF** was published for any OCI runtime start in the sources read; only mean ± sigma, min/max or a range (S3-S5, S7). Hex's own record has N=200, too few for a p99.9 (`docs/research/runtime/matrix.md` states N >= 10,000).
3. **No end-to-end decomposition of a current crun start.** The only breakdown is Scrivano's, as a chain of fixes on two kernels (S7, S10-S12); hardware, CPU model and mount counts are not stated.
4. **No per-primitive cost table for Linux 6.x/7.x** for: cgroup v2 mkdir/rmdir, `proc`/`sysfs`/`devpts`/`tmpfs` mount, PID and user namespace creation, `execve` of a small static binary, seccomp load, `pivot_root`, and teardown (`cgroup.kill`, namespace
   release). I searched for them and found none; Docker-level decompositions (arXiv 2602.15214) report 8 ms for "namespace creation" without a method.
5. **No performance measurement for** `CLONE_EMPTY_MNTNS`, `UNSHARE_EMPTY_MNTNS`, `FSMOUNT_NAMESPACE`, `CLONE_AUTOREAP`, `CLONE_NNP`, `CLONE_PIDFD_AUTOKILL` (7.1) or for `OPEN_TREE_NAMESPACE` beyond S12-S13. The 73k vs 109k
   containers/60 s figure is secondary (Phoronix); the lore.kernel.org message is behind a bot check I could not pass.
6. **Which kernels Hex's runners run, and whether `favordynmods` is on.** Only fedora (6.19) is documented in the repo; adacachy's and the Mac/Windows hosts' are not. The 7.0/7.1 primitives need runtime detection and a fallback.
7. **Reuse hazards of pooled cgroups and namespaces** are not covered by any source beyond AFaaS's drain-connections rule and its clean-stack veth pairs: which counters survive in a reused cgroup (the kernel documents `memory.peak` as "since the creation of the cgroup or the most
   recent reset for that FD", `cgroup-v2.rst:1443-1448`), what a pool must scrub, and the isolation review for multi-tenant agent code.
8. **youki's and runc's seccomp paths** were not read; the comparison of seccomp costs rests on crun's code and measurements.
9. **Android zygote speedups** have no published number in the sources I opened; only the design statement (`ZygoteInit.java`) and Chromium's relocation measurements.
10. **Memory per created container** (RSS of the blocked `init` in the `created` state for runc's Go process versus crun's C process, and the page-cache sharing of rootfs mounts) is not published. The only memory figures are S2 (runtime process, 15,120 vs 3,752 KB) and the
    undefined/low-rigor ones in S5, S6.
11. **Teardown.** Delete/exit cost (the second half of crun's "create and destroy" 5.6 ms) is not separated from creation in any source; SOCK and Scrivano show RCU waits but only for namespaces.
12. **Whether the 2018-2022 netns/IPC/mqueue fixes hold under concurrency on current kernels.** AFaaS's collapse is on Linux 5.10 (with its IPC-namespace explanation); no source re-measures it on 6.x.
13. **Out of this note's scope and not read:** gVisor, Kata and agent-sandbox platforms (sibling notes), CRIU, Dirigent and Molecule (cited by AFaaS but not opened), E2B's network pool (the repository layout changed and I did not find the code), composefs on a real container start.

## References

All accessed 2026-10-22. Repositories were read at a pinned commit (files fetched from `raw.githubusercontent.com/<org>/<repo>/<commit>/<path>`), papers as the publisher's or authors' PDF converted
to text, web pages as served. Sources that could not be opened: `lore.kernel.org` and `git.kernel.org` (bot challenge), the English URL of the Red Hat crun post (404; the `/es/` URL serves the same English text).

**Code and specifications**

- [OCI-1] runc (opencontainers/runc), commit `976d95abc4eeece5f7e31ef043f465b885f5b79a`, 2026-10-21. https://github.com/opencontainers/runc/tree/976d95abc4eeece5f7e31ef043f465b885f5b79a . Read: `libcontainer/nsenter/{nsexec.c,README.md}`,
  `libcontainer/{container_linux.go,process_linux.go,init_linux.go,standard_init_linux.go,rootfs_linux.go,cmd_clone.go}`, `libcontainer/exeseal/{doc.go,cloned_binary_linux.go}`.
- [OCI-2] crun (containers/crun), commit `3fc26242d842308dc08a4be6c84f8b6404dadf28`, 2026-10-22. https://github.com/containers/crun/tree/3fc26242d842308dc08a4be6c84f8b6404dadf28 . Read: `README.md`, `NEWS`,
  `src/create.c`, `src/libcrun/{container.c,linux.c,status.c,cgroup.c,cgroup-setup.c,cgroup-cgroupfs.c,seccomp.c}`.
- [OCI-3] youki (youki-dev/youki), commit `cb3214c2204a03f1b99ae7c5870138439fd84345`, 2026-10-21. https://github.com/youki-dev/youki/tree/cb3214c2204a03f1b99ae7c5870138439fd84345 . Read: `README.md`,
  `.github/workflows/benchmark_execution_time.yml`, `crates/libcontainer/src/{container/builder_impl.rs,container/container_start.rs,process/fork.rs,process/container_main_process.rs,process/container_intermediate_process.rs,process/init/process.rs,notify_socket.rs,syscall/linux.rs}`.
- [OCI-4] Hex repository, branch `cp/t7-runtime` working tree (read-only), 2026-10-22: `crates/hex-runtime/src/linux/{sys.rs,process.rs}`, `crates/hex-runtime/src/linux/oci/{run.rs,init.rs,sys.rs,mod.rs,spec.rs}`, `docs/design/runtime.md`,
  `docs/requirements.toml`, `bench/hex_start/RESULT.md`, `docs/research/README.md`, `docs/research/runtime/matrix.md`.
- [OCI-5] Linux kernel, tag v7.2 (Torvalds et al.). https://github.com/torvalds/linux/tree/v7.2 . Read: `kernel/fork.c`, `kernel/cgroup/cgroup.c`, `fs/namespace.c`, `net/core/net_namespace.c`, `kernel/seccomp.c`,
  `Documentation/admin-guide/cgroup-v2.rst`, `Documentation/userspace-api/seccomp_filter.rst`, `Documentation/filesystems/{idmappings.rst,overlayfs.rst,erofs.rst}`.
- [OCI-6] Linux kernel, tags v7.0 and v7.1, `kernel/fork.c` (presence or absence of `CLONE_EMPTY_MNTNS`, `CLONE_AUTOREAP`, `CLONE_NNP`, `CLONE_PIDFD_AUTOKILL`). https://github.com/torvalds/linux/blob/v7.0/kernel/fork.c , https://github.com/torvalds/linux/blob/v7.1/kernel/fork.c .
- [OCI-7] Linux UAPI headers `include/uapi/linux/{sched.h,mount.h}` as installed by Arch `linux-api-headers 7.1-1` on this workstation (`/usr/include/linux/`), 2026-10-22.
- [OCI-8] Rust standard library, `library/std/src/sys/process/unix/unix.rs`, rust-lang/rust `master` (not pinned; line numbers may drift). https://github.com/rust-lang/rust/blob/master/library/std/src/sys/process/unix/unix.rs
- [OCI-9] Go standard library, `src/syscall/exec_linux.go`, golang/go `master` (not pinned). https://github.com/golang/go/blob/master/src/syscall/exec_linux.go
- [OCI-10] Open Container Initiative, *Runtime Specification*, `runtime.md` (main). https://github.com/opencontainers/runtime-spec/blob/main/runtime.md
- [OCI-11] Linux man-pages 6.19 (man7.org): clone(2) (2026-06-06) https://man7.org/linux/man-pages/man2/clone.2.html ; vfork(2) (2026-06-05) https://man7.org/linux/man-pages/man2/vfork.2.html ; posix_spawn(3) (2026-02-08)
  https://man7.org/linux/man-pages/man3/posix_spawn.3.html ; open_tree(2) https://man7.org/linux/man-pages/man2/open_tree.2.html ; fsmount(2) https://man7.org/linux/man-pages/man2/fsmount.2.html ; mount_setattr(2)
  https://man7.org/linux/man-pages/man2/mount_setattr.2.html ; move_mount(2) https://man7.org/linux/man-pages/man2/move_mount.2.html ; pidfd_open(2) https://man7.org/linux/man-pages/man2/pidfd_open.2.html ; unshare(2), pivot_root(2) (read, not cited).
- [OCI-12] Android Open Source Project, `ZygoteInit.java` (aosp-mirror/platform_frameworks_base `main`). https://github.com/aosp-mirror/platform_frameworks_base/blob/main/core/java/com/android/internal/os/ZygoteInit.java
- [OCI-13] Chromium project, `docs/linux/zygote.md`. https://github.com/chromium/chromium/blob/main/docs/linux/zygote.md
- [OCI-14] M. Zalewski, AFL, `docs/technical_details.txt`. https://github.com/google/AFL/blob/master/docs/technical_details.txt

**Kernel mailing-list and review material, issues, pull requests**

- [OCI-15] C. Brauner, "[PATCH v6 5/6] clone3: allow spawning processes into cgroups", 2020-02-05. https://lkml.iu.edu/hypermail/linux/kernel/2002.0/03037.html
- [OCI-16] K. Tkhai, "net: Introduce net_sem for protection of pernet_list" (commit `1a57feb847c5`), 2018-02-13. https://android-kvm.googlesource.com/linux/+/1a57feb847c56d6193f67d0e892c24e71f9e3ab1
- [OCI-17] C. Brauner, "[PATCH 0/2] mount: add OPEN_TREE_NAMESPACE", 2025-12-29. https://lwn.net/Articles/1052262
- [OCI-18] C. Brauner, "[GIT PULL 00/12 for v7.0] v7.0", 2026-02-06. https://lkml.iu.edu/hypermail/linux/kernel/2602.0/09464.html
- [OCI-19] C. Brauner, "[GIT PULL 00/12 for v7.1] v7.1", 2026-04-10 (message id `20260410-vfs-v71-b055f260060c@brauner`). https://patchew.org/linux/20260410-vfs-v71-b055f260060c@brauner/
- [OCI-20] Yi Tao, "[PATCH v4 1/3] cgroup: replace global percpu_rwsem with per threadgroup resem when writing to cgroup.procs", 2025-09-09. https://lkml.iu.edu/2509.1/02412.html
- [OCI-21] Yi Tao, "[RFC PATCH 0/2] support cgroup pool in v1", 2021-09-08, https://lkml.iu.edu/hypermail/linux/kernel/2109.1/00388.html ; reply by Greg KH, https://lkml.rescloud.iu.edu/2109.1/00423.html
- [OCI-22] "Re: [PATCH v3 0/6] Composefs: an opportunistically sharing verified image filesystem" (A. Goldstein quoting G. Scrivano), 2023-01-25. https://lkml.iu.edu/hypermail/linux/kernel/2301.3/01787.html
- [OCI-23] containers/crun issue #2086 "Use OPEN_TREE_NAMESPACE" and PR #2094 "Refactor mount operations to use procfd and new mount API" (comments by G. Scrivano, 2026-05-07; C. Brauner), read through the GitHub API.
  https://github.com/containers/crun/issues/2086 , https://github.com/containers/crun/pull/2094
- [OCI-24] G. Scrivano, crun commit `25790ca1e228` "linux: add OPEN_TREE_NAMESPACE support", 2026-06-24. https://github.com/containers/crun/commit/25790ca1e22893523c3e78db745271d44976104b
- [OCI-25] containerd PR #5890 "RFC: Initial support of idmapped mount points" (artqzn; opened 2021-08-17, merged 2023-09-05). https://github.com/containerd/containerd/pull/5890
- [OCI-26] Phoronix, "OPEN_TREE_NAMESPACE To Provide A Security & Performance Win For Dealing With Containers", 2026-01-19 (secondary; quotes a mailing-list message). https://www.phoronix.com/news/Linux-Open-Tree-Namespace

**Papers**

- [OCI-27] E. Oakes, L. Yang, D. Zhou, K. Houck, T. Harter, A. C. Arpaci-Dusseau, R. H. Arpaci-Dusseau, "SOCK: Rapid Task Provisioning with Serverless-Optimized Containers", USENIX ATC 2018 (2018-07-11).
  https://www.usenix.org/system/files/conference/atc18/atc18-oakes.pdf ; https://www.usenix.org/conference/atc18/presentation/oakes
- [OCI-28] D. Du et al., "Catalyzer: Sub-millisecond Startup for Serverless Computing with Initialization-less Booting", ASPLOS 2020 (2020-03-16), doi 10.1145/3373376.3378512. https://ipads.se.sjtu.edu.cn/zh/publications/DuASPLOS20.pdf
- [OCI-29] J. Cadden, T. Unger, Y. Awad, H. Dong, O. Krieger, J. Appavoo, "SEUSS: Skip Redundant Paths to Make Serverless Fast", EuroSys 2020 (2020-04-27), doi 10.1145/3342195.3392698. https://www.cs.bu.edu/~jappavoo/Resources/Papers/seuss.pdf
- [OCI-30] A. Agache et al., "Firecracker: Lightweight Virtualization for Serverless Applications", NSDI 2020. https://www.usenix.org/system/files/nsdi20-paper-agache.pdf
- [OCI-31] Z. Li et al., "RunD: A Lightweight Secure Container Runtime for High-density Deployment and High-concurrency Startup in Serverless Computing", USENIX ATC 2022 (2022-07-11). https://www.usenix.org/system/files/atc22-li-zijun-rund.pdf
- [OCI-32] A. Baumann, J. Appavoo, O. Krieger, T. Roscoe, "A fork() in the road", HotOS 2019 (2019-05-13). https://www.microsoft.com/en-us/research/uploads/prod/2019/04/fork-hotos19.pdf
- [OCI-33] K. Zhao, S. Gong, P. Fonseca, "On-demand-fork: A Microsecond Fork for Memory-Intensive and Latency-Sensitive Applications", EuroSys 2021 (2021-04-26), doi 10.1145/3447786.3456258. https://www.cs.purdue.edu/homes/pfonseca/papers/eurosys21-odf.pdf
- [OCI-34] J. Triplett, "io_uring_spawn" (slides), Linux Plumbers Conference 2022 (2022-09-12). https://linuxplumbersconf.org/event/16/contributions/1213/attachments/1012/1945/io-uring-spawn.pdf
- [OCI-35] X. Chai et al., "Fork in the Road: Reflections and Optimizations for Cold Start Latency in Production Serverless Systems", USENIX OSDI 2025 (2025-07-07).
  https://madsys.cs.tsinghua.edu.cn/publication/fork-in-the-road-reflections-and-optimizations-for-cold-start-latency-in-production-serverless-systems/OSDI2025-chai.pdf ; https://www.usenix.org/conference/osdi25/presentation/chai-xiaohu
- [OCI-36] A. Mohan, H. Sane, K. Doshi, S. Edupuganti, N. Nayak, V. Sukhomlinov, "Agile Cold Starts for Scalable Serverless", HotCloud 2019. https://www.usenix.org/system/files/hotcloud19-paper-mohan.pdf
- [OCI-37] S. Thomas, L. Ao, G. M. Voelker, G. Porter, "Particle: Ephemeral Endpoints for Serverless Networking", SoCC 2020 (2020-10-19), doi 10.1145/3419111.3421275. https://www.sysnet.ucsd.edu/~voelker/pubs/particle-socc20.pdf
- [OCI-38] Y. Tan, D. Liu, N. Li, A. Levy, "How Low Can You Go? Practical cold-start performance limits in FaaS", arXiv 2109.13319, 2021-09-27. https://arxiv.org/pdf/2109.13319
- [OCI-39] L. Ao, G. Porter, G. M. Voelker, "FaaSnap: FaaS Made Fast Using Snapshot-based VMs", EuroSys 2022 (2022-04-05), doi 10.1145/3492321.3524270. https://www.sysnet.ucsd.edu/~voelker/pubs/faasnap-eurosys22.pdf
- [OCI-40] D. Ustiugov, P. Petrov, M. Kogias, E. Bugnion, B. Grot, "Benchmarking, Analysis, and Optimization of Serverless Function Snapshots" (vHive, REAP), ASPLOS 2021, arXiv 2101.09355. https://arxiv.org/abs/2101.09355
- [OCI-41] X. Wei et al., "No Provisioned Concurrency: Fast RDMA-codesigned Remote Fork for Serverless Computing" (MITOSIS), OSDI 2023, arXiv 2203.10225. https://arxiv.org/abs/2203.10225
- [OCI-42] R. Niroula, J. Shan, X. Ding, "Performance Study of Serverless Workloads in Confidential Virtual Machines", arXiv 2609.04478 (preprint), 2026-09-03. https://arxiv.org/pdf/2609.04478
- [OCI-43] S. Khan, "Decomposing Docker Container Startup Performance: A Three-Tier Measurement Study on Heterogeneous Infrastructure", arXiv 2602.15214 (preprint), 2026-02-16. https://arxiv.org/pdf/2602.15214

**Engineering write-ups**

- [OCI-44] G. Scrivano, "How I decreased the time to create and destroy an OCI container to 5 milliseconds", Red Hat Developer blog, 2022-11-16. https://www.redhat.com/en/blog/speed-oci-containers
- [OCI-45] D. Walsh, V. Rothberg, G. Scrivano, "An introduction to crun, a fast and low-memory footprint container runtime", Red Hat Developer blog, 2020-08-03. https://www.redhat.com/es/blog/introduction-crun
  (the `/en/` URL returned 404 on 2026-10-22; the page body is English)
- [OCI-46] utam0k, "Hello, youki!" (youki first release), 2021-12-27. https://www.utam0k.jp/en/blog/2021/12/27/youki_first_release/
- [OCI-47] H. Gerdes, "Benchmarking what actually drive our containers", 2024-07-01 (page header; the index lists 2024-06-12). https://henrikgerdes.me/blog/2024-07-kubernetes-cri-bench/
- [OCI-48] GnTech, "Container Runtimes: runc vs crun for Docker Performance in Homelab", 2026-06-16. https://blog.gntech.me/posts/2026-06-16-container-runtimes-runc-vs-crun-docker/
- [OCI-49] W. Vogels, "The invisible engineering behind Lambda's network", All Things Distributed, 2026-04-22. https://www.allthingsdistributed.com/2026/04/the-invisible-engineering-behind-lambdas-network.html

