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
| Linux images on macOS and Windows: one Shards guest per node running Hex's Linux node unchanged, its wire over vsock, reached through a `shards-api` seam (`shards-guest.md`) | native plan step 5 | proposed 2026-11-04; seam agreed with Shards' owner 2026-11-04 |
| Hex's wire over hyper-transport: hex-client's transport and the node's listener carry hex-core's wire on hyper-quic lanes (control, a stream lane per stdio stream, staging and unstaging, logs); AES-256-GCM alone, each end asserting the suite it negotiated; identity by CSR enrollment with kiln's CA, so keys never move; waits learned per call kind and class (`hex-quic.md`, Q1-Q9) | joint plan step 4, CP-T7.3 | decided 2026-11-12; built on main (`cp/t7-goal4-transport`); the step's acceptance, the conformance suite over it on Linux, macOS and Windows (Windows through a kiln agent on jupiter), open on Hex ROOT's goal stack |
| The Shards class on Agentfile images: a microVM per box that Shards' image boot (D231) boots under shards-init, pinned by digest and sized by its admitted allocation; the members its caller drives handed over with their stdin, stdout, stderr and end (`Runtime::member`, joint plan step 17), their exec and signals through Shards' agent-control endpoint once built, refused typed until then; an owner's crash holds each run's claim until a reboot proves it ended (`shards-image-class.md`) | joint plan step 12 | built 2026-11-12, to Shards `e82b9403` since, on Linux, macOS and Windows hosts (its run host started holding only its stdio, vendor/patches/shards 0001); its waits' records, its real-VM runs and a member's streams over the node's wire owed; upstream, change 0001 is D248 (Shards `9bdd4a7ec`), and `image_real`'s HEX-IMG-3, a run's exit read wrong, is answered by D249 (Shards `67cbbfec3`), not yet vendored here |
| Hex's node under shards-init: the `oci` class on macOS and Windows runs its node as the workload of a Shards VM booted through shards-init, with Shards' nesting grant (D234), reaching its host over Shards' agent-control endpoint; Hex's own PID 1 kept, shards-init working with it, and the hex-guest path (`hex` as PID 1) still passing on its own (`node-under-shards-init.md`; `shards-image-class.md` §7) | joint plan step 39 | open: waits on Shards' plan steps 11 (the endpoint: its connection, its microVM's pinned key, its members' stdio lanes and the controller's side of them built at Shards `a8eee1382`; a command run in a member, built as the run protocol's exec at Shards `628de209b` (D243), carried on exec lanes of its own, whose endpoint's and controller's sides are built at Shards `cc36a60d3` and the daemon's exec host at `789f9ab08` (D238); its signal, inspect, terminal and log lanes building) and 38 (D234, built; its real-VM runs owed); where the endpoint's TLS ends is decided, at the host (plan step 10, D238 at Shards `1b6e1203d`); Hex ROOT's, with Shards ROOT measuring start latency and memory per node VM against the hex-guest path |
| Snapshot templates, pool sizing and working-set prefetch | CP-T7.2 | open |
| The scheduler's architecture: goodput over balance, a sampled fast path with an optimiser tier, suitability filters on declared needs, Vivaldi proximity on hyper-swim, priority/fair-share/gangs/preemption, correctness against a serial spec, laptop-to-cell-to-region on one code path, fast isolation of bad workloads and nodes, and hex-sim first (`scheduler-architecture.md`) | CP-T7.3-T7.12 | decided 2026-10-24; implementation open |
| The control plane's state and watches in Amphora | CP-T7.3 | open (within `scheduler-architecture.md`, S6) |
| The scheduler's framework and scoring | CP-T7.4 | decided (`scheduler-architecture.md`, S1-S3) |
| A GPU for a Firecracker microVM: the administrator binds its IOMMU group to `vfio-pci`, opens `/dev/vfio` to the node and lets it lock guest memory; the guest catalogs what its VMM attached by its own PCI address, joined to the host's selection only through the VMM's witness (`firecracker-gpu-host.md`) | AC-T7.4.1 (Hex's lead, 2026-11-11) | decided 2026-11-11; no lab host bound yet |
| GPU telemetry for the balancing: NVIDIA through NVML loaded at run time, two calls a GPU a sample; AMD through amdgpu's sysfs counters kept open on Linux, one read a counter, and through ADL on Windows, two calls a GPU; macOS refused by type (`gpu-telemetry.md`, G1-G4) | CP-T7.3 (Ada, 2026-11-09) | decided 2026-11-09; built in `hex-gpu` |
| Deadline waits: Hex's condition variable, channel and timer (`hex-wait`): std's condition variable on Linux and Windows, and on macOS a waiter's own kqueue whose deadline is a one-shot `NOTE_CRITICAL` timer, since XNU gives std's timed waits their thread's QoS leeway, at utility up to four times the wait and 75 ms (`deadline-waits.md`) | AC-T7.4.1 (Hex's lead, 2026-11-12) | decided 2026-11-12; built in `hex-wait`, the condition variable's 12 sites and the channel's and timer's moved; Windows's measurement owed |
| Quotas, fair share, preemption and gangs | CP-T7.5 | decided (`scheduler-architecture.md`, S5) |
| A microVM charged its learned use, its envelope the ceiling; CPU shared by booking, memory held to a budget, the over VM ended last; budgets per fleet as each guest's D188 room (`learned-charge.md`) | joint plan step 14 | charge decided by Ada 2026-11-11; its rule (C1-C7) and step 13's interface (C8) taken by Shards ROOT 2026-11-12 (`learned-charge.md`); built on main for macOS and Windows nodes' managed VMs, each capped at its water-filled share each window (`b19ef663`), their last resort ending a VM through its managed receipt, why written into its record first (`6ff188e2`, `803d311f`); its Shards inputs (D241's room and cost, D247's rate) built upstream, not yet vendored here |
| One capacity view per machine: Hex's node agent keeps the machine's one ledger of room and kiln's runner reserves through it; one rule for full on both sides, fences by epochs and sessions, never a release on silence, the person's release of a holder gone for good, no leases, a holder's used memory counted twice and never credited (`capacity-view.md`, CV1-CV12) | joint plan step 13 | proposed 2026-11-12; protocol sent to kiln's Load Balancing agent; cores, seam, the node's shell and its QUIC lane built; CV12's credit withdrawn 2026-11-12 (no read of the OS's makes it safe); kiln's side: its runner's ledger and its holding built on kiln main (kiln `docs/lb.md` "A machine's keeper"; `e94a1953f`, `562268830`, `035813991`): Hex's holder core driven for the ledger, no more claims than it may reserve (CV8, CV9), a claim's room reserved before its run starts, its waits on the keeper following the keeper's liveness and progress alone, a run that asks nothing started with no reservation, as the keeper refuses an empty one (agreed with Hex's lead, 2026-11-13), and kiln-ci's simulator holding it to Hex's keeper (`capacity::sim::SimView`) under crashes, partitions and heals; its runner's shell, the holding over `CAPACITY_PORT` with its identity enrolled, next |
| Networking: namespaces, CNI-compatible plugins, policies from Agentfile grants | CP-T7.7 | open |
| Network grants for a microVM's members: who, to whom, which ports, which way, default deny; Hex decides each VM's policy and Shards' per-VM network process enforces it; what Shards can hold exactly is programmed: each destination a grant names by TCP kept and every member held to its own reach (D236, Hex's recorded Shards change 0002), a publish held to the receivers Shards names (`Bound::to`); the rest refused typed (`network-grants.md`) | joint plan step 15, CP-T7.7 | decided 2026-11-12; built to Shards `e82b9403`, its real-VM runs owed; upstream, change 0002 is D246 (Shards `7a4ef5c02`, image channel version 12, where change 0002 writes 9), and a run that keeps nothing past its microVM keeps the network its image links its members to, its network process letting nothing past, D250 (Shards `3a16dbd92`, from `image_real`'s Codex refused for no network), not yet vendored here |
| Storage: a CSI-like interface; images from Reliquary, lazily | CP-T7.8 | open |
| The Kubernetes API subset | CP-T7.13 | open |

`vendor/shards` pins Shards main `e82b9403` (`482c5d78` before), which holds what Shards has built since, each
recorded in Shards' `docs/design/architecture.md`: destinations by name and a published port's receiving member (D236,
D231's `Bound::to`; joint plan step 9, whose use `network-grants.md` programs, step 15); a member's stdin on the
orchestrator's path (`ImageGuest::take_input`, step 5, D240); a member's environment, home and working directory (step
7, D240); ring pins within a guest's room (D230, D188; `learned-charge.md` C7), which no call awaits since `37f940c5`
(`RingPinsWithinRoom`); each vCPU's guest time (D43), not yet in shards-api's image `Stats`; AES-256-GCM alone on
Shards' own links (D239); and agent control over hyper-transport, where its TLS ends still open (D238). Since
`e82b9403`, Shards main has decided that its TLS ends at the host (plan step 10; D238 and PM M540 at `1b6e1203d`) and
built the agent-control endpoint's connection, its microVM's pinned key and its members' stdio lanes (step 11; D238 at
`60c5dfbc6`) with the controller's side Hex is to reach them through (`control::Controller`, D238 at `a8eee1382`); a
guest's room told through shards-api with an image run's microVM cost, each vCPU's guest time among it, in its stats
(D241 at `9bf25f367`, which `Awaits(GuestRoom)` and `Awaits(GuestCpuTimePerVcpu)` wait on); a command run in a member
as the run protocol's exec, with no exec of shards-api's own (D243 at `628de209b`, Shards ROOT's decision), carried on
exec lanes of its own, Hex's `MemberExec` an exec lane's ask, their endpoint's and controller's sides built (D238 at
`cc36a60d3`, Shards ROOT's design (B)), and the daemon's exec host (D238 at `789f9ab08`); each member held to its own
reach of the names kept (D246 at `7a4ef5c02`, Hex's recorded change 0002 upstreamed); the rate Hex caps each image VM
at, told through its run host in milli-CPU (D247 at `4c5341a3d`, image channel version 13, for `learned-charge.md`'s
caps on a Shards image VM); a run host started holding its three standard streams alone (D248 at `9bdd4a7ec`, Hex's
recorded change 0001 upstreamed, since which no call awaits `RunHostStdioOnly`); the GPU's VM process built and
carried for Windows on arm64 (D232 at `9e01ef94a`, Hex's recorded shards-gpu change 0001 upstreamed); a run whose
command never started ended failed in its VM's words, its run host reading `DONE` as Shards' daemon does (D249 at
`67cbbfec3`, from `image_real`'s HEX-IMG-3 and the patches Hex's lead drafted for it); and a run that keeps nothing
past its microVM given the network its image links its members to, its network process letting nothing past (D250 at
`3a16dbd92`, from `image_real`'s Codex refused for no network). None of it is vendored here yet: `vendor/shards` is
Shards `e82b9403` with Hex's recorded changes 0001 and 0002. The joint plan's every decision and where it is recorded:
Shards' `docs/research/s37-decision-inventory.md` (plan step 37).

## The gate's skips

A test that can't run where the gate runs (a host without cgroup v2 delegation or `systemd --user`, a Kubernetes
pod's root without CAP_SYS_ADMIN, as in kiln CI) skips only through a check that says what it lacked and why
(CLAUDE.md §3). libtest captures a passing test's output and shows it only for one that fails, so a skip said on
stderr alone never reaches a CI log. Each skip is therefore also appended, one line of the binary, the test, what
it lacked and why, to the skip record (`hex_testkit::skip`): `tmp/hex-skips.tsv` under the target directory,
which `scripts/gates.sh` names in `HEX_SKIPS`, empties before `cargo test` and prints after it, pass or fail
(HEX-GATE-1). The record's tests: a skip lands as one line of its four fields, skips from many threads land whole,
a long reason is cut to the line's bound, and the gate prints the record it names (`crates/hex-testkit`). In kiln's
gates pod the cgroup-delegation tests are the case: each is in the record, and so in the CI log. What would
falsify it: a skip that reaches no record (a test returning early without `skip`), which the record's absence
from a CI log can't show; a test of each `return` before a test's work is the remedy if one appears.
