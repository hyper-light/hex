# Goal 5's added levers: fewer cgroup_mutex holds per box, and the device program loaded once per policy

Source notes for goal 5's added steps (Ada's call, 2026-10-31: the bar stays at a quarter of libcrun's create p50 and
p99.9), the two levers researched here:

- **(C) fewer holds of the kernel's host-wide `cgroup_mutex` per box lifecycle**: a box's cgroup reused for the next
  box of the same owner once it is provably clean, else a fresh one; the default device program attached once at the
  box root; a box ended through its init's pidfd rather than `cgroup.kill`;
- **(A) the device program loaded once per policy**: a create that looks for a loaded program first and loads only
  when none is there.

Each finding is in kiln research `01a190c51f137042a9697949c8fe2c7a` (goal 5's extension, plan
`01a190c51f07757ab54b09f1aada633c`), graded there from the sources below; the ids are given with each claim. This note
decides nothing: the decision goes in `docs/design/runtime-architecture.md` (decision 5) with these sources (CLAUDE.md
§5).

**Conventions.** Kernel files were read at the Linux v6.19 tag from
`https://raw.githubusercontent.com/torvalds/linux/v6.19/<path>`, retrieved 2026-10-31; `path:line` is a line there.
Man pages are man7.org's (man-pages 6.19). Papers were read from their USENIX PDFs with `pdftotext` (poppler) on
adacachy, 2026-10-31, the PDFs' sha256 given with each; page numbers are the proceedings'. "Probe" is the fedora probe
of 2026-10-31 (below, "The probes"), not of record.

## The baseline (run 4, fedora, create-breakdown-20261031T061841Z)

- Hex's create p50 through the seam is 2.020 ms against libcrun's 2.750 (`bench/competitive/probes/create_breakdown.md:352-353`).
- Of a create, pooled p50 (p99): the device program loaded (the verifier) 0.235 (0.333); its identity, hints and
  attach 0.041 (0.705); the cgroup's `mkdir` 0.130 (0.732); its bounds written 0.045 (0.091); the init's clone into its
  cgroup and new namespaces 0.174 (0.840) (`create_breakdown.md:368-376`). (f18)
- Each create-to-delete lifecycle takes `cgroup_mutex` 17 times, and at concurrency 32 the lock was held 83.4% of the
  run's time and handed off 6.8% more (`bench/competitive/probes/create_serial.md:200-201`, `:226`). (f19)

## Where Linux v6.19 takes cgroup_mutex

- **`mkdir` and `rmdir`** of a cgroup v2 directory: `cgroup_mkdir` calls `parent = cgroup_kn_lock_live(parent_kn, false);`
  (`kernel/cgroup/cgroup.c:5995`), `cgroup_rmdir` calls `cgrp = cgroup_kn_lock_live(kn, false);` (`:6225`), and
  `cgroup_kn_lock_live` takes the lock, `cgroup_lock();` (`:1732`). (f20)
- **`cgroup.kill`, `cgroup.procs`, `cgroup.subtree_control` writes**: `cgroup_kill_write` (`:4215`) calls
  `cgroup_kn_lock_live(of->kn, false)` (`:4229`); `__cgroup_procs_write` (`:5358`) the same (`:5367`);
  `cgroup_subtree_control_write` (`:3540`) with draining (`:3578`). (f21)
- **A clone with `CLONE_INTO_CGROUP`**: "CLONE_INTO_CGROUP is requested this function will grab cgroup mutex"
  (`cgroup_css_set_fork`, `:6682`), `if (kargs->flags & CLONE_INTO_CGROUP)` then `cgroup_lock();` (`:6694-6695`),
  released in `cgroup_css_set_put_fork` (`:6802-6803`) at the fork's end. A clone without the flag doesn't take it. (f22)
- **The release of a removed cgroup, in kworkers**: `css_killed_work_fn` (`:6044`, `cgroup_lock();` at `:6049`) for each
  controller css killed, `css_release_work_fn` (`:5622`, lock at `:5629`) for each css released, and
  `cgroup_bpf_release` (`kernel/bpf/cgroup.c:304`, lock at `:314`) for every removed cgroup, a program attached or not.
  With Hex's four controllers (`crates/hex-core/src/cgroup.rs:12`: cpu, io, memory, pids) and `perf_event`, which is
  "automatically enabled on the v2 hierarchy" (`Documentation/admin-guide/cgroup-v2.rst:2945-2946`), that is the five
  offlines and six releases (`self` with them) `create_serial.md:155-156` counted. (f23)
- **BPF attach and detach** of a cgroup program: `cgroup_bpf_attach` (`kernel/bpf/cgroup.c:906`, `cgroup_lock();` at
  `:914`) and `cgroup_bpf_detach` (`:1198`, `:1203`). (f24)
