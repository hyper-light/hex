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
| Hex's wire over hyper-transport: hex-client's transport and the node's listener carry hex-core's wire on hyper-quic lanes (control, a stream lane per stdio stream, staging and unstaging, logs); AES-256-GCM alone, each end asserting the suite it negotiated; identity by CSR enrollment with kiln's CA, so keys never move; waits learned per call kind and class; the node's report of each link's peer, suite and group, on its wire and `hex node links` (`hex-quic.md`, Q1-Q12) | joint plan step 4, CP-T7.3; Q12 Shards plan step 34 | decided 2026-11-12, Q12 2026-11-13; built on main (`cp/t7-goal4-transport`); the step's acceptance, the conformance suite over it on Linux, macOS and Windows (Windows through a kiln agent on jupiter), open on Hex ROOT's goal stack |
| The Shards class on Agentfile images: a microVM per box that Shards' image boot (D231) boots under shards-init, pinned by digest and sized by its admitted allocation; the members its caller drives handed over with their stdin, stdout, stderr and end (`Runtime::member`, joint plan step 17), their exec and signals through Shards' agent-control endpoint once built, refused typed until then; an owner's crash holds each run's claim until a reboot proves it ended (`shards-image-class.md`) | joint plan step 12 | built 2026-11-12, to Shards `db4168345` since, on Linux, macOS and Windows hosts (its run host started holding only its stdio, D248 at Shards `9bdd4a7ec`, Hex's change 0001 upstreamed and dropped; each vCPU's guest time in its usage, D241; its size held to Shards' `envelope`, `check`'s rule, at `672b6fb47`; `image_real`'s HEX-IMG-3: under api-run its command never starts, which D249 at Shards `67cbbfec3` makes its run say and `BoxInfo.ended` shows since Hex `32b66a10`, the fault api-run's, owed by Shards); a member's streams over the node's wire, on its socket and over QUIC; its waits' records and its real-VM runs owed |
| Hex's node under shards-init: the `oci` class on macOS and Windows runs its node as the workload of a Shards VM booted through shards-init, with Shards' nesting grant (D234), reaching its host over Shards' agent-control endpoint; Hex's own PID 1 kept, shards-init working with it, and the hex-guest path (`hex` as PID 1) still passing on its own (`node-under-shards-init.md`; `shards-image-class.md` §7) | joint plan step 39 | decided 2026-11-13: Hex's PID 1 is the run's own command under D234's grant, PID 1 of the workload's own PID namespace, and shards-init stays the VM's. Built: init's shards-init host (hex-cli `guest.rs`: a tmpfs for state and staged, the told deadline, the relay on AF_UNIX sockets, READY=1, an exit) and the node image (`scripts/guest/node.Agentfile`, `node-image.sh`). A node configured for that form is refused `Awaits(NestedChannel)` before anything boots, until Shards' step 11 gives a lane to a nested workload's socket and an endpoint for D231's run host (Shards `7a4ef5c0` has neither). Step 11's built parts: the endpoint's connection, its microVM's pinned key, its members' stdio lanes and the controller's side of them (Shards `a8eee1382`); a command run in a member, built as the run protocol's exec at Shards `628de209b` (D243), carried on exec lanes of its own, whose endpoint's and controller's sides are built at Shards `cc36a60d3` and the daemon's exec host at `789f9ab08` (D238); its signal, inspect, terminal and log lanes building; where the endpoint's TLS ends is decided, at the host (plan step 10, D238 at Shards `1b6e1203d`). D234 is built (plan step 38), its `gpus=` and its real-VM runs owed. Hex ROOT's, with Shards ROOT: the conformance suite through shards-init on HVF and WHP once the lane lands, and start latency and memory per node VM both ways (`node_hosted_real`, `guest_boot_record`) |
| Snapshot templates, pool sizing and working-set prefetch | CP-T7.2 | open |
| The scheduler's architecture: goodput over balance, a sampled fast path with an optimiser tier, suitability filters on declared needs, Vivaldi proximity on hyper-swim, priority/fair-share/gangs/preemption, correctness against a serial spec, laptop-to-cell-to-region on one code path, fast isolation of bad workloads and nodes, and hex-sim first (`scheduler-architecture.md`) | CP-T7.3-T7.12 | decided 2026-10-24; implementation open |
| The control plane's state and watches in Amphora | CP-T7.3 | open (within `scheduler-architecture.md`, S6) |
| The scheduler's framework and scoring | CP-T7.4 | decided (`scheduler-architecture.md`, S1-S3) |
| A GPU for a Firecracker microVM: the administrator binds its IOMMU group to `vfio-pci`, opens `/dev/vfio` to the node and lets it lock guest memory; the guest catalogs what its VMM attached by its own PCI address, joined to the host's selection only through the VMM's witness (`firecracker-gpu-host.md`) | AC-T7.4.1 (Hex's lead, 2026-11-11) | decided 2026-11-11; no lab host bound yet |
| GPU telemetry for the balancing: NVIDIA through NVML loaded at run time, two calls a GPU a sample; AMD through amdgpu's sysfs counters kept open on Linux, one read a counter, and through ADL on Windows, two calls a GPU; macOS refused by type (`gpu-telemetry.md`, G1-G4) | CP-T7.3 (Ada, 2026-11-09) | decided 2026-11-09; built in `hex-gpu` |
| Deadline waits: Hex's condition variable, channel and timer (`hex-wait`): std's condition variable on Linux and Windows, and on macOS a waiter's own kqueue whose deadline is a one-shot `NOTE_CRITICAL` timer, since XNU gives std's timed waits their thread's QoS leeway, at utility up to four times the wait and 75 ms (`deadline-waits.md`) | AC-T7.4.1 (Hex's lead, 2026-11-12) | decided 2026-11-12; built in `hex-wait`, the condition variable's 12 sites and the channel's and timer's moved; Windows's measurement owed |
| Quotas, fair share, preemption and gangs | CP-T7.5 | decided (`scheduler-architecture.md`, S5) |
| A microVM charged its learned use, its envelope the ceiling; CPU shared by booking, memory held to a budget, the over VM ended last; budgets per fleet as each guest's D188 room (`learned-charge.md`) | joint plan step 14 | charge decided by Ada 2026-11-11; its rule (C1-C7) and step 13's interface (C8) taken by Shards ROOT 2026-11-12 (`learned-charge.md`); built on main for macOS and Windows nodes' managed VMs, each capped at its water-filled share each window (`b19ef663`), their last resort ending a VM through its managed receipt, why written into its record first (`6ff188e2`, `803d311f`); its Shards inputs (D241's room and cost, D247's rate) vendored here since Shards `1389dc0b`, their use building |
| One capacity view per machine: Hex's node agent keeps the machine's one ledger of room and kiln's runner reserves through it; one rule for full on both sides, fences by epochs and sessions, never a release on silence, the person's release of a holder gone for good, no leases, a holder's used memory counted twice and never credited (`capacity-view.md`, CV1-CV12) | joint plan step 13 | proposed 2026-11-12; protocol sent to kiln's Load Balancing agent; cores, seam, the node's shell and its QUIC lane built; CV12's credit withdrawn 2026-11-12 (no read of the OS's makes it safe); kiln's side: its runner's ledger and its holding built on kiln main (kiln `docs/lb.md` "A machine's keeper"; `e94a1953f`, `562268830`, `035813991`): Hex's holder core driven for the ledger, no more claims than it may reserve (CV8, CV9), a claim's room reserved before its run starts, its waits on the keeper following the keeper's liveness and progress alone, a run that asks nothing started with no reservation, as the keeper refuses an empty one (agreed with Hex's lead, 2026-11-13), and kiln-ci's simulator holding it to Hex's keeper (`capacity::sim::SimView`) under crashes, partitions and heals; its runner's shell, the holding over `CAPACITY_PORT` with its identity enrolled, next |
| Networking: namespaces, CNI-compatible plugins, policies from Agentfile grants | CP-T7.7 | open |
| Network grants for a microVM's members: who, to whom, which ports, which way, default deny; Hex decides each VM's policy and Shards' per-VM network process enforces it; what Shards can hold exactly is programmed: each destination a grant names by TCP kept and every member held to its own reach (D236; D246, upstreamed from Hex's change 0002), a publish held to the receivers Shards names (`Bound::to`); the rest refused typed (`network-grants.md`) | joint plan step 15, CP-T7.7 | decided 2026-11-12; built to Shards `e82b9403`, its real-VM runs owed; D246 (Shards `7a4ef5c02`, change 0002 dropped) and D250 (Shards `3a16dbd92`: a run that keeps nothing past its microVM keeps the network its image links its members to, its network process letting nothing past, from `image_real`'s Codex refused for no network) vendored here since Shards `1389dc0b` |
| Storage: a CSI-like interface; images from Reliquary, lazily | CP-T7.8 | open |
| The Kubernetes API subset | CP-T7.13 | open |

`vendor/shards` pins Shards main `db4168345` (`740fa372` before), which holds what Shards has built since, each recorded
in Shards' `docs/design/architecture.md`: destinations by name and a published port's receiving member (D236, D231's
`Bound::to`; joint plan step 9, whose use `network-grants.md` records as step 15's to program); a member's stdin on the
orchestrator's path (`ImageGuest::take_input`, step 5, D240); a member's environment, home and working directory (step
7, D240); ring pins within a guest's room (D230, D188; `learned-charge.md` C7), which no call awaits since `37f940c5`
(`RingPinsWithinRoom`); AES-256-GCM alone on Shards' own links (D239); agent control over hyper-transport, its TLS
ending at the host (plan step 10; D238 and PM M540 at `1b6e1203d`), its endpoint's connection, its microVM's pinned key
and its members' stdio lanes (step 11; D238 at `60c5dfbc6`) with the controller's side Hex is to reach them through
(`control::Controller`, D238 at `a8eee1382`); a guest's room told through shards-api and an image run's microVM cost,
each vCPU's guest time among it, in its stats (D241 at `9bf25f367`: the image class reads the guest time,
`shards-image-class.md` §4, and `Awaits(GuestRoom)` waits on it, step 14's to lift); a command run in a member as the
run protocol's exec, with no exec of shards-api's own (D243 at `628de209b`, Shards ROOT's decision), carried on exec
lanes of its own, Hex's `MemberExec` an exec lane's ask, their endpoint's and controller's sides built (D238 at
`cc36a60d3`, Shards ROOT's design (B)), and the daemon's exec host (D238 at `789f9ab08`); credentials granted by
reference (D242); a member's terminal (D244); each member held to its own reach of the names kept (D246 at `7a4ef5c02`,
Hex's change 0002 upstreamed and dropped); the run's CPU rate, told through its run host in milli-CPU (D247 at
`4c5341a3d`, for `learned-charge.md`'s caps on a Shards image VM); a run host started holding its three standard streams
alone, through `shards_ipc`'s spawn (D248 at `9bdd4a7ec`, Hex's change 0001 upstreamed and dropped, since which no call
awaits `RunHostStdioOnly`); the GPU's VM process built and carried for Windows on arm64 (D232 at `9e01ef94a`, Hex's
shards-gpu change 0001 upstreamed and dropped); a run whose command never started ended failed in its VM's words, its
run host reading `DONE` as Shards' daemon does (D249 at `67cbbfec3`, from `image_real`'s HEX-IMG-3); and a run that
keeps nothing past its microVM given the network its image links its members to, its network process letting nothing
past (D250 at `3a16dbd92`, from `image_real`'s Codex refused for no network); a guest's size held to `check`'s rule from
one source, `envelope` (at `672b6fb47`), which Hex sizes an image VM by rather than restate it; each vCPU's unread cause
typed (`shards_abi::guest_time::Unread`, version 14, `2fb8e9ba3`); waits judged by their progress, not recorded (Part B,
`ca7a04d0d`: `Refusal::Stalled`, no `shards_api::record`, no conformance patiences); a lane to a socket of the run's own
command and the run's own agent-control endpoint, bound by its run host, whose controller shards-api carries from
shards-remote (D251, D252: `ImageSpec::control`, version 15, `5bd110792`); the run's GPUs by identity under the nesting
grant (D234's `gpus=`, `ImageSpec::gpu_devices`, version 16, `e46299c07`), its workload's cgroup sized by its microVM's
CPUs (`a853a8138`); a VM process's refusal of its GPU selection typed (`GpuProbeError::Refused`, `Unreported`;
`18f59d497`); the period a run's rate is held over (`ImageGuest::rate_period`, version 17, `1acbf2cb9`); a member's
run-time arguments (D256, `MemberSpec::args`, version 18); how a run's microVM started, from its image's template or a
warm run host's (D253, `ImageGuest::started_from`, versions 19 and 21, which a box's usage says and its record keeps;
`ImagePools`, not used yet); its credential grants' values leased by the orchestrator (D242, `ImageSpec::lessor`,
version 20: none here, as no box grants a credential); the setup keys a run's spec may name (D257); and a run's end
saying why, its run host's diagnostics kept whole in a file its orchestrator names (D258,
`ImageGuest::keep_diagnostics`); each credential grant's placeholder, which the run's injector fills a value in where it
stands (D242's step 3, version 22, `51e674604`: `shards_dns::credential`, which `credential-grants.md` follows); and the
run's microVM saved whole for its owner (D259, `ImageGuest::save`, version 23, `8a9fa413d`, which Hex asks none of
yet): image channel version 23. shards-remote's hyper-quic is Hex's one vendored copy
(vendor/hyper-raft at `045cf3ff`, hyper-raft's main, the commit shards-remote pins: its patch Q34 is the
`Connection::idle_timeout` shards-remote's keep-alive reads), patched over upstream's git source. The joint plan's every
decision and where it is recorded:
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
