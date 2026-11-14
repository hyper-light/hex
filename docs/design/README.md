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

The statuses of joint plan steps 4, 12 to 15 and 39 name what Hex holds by SHA, with built, merged and live kept apart
(CLAUDE.md, "Working here"). Live means on GitHub's main (`025aae6d` on 2026-11-13). Merged means on Hex's main and
live from its next push. Built means on a branch and not merged.

| Decision | Made by | Status |
|---|---|---|
| The OCI runtime: Hex's own or youki's libcontainer, by conformance, start latency and the no-panic rule | CP-T7.1 | decided: Hex's own (`runtime.md`) |
| The runtime's architecture against the 4-5x bar: volatile state, one supervisor per node, the `Runtime` seam as the fast path, network-namespace work off the path, a seed per node, mount namespaces without copying, no wrapper per box (`runtime-architecture.md`) | goal 4 (kiln plan `01a16e75781c776390a6efeb797a9ad6`) | decided 2026-10-24; implementation open |
| The `process` box per platform | CP-T7.1 | open |
| Linux images on macOS and Windows: one Shards guest per node running Hex's Linux node unchanged, its wire over vsock, reached through a `shards-api` seam (`shards-guest.md`) | native plan step 5 | proposed 2026-11-04; seam agreed with Shards' owner 2026-11-04 |
| Hex's wire over hyper-transport: hex-client's transport and the node's listener carry hex-core's wire on hyper-quic lanes (control, a stream lane per stdio stream, staging and unstaging, logs); AES-256-GCM alone, each end asserting the suite it negotiated; identity by CSR enrollment with kiln's CA, so keys never move; waits learned per call kind and class; the node's report of each link's peer, suite and group, on its wire and `hex node links` (`hex-quic.md`, Q1-Q12) | joint plan step 4, CP-T7.3; Q12 Shards plan step 34 | decided 2026-11-12, Q12 2026-11-13; met 2026-11-13 (Hex ROOT's goal for step 4). Live: the transport and the node's listener (`1f38bd36`), a connection's end ending its bridges (`3045b824`), a stream cut short told as cut (`3c44c7f6`, `a10f7fe7`), a Windows node dialable (`7e48ef7d`), the lanes a node grants a peer (`49500736`, HEX-QUIC-27) and Q12's links report (`09431b42`: `Call::Links` 0x0018, `hex node links`). HEX-QUIC-25 passed across Linux and macOS at `3045b824`, both its kill and its partition halves, and the QUIC conformance suite passed on Windows at `7e48ef7d`. Merged: one hyper-quic, from hyper-raft's main `045cf3ff` (`8aa7c028`), then `e0613f52`, patch Q35 on hyper-raft's `p/q35-handshake-progress-v1`, main-bound (`1314e5f2`), and the bridge shared with hex-client (`a8f6bfe4`). Owed: HEX-QUIC-27 on Windows, and lanes raised on a live connection, an ask of hyper-transport (Q8) |
| The Shards class on Agentfile images: a microVM per box that Shards' image boot (D231) boots under shards-init, pinned by digest and sized by its admitted allocation; the members its caller drives handed over with their stdin, stdout, stderr and end (`Runtime::member`, joint plan step 17), their exec and signals through Shards' agent-control endpoint once built, refused typed until then; an owner's crash holds each run's claim until a reboot proves it ended (`shards-image-class.md`) | joint plan step 12 | built 2026-11-12, on Linux, macOS and Windows hosts. Live: the class on D231 (`68ed140a`), its cases through a real node on Linux KVM (`d71d6924`), and Shards `1389dc0b` with no Hex change (`2c728252`): D248's run host holding only its stdio (Hex's 0001 upstreamed), D246 (Hex's 0002 upstreamed) and each vCPU's guest time (D241). Merged: Shards `e0cc059c` (`c8b4ce6b`: D251 and D252's control, D234's `gpus=`, `rate_period`, its size held to Shards' `envelope`, the cpuset fix behind HEX-IMG-3) then `740fa372` (`8aa7c028`: D258's diagnostics, how its microVM started), then `db4168345` (`1d60abad`, `1c4d11b3`: image channel v23, each credential grant's placeholder, D242's step 3, with C2's per-host rule gone and its placeholder rules in its place; D259's save, which the class asks none of), then `3114e082f` (`8dfe6df8`, `31c651fb`: image channel v24, a delete stopping a boot under way with D231's `BootStop`, and the sign-ins a run's injector captures, none granted here yet), then `097ec0047` (`1314e5f2`, `2f5d20e7`: image channel v25, D254's MCP bridge on a member's spec, refused by its field while the class asks no agent-control endpoint, and each grant's field held to the network process's wire, refused as a spec), then `37ddeb080` (`2f55dccf`: image channel v26, D242's upstream roots, none named here, and an orchestrator's member lanes served by the run host), then `16451e19f` (`cf844022`: image channel v27, a member's end typed, a member a signal killed `Exit::Signal(n)`); a failed run's end in its runtime's words (`32b66a10`, `BoxInfo.ended`; `94d519d6`, `Refusal::Failed`); the class whole as `ImageClass` (`ae7038d7`) and its host use read to its end (`2a8bf6be`); a member's run-time arguments and the VM's shares (`cf535152`); a failed build's own words (`28ab15df`); `NoSuchBox` for an id a macOS or Windows node's shared guest holds no record of (`fac528d9`); credential grants by reference (`c7603b0b`, `credential-grants.md`). `image_real`'s HEX-IMG-1 to 7 passed on Linux KVM at `c8b4ce6b` and `8aa7c028`, and `image_record` at `c8b4ce6b` gave n=299, p50 99.8 ms, p99 133.2 ms. On macOS HVF at `cf535152`, HEX-IMG-2 to 5 passed and `image_record` gave p50 94.7 ms, p99 650.7 ms. Owed: HEX-IMG-1, 6 and 7 on HVF at `fac528d9` or later; every case on Windows WHP; `image_record` on KVM again in a quiet window, since load skewed it at `8aa7c028`; and what Shards owes it (`shards-image-class.md` §8) |
| Hex's node under shards-init: the `oci` class on macOS and Windows runs its node as the workload of a Shards VM booted through shards-init, with Shards' nesting grant (D234), reaching its host over Shards' agent-control endpoint; Hex's own PID 1 kept, shards-init working with it, and the hex-guest path (`hex` as PID 1) still passing on its own (`node-under-shards-init.md`; `shards-image-class.md` §7) | joint plan step 39 | decided 2026-11-13: Hex's PID 1 is the run's own command under D234's grant, PID 1 of the workload's own PID namespace, and shards-init stays the VM's init. shards-init works with Hex's PID 1 and never replaces it. Live: init's shards-init host (hex-cli `guest.rs`: a tmpfs for state and staged, the told deadline, the relay on AF_UNIX sockets, READY=1, an exit) and the node image (`scripts/guest/node.Agentfile`, `node-image.sh`) (`7f4bda5b`, `982f1a6b`), the host's boot of it to READY (`b29e0bc3`, `guest::hosted`), and Hex's PID-1 path booting again (`9590d4d4`). Merged: Shards' step 11 (a) and (b), a lane to a socket of the run's own command and the run's own agent-control endpoint (D251, D252), vendored since `c8b4ce6b`; a guest's boot as one form, `Boot::Kernel` or `Boot::Image`, and the Link's lanes (`a8f6bfe4`); the node's descriptors fitted on every OS with each hosted VM's lanes among them (`f2e6d8a2`), and Windows' handles at its start (`885738a1`); a park following its init past W* (`6d456c3d`). Measured, READY through shards-init against the direct boot: on Linux KVM at `8aa7c028`, n=23, p50 152.4 ms against 134.3 ms, with 3.65 MB a run; on macOS HVF at `cf535152`, p50 about 165 ms against 103 ms, the first (cold) boot 765 ms. Built: a node VM as a holder of the node's ledger in hex-sim (`cp/t7-link`, `9505cc1d`). In progress, the node VM's entry: E1, its record kind and `NodeRun`; E2, `ImageClass::boot_node`; E3, `Boot::Image` in the guest runtime, which lifts the `Awaits(NestedChannel)` a node in that form is refused until then; E4, `node-under-shards-init.md` §5 to §9. Owed, Hex ROOT's with Shards ROOT: the conformance suite through shards-init on HVF and WHP, the host VMM's memory both ways, and every WHP run |
| Snapshot templates, pool sizing and working-set prefetch | CP-T7.2 | open |
| The scheduler's architecture: goodput over balance, a sampled fast path with an optimiser tier, suitability filters on declared needs, Vivaldi proximity on hyper-swim, priority/fair-share/gangs/preemption, correctness against a serial spec, laptop-to-cell-to-region on one code path, fast isolation of bad workloads and nodes, and hex-sim first (`scheduler-architecture.md`) | CP-T7.3-T7.12 | decided 2026-10-24; implementation open |
| The control plane's state and watches in Amphora | CP-T7.3 | open (within `scheduler-architecture.md`, S6) |
| The scheduler's framework and scoring | CP-T7.4 | decided (`scheduler-architecture.md`, S1-S3) |
| A GPU for a Firecracker microVM: the administrator binds its IOMMU group to `vfio-pci`, opens `/dev/vfio` to the node and lets it lock guest memory; the guest catalogs what its VMM attached by its own PCI address, joined to the host's selection only through the VMM's witness (`firecracker-gpu-host.md`) | AC-T7.4.1 (Hex's lead, 2026-11-11) | decided 2026-11-11; no lab host bound yet |
| GPU telemetry for the balancing: NVIDIA through NVML loaded at run time, two calls a GPU a sample; AMD through amdgpu's sysfs counters kept open on Linux, one read a counter, and through ADL on Windows, two calls a GPU; macOS refused by type (`gpu-telemetry.md`, G1-G4) | CP-T7.3 (Ada, 2026-11-09) | decided 2026-11-09; built in `hex-gpu` |
| Deadline waits: Hex's condition variable, channel and timer (`hex-wait`): std's condition variable on Linux and Windows, and on macOS a waiter's own kqueue whose deadline is a one-shot `NOTE_CRITICAL` timer, since XNU gives std's timed waits their thread's QoS leeway, at utility up to four times the wait and 75 ms (`deadline-waits.md`) | AC-T7.4.1 (Hex's lead, 2026-11-12) | decided 2026-11-12; built in `hex-wait`, the condition variable's 12 sites and the channel's and timer's moved; Windows's measurement owed |
| Quotas, fair share, preemption and gangs | CP-T7.5 | decided (`scheduler-architecture.md`, S5) |
| A microVM charged its learned use, its envelope the ceiling; CPU shared by booking, memory held to a budget, the over VM ended last; budgets per fleet as each guest's D188 room (`learned-charge.md`) | joint plan step 14 | charge decided by Ada 2026-11-11; its rule (C1-C7) and step 13's interface (C8) taken by Shards ROOT 2026-11-12 (`learned-charge.md`). Live: the pure core (`d02a039d`), admission at the learned charge (`b38ce550`), C4's placement (`3e30392d`), the last resort (`eff0eaea`), and macOS and Windows nodes' managed VMs, each capped at its water-filled share each window (`b19ef663`) and ended through its managed receipt, with why written into its record first (`6ff188e2`, `803d311f`). Also live: one derived bound of VMs and classes (`21c779cf`), the density driver and the trace recorder (`0b483d0c`, `7f1d7334`), a Shards image VM's room, rate and host cost through its runtime (D241, D247; `6df918db`), a cold class booked at its admission (`2cd5f8f3`), and a VM's ceiling (`2964330d`). Merged: a census's cost as evidence alone, and the VMs recorded under it retired (`97f77b14`, `5362acb3`); no `memory.max` at a VM's create (`9f02ca8b`); the recorder's memory from resident sets (`70a97b51`) and hex-sim replaying recorded traces (`9e0c5805`); macOS's CPU control learning its response (`a8dc671c`, `0ac9ce2a`, `2fd40b14`), its holds read once and stopped again (`312f4e98`, `c9d5cf93`); a backing's own guest overhead measured (`3278e018`, `6f40da16`); an unread VM counted whole (`0e401dbe`); and a managed stop's own wait (`788dda27`). Built: the learner's window from each image VM's rate period (`cp/t7-goal14-rate-period`, `f95ed62b`). Owed: density, fairness and the CPU violation rate per OS, on real harness VMs and recorded traces (ROOT's); a node VM's charge (step 39's `NodeRun`); and a `Backing` refusal's why at each site |
| One capacity view per machine: Hex's node agent keeps the machine's one ledger of room and kiln's runner reserves through it; one rule for full on both sides, fences by epochs and sessions, never a release on silence, the person's release of a holder gone for good, no leases, a holder's used memory counted twice and never credited (`capacity-view.md`, CV1-CV12) | joint plan step 13 | proposed 2026-11-12; protocol sent to kiln's Load Balancing agent. Live: the keeper and holder cores, their seam and simulator (`1cd01f67`), the node's keeper under its admission (`6ade789a`, `7d5fc109`), hex-sim's nemesis (`8e0ee510`), and the QUIC lane (`41b3c4c7`), which hears only the holders it names (`40f06ce3`, `f6d0ef7d`). Also live: the person's release (`25d848d3`), CV12's credit withdrawn because no read of the OS's makes it safe, and the stand-in holder with HEX-CV-L1 and HEX-CV-2H (`90c5c108`, `80db559a`). HEX-CV-2H passed between Linux and macOS at `90c5c108`, with each host as the keeper in turn. Merged: a Windows node's process route in its `oci` form, initialized or not, and hex-sim's settling bounded by what is in flight (`54a896cc`); the waits under decision 9 (`8c5197be`); HEX-CV-L1 passes at both on Linux, macOS and Windows. kiln's side: its runner's ledger and its holding built on kiln main (kiln `docs/lb.md` "A machine's keeper"; `e94a1953f`, `562268830`, `035813991`): Hex's holder core driven for the ledger, no more claims than it may reserve (CV8, CV9), a claim's room reserved before its run starts, its waits on the keeper following the keeper's liveness and progress alone, a run that asks nothing started with no reservation, as the keeper refuses an empty one (agreed with Hex's lead, 2026-11-13), and kiln-ci's simulator holding it to Hex's keeper (`capacity::sim::SimView`) under crashes, partitions and heals. Owed: kiln's runner shell, the holding over `CAPACITY_PORT` with its identity enrolled (kiln goal 54); then ROOT's HEX-CV-2K, with kiln's holder in the stand-in's place; and ROOT's HEX-CV-2H partition phase between Linux and Windows, whose other phases passed at `80db559a` |
| Networking: namespaces, CNI-compatible plugins, policies from Agentfile grants | CP-T7.7 | open |
| Network grants for a microVM's members: who, to whom, which ports, which way, default deny; Hex decides each VM's policy and Shards' per-VM network process enforces it; what Shards can hold exactly is programmed: each destination a grant names by TCP kept and every member held to its own reach (D236; D246, upstreamed from Hex's change 0002), a publish held to the receivers Shards names (`Bound::to`); the rest refused typed (`network-grants.md`) | joint plan step 15, CP-T7.7 | decided 2026-11-12. Live: the grants and their one check wherever they enter (`944ab292`), the image class's boot programmed from them or refused typed (`9bdcc666`, `7767d4db`), a harness reaching its granted providers and kiln's endpoint alone, each destination by TCP kept and every member held to its own reach (`25237f5e`), and each boot naming each member's reach, with check 2 holding a VM granted nothing (`6027470c`). All of it is on Shards `1389dc0b`: D236; D246 (Shards `7a4ef5c02`, Hex's change 0002 upstreamed and dropped); and D250 (Shards `3a16dbd92`: a run that keeps nothing past its microVM keeps the network its image links its members to, its network process letting nothing past, from `image_real`'s Codex refused for no network). Merged: check 2 waiting for its probe while its box settles (`4b1b31c2`), a run that ended on its own recorded with its end (`c56fcfbb`), and the VM granted nothing booted both undriven and driven (`8b65e191`), on Shards `740fa372`. `image_grants_real` passed on Linux KVM at `8aa7c028` and on macOS HVF at `cf535152`. On HVF its three host readings of the run's processes and sockets said `GRANTS-OWED`, as macOS keeps no /proc. Built: readers for them on macOS and Windows (`cp/t7-goal15-host-readers`, `66cb1b1a`). Owed: those readings on HVF, every case on Windows WHP, and N2's enforcer cases on real VMs (`network-grants.md`, Tests) |
| Storage: a CSI-like interface; images from Reliquary, lazily | CP-T7.8 | open |
| The Kubernetes API subset | CP-T7.13 | open |

`vendor/shards` pins Shards main `16451e19f` on Hex's main, merged with `cf844022` (`37ddeb080` before it, at `2f55dccf`;
`097ec0047` before that, at
`1314e5f2`; `3114e082f` before that, at `8dfe6df8`; `db4168345` before that, at `1d60abad`; `740fa372` before that, at `8aa7c028`; `e0cc059c` before that, at
`c8b4ce6b`). GitHub's main, the live one, pins Shards `1389dc0b` (`2c728252`) until the next push. `097ec0047` holds, each recorded in Shards'
`docs/design/architecture.md`: destinations by name and a published port's receiving member (D236, D231's `Bound::to`;
joint plan step 9, whose use `network-grants.md` records as step 15's to program); a member's stdin on the
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
run's microVM saved whole for its owner (D259, `ImageGuest::save`, version 23, `8a9fa413d`, which Hex asks none of yet):
image channel version 23; a stop for a boot under way (D231's `BootStop`, `ed1136305`), which a delete of a booting box
asks; the sign-ins a run's injector captures and refreshes (D242's step 8 slice 2b, `ImageSpec::sign_ins`, none granted
here yet); a cut lane reset at the host (D251 amended, `8754b451f`) and the guest legs' cure (`275b4e178`): image
channel version 24; a member's MCP bridge paired with its controller's asks (D254, `MemberSpec::mcp`, `af8f1ebe5`), each
grant's field held to the network process's wire (`17fc36ecd`), D231's boot stop on Windows (`9a65eb9bb`), a cut lane
reset on Windows too (`e1973ee50`), a run host that ends before its microVM holds its layer leaving no layer file
(`712f791b3`), a handshake that answers but never advances giving its place after a whole window (D180, `4fa48def2`)
and a managed guest's use (D260, `867053aa1`): image channel version 25; a run's upstream roots (D242, `b6814785d`,
its injector trusting no `SSL_CERT_FILE` or `SSL_CERT_DIR`) and an orchestrator's member lanes served by its run host
(D238, `c1773281f`, their handles re-exported by `shards_api::control`): image channel version 26; a member's end typed,
`Signaled(n)` beside its exit (#72, `7aab838e0`): image channel version 27. On Hex's main, shards-remote's hyper-quic is
Hex's one vendored copy (vendor/hyper-raft at `e0613f52`, patch Q35 on hyper-raft's branch
`p/q35-handshake-progress-v1`, bound for its main, the commit shards-remote pins: its patch Q34 is the
`Connection::idle_timeout` shards-remote's keep-alive reads, Q35 the `Connection::handshake_progress` its listeners
judge a handshake by), patched over upstream's git source. GitHub's main vendors hyper-raft at `0856e05d`
and no shards-remote. The joint plan's every decision and where it is recorded:
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