- **Not controller files.** `cgroup_file_write` hands a write to the controller's handler,
  `return cft->write(of, buf, nbytes, off);` (`cgroup.c:4309`), and neither `mm/memcontrol.c` nor
  `kernel/cgroup/pids.c` takes `cgroup_mutex` anywhere (read whole; `memcontrol.c:2620` names it in a comment only):
  `memory.max`, `memory.swap.max`, `pids.max`, `memory.reclaim` (`memory_reclaim` calls `user_proactive_reclaim`,
  `memcontrol.c:4601`) and `memory.peak` writes don't take it. (f25, inferred from reading the files)
- **`cpu.max` takes other host-wide locks**: `cpu_max_write` → `tg_set_bandwidth` → `tg_set_cfs_bandwidth`
  (`kernel/sched/core.c:9755`), which takes `guard(cpus_read_lock)();` and `guard(mutex)(&cfs_constraints_mutex);`
  (`:9461-9462`; the mutex is `static DEFINE_MUTEX(cfs_constraints_mutex);`, `:9437`). Not `cgroup_mutex`, but a box
  with a CPU bound serializes its bound's write host-wide on these. (f26)

So of the 17 holds: `mkdir` 1, `rmdir` 1, `cgroup.kill` 1, the device program's attach 1, the clone into the cgroup 1,
and the kworkers' 5 + 6 + 1 = 12.
- **Queries, not reads.** `BPF_PROG_QUERY` on a cgroup takes the lock (`cgroup_bpf_query`, `kernel/bpf/cgroup.c:1314`,
  `cgroup_lock();` at `:1319`), while reading `cgroup.events` (`cgroup_events_show`, `cgroup.c:3815-3819`) and
  `cgroup.stat` (`cgroup_stat_show`, under `rcu_read_lock();` at `:3842`) takes none. A check made of those reads costs
  no hold: four such reads took 0.0179 ms p50 and 0.0296 p99 on fedora (probe). (f61)

## What a cgroup keeps after its last process exits

- **Memory stays charged.** "A memory area is charged to the cgroup which instantiated it and stays" / "charged to the
  cgroup until the area is released." (`Documentation/admin-guide/cgroup-v2.rst:1964-1965`, Memory Ownership). On
  fedora a box's one process (48 MiB anonymous, a 64 MiB file written to the overlay, a 16 MiB `/dev/shm` file left)
  exited and its cgroup still read `memory.current 85049344 bytes`: file 83,886,080 (dirty 67,108,864), shmem
  16,777,216 of it, kernel 315,392 (probe). (f27)
- **`memory.reclaim` can't take it to 0.** It is "A write-only nested-keyed file which exists for all cgroups." and
  "the kernel can over or under reclaim from" the cgroup, returning EAGAIN when it reclaims less than asked
  (`cgroup-v2.rst:1409`, `:1418-1420`); it is in the document from v5.19, not v5.18. On fedora it returned EAGAIN after
  21 ms and left 892,928 bytes (the shmem pushed to zram swap); with the box's files removed and another reclaim,
  892,928 stayed (probe). (f28)
- **And it is slow and host-wide on an empty cgroup.** `memory.reclaim` on an emptied box took 1.74 ms p50, 5.26 ms p99
  (probe): `user_proactive_reclaim` retries `MAX_RECLAIM_RETRIES` times (`mm/vmscan.c:7735`, `:7819`) and calls
  `lru_add_drain_all();` (`:7786`) before its last try, which drains every CPU. It can't sit on a create's path, and at
  2,000 lifecycles a second it would take about 3.5 s of reclaim every second. (f29)
- **What stays is mostly charge cached per CPU, the rest kernel memory.** A `memory.max = 0` write, which drains those
  caches first (`drain_all_stock(memcg);`, `mm/memcontrol.c:4455`), took an emptied box from 544,768 bytes to 49,152,
  all of it kernel memory, but recorded an `oom` event in its `memory.events` (`memcg_memory_event(memcg, MEMCG_OOM);`,
  `:4467`; residual probe), so it can't clean a cgroup for the next box either. (f30)
- **`memory.peak` resets per descriptor only** (from v6.12): "A write of any non-empty string to this file resets it
  to the" / "current memory usage for subsequent reads through the same" file descriptor (`cgroup-v2.rst:1449-1450`;
  in v6.12's at `:1351`, absent from v6.11's). `peak_show` reads the cgroup's lifetime `pc->watermark` for a
  descriptor that never wrote (`memcontrol.c:4245-4246`). On fedora: 85,049,344 bytes through the written descriptor,
  153,350,144 through a fresh one (probe). (f31)
- **`pids.peak` never resets**: "The maximum value that the number of processes in the cgroup and its" / "descendants
  has ever reached." (`cgroup-v2.rst:2411-2412`); its cftype has only `.read_s64 = pids_peak_read,`
  (`kernel/cgroup/pids.c:403-405`). (f32)
