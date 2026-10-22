# Agent substrates and the sandboxes beneath them

Source note (CLAUDE.md §5), not a design decision. Researched 2026-10-22: every source in References
(`[SUB-n]`) was fetched and read that day, and every number below was found in its source's own text
(PDFs converted with `pdftotext`, quotations checked line by line). Cross-references to the sibling
notes use their own labels (`[ASP-n]` agent-sandbox-platforms.md, `[OCI-n]`
oci-runtimes-and-fast-start.md). Serves CP-T7.2 (microVM class, snapshots, warm pools), CP-T7.6 (agents
as workloads) and the runtime design goal.

**Scope.** The agent substrates — Google's AX and Agent Substrate, and the Kubernetes Agent Sandbox
project — and the isolation layers they and the agent platforms build on: gVisor, Kata Containers,
Firecracker (boot, snapshots, and the snapshot-restore research), and CRIU.

## 0. Conventions

Kinds per the [method](../README.md#method): `MI` measured-independent, `MV` measured-vendor (the
project or the authors measured their own system), `CL` claimed (no method), `DE` design fact. `n/p` =
not published. Percentiles are quoted exactly as written; **no source in this note publishes a p99.9**,
and most publish no tail at all. `DERIVED` marks arithmetic done here.

## 1. Summary table

| System | Isolation | Start / boot | Restore / resume | Throughput | Density / overhead |
|---|---|---|---|---|---|
| **Google AX** | delegates to Agent Substrate `DE` [SUB-1] | n/p | `ax suspend` / `ax resume` checkpoint actor state `DE` [SUB-1] | target "billions of autonomous agent workloads in a cluster" `CL` [SUB-1] | n/p |
| **Agent Substrate** | gVisor (default) or Kata + Cloud Hypervisor microVM; plain containers unsupported `DE` [SUB-2] | n/p | resume "less than 400ms" `CL` [SUB-2]; north-star activation p95 100 ms `DE` target [SUB-3] | "more than 1500 activations per second" `CL` [SUB-2]; north-star 1,000 wakeups/s per cluster `DE` target [SUB-3] | "10x higher density than standard containers", "10x–30x oversubscription" `CL`; demo: ~250 actors on 8 pods `MV` [SUB-2]; north-star 1 billion agents per cluster `DE` target [SUB-3] |
| **K8s Agent Sandbox** | gVisor or Kata via `RuntimeClass` `DE` [SUB-4] | n/p | warm pool (`SandboxWarmPool`) `DE` [SUB-4] | GKE: 300 sandboxes/s per cluster, 90% of allocations ≤ 200 ms `CL` [ASP-80] | n/p |
| **gVisor** | user-space kernel (Sentry) on Systrap (default since mid-2023) or KVM `DE` [SUB-5] | Catalyzer's gVisor baseline: 142 ms for a C hello `MV` [OCI-28] | native checkpoint/restore (used by Substrate) `DE` [SUB-3] | n/p | vs runc (2019): syscalls 2.2×, memory allocation 2.5×, large downloads 2.8×, file opens 216× slower `MI` [SUB-7] |
| **Kata Containers** | VM per sandbox, `kata-agent` inside; QEMU, Cloud Hypervisor, Firecracker or Dragonball `DE` [SUB-8] | RunD's measurement: 200 concurrent sandboxes in 47.6 s (Kata-FC), 6.85 s (kata-qemu), 2.98 s (kata-template) `MV` [OCI-31] | Substrate microVM class: memory-only snapshot, `userfaultfd` demand paging `DE` [SUB-3] | n/p | n/p |
| **Firecracker** | KVM microVM, minimal device model `DE` [SUB-11] | ≤ 125 ms from `InstanceStart` to guest `/sbin/init`; VMM up in ≤ 8 CPU-ms (wall 6-60 ms, typically ~12) `MV`, CI-enforced [SUB-9] | REAP: record-and-prefetch working set, 3.7× lower cold-start delay than baseline snapshots; 70 → 185 ms from 1 to 8 concurrent `MV` [SUB-12] | "up to 150 MicroVMs per second per host" `MV` [SUB-11] | VMM memory ≤ 5 MiB (1 vCPU, 128 MiB guest) `MV`, CI-enforced [SUB-9]; "less than 5MB per container" `MV` [SUB-11] |
| **CRIU** | restores a process tree (no isolation of its own) `DE` [SUB-13] | — | 193-2,278 ms startup, image cached in memory (GPU apps) `MI` [SUB-14]; 1.7 s at P99 under heavy load (competitor's measurement) `MI` [SUB-15] | n/p | n/p |

## 2. Agent substrates

### 2.1 Google AX

AX is "a high-throughput, declarative orchestrator to run billions of autonomous agent workloads in a
cluster", built on Agent Substrate for sandboxed execution and shaped like Kubernetes (`ax apply`,
`get`, `watch`) [SUB-1]. Its README frames the workload better than any paper found:

