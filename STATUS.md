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
  program's status as exec's and the caller's signals passed on (`bench/oci-validation/cgroups.sh`, 28
  checks). The runtime decision and its evidence are in `docs/design/runtime.md`.
- **The node agent** (`hex-node`, Linux): `Runtime` over the `process` class, durable per-box records
  and recovery.
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
- **The `oci` class ignores a spec's `linux.seccomp`, `process.apparmorProfile`, `process.selinuxLabel`
  and `linux.mountLabel`**: `spec.rs` doesn't read them, so `Spec::check` can't refuse them, and a
  container whose spec names a seccomp profile runs without one (found 2026-10-24). CLAUDE.md §3 asks
  for a typed refusal until each is implemented. The competitive bench is unaffected: its bundles name
  none of them, for every runtime.

## Next

- **CP-T7.2**: The `firecracker` and `shards` classes, with snapshot templates and warm pools.
- **CP-T7.3**: The control plane, from one node to a cell; kiln's CI legs placed by resources, not slots.

## Open

Everything else the track's plan names (kiln plan `01a157fe102576a8bf44ccaa5d835de7`): each goal, its criteria and how each is
measured are in `docs/requirements.toml`.
