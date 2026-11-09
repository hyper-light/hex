# Status

Updated 2026-10-24. A piece is done when its tests pass in the gate (`scripts/gates.sh`: Linux, with the
macOS and Windows lints) and the evidence its requirement names is recorded.

## Implemented

- **The bar** (kiln's CP-M1): a Cargo workspace whose lints deny panics, this repository's rules
  (`CLAUDE.md`), the requirement registry (`docs/requirements.toml`), the design's starting point
  (`docs/design/README.md`), the gate (`scripts/gates.sh`, run by kiln CI from `.kiln/ci.yaml`), and the
  vendoring tool (`scripts/vendor.py`, kiln's).
- **The `Runtime` seam** (`hex-api`): `BoxId`, `Bounds`, `BoxSpec`/`ClassSpec`, the `Runtime` trait,
  a simulator and a conformance suite (`HEX-RT-1..9`) that the simulator and both real classes pass.
- **The sans-IO cores** (`hex-core`): bounds → cgroup v2 writes, cgroup files → usage, box records,
  and crash recovery with a model-based crash-at-every-step property test (T-T7.1.2).
- **The `process` class** (`hex-runtime`, Linux): cgroup v2 resource boxes; programs start inside the
  box; pidfd waits; the io bounds target the right disks. Real-cgroup conformance + crash recovery
  pass (`hex-cli/tests/process_box.rs`).
- **The `oci` class** (`hex-runtime`, Linux): Hex's own OCI runtime (`hex oci …`), passing **every
  opencontainers/runtime-tools test crun passes, on each host** (AC-T7.1.4; goal 17's bar, 2026-11-06,
  cp/t7-g6-lsm, built, not merged): 28 of crun's 28 on SELinux-enforcing fedora, 30 of crun 1.27's 30 in an Ubuntu
  24.04 AppArmor guest. On each host one of the two LSM tests passes with its label applied, and the other
  passes with the label skipped and warned of, as crun runs it; `HEX_OCI_LSM=strict` refuses it instead (below), recorded in `bench/oci-validation/`. Each container has its own cgroup, with `linux.resources` and
  its device list in force, and is born in it. `hex oci exec` runs a program in the foreground inside a
  created or running container: born in its cgroup, in its namespaces, as its process, with the
  program's status as exec's and the caller's signals passed on, also where cgroup2 is mounted
  `nsdelegate`. Containers with one device policy share one loaded device program, and a container's
  program holds only its stdio (`bench/oci-validation/cgroups.sh`, 32 checks). The runtime decision and
  its evidence are in `docs/design/runtime.md`.
- **The node agent** (`hex-node`, Linux): `Runtime` over the `process` class, per-box records that a
  crash of the agent leaves whole (fsynced only for a box that owns directories, design decision 1), and
  recovery.
- **The node agent's local socket** (goal 3, plan step 4; decision 3 of `docs/design/runtime-architecture.md`):
  `hex node --socket <path> --state <dir>` serves the `Runtime` seam on a Unix `SOCK_SEQPACKET` socket, in a
  versioned wire format that changes append-only (`hex-core::wire`, a pure core with version 1's bytes recorded),
  exec's stdio crossing as `SCM_RIGHTS` descriptors and a program's end coming back as a message. `hex-client` is a
  `Runtime` over it, and the conformance suite (HEX-RT-1 to 13) passes against it over a live `hex node`, on `oci`
  and `process` boxes. Its bounds (connections, message size, requests in flight, programs watched) are refused
  past by type, naming them; hostile messages are refused and the connection served after; a client that goes away
  has the programs it still watches ended, reaped and counted in their box (`BoxInfo::abandoned`). HEX-SOCK-1 to 9.
  A call costs 0.031 ms more over the socket than in process at p50, an exec 0.204 ms, both drivers measured in one
  run on fedora (`bench/competitive/probes/socket_cost.md`, not of record: load 12 to 15).
- **The `hex` binary**: `hex --version`/`--help`, `hex oci …` (the OCI runtime), `hex box run
  <limits> -- cmd` (runs a program in a `process` box, for kiln to wrap a CI leg or harness in),
  and `hex selftest-box` (box isolation self-test, incl. `fsync-probe`/`disk-hog` for the bench).
- **AC-T7.1.5** (node memory ≤ 1 MiB/box): measured at 98 bytes/box (`bench/` node_memory).
- **AC-T7.1.2** (noisy-neighbor fsync p99 ≤ 2× idle): measured **1.73×** (idle 3616 µs → loaded
  6267 µs) on fedora NVMe in a sustained load-1.16 window, 2026-10-22 (`bench/noisy_neighbor/RESULT.md`).
- **AC-T7.1.3** (oci container start p50 ≤ 50 ms): measured **20.6 ms** (p99 26.4, max 31.2; N=200) on
  fedora NVMe, 2026-10-24, on the binary whose containers each have a cgroup and are born in it
  (`bench/hex_start/RESULT.md`; 14.4 ms on 2026-10-22, before containers had cgroups).
- **kiln runner unit** (kiln `cp/t7-hex`): `Delegate=yes`, so the runner's cgroup can be subdivided
  for Hex boxes (the prerequisite; inert until the `hex` binary is deployed to runners).

## In progress (CP-T7.1; see NOTES.md)

- Both AC benchmarks are now of-record (above). Remaining CP-T7.1 work: the first-consumer default-on
  flip (gated on a full dispatched-leg smoke) and the second consumer (the `hex` CI executor).
- **Consumer #1** (host CI legs + harness in `process` boxes, kiln `cp/t7-hex` behind `KILN_HEX_BOX`):
  the boxed path is e2e-validated under real delegation — the `the_boxed_path_holds_its_bounds_under_
  delegation` lab passes on fedora (vacate to supervisor, cpu/io/memory/pids delegated, fair-share
  bounds, legs' parent `memory.high` + runner/agents `memory.low`, crash sweep leaves no leg box).
  Default-on awaits a full dispatched-leg smoke (a scratch server + a `Delegate=yes` runner + one
  routed leg) — not flipped unilaterally (it changes every Linux CI host).
- **Consumer #2** (the `hex` CI executor, container legs, no Docker): the OCI image unpacker is
  validated end-to-end against a real registry (alpine from Docker Hub, `#[ignore]` integration test).
  The run-path design is settled (`docs/design/runtime.md`): the executor shells out to `hex oci`
  (persistent container + `hex oci exec` per step, like the docker executor shells to `docker`).
  `hex oci exec` is built (2026-10-24); next come the re-vendor and the dispatch wiring (mapped in
  NOTES.md).
- **Seccomp, the mount label and the AppArmor profile are applied; selinuxLabel is refused, not dropped** (CLAUDE.md §3; was: the
  `oci` class ignored all four, found 2026-10-24). `spec.rs` now reads all four. The class compiles a
  spec's `linux.seccomp` to a BPF program (vendored seccompiler, `oci/seccomp.rs`) and loads it in the
  container init before `execve`: before the user and capabilities change when the spec leaves
  `noNewPrivileges` off (as runc does), last when it sets it. no_new_privs is exactly the spec's (it
  had been set for every filter, which failed ten runtime-tools tests on fedora, 2026-10-27). The
  containers/common default profile compiles — it needs per-rule actions, its `socket` refusing
  `AF_NETLINK`+`NETLINK_AUDIT` with EINVAL while allowing every other `socket` — and a behavioral test
  proves a denied syscall is refused inside a rootless container while the rest run
  (`hex-cli/tests/oci_seccomp.rs`, through the seam: probe exits 42 with the profile, 10 without). An
  unsupported profile is a typed refusal. `linux.mountLabel` goes on every filesystem the container's mounts
  make, as the `context` of its fsconfig (mqueue by its `security.selinux` xattr), and
  `process.apparmorProfile` is written to the AppArmor exec attribute before `execve`, on every path (goal 6,
  `oci/lsm.rs`). Where the kernel runs the module, a label it can't apply is refused by name. Where it doesn't,
  the create skips the label and warns, naming it, as crun runs such a container, and `HEX_OCI_LSM=strict`, read
  by the command line and the node, refuses it by name instead. Hex's own test reads both labels back from
  inside a container (`hex-cli/tests/oci_lsm.rs`), on fedora and in the AppArmor guest
  (`bench/oci-validation/goal6-lsm.md`). `process.selinuxLabel` stays refused by name. Skipping by default is
  the lead's bar for goal 17 (2026-11-06); Ada's 2026-10-27 call had counted the two tests as deliberate
  differences.
- **Rootless device policy, enforced by construction** (`docs/design/runtime.md`, "Rootless devices"):
  a rootless runtime can't load the cgroup device program (CAP_BPF, which it lacks), so a deny-by-default
  allow-list is enforced by construction instead — only its devices get nodes, `mknod` is refused across
  the user-namespace boundary, and the mapped host uid's DAC governs access (measured). An allow-by-default
  policy with denials is refused (§3). Without this the `oci` class could start no standard container
  rootless. crun/runc/podman do the same.
- **A rootless box can't change another box's cgroup through the host `/sys` it binds** (goal 5's
  extension, AC-T7.1.1; `crates/hex-cli/tests/oci_cgroupfs_box.rs`, fedora 6.19.10). A rootless box on
  the host's network binds the host's `/sys` (it can't mount a sysfs in the host's network namespace),
  which carried the host's cgroup filesystem **writable** beneath the box's own cgroup mounts: a box
  holding `CAP_SYS_ADMIN` in its user namespace could unmount its own cgroup mounts off and write every
  box's `cgroup.kill`, `memory.max` and `cgroup.procs` (nsdelegate doesn't guard an init-namespace mount
  and the files are the node uid's). The runtime now remounts the carried cgroup filesystem read-only (a
  recursive bind's read-only otherwise covers only its top; `hex_runtime`'s init `remount_cgroupfs_readonly`),
  so a box that reached it would find a read-only host tree. Reads of that tree stayed open to such a box
  (the host cgroup submount is locked and can't be removed, only made read-only), so on Ada's call
  (2026-10-31) the spec's check now refuses the combination by name: a container in a user namespace
  holding `CAP_SYS_ADMIN` without a network namespace of its own (`Spec::check`), named in a capability
  set or kept from its creator where the spec names no capabilities (found 2026-11-03). The lab test
  (`a_rootless_host_network_box_cannot_change_another_boxs_cgroup`) expects that refusal, and a plain
  host-network box's carried host cgroup filesystem read-only. Built on cp/t7-seed, not merged: it
  reaches main with goal 5's merge, on Ada's call.

- **Goal 5 extended, plan step 3, the init's mounts at the system calls' cost: closes on its written bars by Ada's ruling
  (2026-11-03).** Built on `cp/t7-ext-s3`, not merged. Part 19's pooled p50 is at most 0.16 ms on both paths (fedora: pruned
  0.044-0.055 ms, classic 0.132-0.133), validation is as before (26 pass, 32 fail, 0 differ), HEX-RT passes over the node and
  the socket, and every mount kind is made on each path. Part 19's p99 at 8 in flight with 8,000 idle threads is under 0.5 ms
  on the merged trees with step 6's reuse (0.280-0.432 ms) and over on step 3's tree alone (0.729-1.568). Her under-load bar is
  now a written bar of runtime design goal 6: part 19's p99 at most 0.5 ms at 8 in flight with 8,000 idle threads, with 6 and 12
  busy loops on the same cores and Docker's paths, the whole create's p99 improving with it, no priority, nice, RT or slice
  change. Today it reads 1.081-1.835 ms at 6 loops and 1.519-2.313 at 12 (without Docker's paths 0.454-0.580 and 0.702-1.068;
  `docs/research/runtime/goal5-mounts.md` §11). The masked directories' tmpfs made ahead was built and measured: part 19's p50
  about 15% lower with Docker's paths, no gain on either p99 or the whole create, so it stays on `cp/t7-ext-s3-maskpool` (§13).

- **Goal 5 of record, its pass on fedora** (2026-11-05, `cp/t7-g5-record` 70376d7, not merged; `bench/competitive/
  RESULT.md`, "Goal 5 of record"; decision 5). The tree: goal 5 extended's integrated one, goal 6, the state directories
  in shards and the tail cut (inits parked ahead, decision 8), gates 7/7 and the root lab 8/8. host.sh's OCI pass, best
  of 2, Hex over libcrun in process (over the socket), every round read whole, none crashed, noisy (load 11 to 17):
  - reuse on (out/20261105T135855Z): create p50 0.152x (0.157x), p99.9 0.060x (0.060x); create+start of `true` p50
    0.133x (0.156x), p99.9 0.064x (0.069x). **Met in both columns**, Hex's create p99.9 under 1 ms in each round, so it
    holds against a quiet libcrun too (libcrun's create p99.9 here 15.38 ms).
  - no owner (out/20261105T134534Z), host.sh's default and so the bar as written: 0.189x (0.200x), 0.463x (0.421x),
    0.160x (0.199x), 0.406x (0.445x). **Not met**: both p99.9 over, Hex's about 2.3 ms against a quiet libcrun's 5.03
    and 5.69. What holds it is mostly Hex's own lock: the create's cgroupfs lookups queued behind Hex's own refill's
    `mkdir` (1.377 ms of the slowest 0.1%), then the teardown of the pass's own boxes (0.3-0.85 ms), no other tenant's
    (`probes/goal16_ab.md`, cp/t7-g16-memslot acf45f9). Reuse within one owner removes both. (The first reading, the
    teardown alone, is superseded.) The lock wait is now gone (cp/t7-g16-prefetch 1190a2c, merged at 309e62b: a
    no-owner create looks up nothing in cgroupfs); it moved the no-owner p99.9 about 2% (0.36x and 0.40x in its A/B),
    so what holds the tail is the kernel's teardown of the pass's own earlier boxes, 1.3-1.4 ms of the slowest 0.1%.
  - Goal 16's finding (2026-11-06, fedora 6.19.10; RESULT.md "Goal 16's finding"): the path is the deleted box's
    memory-cgroup offline (`mem_cgroup_css_offline`, 128-256 us a box most often), run by a per-CPU kworker
    (`cgroup_offline_wq`, `WQ_PERCPU`) on the very CPU whose RCU callback queued it, a create CPU, preempting the next
    create there (256 us to 2 ms); not a lock the create waits on. Being cut by doing less (goal 16).
  - What's left, measured (2026-11-06; RESULT.md "What's left of goal 16's tail"): the offline is the kernel's walk of
    every memcg-aware list_lru on the host (85-87% of it), 43-75 ns a list_lru on 6.19 and 56 ns on 7.2 at the same
    count, so 7.2 alone doesn't move it; Hex holds at most 2.5% of fedora's ~1,950 superblocks. Hex can cut at most
    about 0.4 ms of the slowest 0.1% (its releases), against the 0.6-0.7 ms 0.25x needs. Open: the levers' pass, and a
    guest-only kernel change that walks only the dying memcg's lists.
  - The Mac Studio through `docker_pass.sh` (2026-11-06, Docker Desktop's VM, 6.12.65-linuxkit aarch64, the same tree):
    **every cell under the bar in both columns and both modes**, no owner 0.107x (0.150x), 0.127x (0.175x), 0.102x
    (0.160x), 0.103x (0.131x); reuse on 0.107x (0.139x), 0.122x (0.151x), 0.099x (0.152x), 0.108x (0.134x), libcrun's
    create p99.9 3.77 and 3.92 ms.
  - jupiter (2026-11-06, Docker Desktop's WSL2 VM, the same tree), no owner: 0.200x (0.242x), 0.310x (0.489x), 0.208x
    (0.287x), 0.342x (0.330x): both p99.9 over, as on fedora. Reuse on: 0.200x (0.236x), 0.182x (0.214x), 0.200x
    (0.265x), 0.166x (0.228x): every in-process cell under, the socket's create+start p50 just over.
  - Open, Ada's: count the bar with an owner, reuse across boxes of no owner, or accept it unmet on 6.19 (fedora's
    no-owner p99.9). Goal 5 lands from this tree on her word.
- **Goal 5 extended, one integrated tree** (2026-11-04, `cp/t7-g5x-integrate`, not merged). Steps 1 to 6 of goal 5
  extended in one tree, with main's fd-leak fix (c4963f9), the EAGAIN retry for a `..` in a container's root, the exec
  FIFO's two ends held at start, the CAP_SYS_ADMIN refusal for a spec naming no capabilities, the rootless delete
  flake's pidfd kill, a box's turn for a create or delete of a box being made (in place of step 6's claim), a
  container made from the spec the runtime checked, and its execs and its start's and delete's hooks too (kept with its
  state; nothing reads the bundle's config.json after create's check),
  and createContainer and startContainer hooks run in the container (cp/t7-hooks a4e0261). The hook sweep (bc8684b) is in by a merge of its own, its CLAUDE.md §12 words held for Ada. The
  seven gates pass at each merge and the root lab at the tip (fedora). It is the tree step 7 measures, and goal 5
  lands from it once the native work is done, on Ada's word. Decision 5 lists what it holds.
  - goal 5x step 7, goal 5's create against its bar on the final tree (f58a101, fedora, 2026-11-05, noisy;
    `bench/competitive/probes/create_breakdown.md`, "Goal 5 extended step 7 again"; decision 5). host.sh's OCI pass,
    best of 2, Hex over libcrun, every round read whole: reuse on (out/20261105T114658Z) create p50 0.160x, p99.9
    0.656x, create+start of `true` p50 0.187x, p99.9 0.148x; reuse off (out/20261105T120118Z) 0.220x, 0.419x, 0.210x,
    0.240x. Create's p99.9 in process is the one over the bar, in the round the table takes; the other round reads 3.54
    and 2.91 ms, and the socket 0.99 and 3.48. Every box ended through its init, none by `cgroup.kill`. The breakdown,
    four runs: the parts' p50s summed are 98.4% to 104.5% of create's p50 (step 7's 10% met by all four). The levers
    for what's left are named for Ada: goal 6's 7.x namespace, the shared empty-directory masks (cp/t7-g6), the
    record's fsync on disk, step 6's reuse.
  - the tree since 1714f72: step 6's final code (a12e526) and records (e943880), the held files' bound counting both
    pools of namespaces made ahead (e31256b), a delete reading its box's record again after forgetting the init
    (3428ddc; 758afc9's Ended-path wait reverted by 2a4a806 on an A/B), host.sh's fetch from the pinned tarball and its
    refusal of a pass with a crashed round. Gates 7 of 7, root lab 8 of 8 at f58a101.
  - goal 5x step 4: ipc/uts pairs made ahead; part 16 0.727 of drained, create 17-37 us faster pairwise, dry <=0.30% at
    c32; cp/t7-ext-s4 209cea4.
  - goal 5x step 3: closed on its written bars by Ada's ruling (2026-11-03), with the under-load bar moved to goal 6;
    tip a600107.

## Design decided, implementation under way (2026-10-24)

After the runtime research, the competitive baseline and the runtime design, the track moved to
implementing the runtime's architecture and deciding the scheduler's. Built, decided and validated are
three different things (CLAUDE.md "Working here"):

- **The runtime's architecture** (`docs/design/runtime-architecture.md`, decided + pushed): the two
  layers (the OCI command line and the `Runtime` seam) against the 4-5x bar, with eight decisions and
  the cited arguments where an axis can't reach it. Its implementation goals:
  - **goal 0, the bench** — `bench/competitive` gains the seam driver, libcrun in process, a stand-in
    ceiling, the sustained and lifecycle_nonet phases. **Run of record recorded 2026-10-25**
    (`bench/competitive/RESULT.md`, run `ror-step5`): libcrun in process, the stand-in ceiling (13,577
    lifecycles/s at concurrency 32, above the 4,152 target), the process class through the seam, and
    throughput re-measured through the new worker-per-slot driver. 25/25 local control-flow checks.
  - **goals 1, 2, 7** (no fsync of state; no shim, `stopped` from the init's pid+start-time; the
    `process` box cloned straight into its cgroup) — committed on `cp/t7-runtime`; all seven gates
    green. **No-fsync measured** (`bench/competitive/probes/oci_breakdown.md` Run 3, 2026-10-26,
    no-fsync binary sha 1cd7deb7; fedora offline, so adacachy with interleaved overlay/tmpfs A/B passes
    that cancel load): with its fsyncs gone Hex's start is within 1.5% overlay-to-tmpfs (0.341 vs
    0.336 ms; the ~6.6 ms penalty gone) and create shows no overlay penalty (overlay 2.68 ≤ tmpfs
    4.39 ms — overlay faster, stronger than the within-10% the goal asks, though create p50 is too
    load-noisy to land within a literal 10%); crun the control is flat either way. A competitive OCI
    pass on the HEAD binary (3e5444d8, adacachy, LOAD_GATE=0, fedora offline) has start p50 at 0.94x
    crun (0.27 vs 0.25 ms; the 1.45x target met) and held memory per idle container 0.42 vs 0.40 MiB,
    with no process beside any idle container (step 3) and runtime-tools at exact crun parity. **Goal 2's
    plan step 2 closed the held-memory gap** (decision 2, "Nothing per container but the container"): the
    ~12 KiB/container left was a device program per container and the create handshake's two pipes, leaked
    into every container's program. One program is now shared per device policy and the pipes are
    close-on-exec; held per idle container is 0.99x crun's (run `20261027T043711Z`, adacachy,
    LOAD_GATE=0), and each change alone leaves Hex above crun (A/B run `20261027T044129Z`).
    runtime-tools is still at crun parity and step 3 still clean. **Of record on fedora** (plan step 4,
    kiln-a094a12d, 92b3b91, 2026-10-27): held 0.9849 and 0.9887 of crun's (run `20261027T061920Z`, 2
    rounds of 1,000, LOAD_GATE=0 at load 9-18: a same-run ratio), but with each container's loopback left
    down, which crun brings up. With it up (3bb57d3, adacachy), held is 0.9981 / 1.0018 / 1.0002 of crun's:
    parity, and a fedora run with loopback up is to come. Step 3 clean, and runtime-tools
    26/28 with the two LSM refusals kept by Ada's call. The fedora run also found every seccomp
    container running with no_new_privs set, fixed in 92b3b91. On adas-mac-studio-2 (Docker Desktop's
    aarch64 VM, load ~3): held 0.9975 and 0.9754, step 3 clean, runtime-tools 23/25 with the same two
    refusals. `hex oci exec` failed where cgroup2 is mounted `nsdelegate` (adacachy's 7.2 kernel):
    it joined the container's cgroup namespace before cloning into its cgroup, which the kernel refuses
    there with ENOENT; the child now joins it from inside. The OCI table of record in
    `bench/competitive/RESULT.md` predates decisions 1 and 2 (start ~13x crun, with fsync).
  - **goal 3, the `oci` class behind the `Runtime` seam** (merged to main 2026-10-29 at 8cc9b43, on Ada's call
    from the passes of record below) — `hex-node` serves `ClassSpec::Bundle` and
    `ClassSpec::Oci` in process and over its socket (cgroup under the box root, `create_in_cgroup`, start as one
    FIFO operation, exec with no process but the program: steps 2 and 3 below), with `seam_oci` to measure it
    against libcrun. The command line's `hex oci exec` keeps its `hex __oci-exec` helper.
    **Plan step 5, `ClassSpec::Oci`** (built, lab-validated on adacachy 2026-10-27; of record with step 6):
    the node renders the structured `OciSpec` to a bundle in a directory of the box's own (a pure core,
    `hex_core::bundle`, with golden tests) and runs it as `docker run` runs an image that names nothing
    else; `Rootfs::Layers` is an overlay the container mounts in its own mount namespace over the box's
    writable layer, which delete removes, and an unrenderable spec is refused by name (HEX-RT-13). The lab
    (`crates/hex-cli/tests/oci_spec_box.rs`) passes rootless and as root in a privileged container. It
    found two faults in the `oci` class, both fixed with a failing check first: a container's own network
    namespace had its loopback down (now brought up, as runc and crun do), and an image's absolute link
    let the init write a mount target on the host (now every path in a root resolves inside it,
    `openat2(RESOLVE_IN_ROOT)`); runtime-tools gives the same 26 of 58 before and after on adacachy.
    Open for Ada: whether OciSpec boxes get the default seccomp profile Docker applies
    (`docs/design/runtime.md`, "a box from an OciSpec").
    **Plan step 1, the floor under exec and where the seam's time goes** (measured on fedora 2026-10-28,
    `bench/competitive/probes/exec_breakdown.md`): the exec target stands. The floor under any exec into a
    running container (a prepared process cloning `true` into its cgroup and namespaces, waited on a pidfd) is
    0.75-0.83 of a quarter of the libcrun exec beside it, in each of three rounds. Exec through the seam is 3.4x
    the floor, about half of it the `hex __oci-exec` helper the seed removes (step 2). Start is 2.4-2.6x libcrun's,
    half of it the rewrite of `state.json` step 3 removes. Every part is timed by a tracer that costs a traced
    call under 5%, and the parts sum to the measured p50 within 7% in each round.
    **Plan step 2, exec with no process but the program** (built, lab-validated rootless and as root, gate-clean,
    2026-10-28): a node that runs as root starts an exec'd program straight from the calling thread, as posix_spawn
    does: the thread sets its children's pid namespace to the container init's, then one
    `clone3(CLONE_VM | CLONE_VFORK | CLONE_INTO_CGROUP | CLONE_PIDFD)` onto a stack of the child's own (an
    inline-assembly trampoline, x86_64 and aarch64); the child joins the container's namespaces by the init's pidfd,
    becomes its process and execs, making system calls alone on what was made before the clone. A rootless node
    can't choose a pid namespace (`EPERM`), so its execs go to a pool of reused `hex __oci-seed` processes, whose
    short-lived intermediate joins the user namespace first. Either way the program is the agent's own child, no
    `hex` process is spawned and no `hex` binary re-exec'd per call; the agent reaps the box's exec'd programs on
    delete. `clone3(CLONE_PARENT)` works with `exit_signal` 0, which an early spike missed
    (`docs/research/runtime/seed-clone-parent.md`). The lab (`crates/hex-cli/tests/oci_box.rs` `program_alone` and
    the HEX-RT conformance over the node and the socket) passes rootless (the seed path) and as root
    (`bench/oci-validation/seam_root.sh`, the direct path). On fedora (`exec-breakdown-20261028T224731Z`, load
    3-5) exec through the seam took 0.487 ms against libcrun's 1.983, **0.245x**, down from 0.77 ms through the
    seed and 1.41 ms through the helper (`docs/design/runtime-architecture.md`, decision 3). Since (built, lab-
    validated as root, gate-clean, 2026-10-29): the clone's child starts on the calling thread's CPU (the thread
    pinned for the clone, the child giving the pin back first), and an exec reads nothing of the box's from disk
    (what it needs of the container is kept with the init the supervisor watches), each set against the build
    before it in one run (`bench/competitive/probes/exec_ab.md`): the pin took exec's p50 from 1.19-1.23 ms to
    1.12-1.14 in fedora's slow state, where every run puts the seam at 0.28-0.31 of libcrun's exec (decision 3,
    "The host's state").
    **Plan step 3, start in one FIFO operation** (built, gate-clean, 2026-10-28): start reads the state once,
    takes the init's byte from the exec FIFO and removes the FIFO; it writes no state. `created`, `running` and
    `stopped` are derived as crun derives them (the FIFO there or not, the init live by pid and start time), an
    oracle table of every combination tested. Start through the seam took 0.084 ms against libcrun's 0.083 in the
    same run, about 1.0x.
    **Plan step 6, goal 3 of record** (measured 2026-10-29 on fedora, jupiter and adas-mac-studio-3,
    `bench/competitive/RESULT.md`, "Goal 3: the oci class through the seam"; merged at 8cc9b43): on fedora,
    host.sh's OCI pass from 8224f97, libcrun beside the seam in process and
    over the socket, best of 2, load 0.86 to 3.58. Start through the seam is **1.005x** libcrun's (bar 1.1, met).
    Exec is **0.289x** (bar 0.25, not met). The run met fedora at rest, where a short burst of work runs at about a
    third of its clock and a short call's ratio to a long one grows; with one busy loop outside the bench's CPUs
    holding the package awake, the same build's exec is 0.203x libcrun's (`bench/competitive/probes/exec_ab.md`,
    not of record). The same pass on other machines (`bench/competitive/docker_pass.sh`, Docker Desktop's Linux VM;
    Ada's call): jupiter (AMD Ryzen AI MAX+ 395) exec 0.245x and start 0.815x; adas-mac-studio-3 (Apple M4 Max)
    exec 0.117x and start 0.500x; both bars met on both. Ada's call (2026-10-29): the three machines' passes
    together stand for goal 3's record, fedora's at-rest exec recorded beside them, and the branch merged on them.
    Open: how a fedora pass of record holds the host (at rest, as CLAUDE.md §7's load under 2 asks, where a short
    call's ratio to a long one grows; or with the package held awake). Create is 3.3x libcrun's on fedora (19.2 ms
    against 5.9), 2.8x on jupiter and 1.0x on the Mac Studio, outside goal 3's bars; open.
  - **goal 4, network namespaces off the path** (kiln plan `01a185679f8a7616ab21db7fb980ffce`, 5 steps; merged to
    main 2026-10-29 at 6446160, on Ada's call from the passes of record below). **Plan step 1, decision 4 restated**
    (2026-10-29): probe run 1's 5.5 ms gap is gone;
    through the seam a container's network namespace now costs Hex's create about 0.55 ms on fedora, jupiter and
    the Mac Studio and nothing on delete (the kernel defers its teardown), against libcrun's 0.55, 0.77 and 0.21 ms
    (`bench/competitive/probes/netns_pool.md`). On fedora and jupiter the 5% bar already holds only because Hex's
    create is 6.0x and 3.2x libcrun's there (goal 5's gap); on the Mac Studio, where it isn't, the namespace is 23%
    of create. The choice stands with its bound measured: a pool of at most 32 fresh namespaces made ahead on one
    thread (5,260-6,127 a second on fedora, 2,280-3,545 on the Mac Studio), each at most 0.34 and 0.74 MiB of kernel
    memory there, joined by setns, never reused, for containers in the agent's user namespace only, its memory
    charged to the agent and counted in Hex's density readings. A doc-truth test holds decision 4's figures to that
    record (`crates/hex-bench/tests/doc_truth.rs`). **Plan step 2, the seam's bench** (built, gate-clean,
    2026-10-29, 2404ec7): seam_oci runs oci_bench.py's lifecycle_nonet and sustained phases by both drivers and
    reads the node's network namespace counts around each phase, in process (`Node::netns`) and over the socket (the
    wire's version 2: a client says hello in version 1 and writes in the highest version both speak); summarize.py
    tables create and delete p50 with the namespace over without, and the sustained phase's count of namespaces made
    on the path, beside libcrun's. Small-N passes on fedora and adas-mac-studio-3 (docker_pass.sh, 2 rounds of 200,
    sustained 2,000; RESULT.md) ran every phase by both drivers with no error, and with no pool yet counted every
    namespace as made on the path: 2,000 of the sustained phase's 2,000 creates. Hex's seam sustains about one
    create's time per lifecycle at concurrency 32 (53/s on fedora against libcrun's 1,644, 530 against 1,439 on the
    Mac Studio), as if its creates ran one at a time: open. **Plan step 3, the pool's core** (built, gate-clean,
    2026-10-29): `hex_core::netns`, the pool's policy as a pure core. It holds at most its bound and refuses a
    namespace made past it, hands each out once and the oldest first, answers a dry take with "make one on the
    path" and counts it, as it counts one made on the path for a create it can't serve (a container with a user
    namespace of its own), pauses the refill after a failed make until the next take, and refuses every take after
    shutdown. Its counts (made ahead, taken, made on the path, discarded, failed) equal an oracle's under random
    histories (HEX-NETNS-1 to 5). **Plan step 4, the pool's shell** (built, 2026-10-29, 5257407): a root node's
    thread `hex-netns` makes each namespace off the create path (unshared on that thread, loopback up, the thread
    back home after), up to the bound; the runtime asks the pool once it has read the spec, and a container that
    makes its own network namespace with no user namespace joins one with setns in place of unsharing, while one in
    a user namespace of its own (every rootless node's: bound 0, no thread) makes it on the path, counted. The
    shim opens the namespace by `/proc/<agent>/fd/<n>`, so the agent's descriptor stays close-on-exec, which fixes
    the first runtime side's handoff (13636cb), where any process the agent started meanwhile inherited it.
    HEX-NETNS-6 to 11; rootless, oci_box, oci_spec_box and node_socket pass on adacachy, and as root, in jupiter's
    Docker Desktop VM (`bench/oci-validation/docker_lab.sh`, seam_root.sh's lab where /usr isn't Fedora's), so do
    those and oci_seccomp. On adas-mac-studio-3 (`docker_pass.sh` at 5257407, the step-2 phases, best of 2) a
    create from the pool costs 1.01x a create without a namespace in process and 0.97x over the socket, against
    1.23x before the pool; every create took from the pool (`bench/competitive/probes/netns_pool.md`). **Plan step
    5, of record** (2026-10-29, 378b1d2; `bench/competitive/RESULT.md`): the step-2 phases, best of 2, fedora by
    host.sh and adas-mac-studio-3 by `docker_pass.sh`. On the Mac Studio a create from the pool costs what one
    without a namespace costs (0.99x in process, 0.95x over the socket; libcrun 1.21x); fedora's figures, taken
    under CI's load, are 0.48x and 0.97x (in process each round's own 0.47x and 1.05x, Hex's create moving 11-25 ms
    between rounds). No sustained phase found the pool dry: 0 of 10,000 on each machine, by each driver, in each
    round. The as-root lab passes on fedora (`seam_root.sh`, 5 of 5). Ada's call (2026-10-29): merged to main at
    6446160 on fedora's and the Mac Studio's passes, all seven gates passing on fedora there; jupiter's followed at
    6f73f9a (`docker_pass.sh`): a create from the pool 1.04x in process and 1.00x over the socket, libcrun 1.16x,
    the pool never dry. The gates found a race older than goal 4, fixed at 6446160: a box's delete reaped an exec'd
    program before recording its exit, so a caller over the socket could read it ended `Exit::Unknown` (HEX-RT-7,
    under load).
  - **goal 5, a seed per node** (kiln plan `01a1877e05c7748ca8865548aab76445`, research
    `01a1877e05cc73b495447bf5113ebda3`, 6 steps; on `cp/t7-seed`, not merged). The research found today's seam
    create on fedora is nearly all two fsynced writes of the box's record (7.1-7.5 ms each on fedora's btrfs), taken
    under the record store's one lock, so creates run one at a time; the seed's saving of the shim's exec (about 1
    ms) comes after the record leaves create's path. **Plan step 1, decision 5 restated** (2026-10-30, 37ce17c): the
    create breakdown (`bench/competitive/probes/create_breakdown.md`, run 1, fedora, hex ef64dbc) cuts a seam create
    into 35 parts that sum to 99.5% of its p50 (18.297 ms; libcrun 5.015 beside it). The record's four fsyncs are
    66.6% of it, the shim's spawn and exec 4.8%, the cgroup 2.6% (9.2% of what's left after those two, under step
    5's tenth, so no pool on this run). The plan's levers leave 5.142 ms, 1.025x libcrun's create and 4.1x goal 5's
    bar. Most of the rest is the init's root (mounts 0.966, pivot_root 0.873, which walks every thread on the host),
    the device program loaded with each create (0.583), and the record's and state's writes on the overlay. Open:
    the bar needs a container's set-up made before the create, which no step of this plan does; put to Ada. **Plan
    step 3, the seeds' core** (built, gate-clean on fedora, 2026-10-29, 5f0b474): `hex_core::seeds`, the policy for
    the node's seeds as a pure core. It keeps at most 32 seeds up or starting, one to a slot (a seed serves one
    create at a time, AFaaS §8; 32 is the bench's busiest concurrency until step 1's measured clone says how many
    fewer would do), and hands each create an idle seed that is up. With none idle the create waits in line, each
    seed that frees going to the create that has waited longest; past 128 in line it is refused at once, and if
    handed none within 1 s it is refused. A seed that dies, or that serves one create for 30 s (taken for hung, for
    the shell to kill), leaves its slot and its create is refused by type; the slot is started again and the restart
    counted, and a late report about the seed is refused, never taken for the next one's. A failed start, or a seed
    dead before it served a create, stops the starting until the next create. Its counts (creates asked, handed,
    waited, done and refused four ways; seeds started, restarted, failed to start, dead, hung; the most busy at
    once) and the creates each slot's seeds were handed equal an oracle's under random histories (HEX-SEEDS-1 to 6).
    **Plan step 2, the record off create's path** (2026-10-30, defda14 to 8416f23): container state is under
    `/run/hex-node` (`$XDG_RUNTIME_DIR/hex-node` rootless), named `<node key>-<box id>`. A record whose directories
    all lie on tmpfs isn't fsynced (0 per seam lifecycle, record-syncs-20261030T002934Z). Per-box record turns,
    bounded by the 65,536-box budget, replace the store's lock, and no lock another box's create takes is held
    across a record write. An init that dies between the shim's spawn and its watch, and is reaped there by a
    sweep, now refuses its create by type and the box is scrubbed (before, the box was made with the init's
    exit). A box made before the move, its state under `<state>/oci`, is read where its record names it, so it is
    taken back and managed after an upgrade (b158760, oci_box's `a_box_made_before_the_move_is_adopted`). Pass of
    record on fedora (20261030T004533Z, /run a tmpfs from 6f7ba39 on): create p50 1.928 ms in process, 2.002 over
    the socket, against libcrun's 2.515; sustained at concurrency 32 2,108 and 2,095/s against libcrun's 1,765 (goal
    4's record: 26 and 51 against 1,294, /run on overlayfs). Creates no longer run one at a time, which closes that
    Open item: its cause was the record's fsyncs under the store's one lock. The network-namespace pool ran dry for
    1.8-2.8% of sustained creates at that rate, decision 4's second falsifier (Open). After step 2 the create
    breakdown, /run a tmpfs (its run 3, create-breakdown-20261030T012433Z, not of record, fedora loaded by Focal's
    fleet test), puts a seam create's p50 at 3.290 ms pooled against libcrun's 4.004. Of the 2.983 ms left once the
    shim goes, the init's root is 1.250 (pivot_root 0.765), the device program 0.356 and the cgroup 0.282 (9.5%,
    under step 5's tenth); what's left is 2.98x goal 5's bar. **Plan step 4, the seed** (built, gate-clean on fedora
    and adacachy, 2026-10-30, 72812eb to ed365e4): a seam create hands a `hex __oci-seed` the container's spec and
    descriptors over its socket (`hex_core::seed`'s Create, append-only), and the seed clones the init with
    `clone3(CLONE_PARENT | CLONE_INTO_CGROUP | CLONE_PIDFD | the spec's namespaces)`, so the init is the node agent's
    own child in one clone, root or rootless. The shim's re-exec is gone from the seam's create but for a spec that
    joins a pid or user namespace by path, which goes through the shim, counted. The node keeps at most 32 seeds
    (`createpool.rs` over `hex_core::seeds`), started ahead of the creates with one spare; a seed is 1.5-2.4 MiB
    resident. On fedora no create execs the hex binary (55 creates, the seeds' two starts aside), and a seed's clone of
    an init takes 0.234 ms p50 to the init's first run against the 1.038 ms exec it replaces
    (seed-path-20261030T060613Z); the labs pass rootless and as root, and the OCI validation suite passes the 26 of 32
    its baseline does. A pass at ca45e1a (20261030T041910Z, load 21.81, not of record): create p50 1.50 ms in process
    and 1.62 over the socket, against libcrun's 2.97; create+start of `true` 1.44 and 1.81 against 2.86, p99.9 2.58
    and 3.18. Its fourth criterion, sustained at concurrency 32 at least 8x concurrency 1, isn't met (4.93x in
    process, 6.31x over the socket): the ceiling is the kernel's `cgroup_mutex` (Open). On Ada's call (2026-10-31)
    step 4 closes on its other criteria, and goal 5 keeps its 0.25x bar and gains steps for the next levers: the
    device program loaded once per policy, the container's root set up without pivot_root's walk of the host's
    threads, and fewer `cgroup_mutex` holds per lifecycle, which takes the 8x. **Plan step 5, a pool of cgroups made
    ahead** (closed on the record, 2026-10-31, 67b8e0a): the create breakdown now cuts the seed path (88556f8), and its
    run 4 (create-breakdown-20261031T061841Z, not of record) puts the cgroup, made and bounded, at 0.184 ms of a 2.134
    ms create, 8.6%: under the step's tenth, so no pool (decision 5). The same run has create p50 at 2.020 ms against
    libcrun's 2.750; the container's root, its mounts and pivot_root, is 0.894 ms of it and the device program 0.276.
    Without the root, the device program and the cgroup's making, 0.825 ms is left, 1.2x goal 5's bar, so the bar
    needs a lever beyond the three added. Step 6 follows the added steps.
  - goal 6 (mount namespaces without copying) — met on its record (2026-11-09, adacachy2, Linux 7.3.3,
    mntns-host-20261109T171050Z at cp/t7-g6 0c29b3ec; `RESULT.md`, "Goal 6 of record"), run in two phases, A the
    classic root interleaved with the ways and B the ways alone, since the classic arm's mount work reaches the ways'
    part 19: (1) the default's create p50 below the classic path's in every cell, 292-299 of 299 rounds; (2) the
    default by the lead's rule, the copy: the ways tie at create p50 by a sign test at 95%, and the copy's part 19 p99
    is the lowest; (3) runtime-tools 28 of 28; (4) part 19 p99 185 and 183 µs at 6 and 12 loops, and the whole create's
    p99 lower than the classic path's in every cell. Classic against the default counts goal 5's prune and pools with
    goal 6's ways. Held in reserve: the init's JSON parse of the checked spec (15.8 µs first, 8.2 warm). Closed once
    merged and gated.
- **The scheduler's architecture** (`docs/design/scheduler-architecture.md`, decided): goodput over
  balance, a sampled fast path with an optimiser tier, suitability filters on declared needs, Vivaldi
  proximity on hyper-swim, priority/fair-share/gangs/preemption, correctness against a serial
  specification, one code path from a laptop to a region, and fast isolation of bad workloads and
  nodes, all measured in **hex-sim** (built first). Implementation open.

## Next

- **CP-T7.2**: The `firecracker` and `shards` classes, with snapshot templates and warm pools.
- **CP-T7.3**: The control plane, from one node to a cell; kiln's CI legs placed by resources, not slots.

## Open

- **An exec straight from the agent's thread copies the agent's descriptor table.** The vfork-style clone (goal 3,
  plan step 2) gives the child a copy of the agent's descriptors (`dup_fd`), and its `execve` closes them again
  (`do_close_on_exec`), so each such exec costs in proportion to what the agent holds open: one pidfd per live `oci`
  box, up to `INIT_BUDGET`. Not yet measured at density; a seed (few descriptors) doesn't pay it. Closes with a
  measurement of exec against the agent's open descriptors and, past where it shows, the program's descriptors
  moved low and the rest left behind at the clone (`close_range(2)`, `CLOSE_RANGE_UNSHARE`) or the exec handed to a
  seed.

- **A create's sync and map pipe ends reach processes the agent starts meanwhile.** `start_shim` clears
  close-on-exec on the shim's ends of its two pipes (`sync_w`, `map_r`; `run.rs`, `keep_across_exec`) before
  `Command::spawn`, and the agent is multi-threaded, so any process it starts in that window (another create's shim,
  a seed, a `process` box's program, a hook, an exec's clone) inherits them and keeps them past its `execve`: nothing
  closes a stray descriptor before it. A stray `sync_w` holds the pipe open, so a shim that dies before its pid
  line leaves its create's `read_sync` blocked until the holder ends; a stray `map_r` lets the map ack's write
  succeed against a dead shim; either way one create's pipe sits in an unrelated container's program. The network
  namespace made ahead had the same exposure and no longer has it (goal 4 plan step 4: the shim opens it by
  `/proc/<agent>/fd/<n>`). Closes with one of: the same handoff by path for the pipes, which needs the agent to
  drop its own `sync_w` once the shim has opened its copy, a change to the sync protocol; a `dup2` onto the
  descriptor in the child, which std's `Command` allows only through an `unsafe` `pre_exec`, and which costs it
  `posix_spawn`; or a lock every spawn in the agent takes. Put to Ada as the fix after goal 4.

- **The network-namespace pool keeps an error's count but not always its detail** (goal 4 plan step 4's review).
  The refill thread counts a make that fails (`Counts::failed`) but drops its reason; `net::make_ahead` drops the
  error of the thread's `setns` back home when the make failed too; and `Networks::take` turns the pool's
  `ShutDown` into `OciError::Broken` with its message, so a caller can't tell it by type. Each failure is still
  counted or refused, but why a kernel refused a make isn't seen. Closes with an event for a failed make (its name
  agreed with kiln's Observe owner, CLAUDE.md §10), both errors kept when both fail, and a shutdown refusal of its
  own type.

- **Over the socket, the sustained phase's rate falls by half as it runs.** On adas-mac-studio-3 (goal 4's pass of
  record, `RESULT.md`) the socket driver's rate over ten windows of 1,000 lifecycles went 579 down to 188-279 a second
  in one round and 577 down to 188-270 in the other, while the in-process driver in the same pass held (570 to 536,
  491-599); on fedora and jupiter, at about a tenth of the rate, the socket's held. The pool isn't it (no create
  found it dry). Something on the socket path slows with the lifecycles it has served: the agent's per-connection or
  per-box state, the client's, or the wire's. Closes with where the time goes in a late window against an early one,
  and its fix, shown by a sustained phase over the socket that holds level.