> Agents are a new kind of workload. They are neither stateless microservices nor run-to-completion
> batch jobs. They accumulate state, need strict isolation, call out to model APIs and tool servers, and
> can burn money in a loop if nobody is watching. [SUB-1]

Three primitives `DE` [SUB-1]: a **`Task`** (untrusted agent code in an isolated sandbox with CPU and
memory limits), a **`Workspace`** (Git repos, MCP servers and skill packages pre-wired "so every agent
starts warm") and a **`Model`** (the LLM the platform uses, credentials from a Kubernetes secret). `ax
suspend` checkpoints actor state; `ax resume` "pick[s] up exactly where it left off". The control plane
is Redis plus gRPC services. The project warns it is "in heavy development" with "major breaking
changes" expected. **No performance figures are published.**

### 2.2 Agent Substrate

**The core idea** `DE` [SUB-2]: Substrate "maps a larger set of 'actors' (stateful workloads such as
agents) onto a smaller pool of ready 'workers'. Because agents spend most of their lives waiting on
models, tools, or humans, Substrate snapshots idle actors out of memory and restores them when a request
arrives." It claims "10x–30x oversubscription" and "10x higher density than standard containers", resume
"in less than 400ms" and "more than 1500 activations per second" — all `CL`, no method stated [SUB-2].
The one demonstration is ~250 stateful actors multiplexed across 8 pods [SUB-2].

**Isolation is never a plain container** `DE` [SUB-2]: every actor runs in a dedicated gVisor (`runsc`)
or Cloud Hypervisor microVM sandbox. Each active actor gets a short-lived SPIFFE X.509 identity; actors
hold no credentials (an egress gateway injects them into approved outbound requests outside the
sandbox); networking is default-deny.

**Lifecycle and scheduling** `DE` [SUB-3]: a per-sandbox-class `ateworker` herder runs inside
pre-started worker pods and performs `RunWorkload`, `CheckpointWorkload` and `RestoreWorkload`. gVisor
actors suspend and resume through gVisor's native checkpoint/restore; microVM actors (Kata on Cloud
Hypervisor) take a memory-only VM snapshot restored with `userfaultfd` demand paging, while root-file-
system writes live on the host (served over virtio-fs) so they cost reclaimable page cache, not guest
RAM. Substrate runs its own control plane and scheduler — "selects a ready worker for a resumption
request" — and bypasses "Kubernetes' eventual consistency" with atomic physical assignments, because
"scheduling a workload on Kubernetes requires several asynchronous processes" [SUB-3].

**Targets, explicitly unmet so far** — the architecture document opens "Much of this architecture is
aspirational, and is not yet implemented!" [SUB-3]. Its north-star metrics `DE` [SUB-3]:

| Metric | Definition | Target |
|---|---|---|
| Activation latency | wakeup event received → agent can receive traffic | 100 ms at p95 |
| Scale | agents, active and idle, in one cluster | 1 billion |
| Throughput | wakeup events handled per second in one cluster | 1,000 / s |

### 2.3 Kubernetes Agent Sandbox

A SIG Apps project: a `Sandbox` CRD and controller for "isolated, stateful, singleton workloads", i.e.
one long-running pod with a stable identity and persistent storage [SUB-4]. It is "a *sandbox
orchestrator*" that delegates isolation to gVisor or Kata through `RuntimeClass`, and adds
`SandboxTemplate`, `SandboxClaim` and `SandboxWarmPool` ("a pool of pre-warmed Sandboxes that can be
quickly allocated") `DE` [SUB-4]. The README publishes no numbers; Google's GKE launch claims 300
sandboxes/s per cluster with 90% of allocations within 200 ms `CL` [ASP-80].

## 3. The sandboxes beneath

### 3.1 gVisor

A user-space kernel (the Sentry) intercepts the application's system calls. The interception mechanism
is the *platform*: `systrap` (seccomp `SECCOMP_RET_TRAP`) "replaced `ptrace` as the default gVisor
platform in mid-2023"; KVM "will provide the best performance" on bare metal, while systrap is better
inside a VM `DE` [SUB-5]. gVisor's own guide says raw memory access carries no added cost and that the
Sentry needs memory of its own per application `DE` [SUB-6]; its density and syscall figures are graphs
whose data was not extracted here (gap).

The one independent study found [SUB-7] measured gVisor against runc in 2019: "system calls are 2.2×
slower, memory allocations are 2.5× slower, large downloads are 2.8× slower, and file opens are 216×
slower" `MI`. **This predates systrap** (gVisor then defaulted to ptrace), so it is an upper bound on
today's cost, not a current figure; the competitive baseline must re-measure `runsc`.

### 3.2 Kata Containers

One lightweight VM per sandbox; the `kata-agent` boots inside it, the runtime calls the agent's
`CreateSandbox`, and the agent spawns the workload in a container environment and reports exit through
`WaitProcess` `DE` [SUB-8]. Hypervisors: QEMU, Cloud Hypervisor, Firecracker, Dragonball [SUB-8]. The
measured cost of that design under concurrency comes from RunD (ATC 2022): starting 200 sandboxes
concurrently took 47.6 s on Kata-Firecracker, 6.85 s on kata-qemu, 2.98 s with kata-template, and 1 s on
RunD `MV` [OCI-31].

### 3.3 Firecracker

**Cold boot.** The NSDI 2020 paper: "memory overhead of less than 5MB per container, boots to
application code in less than 125ms, and allows creation of up to 150 MicroVMs per second per host" `MV`
[SUB-11]. The repository's specification turns these into integration tests run on every PR (m5d.metal,
m6g.metal) `MV` [SUB-9]: VMM start to API socket within 8 CPU-ms (wall clock "spanning 6 ms to 60 ms,
with typical durations around 12 ms"); VMM-thread memory ≤ 5 MiB for a 1-vCPU, 128 MiB guest; ≤ 125 ms
from `InstanceStart` to the guest's `/sbin/init`.

**Snapshots** `DE` [SUB-10]: full snapshots are resumable; diff snapshots ("developer preview") record
changed pages and must be merged with a base. A listed limitation: "High snapshot restoration latency
when cgroups V1 are in use." The docs publish no restore latency.

**Why restore is slow, and the fix** — REAP (ASPLOS 2021) [SUB-12]: a restored function's state "is
brought from disk into guest memory one page at a time" by page faults; but functions "access the same
stable working set of pages across different invocations", so REAP records that set once and prefetches
it: "Compared to baseline snapshotting, REAP slashes the cold-start delays by 3.7×, on average" `MV`. It
scales well to moderate concurrency — "increasing from 70ms to 185ms when the number of concurrent
function arrivals goes from 1" to 8 — and becomes disk-bandwidth-bound beyond that `MV` [SUB-12].

### 3.4 CRIU

CRIU checkpoints and restores Linux process trees in user space. Its default restore copies memory into
the restored task; "lazy, or post-copy" migration instead "does not copy the task's memory, but rather
keeps the memory pages at the source node", and a lazy-pages daemon "is responsible for handling the
task's page faults and injecting the memory pages into the task's address space" (via `userfaultfd`) `DE`
[SUB-13]. Measured by others: gCROP (SoCC 2024) reports CRIU "still has 193ms–2,278ms startup latency
(image cached in memory)" for GPU applications `MI` [SUB-14]; TrEnv-X attributes CRIU's slowness to "the
expensive copy-based memory restoration" and measured "launching a CR instance takes 1.7 s at P99" under
heavy load, against 15 ms for its own system `MI` (a competitor's measurement) [SUB-15]. For process-level
checkpoint/restore done well, DeltaBox reports 1.86 ms restore `MV` [ASP-78].

## 4. Techniques worth stealing

1. **Actor multiplexing: snapshot the idle, restore on demand.** Agents mostly wait; keeping them
   resident wastes the machine. Substrate's whole design is this (actors over a smaller warm worker
   pool) [SUB-2][SUB-3]; its density and oversubscription claims are unproven but the premise is sound.
2. **Restore by sharing or prefetching memory, never by copying it.** Copy-based restore is the slow
   path (CRIU 1.7 s p99 [SUB-15]); fault-on-demand from disk is next (REAP's baseline [SUB-12]);
   working-set prefetch gives 3.7× [SUB-12]; copy-on-write sharing of a snapshot's pages gives
   sub-millisecond forks (Zeroboot 0.79 ms p50 `MV` [ASP-77]).
3. **Pre-started workers and pooled devices.** Substrate pre-starts worker pods and assigns actors
   atomically, outside Kubernetes' async loops [SUB-3]; RunD's templates and pooling cut 200-sandbox
   start from 47.6 s to 1 s [OCI-31]; CubeSandbox pre-creates network devices [ASP-75].
4. **Keep writes on the host.** Substrate's microVM class serves rootfs writes from the host over
   virtio-fs, so they are page cache, not guest RAM, and snapshots stay memory-only [SUB-3].
5. **Credentials never enter the sandbox.** Substrate's egress gateway injects them outside [SUB-2] — a
   design to adopt for untrusted agent code regardless of speed.

## 5. The bar these set (DERIVED; different hardware and definitions — leads, not measurements)

| Against | Their figure | 4x / 5x better |
|---|---|---|
| Substrate north-star activation | p95 100 ms [SUB-3] | p95 ≤ 25 / 20 ms |
| Substrate north-star throughput | 1,000 wakeups/s per cluster [SUB-3] | ≥ 4,000 / 5,000 per cluster |
| Substrate claimed activations | > 1,500/s [SUB-2] | ≥ 6,000 / 7,500 per cluster |
| Firecracker cold boot to init | ≤ 125 ms [SUB-9] | ≤ 31 / 25 ms |
| Firecracker creation rate | 150 microVMs/s per host [SUB-11] | ≥ 600 / 750 per host |
| Firecracker VMM overhead | ≤ 5 MiB [SUB-9] | ≤ 1.25 / 1 MiB |
| REAP restore at 8 concurrent | 185 ms [SUB-12] | ≤ 46 / 37 ms |

The hardest is the restore class: CubeSandbox (41.8 ms resume) and Zeroboot (0.79 ms fork) already beat
several rows above [ASP-75][ASP-77], so the bar for a self-hostable microVM is set by them, not by
Firecracker's defaults.

## 6. Findings for Hex

1. **The agent class is an actor class.** Both Google systems treat agents as stateful, mostly-idle
   actors that suspend and resume; resume latency and activation throughput, not cold start, are the
   metrics that matter for agents [SUB-1][SUB-2][SUB-3].
2. **Untrusted agent code runs behind a VM-grade boundary in every substrate surveyed** — Substrate
   refuses plain containers [SUB-2]; Agent Sandbox delegates to gVisor or Kata [SUB-4]. Hex's `process`
   and `oci` classes need a stated trust tier; the `firecracker` / `shards` classes carry untrusted work.
3. **Memory is the cost of restore.** Copying it is slow, faulting it from disk is slower than
   prefetching, and sharing it copy-on-write is fastest [SUB-12][SUB-15][ASP-77].
4. **gVisor's tax is real but dated.** 216× on file opens and 2.2× on syscalls (2019, ptrace era)
   [SUB-7]; agent workloads are file- and tool-heavy, so `runsc` must be re-measured before it is
   compared or adopted.
5. **No substrate publishes a measured tail.** Substrate's figures are claims and targets; Firecracker
   publishes bounds and typical values; nobody publishes p99.9 [SUB-2][SUB-3][SUB-9].

## 7. Gaps (not verified)

- **AX's release date and any AX or Substrate measurements.** No benchmark or method is published for
  Substrate's "<400ms" and ">1500 activations per second"; AX publishes none at all.
- **gVisor's current numbers.** The performance guide's density and syscall data are graphs backed by
  CSV files, not extracted here; no independent post-systrap study was found.
- **FaaSnap (EuroSys 2022)**, REAP's successor, could not be retrieved, so it is not cited.
- **Kata start latency** beyond RunD's concurrency figures, and Cloud Hypervisor's own boot figures.
- **CRIU** restore latency on a non-GPU workload from a neutral party; both figures here come from
  competing systems' authors.

## References

- [SUB-1] google/ax, `README.md` (main), Google, accessed 2026-10-22. https://github.com/google/ax/blob/main/README.md
- [SUB-2] agent-substrate/substrate, `README.md` (main), accessed 2026-10-22. https://github.com/agent-substrate/substrate/blob/main/README.md
- [SUB-3] agent-substrate/substrate, `docs/architecture.md` (main), accessed 2026-10-22. https://github.com/agent-substrate/substrate/blob/main/docs/architecture.md
- [SUB-4] kubernetes-sigs/agent-sandbox, `README.md` (main), Kubernetes SIG Apps, accessed 2026-10-22. https://github.com/kubernetes-sigs/agent-sandbox/blob/main/README.md
- [SUB-5] google/gvisor, `g3doc/architecture_guide/platforms.md` (master), accessed 2026-10-22. https://github.com/google/gvisor/blob/master/g3doc/architecture_guide/platforms.md
- [SUB-6] google/gvisor, `g3doc/architecture_guide/performance.md` (master), accessed 2026-10-22. https://github.com/google/gvisor/blob/master/g3doc/architecture_guide/performance.md
- [SUB-7] E. G. Young, P. Zhu, T. Caraza-Harter, A. C. Arpaci-Dusseau, R. H. Arpaci-Dusseau, "The True Cost of Containing: A gVisor Case Study", HotCloud 2019. https://www.usenix.org/system/files/hotcloud19-paper-young.pdf
- [SUB-8] kata-containers/kata-containers, `docs/design/architecture/README.md` (main), accessed 2026-10-22. https://github.com/kata-containers/kata-containers/blob/main/docs/design/architecture/README.md
- [SUB-9] firecracker-microvm/firecracker, `SPECIFICATION.md` (main), accessed 2026-10-22. https://github.com/firecracker-microvm/firecracker/blob/main/SPECIFICATION.md
- [SUB-10] firecracker-microvm/firecracker, `docs/snapshotting/snapshot-support.md` (main), accessed 2026-10-22. https://github.com/firecracker-microvm/firecracker/blob/main/docs/snapshotting/snapshot-support.md
- [SUB-11] A. Agache, M. Brooker, A. Florescu, A. Iordache, A. Liguori, R. Neugebauer, P. Piwonka, D.-M. Popa, "Firecracker: Lightweight Virtualization for Serverless Applications", NSDI 2020. https://www.usenix.org/system/files/nsdi20-paper-agache.pdf
- [SUB-12] D. Ustiugov, P. Petrov, M. Kogias, E. Bugnion, B. Grot, "Benchmarking, Analysis, and Optimization of Serverless Function Snapshots", ASPLOS 2021. https://arxiv.org/pdf/2101.09355
- [SUB-13] CRIU, "Lazy migration", criu.org, accessed 2026-10-22. https://criu.org/Lazy_migration
- [SUB-14] Y. Yang, D. Du, H. Song, Y. Xia, "On-demand and Parallel Checkpoint/Restore for GPU Applications" (gCROP), SoCC 2024. https://ipads.se.sjtu.edu.cn/_media/publications/yang-socc24.pdf
- [SUB-15] J. Huang, T. Ma, Z. Liu, S. Lin et al., "TrEnv-X: Transparently Share Serverless Execution Environments Across Different Functions and Nodes", arXiv 2509.09525v2, 2026-03-21. https://arxiv.org/pdf/2509.09525
