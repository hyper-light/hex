# A seed per node: what a seam create is made of, and what clones containers from a prepared process

Source notes for decision 5 of `docs/design/runtime-architecture.md` (goal 5; kiln research
`01a1877e05cc73b495447bf5113ebda3`, where each finding, f1-f17, is graded from these sources). Hex's own measurements
are in `bench/competitive/probes/create_breakdown.md`; this note holds the code that measurement cuts at and what
others document. The two papers' words were checked against their text (`pdftotext` of the USENIX PDFs below,
2026-10-30); page numbers are the proceedings'.

## What a seam create does today (Hex at ef64dbc)

- **Two durable writes of the box's record.** `create_oci` records the box as being made before it makes anything,
  and as ready once its init is watched: `self.store.put(&record)?;` at `crates/hex-node/src/node.rs:433` and `:473`.
  An oci box's record names the container's state directory as the box's,
  `let mut dirs = vec![self.oci_root.join(id.as_str())];` (`node.rs:428`), and the store fsyncs every record that names
  a directory, the file (`f.sync_all()`, `crates/hex-node/src/store.rs:78`) and the store's directory (`sync_dir`),
  because `must_outlive_host` is `!record.dirs.is_empty()` (`store.rs:152`). (f4)
- **One lock over every write.** `put` takes the store's one write lock, `let _one = self.one_write()?;`
  (`store.rs:51`), and holds it across the write and both fsyncs, so concurrent creates' record writes run one at a
  time (f5). At concurrency 32 the seam sustained about one create's time per lifecycle: "Hex's seam runs about one
  create at a time under concurrency." (`STATUS.md`, Open; f8).
