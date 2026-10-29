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
  - **goal 3, the `oci` class behind the `Runtime` seam** — `hex-node` serves `ClassSpec::Bundle` and
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
  - goals 4 (network namespaces off the path), 5 (a seed per node), 6 (mount namespaces without
    copying) — open.
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

Everything else the track's plan names (kiln plan `01a157fe102576a8bf44ccaa5d835de7`): each goal, its criteria and how each is
measured are in `docs/requirements.toml`.
