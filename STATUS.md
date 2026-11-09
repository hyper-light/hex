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
  opencontainers/runtime-tools test crun passes on fedora but two it refuses by decision** (26/58
  against crun's 28, 2026-10-27, 92b3b91; AC-T7.1.4): a mount label and an AppArmor profile the
  suite's container can't apply, which crun drops (below), recorded in `bench/oci-validation/`. Each container has its own cgroup, with `linux.resources` and
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

- **CLAUDE.md's unsafe-file lines (§12) and Gates lines: approved by Ada as written, 2026-11-06** ("Approve all
  of them", to the lead). In this tree: the macOS box helper's `helper_sys.rs` (d4252f3, a960732), the helper's child
  setting `RLIMIT_NOFILE` (caa6e9f), the Windows calls that find the Shards guest's default shares (0b8bcd1), and
  step 7's two Gates lines (1b1fea3, 47a9683, whose clippy targets `scripts/gates.sh` runs) and §12's `kern.argmax`
  (08d52a0), in with step 7's tip. Approved and coming with their steps' tips: step 4's `windows/oci/sys.rs` call
  list (c4961c1), which names calls step 4's later commits add and replaces the older line this tree still carries
  (aadbd4e, not itself approved), and the macOS line's `kern.maxprocperuid` (1c701ed, approved later on
  2026-11-06). Merged, the macOS line names `kern.maxproc`, `kern.maxprocperuid` and `kern.argmax`. Goal 7's
  clone line (e71daaf) is approved on its own branch. A later commit that changes any of these call lists is new,
  and goes back to Ada.

## In progress (CP-T7.1; see NOTES.md)

- Both AC benchmarks are now of-record (above). Remaining CP-T7.1 work: the first-consumer default-on
  flip (gated on a full dispatched-leg smoke) and the second consumer (the `hex` CI executor).
- **The Windows `oci` class** (native plan step 4; `docs/design/runtime.md`, "the Windows `oci` class is an HCS
  silo holding a Hex job"; `docs/research/runtime/windows-hcs.md`): built, not yet run on Windows.
  `hex_core::hcs` is the sans-IO core: Microsoft's host/image version table, a bundle's `windows` section or an
  `OciSpec` to the container, every field honored or refused by name, the HCS v2 document, HCS's error kinds,
  and the launcher's wire. Its tests pass on Linux. `hex_runtime::windows::oci` makes a process-isolated
  compute system over the image's layers, with the box's job nested in the silo. Every program starts suspended
  in that job, through `hex oci-launch`; HCS is loaded at run time. It is cross-checked by the Windows clippy
  gate. What closes it: jupiter (offline) runs `crates/hex-cli/tests/windows_oci_box.rs` — HEX-RT-1..13, a
  device not granted isn't visible, a read-only mount refuses writes, a mismatched image refused by name — and
  settles the research note's §9 probes. Create, start and delete are then measured against Docker's and
  containerd's runhcs on jupiter, recorded in `bench/`. Open until then: the silo's own memory baseline, which
  sets an `OciSpec` box's container-wide HCS memory bound (probe 3); until it is measured, such a box's bound
  holds on its members only.
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
- **Seccomp is applied; apparmor/SELinux/mountLabel are refused, not dropped** (CLAUDE.md §3; was: the
  `oci` class ignored all four, found 2026-10-24). `spec.rs` now reads all four. The class compiles a
  spec's `linux.seccomp` to a BPF program (vendored seccompiler, `oci/seccomp.rs`) and loads it in the
  container init before `execve`: before the user and capabilities change when the spec leaves
  `noNewPrivileges` off (as runc does), last when it sets it. no_new_privs is exactly the spec's (it
  had been set for every filter, which failed ten runtime-tools tests on fedora, 2026-10-27). The
  containers/common default profile compiles — it needs per-rule actions, its `socket` refusing
  `AF_NETLINK`+`NETLINK_AUDIT` with EINVAL while allowing every other `socket` — and a behavioral test
  proves a denied syscall is refused inside a rootless container while the rest run
  (`hex-cli/tests/oci_seccomp.rs`, through the seam: probe exits 42 with the profile, 10 without). An
  unsupported profile is a typed refusal. `process.apparmorProfile`, `process.selinuxLabel` and
  `linux.mountLabel` stay refused by name in `Spec::check` (their apply path is LSM-host-dependent,
  unvalidatable on the bench hosts) — a typed refusal until each is implemented, never a silent drop.
  That costs two runtime-tools tests crun passes on fedora, `linux_mount_label` and
  `linux_process_apparmor_profile`: in the suite's container neither SELinux nor AppArmor is visible,
  and crun runs the container without the label or profile. Ada's call (2026-10-27): keep refusing,
  and count those two as deliberate differences. The competitive bench is unaffected: its bundles name
  none of them.
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
  holding `CAP_SYS_ADMIN` without a network namespace of its own (`Spec::check`). The lab test
  (`a_rootless_host_network_box_cannot_change_another_boxs_cgroup`) expects that refusal, and a plain
  host-network box's carried host cgroup filesystem read-only. Built on cp/t7-seed, not merged: it
  reaches main with goal 5's merge, on Ada's call.

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
  - goal 6 (mount namespaces without copying) — open.
