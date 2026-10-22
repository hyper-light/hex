# Runtime matrix: isolation, start latency, throughput, density and overhead

The cross-system comparison behind Hex's runtime design. Every cell is cited to a numbered reference in
the topic note it came from (`[ASP-n]` agent-sandbox-platforms.md, `[SUB-n]` substrates-and-sandboxes.md,
`[OCI-n]` oci-runtimes-and-fast-start.md, `[CG-n]` cgroup-v2-resource-control.md) or to a record in this
repository. Kinds follow [the method](../README.md#method): `MI` measured-independent, `MV`
measured-vendor, `CL` claimed, `DE` design fact. `n/p` = not published.

## How to read it — compare like with like

"Start" means different work in different systems, and the matrix never compares across these:

| Class | What "start" covers |
|---|---|
| **Process / OCI runtime** | runtime invoked → workload's first instruction: namespaces, cgroup, rootfs, `execve` (no image pull) |
| **Sandbox kernel / VM boot** | a fresh guest (gVisor Sentry, Kata VM, Firecracker microVM) booted to the workload |
| **Snapshot restore** | a paused guest or process image resumed (Firecracker snapshots, CRIU, REAP, FaaSnap, Catalyzer) |
| **Agent-sandbox platform** | an API call → a usable sandbox, end to end over the network, as a customer sees it |

Most sources publish a mean or a p50, sometimes a p99, and almost never p99.9 or max; the matrix
quotes exactly what was reported and marks the rest `n/p`. Vendor and claimed numbers are leads until
the competitive baseline (`bench/`, CLAUDE.md §7) re-measures the self-hostable systems on our own
hardware.

## Hex today (our own measurements)

| Metric | Value | Kind | Source |
|---|---|---|---|
| oci create+start, image present | p50 14.4 ms, p90 15.3, p99 16.7, min 12.7, max 18.1 (n=200) | MV | `bench/hex_start/RESULT.md` (fedora, 32 cores, NVMe, build `8424db7`, 2026-10-22) |
| oci create+start p99.9 | not estimable: n=200 is too few samples (needs n ≥ 10,000) | — | same |
| noisy-neighbour isolation | victim fsync p99 1.73x idle under a saturating builder (bar ≤ 2x) | MV | `bench/noisy_neighbor/RESULT.md` |
| node agent bookkeeping per box | 98 B (agent RSS growth per box; *not* a per-sandbox overhead) | MV | `crates/hex-bench/benches/node_memory.rs`; result in `STATUS.md`, not yet recorded in `bench/` |
| oci conformance | 28/58 runtime-tools validation tests (crun's set on this host) | MV | `bench/oci-validation/SUMMARY-hex.txt` |

Throughput (starts per second), density and per-workload memory/CPU overhead for Hex have **not been
measured** — the competitive baseline (goal 3) measures them alongside every competitor.

## Process / OCI runtimes (namespace containers, on one node)

| Runtime | Start measured (operation, conditions) | Value | Kind | Source |
|---|---|---|---|---|
| **crun** (best published) | create + run + delete of `/bin/true`, hyperfine, seccomp-BPF cache (2022) | 5.6 ± 3.0 ms (user 1.0, system 4.5; range 4.2-26.8) | MV | [OCI-44] |
| crun / youki / runc | `create && start && delete -f`, page cache dropped, ≥100 runs; Ubuntu 22.04, Linux 6.5, 16 cores | crun 47.3 ± 2.8, youki 111.5 ± 11.6, runc 224.6 ± 12.0 ms (runc/crun 4.7x) | MV | [OCI-3] |
| crun / youki / runc / runsc | 1,000 serial creates through nerdctl + containerd (stack included; "not a scientific benchmark") | means 0.28 / 0.32 / 0.34 / 0.62 s; youki failed 36/1,000 | MI | [OCI-47] |
| runc | alone, GCP VMs, Linux 6.8 | 44 ms (1 vCPU), > 60 ms (multi-vCPU); up to 50% of it in `percpu_down_write` on the cgroup PID write | MI | [OCI-42] |
| runc vs crun memory | 100 sequential runs, GNU `time -v` | max RSS 15,120 KB vs 3,752 KB | MV | [OCI-45] |
| **Hex today** | `oci create` + `start`, `/usr/bin/true`, n=200; **no seccomp, no resource cgroup, delete untimed** | p50 14.4, p99 16.7, max 18.1 ms | MV | `bench/hex_start/RESULT.md` |

No source publishes a p99.9 for any OCI runtime; hyperfine reports mean ± σ and a min-max range. **The
container-class bar:** crun's 5.6 ms makes 4-5x ≈ **1.1-1.4 ms** for a full create-run-delete
(DERIVED [OCI-44]) — below every published namespace-container figure; only forks of pre-built
sandboxes have been measured that low. Hex's 14.4 ms is about 2.6x *slower* than crun's best, before
seccomp and cgroups are added. In crun's 5.6 ms, 4.5 ms is kernel time: the cost is kernel objects
(mount-namespace copy, network and IPC namespaces, cgroup create/join, seccomp load), not the runtime's
language [OCI-44][OCI-45].

*Verified:* the crun/youki/runc figures read in youki's README at the cited commit (lines 42-44) and
crun's 159.2 → 28.9 → 5.6 ms series read in its author's post, 2026-10-22 — 0 mismatches.

## Fast-start techniques (measured speedups; the levers for the bar)

| Technique | Measured effect | Kind | Source |
|---|---|---|---|
| Fork a pre-built seed (zygote / sfork) | Catalyzer fork boot 0.97 ms vs 40 ms cold, 142 ms gVisor baseline; SEUSS hot 0.82 ms vs cold 7.67 ms | MV | [OCI-28][OCI-29] |
| Seed + pooled objects in production | AFaaS 5.45-9.41 ms start; 5.1-5.5x vs Catalyzer at 24-way; pre-allocates 1,000 veths, 600 cgroups | MV | [OCI-35] |
| Lean containers + zygotes | SOCK: 18x Docker throughput, 19x latency; zygotes add 3x | MV | [OCI-27] |
| vfork-class spawn instead of fork | 31.9 µs vs 7,581.8 µs for a 1 GB parent (238x); fork costs ~6.5 ms per GB of parent | MV / MI | [OCI-34][OCI-33] |
| No per-start network namespace | 200 → 900 containers/s; pre-created endpoints −80% startup at 100 concurrent | MI / MV | [OCI-27][OCI-36] |
| cgroup pool + `CLONE_INTO_CGROUP` | pool −94% cgroup creation; skipping the cgroup join −27% runc launch | MV / MI | [OCI-31][OCI-42] |
| Mount table without copying (`OPEN_TREE_NAMESPACE`, kernel 7.0) | mount-ns setup 0.343 → 0.044 ms (7.8x), 21.5x with many host mounts; +40% creation throughput | MV | [OCI-23][OCI-18] |
| Idmapped mounts instead of recursive chown | container start 37x / 92x / 271x faster (BusyBox / Ubuntu / Fedora) | MV | [OCI-25] |
| Cached / precompiled seccomp | `crun run` 28.9 → 5.6 ms | MV | [OCI-44] |

None reaches 4-5x alone; they compose. Every seed/fork number is from a VM- or lambda-class system,
so a seed fork of Hex's actual sandbox (namespaces + mounts + cgroup + seccomp) must be measured on our
hardware before it counts.

## Sandbox kernels and microVMs (cold start, on one node)

| System | Isolation | Start | Throughput | Overhead | Kind |
|---|---|---|---|---|---|
| **Firecracker** | KVM microVM | ≤ 125 ms `InstanceStart` → guest `/sbin/init`; VMM up in ≤ 8 CPU-ms (wall 6-60 ms, typically ~12) | "up to 150 MicroVMs per second per host" | VMM ≤ 5 MiB (1 vCPU, 128 MiB guest) | MV, CI-enforced [SUB-9][SUB-11] |
| **gVisor** | user-space kernel (Systrap default since mid-2023) | 142 ms for a C hello (Catalyzer's baseline) | n/p | vs runc, 2019 (pre-Systrap): syscalls 2.2×, malloc 2.5×, large downloads 2.8×, file opens 216× slower | MI [SUB-7]; MV [OCI-28] |
| **Kata Containers** | VM per sandbox + in-guest agent | 200 concurrent: Kata-FC 47.6 s, kata-qemu 6.85 s, kata-template 2.98 s (RunD: 1 s) | n/p | n/p | MV [OCI-31] |

The on-node microVM bar is now set by CubeSandbox (47.8 ms create) and Zeroboot (0.79 ms CoW fork) —
see the self-hostable table above — not by Firecracker's defaults.

## Snapshot restore

| Mechanism | Restore / resume | Kind | Source |
|---|---|---|---|
| Firecracker snapshot, baseline | guest pages fault in from disk one at a time; docs publish no latency; restore slow under cgroups v1 | DE | [SUB-12][SUB-10] |
| REAP (record + prefetch the working set) | 3.7× lower cold-start delay than baseline snapshots; 70 → 185 ms from 1 to 8 concurrent; disk-bound beyond | MV | [SUB-12] |
| CRIU (copy-based restore) | 193-2,278 ms startup with the image in memory (GPU apps); 1.7 s at P99 under heavy load (a competitor's measurement) | MI | [SUB-14][SUB-15] |
| Process-level C/R in a microVM (DeltaBox) | restore 1.86 ms | MV | [ASP-78] |
| CubeSandbox resume | avg 41.8 ms (p95 65.1, n=5) | MV | [ASP-75] |
| CoW fork of a snapshot (Zeroboot) | p50 0.79 ms, p99 1.74 ms | MV | [ASP-77] |

The order is the lesson: copying memory is slowest, faulting it from disk next, prefetching it better,
and sharing it copy-on-write fastest.

## Agent substrates

| Substrate | Isolation | Resume / activation | Throughput | Density / scale | Kind |
|---|---|---|---|---|---|
| **Google Agent Substrate** | gVisor or Kata + Cloud Hypervisor; never plain containers | resume "less than 400ms" (CL); north-star p95 100 ms (target) | "more than 1500 activations per second" (CL); north-star 1,000 wakeups/s per cluster (target) | "10x–30x oversubscription" (CL); demo ~250 actors on 8 pods (MV); north-star 1 billion agents per cluster (target) | CL / DE [SUB-2][SUB-3] |
| **Google AX** | delegates to Agent Substrate | suspend / resume checkpoints actor state | target "billions of autonomous agent workloads in a cluster" (CL) | n/p | DE / CL [SUB-1] |
| **Kubernetes Agent Sandbox** | gVisor or Kata via `RuntimeClass` | warm pool (`SandboxWarmPool`) | GKE: 300 sandboxes/s per cluster, 90% ≤ 200 ms (CL) | n/p | DE [SUB-4]; CL [ASP-80] |

Substrate's own architecture document says "Much of this architecture is aspirational, and is not yet
implemented!" [SUB-3]: its figures are targets and claims, not measurements.

## Agent-sandbox platforms (end to end, as a customer sees them)

**Independent create-to-first-command** — ComputeSDK, 100 sandboxes launched at once, 2026-10-20, over
the public internet (includes network round trip, provider queueing and the first command, so an upper
bound on the platform's own start, not comparable with an on-node start). Untrimmed nearest-rank values
re-derived from the published per-iteration data [ASP-3]; ComputeSDK's own displayed percentiles trim
the best and worst 5% and understate the tail [ASP-2].

| Platform | Isolation | p50 ms | p99 ms | max ms | Vendor-stated start (kind) |
|---|---|---|---|---|---|
| Isorun | KVM microVM [ASP-65] | 59 | 74 | 74 | "ready in 10ms" (CL) [ASP-65] |
| Vercel Sandbox | Firecracker [ASP-53] | 416 | 708 | 710 | "milliseconds" (CL) [ASP-52] |
| Blaxel | microVM [ASP-62] | 481 | 525 | 539 | resume 25 ms (CL) [ASP-62] |
| Cloudflare Containers | Firecracker [ASP-57] | 657 | 1,612 | 2,010 | cold 1-3 s (CL) [ASP-57] |
| Daytona | shared-kernel container (VM class closed) [ASP-12] | 670 | 1,023 | 1,025 | "under 90 ms" (CL) [ASP-8] |
| Modal | gVisor [ASP-30] | 1,070 | 3,561 | 6,227 | median "less than half a second" (MV, beta) [ASP-32] |
| E2B | Firecracker [ASP-21] | 1,329 | 1,620 | 3,520 | "~150 ms" (CL) [ASP-26] |
| CodeSandbox | Firecracker [ASP-42] | 38,356 | 66,856 | 66,856 | 1-3 s (CL) [ASP-42] |

One-at-a-time runs (2026-08-07, the retired sequential test) show the tail is the weak flank: Daytona
p50 984 / p99 26,421 / max 38,552 ms; Modal's burst p99 swung 800-16,712 ms across five runs, Mar-Oct
2026 (MI) [ASP-4][ASP-3]. **No platform publishes a p99.9.**

**The self-hostable bar** (what we can re-measure on our own hardware) is set by on-node microVM
runtimes, not by the hosted platforms:

| System | Create | From snapshot / resume | Throughput | Density | Kind |
|---|---|---|---|---|---|
| CubeSandbox (Tencent, RustVMM+KVM, Apache-2.0) | avg 47.8 ms (min 43.5, p95 57.4, max 60.4; n=20, 1 at a time) | snapshot avg 63.9 ms (p95 66.1, n=3); resume avg 41.8 ms (p95 65.1, n=5) | 180.9 creates/s per node at 20 concurrent | ~21.5-25.7 MB per idle 2 GiB VM (README claims "<5MB" — contradicted) | MV [ASP-75][ASP-74] |
| Zeroboot (prototype, Apache-2.0) | spawn p50 0.79 ms, p99 1.74 ms (CoW fork of a snapshot; 1 vCPU, no network) | — | 1,000 concurrent forks in 815 ms | ~265 KB per sandbox | MV [ASP-77] |
| E2B self-hosted (Firecracker, UFFD) | n/p | incremental pause / resume avg 524 / 900 ms per event | n/p | n/p | MI [ASP-78] |

**Scale (cluster / control plane):** Modal created 1,000,000 sandboxes in under a minute on its new
scheduler (MV) [ASP-32]; Daytona reports 1,000 sandboxes ready in 10.8 s on one NVIDIA Vera CPU (MV)
[ASP-16]; GKE Agent Sandbox claims 300 sandboxes/s per cluster with 90% of allocations ≤ 200 ms (CL)
[ASP-80]; Agent Substrate targets p95 100 ms activation and 1,000 wakeups/s, "aspirational" (DE target)
[ASP-82].

*Verified:* 15 load-bearing numbers re-checked against their sources on 2026-10-22 — the six burst rows
above recomputed from ComputeSDK's raw iteration arrays, and the CubeSandbox and Zeroboot figures read
in their own reports — 0 mismatches.

## Resource control (machine protection)

_From `cgroup-v2-resource-control.md` — filled when it lands._