- **The counters accumulate over the cgroup's life**: `cpu.stat`, `io.stat`, `memory.events`, `memory.stat`'s event
  counts and PSI's `total` ("The total absolute stall time", `Documentation/accounting/psi.rst:62`), with PSI's
  averages over "ten, sixty, and" three hundred second windows (`:60-61`) carrying an earlier box's stalls for
  minutes. A second process in the same cgroup carried `cpu.stat`'s `usage_usec` on from 179,390 to 179,673 (probe).
  (f33)
- **Removing a cgroup doesn't free it either.** At offline Linux moves its kernel memory objects to the parent
  (`memcg_reparent_objcgs(memcg, parent);`, `memcontrol.c:3343`), and the removed cgroup stays dying "for some time
  undefined time" while what is charged to it lingers (`cgroup-v2.rst:991-999`). So today's fresh cgroup per box leaves
  dying cgroups behind its page cache; reuse would instead charge that cache to the owner's next box. (f82, inferred)

## What a box sees of its own cgroup, and what it can change

- **It reads all of it.** "Each cgroup namespace has its own set of cgroup root directories." and a new namespace's
  creator's "current cgroups directories become the cgroup root" (cgroup_namespaces(7), DESCRIPTION). Hex's rendered
  containers get `Namespace { kind: "cgroup" },` (`crates/hex-core/src/bundle.rs:277`) and their own read-only cgroup2
  filesystem at `/sys/fs/cgroup` (`:483-485`, doc at `:415`). So a box reads its cgroup's counters and peaks, and in a
  reused cgroup those of the boxes before it. (f34, inferred)
- **It can change little, but not nothing.** fedora mounts cgroup2 `nsdelegate` (`rw,seclabel,nsdelegate,
  memory_recursiveprot,memory_hugetlb_accounting`), under which "the kernel rejects writes to all" / "files on a
  namespace root from inside the cgroup namespace, except for" the delegatable ones (`cgroup-v2.rst:554-557`;
  `cgroup.c:4298-4306`), which fedora's 6.19 lists as `cgroup.procs`, `cgroup.threads`, `cgroup.subtree_control`,
  `memory.oom.group` and `memory.reclaim` (`/sys/kernel/cgroup/delegate`), and a delegatee "can build sub-hierarchy
  under the directory" (`:560`). A box that can write its cgroupfs at all (a privileged one) can leave sub-cgroups, a
  `subtree_control`, a `memory.oom.group` and processes behind; the rendered bundle's mount is read-only. (f81, inferred)

## A cgroup v2 can't be renamed, and Hex's box has no owner

- cgroup v2's kernfs operations are `show_options`, `mkdir`, `rmdir` and `show_path`
  (`static struct kernfs_syscall_ops cgroup_kf_syscall_ops = {`, `cgroup.c:6237-6242`); kernfs refuses a rename without
  one, `if (!scops || !scops->rename)` → `return -EPERM;` (`fs/kernfs/dir.c:1310-1311`); on fedora,
  `rename of a cgroup v2 directory: [Errno 1] Operation not permitted` (probe). Hex names each box's cgroup by its id,
  `let dir = self.dir.join(id.as_str());` (`crates/hex-runtime/src/linux/cgroup.rs:168`). (f35)
- `BoxSpec` is an id, bounds and a class (`crates/hex-api/src/spec.rs:31-34`); `BoxRecord` names no cgroup
  (`crates/hex-core/src/record.rs:18`); recovery reads each cgroup's name as a box id,
  `if let Ok(id) = BoxId::new(&name) {` (`crates/hex-node/src/node.rs:407`). Reuse needs an owner in the spec and the
  cgroup's name in the record, both append-only, and recovery that tells an idle cgroup from a box's. (f64, inferred)

## What others did

- **SOCK** (Oakes et al., USENIX ATC 2018; PDF sha256 46eaf854117d0158; Linux "4.13.0-37", p. 58): "Reusing is at
  least twice as fast as" "creating new cgroups each time." (§2.3, p. 60); OpenLambda keeps a pool that "be used upon
  SOCK container creation; cgroups are returned to the pool after container termination." (§4.1, p. 62). No condition
  on owner or state is stated. (f36; the PDF's f16 quotes, checked here through `pdftotext`)
- **AFaaS** (Chai et al., USENIX OSDI 2025; sha256 5981a313888e9135): a "cgroup pool that recycles cgroups when the
  sandbox instance" is deleted (§4.2.1, p. 506); "There is only one container inside the cgroup, and each cgroup" "is
  properly released after the exit of the function." (Discussion, Security Analysis, p. 511); and other workloads may
  "hold the global lock of a cgroup", while "recycling of a cgroup to an instance still requires lightweight" locks,
  hence jitter (§8 (2), p. 512). (f37) Its sandboxes are built on gVisor (its Discussion), so a function sees gVisor's
  kernel rather than the host's cgroup files (inferred).
- **RunD** (Li et al., USENIX ATC 2022; sha256 b2a3205ad082516e): cgroup creation is serialized by "css_set_lock,
  freezer_mutex) to serialize cgroup operations." (§3.3, p. 57); it pools pre-created cgroups and renames them, which
  "is a lightweight operation without acquiring any global lock." (§4.4, p. 59). Not on Hex's kernel: v6.19's v1
  rename takes the lock (`cgroup1_rename`, `kernel/cgroup/cgroup-v1.c:870`) and v2 has none (f35). (f38) It also
  reports the scheduler's load tracking iterating "over all cgroups and processes" at over 10,000 cgroups, 7.6% of a
  node's cycles (§3.3, p. 57): idle cgroups kept for reuse need a bound. (f65)