- **The scheduler's architecture** (`docs/design/scheduler-architecture.md`, decided): goodput over
  balance, a sampled fast path with an optimiser tier, suitability filters on declared needs, Vivaldi
  proximity on hyper-swim, priority/fair-share/gangs/preemption, correctness against a serial
  specification, one code path from a laptop to a region, and fast isolation of bad workloads and
  nodes, all measured in **hex-sim** (built first). Implementation open.
  - **Priority, fair share, gangs and preemption** between the owners of Hex's workloads, OCI containers,
    Firecracker microVMs and Shards microVMs (implementation goal 4, CP-T7.5; `docs/design/scheduler-fairness.md`),
    in hex-sim's control plane and its serial specification, 2026-11-06: queues by band then weighted DRF
    (AC-T7.5.2: four equal-weight owners within 479 ppm of a quarter on a generated day, bar 5,000; FIFO
    133,982), gangs in one store write (partial gangs 0 over crashes at every write), preemption within
    disruption budgets (AC-T7.5.3: breaches 0 by a 256-case proptest with the crash nemesis), and a burst of
    1,000 workloads leaving the other owners' start p95 as it was (T-T7.5.1; plain DRF and FIFO fail it, a
    Kueue cohort model keeps it). On G2's recorded day (Huawei Cloud's serverless sandboxes) neither bar can
    be shown (`bench/sched/g4_huawei.md`): open for Ada's call. Days of kiln's CI are a side measurement
    (`bench/sched/kiln_workload.md`): Hex schedules OCI containers, Firecracker microVMs and Shards microVMs,
    not CI jobs (Ada, 2026-11-06).
- **Scheduler goal 5, failure detection and isolation (S8)** (`docs/design/failure-isolation.md`; built
  on `cp/t7-sched-g5`, not merged): `hex-health`, the pure core of the detectors and penalties; hex-sim's
  fault nemesis, each fault class within its stated bound over 1,000 seeds (gray drained ≤ 3.9 s; a
  tripped OCI container stopped, then re-placed through the fast path anywhere in the cell as a
  Firecracker or Shards microVM, else refused, and a microVM's trip refused, over 1,000 seeds of each
  kind);
  the crash class on hyper-swim's own detector, vendored (every crash condemned within the bound its
  condemner stated; at one probe a second, p99 12.1 s, though the stated bound runs to 1,090 s, a
  finding for hyper-swim's owner), memberlist's SWIM+Lifeguard beside it as the competitor, its
  slow-member false positives ended by health-gated confirmations; the node's disk probe, and `AC-T7.10.3`'s dm-delay drill
  (`bench/health/records/2026-11-05-dm-delay-drill.md`: drained 3.5 s after the stall, no work failed;
  8 of 8 failed with the probe off). Open: the node agent running hyper-swim (it runs no membership
  yet), and the control plane acting on the verdicts (CP-T7.3).

## What works where (native plan step 7; `docs/design/runtime.md`, decision "one differential suite")