- **What a durable record write costs on fedora.** A probe of the store's write on fedora's `/home` (btrfs,
  `compress=zstd:1`, the same filesystem as Docker's root there), 300 writes a pass: write, fsync the file, rename,
  fsync the directory took "p50 7.138 ms, p99 11.245 ms" and "p50 7.481 ms, p99 12.650 ms" in two passes; written and
  renamed without the fsyncs, "p50 0.088 ms, p99 0.158 ms" (`python3 -I` probe in `~/.cache/hex-t7/fsync-probe` on
  fedora, 2026-10-29T11:55Z; f6).
- **A second exec of the hex binary.** Create spawns the shim with std's `Command`: `let mut cmd = Command::new(exe);`
  and `cmd.arg("__oci-init")` (`crates/hex-runtime/src/linux/oci/run.rs:273-274`; f2). Spawning and waiting for it
  costs about what `hex --version` does: 1.038 ms p50 on fedora, against 0.666 ms for `/usr/bin/true`
  (`bench/competitive/probes/oci_breakdown.md:55-56`, run 1; f3).
- **Without the state's fsyncs.** With the runtime's own `state.json` write no longer fsynced (decision 1), Hex's
  command-line create on adacachy was 2.68 ms p50 against crun's 1.68 ms on the same overlay
  (`oci_breakdown.md:145-147`, run 3; f10).

## What pivot_root costs

- Linux v6.19 (<https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/plain/fs/namespace.c?h=v6.19>):
  `SYSCALL_DEFINE2(pivot_root, const char __user *, new_root,` (`fs/namespace.c:4526`) ends its work with
  `chroot_fs_refs(&root, &new);` (`:4606`), which, in `fs/fs_struct.c` at v6.19, takes "read_lock(&tasklist_lock);"
  and walks "for_each_process_thread(g, p) {" (lines 60-61), taking each thread's lock to move its root and working
  directory if they were the old root. Its cost follows the number of threads on the host, not the container's.
- crun pivots the same way: `ret = pivot_root (".", ".");` (containers/crun 1.30.1, `src/libcrun/linux.c:2567` at
  079ff6a7), unless its `no_pivot` option is set, which the bench's libcrun driver leaves unset
  (`bench/competitive/libcrun/driver.c:79-91`).

## Where container runtimes keep a container's state

- runc's `--root`: "root directory for storage of container state (this should be located in tmpfs)"
  (opencontainers/runc `main.go:124` on main, retrieved 2026-10-30).
- crun(1), STATE: "By default, when running as root user, crun saves its state under the **/run/crun** directory.
  As unprivileged user, instead the *XDG_RUNTIME_DIR* environment variable is honored, and the directory
  **$XDG_RUNTIME_DIR/crun** is used." (containers/crun `crun.1.md:75-78` on main, retrieved 2026-10-30).

Neither keeps a container's state where it outlives the host (f11): a host crash takes the state with the containers
it describes. Plan step 2 holds the box's record of an oci box to the same rule.

## Cloning a container from a prepared process

- clone(2) (<https://man7.org/linux/man-pages/man2/clone.2.html>): `CLONE_INTO_CGROUP` (since Linux 5.7), "The
  CLONE_INTO_CGROUP flag allows the child process to be created in a different version 2 cgroup."; `CLONE_PIDFD`
  (since Linux 5.2), "a PID file descriptor referring to the child process is allocated" (f12).
- Hex's exec seed already clones so (decision 3): `crates/hex-runtime/src/linux/oci/seed.rs:10`, "clones the program
  with `clone3(CLONE_PARENT | CLONE_INTO_CGROUP |" (f13). Goal 5 extends the mechanism from exec to create.
- **AFaaS** (Chai et al., "Fork in the Road: Reflections and Optimizations for Cold Start Latency in Production
  Serverless Systems", USENIX OSDI 2025, <https://www.usenix.org/system/files/osdi25-chai-xiaohu.pdf>), §8 Lessons
  Learned and Experiences, p.511:
  - A seed is serial: "First, the seed itself may become a bottleneck because only one instance can be cloned from it
    at a time (serially) as concurrency increases. AFaaS further enhances concurrency by deploying multiple instances
    of the same seed type." (f14)
  - State unique to an instance: "While reusing a running seed’s state can reduce startup latency, unique states such
    as random numbers or cryptographic tokens must be reinitialized carefully instead of being reused directly." (f15)
  - Churn of kernel objects: "Second, when functions are repeatedly created and destroyed at a high frequency over an
    extended period, seccomp installation failures may arise due to bpf_jit_limit leaks". Hex loads a device program
    with each create (decision 2); a seed doesn't change that, and the sustained phase is where a leak would show.
- **Firecracker**, "Entropy for Clones" (firecracker-microvm `docs/snapshotting/random-for-clones.md:8-10` on main,
  retrieved 2026-10-30): "Some userspace applications or libraries keep their own equivalent of entropy pools and
  suffer from the same potential issues after being cloned." (f15) A container's init execs its program, so the
  program's own state is made afresh; what the seed's child makes before that exec is the seed's to make anew.
- **SOCK** (Oakes et al., "SOCK: Rapid Task Provisioning with Serverless-Optimized Containers", USENIX ATC 2018,
  <https://www.usenix.org/system/files/conference/atc18/atc18-oakes.pdf>), §6 Related Work, p.67: "Lee et al. [26]
  observed that forking many child processes from the same parent without calling exec undermines address-space
  randomization; their solution was Morula, a system that runs exec every time, but maintains a pool" (f17). A seed's
  child execs the container's program, so the program's address space is randomized afresh.

## Cgroups made ahead

- **SOCK**, on Linux 4.13: §2.4 Serverless Implications, p.60, "Third, reusing cgroups is twice as fast as creating
  new cgroups, suggesting that maintaining a pool of initialized cgroups may reduce startup latency and improve overall
  throughput."; §4.1 Lean Containers, p.62, "It is relatively expensive to create cgroups; thus, OpenLambda creates a
  pool of cgroups (shown in Figure 10) that can be used upon SOCK container creation; cgroups are returned to the pool
  after container termination." (f16)
- **AFaaS**, §4.2.1, p.506: "In traditional cloud-native environments, new cgroups are created for each container and
  removed after the container is destroyed. This management mechanism can be time-consuming under high concurrency.
  AFaaS introduces a cgroup pool that recycles cgroups when the sandbox instance is deleted." And §8 (3), p.512, on
  what to pool: "First, not all kernel resources require pooling; it is crucial to identify the minimal necessary
  set."

Both reuse a cgroup once its container is gone. Decision 4's pool of network namespaces hands each one to one container
and never back, and plan step 5 would hold cgroups to the same rule; whether it is built at all rests on the measured
share of a create that making the cgroup is (`create_breakdown.md`), as AFaaS's own lesson asks.