- **The network-namespace pool's one refill thread falls behind at about 2,100 creates a second.** With the record
  off create's path (goal 5 plan step 2) the seam ran 2,108 lifecycles a second at concurrency 32, and the pool ran
  dry for 181-279 of each sustained phase's 10,000 creates (1.8-2.8%; `RESULT.md`, 20261030T004533Z), each made on
  the path and counted, none failed: decision 4's second falsifier (more than 1% dry). Goal 4's 553 a second never
  reached it. A dry create makes its namespace on its path, 0.152-0.161 ms p50 and 0.333-0.401 ms p99 for one
  thread on an unloaded fedora (`probes/netns_pool.md`), more under load; at 2.5% of creates those sit above a
  create's p97.5, where goal 5's p99.9 (plan step 6) reads them. Closes with a second refill thread, or a bound sized
  to the rate, and a sustained phase at that rate with under 1% of its creates finding the pool dry.

- **An adopted box that was created but never started can't be started.** After a node agent's restart, `start`
  judges the init's liveness by the supervisor's pidfd, and an adopted init isn't watched (found in goal 5 plan step
  2's crash tests; older than goal 5). Closes with `start` judging an adopted init by its pid and start time, as
  `state` does, or the supervisor watching adopted inits, shown by a test that restarts the agent between create and
  start.