One suite, `hex_api::differential` (the conformance cases HEX-RT-1..13 and the scenarios HEX-DIFF-1..6, each
real class held to the simulator's trace), runs as each OS's own tests. Where it has passed, and on what:

| | `process` | `oci` |
|---|---|---|
| Linux amd64 (fedora) | **Passes** (cgroup v2; `process_box.rs`, and over the node's socket) | **Passes** (bundles and specs, rootless; `oci_box.rs`, `oci_spec_box.rs`, over the socket) |
| Linux arm64 | **Built and linted**: the Linux clippy gate checks `aarch64-unknown-linux-gnu` and `-musl` with `-D warnings` (gates 7/7 at e1b4c01, fedora unit hex-native-s7-gates-20261106T132708Z); the musl aarch64 `hex` is what step 5's Shards guest runs. The suite isn't run natively: no Linux arm64 machine in the lab | As `process` |
| macOS arm64 (the Mac Studio) | **Passes** (rootless; `macos_process_box.rs` 7/7 on adas-mac-studio-2 at abdbfd0, a launchd job outside kiln's pacing) | Linux images through a Shards guest: native step 5, integrated into `cp/t7-native` (ebf01f7); its record is step 5's |
| macOS amd64 (Intel) | **Built and linted**: the macOS clippy gate checks `x86_64-apple-darwin` with `-D warnings` (gates 7/7 at a7670e0, fedora unit hex-native-s7-gates-20261106T135145Z), and `cargo build --workspace --target x86_64-apple-darwin` at a7670e0 exits 0 on adalundhe-macbook-pro (Apple M5 Max, macOS 26.5.2; a launchd one-shot, a fresh target dir, 2026-11-06 13:52Z), an x86_64 Mach-O `hex`. Not run: no Intel Mac in the lab | Refused by name: Shards has no x86_64 macOS backend |
| Windows amd64 (jupiter) | **Passes** (job objects; `windows_process_box` 9/9 on jupiter at 1a2e1eb and e322d7b, the Windows agent's record `bench/install/records/windows-jupiter-20261105.txt` on `cp/t7-native-windows`) | **Passes** (HCS, native step 4): `windows_oci_box` 2/2 on jupiter at `cp/t7-native-s4` 5f712ff, real layers, no skips, HEX-RT-14 included (step 4's log `suite-20261106T191231Z.log`); jupiter's `cargo test --workspace` at 5f712ff exits 0 (`wstest-20261106T191231Z.log`): conformance and HEX-DIFF-1..6 match the oracle, TERM ending the init as `Signal(15)`, and the step's criteria pass |
| Windows arm64 | Skipped for now (Ada, 2026-11-03) | Skipped |


The integrated native tree, `cp/t7-native` at fb0716e (2026-11-06; steps 1-3, 5, 6 and 7 through their gated tips, the
macOS line through a7c9a8c, the Windows line through 702ce7e; step 4 through aff4195, its converted tip to come):
fedora's 7 gates exit 0 (379 tests, 0 failed) and the root lab passes 8 of 8 as root, in one unit
(`hex-integrate-native-20261106T210700Z`, 21:07-21:44Z). On adas-mac-studio-2, run as a launchd job outside kiln's
pacing (`hex-integrate-mac-fb0716e-20261106T210742Z`): clippy for macOS and Windows exit 0, `cargo test --workspace`
exits 0, and `macos_process_box` passes 7 with 1 ignored (`box_user_boxes_hold_the_contract`, which needs the root
helper: Ada's boxlab). jupiter's `cargo test --workspace` at fb0716e's code (run at 8ba39bd, which adds only this
file; 23:33-23:46Z) exits 101: every binary passes but `windows_process_box`, which fails 2 of 10. HEX-RT-14 fails
because step 7's exec-bound case reached the tree before step 4's 3c35f15, which states Windows' exec bound for both
Windows classes; it clears when step 4's tip merges. HEX-DIFF-5 (step 6) and `a_box_past_its_bound_is_refused_at_the_bound`
read no memory-bound crossing (`oom_kills` 0) with the peak past the notification line: under load Windows records
the job's memory-limit violation late or never (339 of 340 passes on jupiter, `bench/install/records/windows-jupiter-20261105.txt`
entry 21). The Windows line fixes it at `cp/t7-native-windows` 3f16fc5 (the bound read from the job's peak commit at
its commit limit, the notification gone), which merges with step 4's final tip. Rerun at 6ac27d6, the same two fail
in 4 of 5 runs.

The integrated native tree at ac8959b (2026-11-07; step 7 through ea8bda6, step 2 through 45dcc79, and ac8959b's fix of
step 2's HEX-RT-3 test to the macOS line's `BoxProc::watch(pid, members)`): fedora's 7 gates exit 0 (382 tests, 0
failed) and the root lab passes 8 of 8 as root, in one unit (`hex-integrate-native-20261107T011029Z`, 01:10-01:49Z).
On adas-mac-studio-2 at ac8959b (`hex-integrate-mac-ac8959b-20261107T020027Z`, 02:00-02:15Z): clippy for macOS and
Windows exit 0, `cargo test --workspace` exits 0 (269 passed, 0 failed, 2 ignored), and `macos_process_box` passes 7 with
1 ignored in two more runs (the box-user case: Ada's boxlab). Open: AppleSystemPolicy refused to execute the built
`hex` twice in each of those two runs (two processes in one millisecond, 18:10:23 and 18:12:42 local), and every test
still passed. ASP's refusals of a launchd job's processes are kiln's to fix (the lead, 2026-11-07, with Ada).

A note for Mac runs: a launchd job (`launchctl submit`) must keep the kiln run that submitted it alive until it ends.
The jobs at 6ac27d6 and ac8959b (`…-6ac27d6-20261107T005156Z`, `…-ac8959b-20261107T011052Z`) died within half a
second of their submitting runs ending. macOS's AppleSystemPolicy refused to execute the job's own script (6ac27d6,
48 ms after launchd spawned it), then cargo's build scripts (ac8959b: libc, serde_core, proc-macro2 and quote, all in
one millisecond); launchd then marked the job inactive and removed it. Nothing on disk differed: no quarantine xattr on
the source copy (`git archive | tar`) or the target dir, the build scripts linker-signed ad hoc as always, and the same
script, source and target dir ran clean (0 denials) from a run kept alive until the job wrote `done`. fb0716e's job
outlived its run at 13:07 local, before kiln's headroom guard restarted on a new build (4dfd3bb19754) at 13:20; every
job since that outlived its run was denied. That points at how the guard's build attributes a job's processes, not
proven. The same denials hit other agents' fresh binaries on that Mac today (Shards' tests, kiln-runner-fan's builds).

The suite's divergences so far, each fixed where the contract says: a Windows box with nothing running took a
signal the others refuse (`NotRunning`), now refused; the simulator ended an `oci` box's init on TERM, which
Linux doesn't (a PID namespace's init takes only KILL and STOP unless it handles the signal), now modelled, and
what TERM does to an init is the class's (`Fixture::term_ends_init`: Windows' `oci` class ends the job, set by
native step 4); a macOS box refused a program after its last ended and took a signal with none alive, both
fixed. The competitor bench found one more: a macOS program killed at its bound was sometimes reported at its
wait's budget (XNU posts the exit event before the zombie), fixed at abdbfd0.

