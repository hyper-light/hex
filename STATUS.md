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
- **The `oci` class** (`hex-runtime`, Linux): Hex's own OCI runtime (`hex oci …`), at **crun parity
  on opencontainers/runtime-tools** (28/58, the set crun passes on this cgroup-v2 host; AC-T7.1.4),
  recorded in `bench/oci-validation/`. Each container has its own cgroup, with `linux.resources` and
  its device list in force, and is born in it. `hex oci exec` runs a program in the foreground inside a
  created or running container: born in its cgroup, in its namespaces, as its process, with the
  program's status as exec's and the caller's signals passed on, also where cgroup2 is mounted
  `nsdelegate`. Containers with one device policy share one loaded device program, and a container's
  program holds only its stdio (`bench/oci-validation/cgroups.sh`, 32 checks). The runtime decision and
  its evidence are in `docs/design/runtime.md`.
- **The node agent** (`hex-node`, Linux): `Runtime` over the `process` class, per-box records that a
  crash of the agent leaves whole (fsynced only for a box that owns directories, design decision 1), and
  recovery.
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
  container init after `no_new_privs`, before `execve`. The containers/common default profile compiles
  — it needs per-rule actions, its `socket` refusing `AF_NETLINK`+`NETLINK_AUDIT` with EINVAL while
  allowing every other `socket` — and a behavioral test proves a denied syscall is refused inside a
  rootless container while the rest run (`hex-cli/tests/oci_seccomp.rs`, through the seam: probe exits
  42 with the profile, 10 without). An unsupported profile is a typed refusal. `process.apparmorProfile`,
  `process.selinuxLabel` and `linux.mountLabel` stay refused by name in `Spec::check` (their apply path
  is LSM-host-dependent, unvalidatable on the bench hosts) — a typed refusal until each is implemented,
  never a silent drop. The competitive bench is unaffected: its bundles name none of them.
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
    runtime-tools is still at crun parity and step 3 still clean. Of record on fedora (plan step 4)
    awaits fedora. `hex oci exec` failed where cgroup2 is mounted `nsdelegate` (adacachy's 7.2 kernel):
    it joined the container's cgroup namespace before cloning into its cgroup, which the kernel refuses
    there with ENOENT; the child now joins it from inside. The OCI table of record in
    `bench/competitive/RESULT.md` predates decisions 1 and 2 (start ~13x crun, with fsync).
  - **goal 3, the `oci` class behind the `Runtime` seam** — `hex-node` serves `ClassSpec::Bundle` in
    process (cgroup under the box root, `create_in_cgroup`, `exec` through a seam-mode `hex __oci-exec`
    helper), with `seam_oci` to measure it against libcrun. Built and reviewed locally; its runtime
    path needs a fedora root lab to validate, and its exec-latency target likely needs goal 5's seed.
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

Everything else the track's plan names (kiln plan `01a157fe102576a8bf44ccaa5d835de7`): each goal, its criteria and how each is
measured are in `docs/requirements.toml`.