- **Concurrency 32 is bound by the kernel's `cgroup_mutex`.** A box's lifecycle takes the host-wide lock 17 times:
  the runtime's `mkdir` and `rmdir` of its cgroup, its `cgroup.kill`, its device program's attach and its clone into
  the cgroup, and the kernel's release of the removed cgroup in kworkers. At concurrency 32 on fedora it was held 83.4%
  of the time and handed off 6.8% more, so the seam sustained 1,942 and 1,994 lifecycles a second, 4.93x and 6.31x
  its rate at concurrency 1 against goal 5 step 4's 8x, and libcrun, contending on the same lock, 1,593
  (`probes/create_serial.md`). Hex's own waits are under a tenth of the box root's, and a pool that makes cgroups
  ahead takes the lock as often. Closes with fewer holds per lifecycle (goal 5's added step, on Ada's call): a box's
  cgroup reused for the next box of the same owner once it is clean (no process, its memory reclaimed, its counters'
  starting values kept), else a fresh one; the default device program attached once at the box root; a box ended
  through its init's pidfd rather than `cgroup.kill`.
- **A create can wait for a seed to start.** At concurrency 32, 15-46 of each 5,000-10,000 creates waited for a
  seed the pool was starting (`probes/seed_path.md`), in the tail goal 5's p99.9 (plan step 6) reads. Closes with
  the pool keeping enough seeds started for the rate it serves, shown by a sustained phase with none waited.