- **Lambda and Firecracker** (Agache et al., USENIX NSDI 2020; sha256 05eb1dc65d9f1740): "Slots are only ever used for
  a single function, and a" single concurrent invocation, "for many serial invocations of the function. The MicroVM"
  "and the process the function is running in are both re-used" (§4.1.1, p. 425). AWS's lifecycle page: "if another
  request arrives for the same function, Lambda can reuse the environment", and "the Lambda reset does not clear the"
  `/tmp` directory (`docs.aws.amazon.com/lambda/latest/dg/lambda-runtime-environment.html`). The reuse boundary is one
  function, an owner's, and the reused environment's state is the function's to see. (f39)

## The default device program at the box root

- **How the kernel composes programs.** `BPF_F_ALLOW_OVERRIDE`: "the program in this cgroup yields to sub-cgroup
  program."; `BPF_F_ALLOW_MULTI`: "that cgroup program gets run in addition to the program in this cgroup."
  (`include/uapi/linux/bpf.h:1181`, `:1184`). `compute_effective_progs` adds an ancestor's programs past the first one
  found only if that ancestor's are MULTI (`kernel/bpf/cgroup.c:429`, `:444`, `:459`), and a device check is refused
  when any program in the array returns 0 (`run_ctx.retval = -EPERM;`, `:87`). (f40)
- **Measured.** With Hex's default program attached once at a box root with OVERRIDE, a box with none of its own was
  refused `/dev/kmsg` and allowed `/dev/null`; a box with its own allow-all program opened both, its effective programs
  its own and the container's MULTI ancestor's; with the root's attached MULTI, that box was refused `/dev/kmsg`
  (probe). (f41)
- **Attaching under it.** `hierarchy_allows_attach` lets a box attach below an OVERRIDE parent (`:415`, `:420`); a
  cgroup holds one OVERRIDE program and "Attaching another program on top of NONE or BPF_F_ALLOW_OVERRIDE will" /
  "release old program and attach the new one. Attach flags has to match." (`bpf.h:1186-1189`): the root's default is
  replaced in one call. (f42)
- **Two things in Hex it meets.** A policy that allows everything gets no program today (`resources.rs:601-602`,
  `run.rs:983`), so under a root default it would inherit the default: allow-all must become an explicit program. (f43,
  inferred) Process boxes and oci boxes share one root (`node.rs:472`, `:511`), so the default must sit on a root of
  the oci boxes' own. (f44, inferred)
- **An attachment outlives its loader.** "whether a specific program continues to live" / "inside the kernel depends
  on how it is further attached" (bpf(2)); runc: "Once the program is attached, the kernel keeps it alive via the
  cgroup" (opencontainers/cgroups v0.1.0 `devices/ebpf_linux.go:367`); the cgroup puts it only when released
  (`bpf_prog_put(pl->prog);`, `kernel/bpf/cgroup.c:326`). The root's program survives an agent restart. (f53)
- Checking the root's attachment takes a hold (`BPF_PROG_QUERY`, f61): at node start, never per create.

## Ending a box through its init's pidfd

- "If the "init" process of a PID namespace terminates, the kernel" / "terminates all of the processes in the namespace
  via a SIGKILL" (pid_namespaces(7)); v6.19's `zap_pid_ns_processes` stops new processes entering
  (`kernel/pid_namespace.c:200-201`), sends SIGKILL (`:231`) and keeps the init unreaped until all are gone
  (`:263-265`); an exiting init reaches it from `find_child_reaper` (`kernel/exit.c:622`). (f45)
- SIGKILL from an ancestor namespace is delivered to an init without a handler: "signals are forcibly delivered when
  sent from an ancestor PID" namespace (pid_namespaces(7)); `force = !task_pid_nr_ns(current, task_active_pid_ns(t));`
  (`kernel/signal.c:1190-1191`). (f46)
- **Not equivalent to `cgroup.kill`** when a process in the box's cgroup is outside the init's pid namespace: on fedora,
  after `pidfd_send_signal(SIGKILL)` to the init, its namespace left the cgroup within 0.73 ms but the outside process
  stayed and `populated` stayed 1, until a `cgroup.kill`, which kills "all processes located in the affected cgroup"
  tree and "will deal with concurrent forks appropriately" (`cgroup-v2.rst:1059`, `:1062`; probe). (f47)
