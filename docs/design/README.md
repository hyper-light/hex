# Hex's design

The target, from track T7 of kiln's critical path (kiln plan `01a157fe102576a8bf44ccaa5d835de7`). Each decision below is
made by the goal named beside it, recorded here with its sources, its evidence and what would
falsify it (CLAUDE.md §5).

## What it is

Hex runs workloads and decides where they run. It holds:
- `hex-core`: the sans-IO lifecycle and scheduling cores;
- `hex-runtime`: the runtime classes;
- `hex-sched`: the scheduler;
- `hex-node`: the node agent ("hexlet");
- `hex-api`: the traits others use, with a simulator implementation;
- `hex-sim`: a simulated cluster with fake nodes, as kwok's;
- the `hex` binary, in the `hex-cli` package (`hex` is a crates.io crate kiln already links).

It keeps its state in Amphora, its events on Quiver, its images in Reliquary, its secrets in Veil
and its volumes on Slates, then Mantle. Kiln vendors it at `vendor/hex`: the node agent runs in
kiln's runner and the control plane in kiln's server, reached only through kiln-seams' `Sandbox`
and `Placement`.

## The target

- **Runtime classes behind one `Runtime` trait:**
  - `process`: a process tree in a resource box (cgroup v2's cpu.weight, memory.max, io.weight and
    io.latency, pids.max on Linux; QoS and limits on macOS; job objects on Windows);
  - `oci`: an OCI runtime-spec container (namespaces, cgroups v2, seccomp, Landlock, rootless
    through user namespaces), Hex's own runtime or youki's libcontainer as CP-T7.1 decides;
  - `firecracker`: a Firecracker microVM through its API socket and jailer, with snapshot templates
    per image and warm pools; refused by name off Linux with KVM;
  - `shards`: a Shards microVM through Shards' Engine API, then its VM processes directly.
- **The control plane:** a declarative API (jobs, tasks, services, agents, volumes, secret
  references) with desired and observed state in Amphora, reconcilers on its change feeds,
  schedulers placing in parallel with optimistic concurrency, and node agents heartbeating on
  Quiver's windowed class.
- **The scheduler:** filter and score plugins; Borg's scoring mixing best and worst fit;
  equivalence classes, cached scores, sampling and late binding; affinity, taints and spread;
  priority bands, quotas with cohorts and borrowing, DRF fair share, gang admission, and
  preemption that keeps disruption budgets. Every pending workload says why it waits, in words.
- **Scale:** a million workloads on 10,000 nodes in one cluster (measured in `hex-sim`), the
  control plane sharded over Amphora's groups, cells as failure domains, region-local control
  planes, on one code path.
- **Compatibility:** a Kubernetes API subset, so kubectl, Helm and Argo CD work against Hex.

## Decisions

| Decision | Made by | Status |
|---|---|---|
| The OCI runtime: Hex's own or youki's libcontainer, by conformance, start latency and the no-panic rule | CP-T7.1 | decided: Hex's own (`runtime.md`) |
| The runtime's architecture against the 4-5x bar: volatile state, one supervisor per node, the `Runtime` seam as the fast path, network-namespace work off the path, a seed per node, mount namespaces without copying, no wrapper per box (`runtime-architecture.md`) | goal 4 (kiln plan `01a16e75781c776390a6efeb797a9ad6`) | decided 2026-10-24; implementation open |
| The `process` box per platform | CP-T7.1 | open |
| Snapshot templates, pool sizing and working-set prefetch | CP-T7.2 | open |
| The scheduler's architecture: goodput over balance, a sampled fast path with an optimiser tier, suitability filters on declared needs, Vivaldi proximity on hyper-swim, priority/fair-share/gangs/preemption, correctness against a serial spec, laptop-to-cell-to-region on one code path, fast isolation of bad workloads and nodes, and hex-sim first (`scheduler-architecture.md`) | CP-T7.3-T7.12 | decided 2026-10-24; implementation open |
| The control plane's state and watches in Amphora | CP-T7.3 | open (within `scheduler-architecture.md`, S6) |
| The scheduler's framework and scoring | CP-T7.4 | decided (`scheduler-architecture.md`, S1-S3) |
| Quotas, fair share, preemption and gangs | CP-T7.5 | decided (`scheduler-architecture.md`, S5) |
| Networking: namespaces, CNI-compatible plugins, policies from Agentfile grants | CP-T7.7 | open |
| Storage: a CSI-like interface; images from Reliquary, lazily | CP-T7.8 | open |
| The Kubernetes API subset | CP-T7.13 | open |