Each OS's `process` class against its competitors, recorded in `bench/native/RESULT.md`: Linux start 11.1x and
delete 14.7x a systemd scope's, both holding the bound; macOS start 1.87x `taskpolicy -m`'s, delete 0.90x, both
holding the bound, and a launchd job's limit holds none; Windows start 2.4x and delete 256x Process Governor's,
both holding the bound. **Native step 7 is closed** on step 4's clean run: the suite passes on fedora (Linux `process` and `oci`), the
Mac Studio (macOS `process`) and jupiter (Windows `process` and `oci`), and in step 5's Shards guest (its record);
Linux arm64 and Intel macOS are linted by the gates (7/7 at a7670e0, unit hex-native-s7-gates-20261106T135145Z),
Intel macOS built on adalundhe-macbook-pro at a7670e0, and the musl aarch64 `hex` runs in step 5's Shards guest;
Windows arm64 is skipped (Ada). Every OS's `process` class is benchmarked against its competitors (`bench/native/RESULT.md`).
Its CLAUDE.md changes are approved by Ada, 2026-11-06, as written: the Gates block's Linux and macOS lint lines
(1b1fea3, 47a9683) and §12's `sysctlbyname("kern.argmax")` for the exec bound (08d52a0).
**Final** at 5f712ff (`cp/t7-native-s4`), on three results at that one commit: fedora's 7 gates exit 0 (unit
hex-native-s4-gates-20261106T215342Z, its exact tree), `windows_oci_box` on jupiter (`suite-20261106T191231Z.log`) and
jupiter's `cargo test --workspace` exit 0 (`wstest-20261106T191231Z.log`).
**Open gap (recorded, not built):** a rootless exec whose arguments and environment lie between the seed wire's one
request (160 KiB, `hex_core::seed::MAX_REQUEST`, under the kernel's bound on one `SOCK_SEQPACKET` record) and the
kernel's own exec bound (a quarter of `RLIMIT_STACK`, 2 MiB at an 8 MiB stack) is refused as `Invalid` "program", where
runc and crun, bounded only by the kernel, run it. As root the exec is cloned from the agent's thread and meets only
the kernel's bound. Closing it needs a request carried over several records, a kind appended to the seed wire; not
this step's to build.

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