- **The create seeds' counts aren't on the socket.** `Node::create_seeds` reads them in process only. Closes with
  an append-only wire call for them, as decision 4's counts got.
- **A seed that dies after its clone but before it answers is untested.** Its cleanup is the shim path's. Closes
  with a crash test at that point.
- **oci_spec_box fails in Docker Desktop's VM.** Its kernel (6.12.65-linuxkit) puts fallback tunnel devices in
  every new network namespace, so a box's own namespace holds more than `lo` (`oci_spec_box.rs:274`); the test
  passes on fedora and fails alike at 1c5074e. Closes with the test reading the namespace as the box's own and `lo`
  up, beside whatever devices the kernel itself puts in every new namespace.
- **A layered box's delete once waited out its 60 s.** In the gates on fedora at e2bf959 (2026-10-30, 01:28-01:30Z,
  beside Focal's fleet test), oci_spec_box's `layered` box stayed populated 60 s after `cgroup.kill` with nothing in
  `cgroup.procs`, and delete refused by type. On Linux v6.19 that means threads of a process whose leader had ended
  held it (`kernel/cgroup/cgroup.c:5112-5121`), which the refusal didn't read. It didn't recur in 20 runs, 12 of them
  beside 8 fsync writers on the same disk. The refusal now names such threads, each with its process, state and
  wchan (3896e46). Closes when a recurrence shows what held the box and that is fixed at its cause. Separately, a
  layered box's teardown syncs the whole filesystem under its writable layer, as its overlay's unmount does
  (`fs/super.c:622`, `fs/overlayfs/super.c:262` at v6.19): 22-33 ms at rest and 67-259 ms beside 4 fsync writers on
  fedora's btrfs, against 3-5 ms with the overlay mounted `volatile`
  (`Documentation/filesystems/overlayfs.rst:794-795`).


Everything else the track's plan names (kiln plan `01a157fe102576a8bf44ccaa5d835de7`): each goal, its criteria and how each is
measured are in `docs/requirements.toml`.