- Programs that entered by `setns` and fork, as Hex's exec'd programs do, die with the namespace but stay zombies until
  their parent, the agent, reaps them (`pid_namespace.c:248-250`), so the agent's reap stays in delete
  (`node.rs:442-446`). (f48) A process box has no init (`node.rs:417`): only `cgroup.kill` ends it. (f49, inferred)
  `kernel/signal.c` never takes `cgroup_mutex`, so the pidfd saves `cgroup.kill`'s one hold (f21). (f50, inferred)

## The device program loaded once per policy

- **Privileges.** `BPF_PROG_GET_FD_BY_ID` needs CAP_SYS_ADMIN (`kernel/bpf/syscall.c:4794`, `:4803`). The translated
  instructions are dumped only to `bpf_capable()` callers (`:5035-5037`) and withheld when the program was blinded and
  the caller lacks raw-dump rights (`:5052`, `:5063`); fedora runs `net.core.bpf_jit_harden = 0` (residual probe).
  (f51, f52)
- **Lifetime and ids.** A program is freed when its last reference goes (`if (atomic64_dec_and_test(&aux->refcnt)) {`,
  `:2421`); ids come from `idr_alloc_cyclic(&prog_idr, prog, 1, INT_MAX, GFP_ATOMIC)` (`:2340`), so an id can come to
  name another program. (f53)
- **The tag is no proof.** It is the first `BPF_TAG_SIZE` (8, `bpf.h:6629`) bytes of a digest of the loaded
  instructions with map descriptors zeroed (`kernel/bpf/core.c:308`): SHA-256 from v6.18 (`:330`), SHA-1 through v6.17
  (v6.17's `core.c:354`). (f54)
- **On fedora the translation is the program.** Hex's default device program (100 instructions) translates to exactly
  its own 800 bytes (`translated instructions equal the loaded instructions byte for byte: True`, probe), so a loaded
  candidate can be proven to be it without a copy; crun warns "Some platforms (e.g. ppc64) modify an EBPF program as it
  is" loaded (`src/libcrun/cgroup-systemd.c:1470-1472`), so a mismatch must fall back to a load. (f55)
- **Cost.** Loading it took 0.0834 ms p50, 0.1454 p99 (500 loads back to back); opening the loaded one by id, reading
  its identity and translation and comparing took 0.0052 ms p50, 0.0085 p99, matching 500 of 500 (probe). Run 4's load
  inside a create is 0.235 ms p50 (f18): a load in the probe's tight loop runs warmer. (f56)
- **runc and crun load per container.** runc v1.5.2 builds with `github.com/opencontainers/cgroups v0.1.0`
  (`go.mod:19`), whose `loadAttachCgroupDeviceFilter` loads (`devices/ebpf_linux.go:363`) and attaches MULTI (`:374`);
  crun 1.30.1's cgroupfs path loads per container (`cgroup-resources.c:694` → `ebpf.c:496`) and attaches MULTI
  (`ebpf.c:414`); only crun's systemd driver compares a pinned program's translation with its own
  (`cgroup-systemd.c:1464`). (f57)
- **Hex loads first today**: `let ours = sys::load_device_program(insns)?;` (`run.rs:1009`) because the copy's tag keys
  the hints (`.candidate(&mine.identity.tag)`, `:1018`) and its translation is what a candidate must equal
  (`devshare.rs:96`). (f58)
- **What a lookup before the load must check** (f59, inferred): the candidate opened by id is a cgroup device program
  whose translation equals the instructions Hex would load, byte for byte; the open descriptor pins it between the check
  and the attach; the hints are keyed by Hex's own digest of its instructions, not the kernel's tag; a withheld or
  different dump means a load, as today. In the node agent, which outlives its containers, the program needs no lookup
  at all: the agent keeps it open per policy and only attaches it, as decision 5 already says
  (`docs/design/runtime-architecture.md:846`). (f60, inferred)

## What step 6 rests on beyond the findings (Linux v6.19, read 2026-11-02)

Read at the v6.19 tag from `https://raw.githubusercontent.com/torvalds/linux/v6.19/<path>`, retrieved 2026-11-02
(sha256 of each file's first 16 hex digits: `kernel/exit.c` bc44f0b5489c871c, `kernel/sched/core.c` bd80c5a7f8094301,
`kernel/cgroup/cgroup.c` 73dff34e37043b02, `mm/memcontrol.c` 42fe3732de9ad604, `kernel/bpf/syscall.c` 6063721f94861be7,
`block/blk-iolatency.c` 9a1b56ebea7f7397, `kernel/pid_namespace.c` 1658cbd93fea367c). Each fact below is what the code
there says; none is graded in kiln's research yet.

- **When a dead task leaves its cgroup.** `do_exit` runs the controllers' exit callbacks, `cgroup_task_exit(tsk);`
  (`kernel/exit.c:975`), before `exit_notify(tsk, group_dead);` (`:983`), which makes the task a zombie and its pidfd
  readable. The task leaves its css_set, and `populated` falls, only at its last switch: `finish_task_switch` calls
  `cgroup_task_dead(prev);` for a `TASK_DEAD` task (`kernel/sched/core.c:5156`), whose `do_cgroup_task_dead` moves it
  off the css_set (`kernel/cgroup/cgroup.c:6998`). So an init's pidfd reads ended a moment before its cgroup can read
  empty, and a delete waits on `cgroup.events` for that, rather than on a time.
- **When a pid namespace's init has ended, its processes are gone.** `zap_pid_ns_processes` waits until
  `pid_ns->pid_allocated == init_pids` (`kernel/pid_namespace.c:270-275`), for "EXIT_ZOMBIE process whose parents
  processes are outside of the pid namespace. Such processes are created with setns()+fork()" (`:248-250`) as for the
  rest, so "when the init process of a pid namespace is reaped all of the processes in the pid namespace are gone"
  (`:262-265`). Anything still in the box's cgroup after its init's end is outside the namespace (f47).
- **Listing a cgroup's tasks takes no `cgroup_mutex`.** `cgroup.procs` and `cgroup.threads` are read through
  `__cgroup_procs_start` (`kernel/cgroup/cgroup.c:5241`), which walks the css_set lists with `css_task_iter_start`
  (`:5256`, `:5260`) under `css_set_lock`, a spinlock. Only their writes check `cgroup_may_write` under the mutex
  (`:5290-5295`).
- **The check's reads take no lock either.** `cgroup_subtree_control_show` (`:3160`), `cgroup_type_show` (`:3691`),
  `cgroup_events_show` (`:3815`) and `cgroup_freeze_show` (`:4144`) read fields of the cgroup; `cgroup_stat_show`
  (`:3825`) reads under RCU; `memory_oom_group_show` (`mm/memcontrol.c:4564`) a `READ_ONCE`; `memory_current_read`
  (`:4229`) a page counter (f61).
- **A peak per descriptor.** `peak_write` (`mm/memcontrol.c:4283`) resets the peak a descriptor reads (f31).
- **`io.latency` off.** A `target=max` write sets the latency target to 0, off (`block/blk-iolatency.c:873-874`), so a
  cgroup handed on can have a bound its last box set taken off.
- **Who can attach a device program to its own cgroup.** Loading a `BPF_PROG_TYPE_CGROUP_DEVICE` program needs
  `CAP_BPF` (or CAP_SYS_ADMIN), `if (type != BPF_PROG_TYPE_SOCKET_FILTER && type != BPF_PROG_TYPE_CGROUP_SKB &&
  !bpf_cap)` (`kernel/bpf/syscall.c:2935-2938`), and CAP_NET_ADMIN, since the type is one of
  `is_net_admin_prog_type`'s (`:2765-2778`, checked at `:2940`). A box holding both could attach a program to the
  cgroup it can open, which would outlive it there, so it never shares one.
- **A cgroup `cgroup.kill` was written to kills every task later cloned into it.** `cgroup_css_set_fork` samples a
  `CLONE_INTO_CGROUP` clone's `kill_seq` before it sets `kargs->cgrp`, so from the forking task's own cgroup
  (`kernel/cgroup/cgroup.c:6702-6705`; `kargs->cgrp = dst_cgrp;` at `:6768`); `cgroup_post_fork` compares it with the
  target's (`:6902`) and kills the child when they differ (`:6941`), and each `cgroup.kill` raises the target's
  (`:4186`). Measured: a child cloned into a fresh cgroup lived, into one `cgroup.kill` had been written to died of
  SIGKILL, twice, on adacachy (7.2.9) and fedora (6.19.10) (`bench/competitive/probes/kill_seq.md`). So a slot whose
  box was ever killed so is never handed on.
- **A write to a file is seen by a watch on its directory.** inotify(7): IN_MODIFY is among the events that "may occur
  both for the directory itself and for objects inside the directory", reported with the object's name. The tests
  watch a cgroup's directory, since `cgroup.kill` is write-only and inotify_add_watch(2) needs read access to what it
  watches: "EACCES Read access to the given file is not permitted."

## What holds concurrency 32 once the cgroup lock is free (Linux v6.19, read 2026-11-03)

Read at the v6.19 tag as above, retrieved 2026-11-03 (sha256 first 16 hex digits: `fs/namespace.c` 9bf94300e1326c3f,
`fs/fs_struct.c` 65ea0740630bfaab, `drivers/net/loopback.c` dcf721b20057b24a, `net/core/dev.c` 24f1d8a24ac03b8b,
`net/core/rtnetlink.c` fa6c25afbf2e0679, `fs/proc/proc_sysctl.c` 268362f9043ecf61), for the waits
`bench/competitive/probes/create_serial.md`'s runs 9 and 10 found.

- **One mount lock for the host.** `static DECLARE_RWSEM(namespace_sem);` (`fs/namespace.c:89`). `namespace_lock`
  takes it to write (`:1723-1725`); `do_lock_mount` calls it (`:2747`) for every mount, bind (`do_loopback`, `:2987`)
  and `pivot_root` (`:4554`), and `LOCK_MOUNT` releases it only at the end of the caller's scope, through
  `__cleanup(unlock_mount)` (`:2799-2801`). A namespace's copy takes it to write too (`copy_mnt_ns`, `:4117`), and a read
  of a mount table takes it to read (`m_start`, `:1541`).
- **`pivot_root` walks every thread inside that hold.** `chroot_fs_refs(&root, &new);` (`fs/namespace.c:4606`) runs
  `for_each_process_thread` under `tasklist_lock`, taking each thread's `task_lock` and its `fs->seq` to write
  (`fs/fs_struct.c:54-77`).
- **One network lock for the host.** A new network namespace registers its loopback device (`loopback_net_init`,
  `register_netdev(dev)`, `drivers/net/loopback.c:207`, `:218`), which takes RTNL: `rtnl_net_lock_killable(net)`
  (`net/core/dev.c:11519`) is `rtnl_lock_killable()` then the namespace's own mutex (`net/core/rtnetlink.c:229-236`),
  and `rtnl_lock_killable` is `mutex_lock_killable(&rtnl_mutex)` (`:89-91`) on `static DEFINE_MUTEX(rtnl_mutex);`
  (`:76`). The init's request bringing that loopback up is an `RTM_NEWLINK`, handled by `rtnl_newlink` under the same
  lock (`crates/hex-runtime/src/linux/oci/net.rs`).
- **One sysctl lock for the host.** `static DEFINE_SPINLOCK(sysctl_lock);` (`fs/proc/proc_sysctl.c:88`) is taken by
  `__register_sysctl_table` (`:1370`, at `:1389` and `:1398`), the call every sysctl table is registered through, and
  by `sysctl_mkdir_p` (`:1299`) for each directory of the table's path, through `get_subdir` (`:995`, at `:1002` and
  `:1011`). Run 10's samples spin there as the boxes' network namespaces are made.

## The probes (fedora, 2026-10-31, not of record)

- **Host:** fedora (`kiln-a094a12d`, Intel Core i9-14900HX, 32 threads), Linux 6.19.10-300.fc44.x86_64, swap on
  `/dev/zram0`. Load: 12.68 at the start of the main probe, 10.45 at the residual probe's; no Hex bench ran (the lead
  confirmed Focal's fleet was down).
- **Main probe** (kiln run r2, 07:18:27Z-07:18:31Z): `python3 -I probe.py 500` (sha256 43e5b8f6b77f5508, kept in
  `~/.cache/hex-t7/goal5-cgroup-lock-probe` on fedora) as root in a privileged `registry.fedoraproject.org/fedora:44`
  container (`--cgroupns=private --network=none --cpuset-cpus=4-31`, the host's `/usr`), its processes moved to a leaf
  and `+cpu +io +memory +pids` enabled at its cgroup root, as `probes/join_cost.sh` does. It loads Hex's default device
  program (`resources::compile` of the default state, rebuilt in Python) 500 times against 500 lookups by id; runs a box
  workload and reads what the cgroup keeps, `memory.peak` through a written and a fresh descriptor, a rename,
  `memory.reclaim` three times, then again with the files removed, a second process's `cpu.stat`, 200 four-file clean
  checks against 200 `mkdir`+`rmdir`, 50 reclaims of the emptied box; attaches the default at a box root with OVERRIDE
  then MULTI beside a box with its own allow-all program and opens `/dev/null` and `/dev/kmsg` from each; and kills a
  pid namespace's init through its pidfd with a process of the cgroup outside the namespace.
- **Residual probe** (07:19:15Z): `residual.py` in the same container shape with `--network=host`: the host's BPF JIT
  sysctls, an emptied box's `memory.current` before and after a reclaim, and a `memory.max = 0` write.
- **Mount check**: fedora's cgroup2 mount options, `/sys/kernel/cgroup/delegate` and `features`.

## The levers

Against run 4 (f18) and the 17 holds (f19), on Linux v6.19.

| Lever | Holds taken off a lifecycle | ms off create (run 4, p50 / p99) | Finding ids |
|---|---|---|---|
| a box's cgroup reused for the owner's next box | 14: `mkdir`, `rmdir`, the kworkers' 12 | `mkdir` 0.130 / 0.732 | f20, f22, f23, f27-f39, f61-f65, f81, f82 |
| the default device program at the oci box root | 1: the attach, for default-policy boxes | load and attach 0.276 / attach's p99 0.705 | f24, f40-f44, f53, f61 |
| a box ended through its init's pidfd | 1: `cgroup.kill` | none on create; delete loses a contended write | f21, f45-f50 |
| the device program loaded once per policy | none | load 0.235 / 0.333 | f51-f60 |

All three cgroup levers together leave one hold, the clone into the cgroup, for a default-policy oci box given a clean
cgroup, and 15 when no cgroup can be reused (f62, inferred). Exec'd programs still take one each.

**Cgroup reuse.**
- *Risks.* The next box inherits what the last one left charged (f27-f30), can read its predecessors' cumulative
  counters and peaks (f31-f34), and inherits whatever it could change: sub-cgroups, `subtree_control`,
  `memory.oom.group` (f81). Its name can't follow the box (f35), and nothing says whose a box is (f64). Idle cgroups
  cost kernel memory and scheduler walks (f65).
- *Guards.* Reuse only within one owner, named in the spec (append-only), and never for a box without one or one that
  can write its cgroupfs (privileged). Name cgroups by slot, never by box id, and record the slot in the box record
  (append-only). Hand a cgroup to the next box only when reads that take no lock show it clean (f61): `populated 0`,
  `nr_descendants 0` and `nr_dying_descendants 0`, an empty `cgroup.subtree_control`, `cgroup.type` domain,
  `cgroup.freeze 0`, `memory.oom.group` as Hex set it, `memory.current` at most a measured threshold, and the same
  device policy. Write every bound again (no hold, f25). Report usage as deltas from a baseline read at reuse, and
  `memory.peak` through a descriptor written at reuse (6.12 on, f31); don't report `pids.peak` per box (f32). Never run
  `memory.reclaim` per lifecycle (f29). Bound the idle cgroups per owner and per node, evicting the oldest by `rmdir` off
  the create path. Recovery removes idle cgroups (none leaked, T-T7.1.2). Any check failing means today's path: `rmdir`
  and a fresh `mkdir`.
- *Falsifiers.* create_serial's locks mode counting more than 3 holds per lifecycle on the reuse path, or more than
  17 on any; the sustained rate at concurrency 32 not rising once the holds fall; a box handed a cgroup the checks
  called clean that wasn't (a sub-cgroup, a process, a changed `memory.oom.group`); the reuse rate on real owners' boxes
  near 0 because residual memory sits above the threshold.

**The default device program at the box root.**
- *Risks.* A box whose policy isn't the default inheriting it (allow-all has no program today, f43); process boxes
  restricted (f44); a box with its own program still bound by the root's (only if the root's were MULTI, f40-f41).
- *Guards.* A root of the oci boxes' own; the root's program attached with OVERRIDE, checked and replaced at node start
  only (f42, f61); every policy but the default, allow-all included, attached as the box's own; a default box attaches
  nothing. Rootless nodes attach nothing, as today.
- *Falsifier.* A box with no program of its own reaching a device the default denies, or a box with its own program
  decided by anything but its own and the MULTI ancestors' (the probe's `/dev/kmsg` test, f41); a process box losing a
  device.

**A box ended through its init's pidfd.**
- *Risks.* Processes in the cgroup but outside the init's namespace survive (f47); a bundle without a pid namespace of
  its own, and process boxes, have no such init (f49); exec'd programs linger as zombies (f48).
- *Guards.* Only for an oci box whose init Hex cloned with `CLONE_NEWPID`; after the kill and the agent's reap of its
  exec'd programs, `cgroup.events` must read `populated 0` within a bound, else `cgroup.kill` as today.
- *Falsifier.* A delete that ends with the cgroup populated, or a kill path that takes the lock (f50).

**The device program loaded once per policy.**
- *Risks.* Attaching a program that isn't the policy's (an id reused, f53; a tag collision, f54); a platform that
  rewrites programs (f55).
- *Guards.* In the agent, the program kept open per policy (bounded, the oldest closed past the bound; an attached
  program lives on, f53), keyed by Hex's own digest of its instructions: no lookup. On the command line, a lookup by id
  that attaches only a cgroup device program whose translation equals Hex's instructions, else a load (f59).
- *Falsifier.* Create p50 not falling by about part 12 (0.235 ms) once a policy is loaded; any attach of a program whose
  translation differs from the policy's instructions.

## Is guarded reuse sound?

Sound for isolation and for Hex's own accounting, not transparent to the box (inferred from f27-f35, f39, f64, f81):

- Nothing crosses owners, which is the line Lambda and Firecracker hold (f39), and every guard above can be checked
  without the lock (f61), with today's path as the fallback.
- The box itself can tell: its cgroup's `memory.current` starts at the residual (f27-f30), and the counters and peaks it
  reads through its cgroup namespace include its predecessors' (f31-f34); `pids.peak` and the cumulative counters can't
  be reset by anyone (f32-f33). That makes the cgroup the owner's environment rather than the box's, Lambda's contract,
  not a container runtime's. It is a product decision, so reuse is offered per owner and off by default.
- No rename and no owner (f35, f64) are costs, not blockers: slot names, an owner field and a record field.

Without reuse, the sound levers left are the root default program and the pidfd end, 17 to 15 holds, plus fewer
controllers per box (each one an offline and a release, f23), which changes per-box isolation. A pool made ahead moves
holds but doesn't remove them (`create_serial.md`, "What wouldn't").
