# Agent-sandbox platforms: start paths, latency, scale, and the bar Hex has to clear

Source note (CLAUDE.md §5), not a design decision. Researched 2026-10-22; every source below was opened and
read that day (References, `[ASP-n]`). Serves CP-T7.2 (microVM class, snapshots, warm pools), CP-T7.6 (agents
as workloads) and CP-T7.3 / CP-T7.4 / CP-T7.11 (control plane, scheduler, scale) in
`docs/requirements.toml` [ASP-84].

**Scope.** The eight platforms named in the brief (Daytona, E2B, Modal Sandboxes, Morph Cloud, CodeSandbox SDK,
Fly.io Machines and Sprites, Vercel Sandbox, Cloudflare Containers / Sandbox SDK / Dynamic Workers), plus the
platforms that publish real technical detail or measurable numbers: Northflank, Blaxel, Tensorlake, Runloop,
Beam, Freestyle, Isorun, Tencent CubeSandbox, Zeroboot, Google GKE Agent Sandbox / Agent Substrate and the
Kubernetes agent-sandbox project (both are covered in depth by `substrates-and-sandboxes.md`; only what I
verified is repeated here), and two research systems used as evidence (DeltaBox, AgentZip).

## 0. Conventions

- **Kinds** (README method): `MI` MEASURED-INDEPENDENT, `MV` MEASURED-VENDOR, `CL` CLAIMED, `DE` DESIGN (a
  fact from code or docs). `n/p` = not published / not found.
- **Percentiles are written as the source wrote them.** `avg` is a mean. "p50" appears only where the source
  says median or p50. No source below publishes a p99.9; the only extreme-tail statistic available anywhere is
  the max of a run, usually of n = 100 or fewer.
- **What "independent" means here.** `ComputeSDK` [ASP-1][ASP-2] is a third party to the providers' engineering but not
  disinterested: it sells a unified compute SDK and paid "stealth benchmarks", lists sponsors and partner tiers, and trims the
  best and worst 5% of each run before computing the percentiles it displays; I found no statement that the providers it ranks
  pay for placement. `DeltaBox` [ASP-78] is a competing research prototype whose authors measured E2B's open-source runtime
  on their own hardware. Both are labelled `MI` with those caveats.
- **Code citations** are GitHub permalinks to the file and lines read, on the commit named in the reference.
- **Numbers I computed** (nearest-rank percentiles over the raw per-iteration arrays ComputeSDK publishes,
  and the "bar" arithmetic in section 5) are labelled `DERIVED`; they are not a kind a source reported.

---

## 1. Summary tables

### 1a. Start, resume, snapshot and fork latency

"Independent create" is ComputeSDK's time from `sandbox.create()` to the first successful `runCommand("node -v")`
over the public internet from Namespace-hosted runners, 100 iterations: *burst* = all 100 launched at once
(2026-10-20), *seq* = one at a time (2026-08-07, the last run of the retired sequential test). Values are
untrimmed nearest-rank p50 / p99 / max computed by me from the published iteration arrays (`DERIVED` from
[ASP-3][ASP-4]); the site's own displayed values are in section 2. They include client-to-API network round
trip, the provider's queueing, and the first command, so they are an upper bound on the platform's own start
cost, and they are not comparable with an on-node start.

| System | Vendor-stated create / start | Independent create, ms: p50 / p99 / max (burst; seq) | Resume / restore / fork | Source kinds |
|---|---|---|---|---|
| **Daytona** (containers; VM class closed) | "under 90 ms from code to execution" `CL` [ASP-8][ASP-11]; the only path in the code with no per-sandbox start work is a warm-pool claim (a DB-row reassignment) `DE` [ASP-9][ASP-10]; Daytona does not say that is what the figure measures | burst 670 / 1,023 / 1,025; seq 984 / 26,421 / 38,552 `MI` [ASP-3][ASP-4] | container: stop/start keeps filesystem only `DE` [ASP-11]; VM class: pause/resume "under a second" `CL` [ASP-15]; fork and memory snapshots exist, no latency published `DE` [ASP-11] | `CL`, `DE`, `MI` |
| **E2B** (Firecracker) | "~150 ms from a snapshot or custom template"; snapshot loads "in ~80ms" `CL` [ASP-26]; repo README/architecture publish no figure `DE` [ASP-20][ASP-21] | burst 1,329 / 1,620 / 3,520; seq 573 / 1,048 / 1,271 `MI` [ASP-3][ASP-4] | resume "approximately 1 second"; pause "approximately 4 seconds per 1 GiB of RAM" `CL` [ASP-23]; self-hosted incremental pause/resume avg 524 ms / 900 ms per event (DeltaBox's guests are 4 vCPU / 8 GB; the paper does not state E2B's guest size) `MI` [ASP-78]; fork up to 100 per API request `DE` [ASP-22] | `CL`, `MI`, `DE` |
| **Modal Sandboxes** (gVisor; VM runtime) | new scheduler: start "less than half a second at the median" (beta, 1M-sandbox test) `MV` [ASP-32]; "containers boot in about one second" (Functions) `CL` [ASP-34] | burst 1,070 / 3,561 / 6,227; seq 549 / 770 / 815 `MI` [ASP-3][ASP-4] | memory snapshots (alpha): snapshotting terminates the sandbox, TTL 7 days, restore only on the same instance type; no latency published `DE` [ASP-31] | `MV`, `CL`, `MI` |
| **Morph Cloud** (Infinibranch VMs) | "<250 ms startup", snapshot / branch / restore "under 250ms" `CL` [ASP-36][ASP-37]; no method, no percentile | n/p (not in ComputeSDK) | same claim; pause/resume preserves memory `DE` [ASP-38] | `CL` |
| **CodeSandbox SDK** (Firecracker) | create from template "ready in 1-3 seconds" `CL` [ASP-42]; "within 2 seconds" `CL` [ASP-43] | burst 38,356 / 66,856 / 66,856 (99% success); seq 2,327 / 5,425 / 22,531 `MI` [ASP-3][ASP-4] | resume from memory+disk snapshot "0.5-2 seconds"; disk-only 5-20 s; archive 20-60 s `CL` [ASP-42]; production resume avg 400 ms, P99 2 s, P01 150 ms (2024) `MV` [ASP-41]; fork avg 921 ms, P99 1,833 ms (2023) `MV` [ASP-40] | `CL`, `MV`, `MI` |
| **Fly.io Machines / Sprites** (Firecracker) | Machine "started" state "well under a second" (the "created" state: "low double digit seconds") `CL` [ASP-49]; Sprite create "a second or two" `CL` [ASP-45] | n/p | Sprite warm wake 100-500 ms, cold wake 1-2 s `CL` [ASP-47]; Machine suspend resume "a few hundred ms" vs cold "~2+ seconds" `CL` [ASP-48] | `CL` |
| **Vercel Sandbox** (Firecracker, "Hive") | "milliseconds" `CL` [ASP-52][ASP-53] | burst 416 / 708 / 710; seq 420 / 1,563 / 1,764 `MI` [ASP-3][ASP-4] | persistent sandboxes snapshot the filesystem on stop; resuming from a snapshot is "even faster than starting a fresh sandbox", no number `DE` [ASP-53] | `CL`, `MI` |
| **Cloudflare** Containers / Sandbox SDK | cold starts "can often be in the 1-3 second range" `CL` [ASP-57]; Dynamic Workers isolate "a few milliseconds" `CL` [ASP-59] | `cloudflare` provider: burst 657 / 1,612 / 2,010; seq 1,995 / 4,817 / 6,508 `MI` [ASP-3][ASP-4] | Containers snapshot (filesystem) restore; no latency `DE` [ASP-57] | `CL`, `MI` |
| **Northflank** (Cloud Hypervisor microVM) | "Ready in 0.8 seconds": schedule 0.12 s + microVM and guest kernel boot 0.41 s + container start 0.27 s (a demo run, cached image) `CL` [ASP-60] | burst: 0 successes (2026-10-20) `MI` [ASP-3]; seq 83 / 123 / 159 `MI` [ASP-4] | n/p | `CL`, `MI` |
| **Blaxel** (microVM) | "resume from standby in 25 milliseconds"; create "under one second" `CL` [ASP-62] | burst 481 / 525 / 539; seq 251 / 456 / 456 (99% success) `MI` [ASP-3][ASP-4] | 25 ms resume `CL` [ASP-62]; independent resume measurement: none found | `CL`, `MI` |
| **Tensorlake** (Cloud Hypervisor) | create "single-digit milliseconds", OS boot "a few hundred milliseconds" `CL` [ASP-68] | burst 2,763 / 2,895 / 2,955; seq 705 / 954 / 961 `MI` [ASP-3][ASP-4] | suspend/resume "sub-second" `CL` [ASP-68] | `CL`, `MI` |
| **Runloop** | n/p in the pages read; "2x faster vCPUs running on our custom bare-metal hypervisor" `CL` [ASP-69] | seq 465 / 672 / 895 `MI` [ASP-4] | n/p | `MI` |
| **Beam** (containers; OSS beta9) | "Sub-second boot times" heading over "cold boot in one to three seconds" `CL` [ASP-71] | burst 920 / 987 / 987; seq 105 / 194 / 1,029 `MI` [ASP-3][ASP-4] | memory snapshot / fork exist, no latency `DE` [ASP-70] | `CL`, `MI` |
| **Freestyle** (VMs) | "about 65ms from API request to ready machine"; wake "under 100ms" `CL` [ASP-64] | burst 2,511 / 4,773 / 4,793 `MI` [ASP-3] | live fork "in milliseconds" `CL` [ASP-64] | `CL`, `MI` |
| **Isorun** (KVM microVM, hosted beta) | "ready in 10ms" `CL` [ASP-65] | burst 59 / 74 / 74; seq 17 / 26 / 189 `MI` [ASP-3][ASP-4] | fork / hibernate / resume exist, no latency `DE` [ASP-66] | `CL`, `MI` |
| **CubeSandbox** (Tencent, RustVMM + KVM, OSS) | "under 60ms" `CL` [ASP-74]; bare-metal avg 47.8 ms (min 43.5, p95 57.4, max 60.4; n = 20, 1 at a time; 2 vCPU / 2 GiB) `MV` [ASP-75] | n/p | create from snapshot avg 63.9 ms (p95 66.1); resume avg 41.8 ms (p95 65.1); clone avg 219.6 ms; rollback avg 81.6 ms; snapshot avg 49.8 ms with ~7 MB dirty, 486.9 ms with 1,136 MB dirty; pause avg 558.4 ms at 2 GiB (full copy) `MV` [ASP-75] | `MV`, `CL` |
| **Zeroboot** (OSS prototype) | spawn p50 0.79 ms, p99 1.74 ms; fork + exec Python ~8 ms; 1 vCPU, no network `MV` [ASP-77] | n/p | 1,000 concurrent forks in 815 ms `MV` [ASP-77] | `MV` |
| **GKE Agent Sandbox / Agent Substrate** (Google, OSS) | warm pool: 300 sandboxes/s per cluster, "90% of allocations completing in 200 milliseconds" `CL` [ASP-80]; Substrate resume "less than 400ms", "more than 1500 activations per second" `CL` [ASP-81]; north-star activation p95 100 ms `DE` target [ASP-82] | n/p | pod snapshots resume "in seconds" `CL` [ASP-80] | `CL`, `DE` |
| **DeltaBox** (research, process-level C/R in Firecracker) | n/a | n/a | checkpoint avg 10.83 ms, restore avg 1.86 ms (weighted); kernel `fork()` p50 0.57 ms (N=1) to 5.47 ms (N=64) `MV` (authors' own system) [ASP-78] | `MV` |

### 1b. Isolation, scale limits, density, self-hosting

| System | Isolation and trust boundary | Published scale limits and throughput | Density / overhead | Self-host on Linux x86_64? |
|---|---|---|---|---|
| **Daytona** | Default: Linux container sharing the host kernel `DE` [ASP-12]; Sysbox user-namespace runtime, runner sets `Privileged` for non-GPU sandboxes `DE` [ASP-19][ASP-9]; Linux-VM class has its own kernel `DE` [ASP-12]. Copy Fail (CVE-2026-31431): corruption primitive was reachable from inside a sandbox and could affect co-tenants on a runner; no escape observed `DE` [ASP-18] | Org tiers: sandbox creation 300-600 per minute, Tier 4 = 500 vCPU / 1,000 GiB / 5,000 GiB `DE` [ASP-13]; 1,000 sandboxes ready in 10.8 s and 2,000 in 27.4 s on one NVIDIA Vera CPU (EPYC 9575F: 32.3 s, EPYC 9755: 88.4 s for 1,000) `MV` [ASP-16]; "millions of sandboxes" per day `CL` [ASP-16] | n/p | Source frozen at v0.190.0 (AGPL-3.0, Docker-based runner), unmaintained since June 2026; later development closed `DE` [ASP-8][ASP-17] |
| **E2B** | One Firecracker microVM per sandbox, own cgroup, netns and nftables egress `DE` [ASP-21]; custom minimal guest kernel without `CONFIG_CRYPTO_USER_API_AEAD` `CL` [ASP-28] | Concurrent: Hobby 20, Pro 100-1,100, Enterprise 1,100+; creation rate 1/s Hobby, 5/s Pro `DE` [ASP-24]; "2B+ sandboxes started" `CL` [ASP-29] | n/p; placement default CPU overcommit R = 4; guest memory from a 2 MiB hugepage pool; UFFD restore copies pages into each instance's private memory `DE` [ASP-22][ASP-78] | Yes: Apache-2.0; `embed/` runs the whole stack on one KVM host (Docker Compose); Terraform for GCP/AWS/Azure; Kubernetes manifest; ships `BenchmarkConcurrentResume` `DE` [ASP-20][ASP-22] |
| **Modal** | gVisor by default, `runtime="vm"` optional, GPU only on gVisor `DE` [ASP-30]; gVisor syscalls at least 2.2x slower than runc in the 2019 case study `MI` [ASP-35] | 50,000 concurrent sandboxes per customer today `CL` [ASP-32]; new scheduler (beta): 1,000,000 created in under a minute, "tens of thousands of sandboxes per second" `MV`/`CL` [ASP-32] | n/p | No (hosted only) |
| **Morph Cloud** | VM; hypervisor not named in the docs read [ASP-36][ASP-37] | n/p | n/p | No self-host docs found |
| **CodeSandbox SDK** | Firecracker microVM `DE` [ASP-42] | ~2.5 million resumes per month, ~250,000 unique VMs per month (2024) `MV` [ASP-41]; "millions of concurrent VMs" `CL` [ASP-43]; limits by plan (numbers n/p) `DE` [ASP-44] | 8 GiB memory snapshot -> 745 MiB (lz4, 8 KiB chunks) or 412 MiB (zstd) `MV` [ASP-41] | No: "we do not offer self hosting" `DE` [ASP-44] |
| **Fly.io** | Firecracker microVMs on dedicated servers `DE` [ASP-85]; Sprites add an inner container `DE` [ASP-45] | Sprite: up to 8 vCPU and 100 GB `DE` [ASP-47]; platform limits n/p | n/p | No; the Sprites post says "Jerome's working on an open-source local Sprite runtime" `CL` [ASP-45] |
| **Vercel** | Firecracker microVM, dedicated kernel `DE` [ASP-53] | 10,000 concurrent (Pro/Enterprise); vCPU allocation up to 5,000 per minute (150/min start, +500/min each minute of sustained use); 24 h sessions; deletes 20/s `DE` [ASP-54] | Active-CPU billing; 64 GB ephemeral NVMe per sandbox `DE` [ASP-54] | No (SDK and CLI open source) [ASP-55] |
| **Cloudflare** | Containers: Firecracker microVM per instance `DE` [ASP-57]; Dynamic Workers: V8 isolates, "a more complicated attack surface than hardware virtual machines" `DE` [ASP-59] | Containers per account default: 1,500 vCPU, 6 TiB memory, 30 TB disk (raisable) `DE` [ASP-58]; Dynamic Workers: "no such limits" `CL` [ASP-59] | `lite` instance = 1/16 vCPU, 256 MiB `DE` [ASP-58]; isolate "a few megabytes" `CL` [ASP-59] | Hosted service; no self-host distribution for Containers found [ASP-56] |
| **Northflank** | Cloud Hypervisor microVMs; gVisor for GPU workloads `DE` [ASP-60] | "over 2 million isolated workloads monthly" `CL` [ASP-61] | n/p | BYOC clusters in the customer's own cloud account; no self-hosted distribution found [ASP-60] |
| **Blaxel** | microVM, root filesystem in memory `CL` [ASP-62]; acquired by Baseten 2026-09-10 [ASP-63] | "100,000+ concurrent sandboxes" by tier quota `CL` [ASP-62] | n/p | Hosted; the sandbox API is open source, no self-host docs found [ASP-62] |
| **Tensorlake** | Cloud Hypervisor microVM `DE` [ASP-68] | scheduler creates "hundreds" of sandboxes per second at peak `CL` [ASP-67] | n/p | n/p |
| **Beam** | container sandboxes; OSS runtime beta9 `DE` [ASP-70][ASP-72] | n/p | n/p | Open-source runtime (beta9); self-hosting path not verified [ASP-72] |
| **CubeSandbox** | KVM microVM per sandbox, eBPF network isolation `DE` [ASP-76] | 180.9 creates/s per node at 20 concurrent; 147.6/s at 50 concurrent (avg 276 ms, p95 508 ms, max 681 ms) `MV` [ASP-75] | ~21.5 MB (100 live) to ~25.7 MB (1,000 live) amortized per idle 2 vCPU / 2 GiB sandbox; README says "<5MB" `MV`/`CL` [ASP-75][ASP-74] | Yes: Apache-2.0, x86_64 Linux + KVM, E2B-SDK compatible [ASP-74] |
| **Zeroboot** | KVM microVM per fork; shared CSPRNG state is a known limitation `DE` [ASP-77] | 1,000 concurrent forks 815 ms `MV` [ASP-77] | ~265 KB per sandbox `MV` [ASP-77] | Yes: Apache-2.0, "any Linux box with KVM" [ASP-77] |
| **GKE Agent Sandbox / Substrate** | gVisor or Cloud Hypervisor / Kata; plain containers unsupported in Substrate `DE` [ASP-81][ASP-80] | 300 sandboxes/s per cluster `CL` [ASP-80]; Substrate targets 1 billion agents per cluster and 1,000 wakeups/s `DE` target, "aspirational" [ASP-82] | "10x higher density than standard containers" `CL` [ASP-81] | Yes: Apache-2.0 [ASP-81] |

---

## 2. Independent measurements that apply to several systems

### 2.1 ComputeSDK: create to first command, 100 iterations (MI)

Method [ASP-2]: `performance.now()` around `compute.sandbox.create()` plus `runCommand('node -v')`; Node 24 on
GitHub Actions "Namespace" runners (location not stated); providers tested one after another in random order;
no retries (failures count against success); the **bottom and top 5% of successful times are trimmed before the
displayed median, p95 and p99 are computed**, so those displayed tails under-state the raw tails. The burst test
launches 100 sandboxes at once. Adapters and warm-pool use per provider are not documented, and the methodology
lists "performance with pre-warmed pools or snapshots" under "What this benchmark does NOT show (yet)" [ASP-2].

Burst, 100 concurrent, run 2026-10-20 [ASP-1][ASP-3]. Displayed (trimmed) median / p95 / p99 from the site; raw
untrimmed nearest-rank p50 / p95 / p99 / max and wall-clock to the last sandbox being ready are `DERIVED`.

| Provider | Displayed median / p95 / p99 (ms) | Raw p50 / p95 / p99 / max (ms) | Wall for 100 (ms) | Success |
|---|---|---|---|---|
| Isorun | 59 / 71 / 71 | 59 / 71 / 74 / 74 | 75 | 100% |
| Vercel | 417 / 539 / 589 | 416 / 589 / 708 / 710 | 711 | 100% |
| Blaxel | 481 / 511 / 515 | 481 / 515 / 525 / 539 | 541 | 100% |
| Cloudflare | 658 / 922 / 946 | 657 / 946 / 1,612 / 2,010 | 2,011 | 100% |
| Daytona | 672 / 943 / 955 | 670 / 955 / 1,023 / 1,025 | 1,026 | 100% |
| Beam | 920 / 982 / 983 | 920 / 983 / 987 / 987 | 988 | 100% |
| Modal | 1,070 / 1,207 / 1,562 | 1,070 / 1,562 / 3,561 / 6,227 | 6,239 | 100% |
| E2B | 1,334 / 1,539 / 1,571 | 1,329 / 1,571 / 1,620 / 3,520 | 3,522 | 100% |
| Microsandbox | 2,364 / 3,179 / 3,448 | 2,360 / 3,448 / 5,093 / 5,127 | 5,127 | 100% |
| Freestyle | 2,756 / 4,484 / 4,697 | 2,511 / 4,697 / 4,773 / 4,793 | 4,794 | 100% |
| Tensorlake | 2,765 / 2,858 / 2,883 | 2,763 / 2,883 / 2,895 / 2,955 | 2,956 | 100% |
| CodeSandbox | 38,356 / 62,385 / 66,783 | 38,356 / 66,783 / 66,856 / 66,856 | 66,857 | 99% |

Sequential, one at a time, run 2026-08-07 (the test is retired but its last results remain) [ASP-4]; raw p50 / p99 / max
(ms, `DERIVED`): Declaw 13 / 23 / 110; Isorun 17 / 26 / 189; CreateOS 75 / 83 / 162; Northflank 83 / 123 / 159;
Beam 105 / 194 / 1,029; Blaxel 251 / 456 / 456 (99%); Vercel 420 / 1,563 / 1,764; Runloop 465 / 672 / 895;
Modal 549 / 770 / 815; E2B 573 / 1,048 / 1,271; Tensorlake 705 / 954 / 961; **Daytona 984 / 26,421 / 38,552**;
Cloudflare 1,995 / 4,817 / 6,508; CodeSandbox 2,327 / 5,425 / 22,531.

Staggered, 100 launches 200 ms apart, same date (2026-08-07) [ASP-5]; raw p50 / p99 / max (ms, `DERIVED`): Isorun 25 / 54 / 57; Beam 92 / 121 / 132; Northflank 96 / 204 / 244;
Blaxel 255 / 712 / 712; Vercel 441 / 1,301 / 1,543; E2B 507 / 1,115 / 1,314; Modal 545 / 779 / 2,350; **Daytona 896 / 27,282 / 27,526**; Cloudflare 3,755 / 5,376 / 5,605; CodeSandbox
2,473 / 6,869 / 7,493. Daytona's multi-second tail therefore appeared in both tests of that day, not once.

**Day-to-day variation, same burst test** (raw p50 / p99 / max in ms, `DERIVED` from [ASP-6][ASP-3]):

| Provider | 2026-03-20 | 2026-04-20 | 2026-05-20 | 2026-06-16 | 2026-10-20 |
|---|---|---|---|---|---|
| Daytona | 336 / 569 / 569 | 259 / 888 / 891 | 433 / 2,205 / 2,271 | 905 / 1,365 / 1,413 | 670 / 1,023 / 1,025 |
| E2B | 719 / 1,352 / 1,352 | 1,223 / 1,943 / 2,046 | 666 / 1,114 / 1,135 | 598 / 1,352 / 4,375 | 1,329 / 1,620 / 3,520 |
| Modal | 2,564 / 16,712 / 16,712 | 1,846 / 3,058 / 3,252 | 1,892 / 2,733 / 2,792 | 661 / 800 / 809 | 1,070 / 3,561 / 6,227 |
| Vercel | 1,904 / 2,402 / 2,517 | 480 / 1,214 / 1,214 | 819 / 1,510 / 1,530 | 829 / 1,087 / 1,216 | 416 / 708 / 710 |
| Cloudflare | 3,341 / 12,303 / 12,907 | 2,111 / 7,574 / 19,586 | 2,186 / 3,379 / 5,283 | 3,189 / 9,335 / 15,055 | 657 / 1,612 / 2,010 |

What this supports: (1) a single published median is not a stable property of any of these platforms (Modal's
p50 ranges 661-2,564 ms and its p99 800-16,712 ms across five runs); (2) in individual runs the max is 2.2-3.2x the
p99 (E2B 4,375 ms vs 1,352 ms on 2026-06-16, 3.2x; Cloudflare 19,586 ms vs 7,574 ms on 2026-04-20, 2.6x; E2B 3,520 ms vs
1,620 ms on 2026-10-20, 2.2x), so the extreme tail is where the incumbents are weakest, and n = 100 cannot even estimate
p99.9; (3) the vendor-stated figures (Daytona <90 ms, E2B ~150 ms, Freestyle 65 ms, Tensorlake "single-digit milliseconds") are not
what a customer measures end to end through the public API at 100-way concurrency, and no vendor publishes the clock
start and stop for its figure. Caveat: ComputeSDK's runner location, adapter choices and warm-pool state are unknown, and
one run per day is a sample of one.

### 2.2 ComputeSDK "Dax": a real workload inside the sandbox (MI)

One iteration per scheduled run, cold clone + install + typecheck of the opencode repository, median total over
runs, 2026-10-20 [ASP-7]: Isorun 34.81 s, Beam 45.86 s, Blaxel 47.13 s, Vercel 75.03 s, Daytona 75.28 s,
Modal 75.35 s, Tensorlake 82.48 s, Cloudflare 125.40 s, Declaw 185.88 s; E2B, CodeSandbox, Freestyle and
Northflank did not complete all seven phases in that run. The spread is 3.6x between Isorun and Cloudflare and 5.3x
between Isorun and Declaw on identical work, so once a sandbox is running, vCPU speed, disk and network matter as much
as the start path (Cloudflare's `install` phase took 38.45 s against Isorun's 8.47 s).

### 2.3 DeltaBox: academic measurement of self-hosted E2B (MI for E2B)

DeltaBox [ASP-78] (Dong et al., arXiv:2605.22781, 2026-05-21, revised 2026-06-08) ran the open-source E2B runtime's
pause/resume as a checkpoint/rollback baseline: a four-socket server with 96 physical cores and 760 GiB RAM,
400 GB NVMe, SWE-bench MCTS trajectories; DeltaBox's own guest is 4 vCPU / 8 GB and the paper does not state the guest
size used for the E2B baseline. **E2B (diff): per-event mean blocking time
checkpoint 524.4 ms, restore 899.7 ms (weighted over workloads; per-workload checkpoint 487.7-552.5 ms, restore
790.2-952.1 ms)** against DeltaBox 10.83 ms and 1.86 ms; the paper states that E2B restore is about 480x DeltaBox's. In a
64-child fan-out from one frozen source, "E2B createSnapshot + create" kept GPU occupation at 29-36% versus 77-80%
for CubeSandbox and 95-97% for DeltaBox (figure 7(c), values given only as ranges). The paper also notes that E2B's
UFFD restore (`UFFDIO_COPY`) "copies pages into each instance's private memory" whereas CubeCoW and DeltaBox share
pages copy-on-write. Raw per-event distributions (p99, max) are not published for E2B in the paper.

### 2.4 gVisor overhead (MI, dated)

Young et al., "The True Cost of Containing: A gVisor Case Study", HotCloud 2019 [ASP-35] (University of
Wisconsin-Madison): simple system calls "at least 2.2x slower" than traditional containers, open/close on an
external tmpfs 216x slower, reading small files 11x slower, large downloads 2.8x slower, memory allocation 2.5x
slower. This measured gVisor's ptrace and KVM platforms in 2019; gVisor has since added other platforms
(Systrap) and I found no newer independent figure, so treat these as a bound on the design, not today's cost.

---

## 3. Per-system notes

### 3.1 Daytona

**What it is and the trust boundary.** Open-source until June 2026, now closed: the public repository's `main`
holds only a README, with the notice "As of June 2026, Daytona's core development has moved to a private
codebase" [ASP-8]; the vendor says the reason is security [ASP-17]. The last code is tag `v0.190.0` [ASP-9]. The
README says sandboxes have "a dedicated kernel", but the docs say the default **container** class "shares the
kernel of the host" and only the **Linux VM** class has its own kernel [ASP-12]. The container runner sets
`Privileged: gpuIndex == nil` on every non-GPU sandbox ([`container_configs.go` L200-206](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/runner/pkg/docker/container_configs.go#L200-L206))
and relies on Sysbox user namespaces ([ASP-19]; `CONTAINER_RUNTIME` is an env var, [`config.go` L33](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/runner/cmd/runner/config/config.go#L33)).
During Copy Fail (CVE-2026-31431, 2026-04-29) Daytona's own advisory says the corruption primitive "was
reachable from inside a Daytona sandbox, and on runners hosting more than one tenant it could affect cached
content observable to co-tenant sandboxes", with no escape observed and all runners remediated within 12 hours
[ASP-18]. E2B's competing post says its Firecracker sandboxes were not affected and that its guest kernels never
enabled `CONFIG_CRYPTO_USER_API_AEAD` [ASP-28] (vendor statement). The VM runner is not in the open repository: the API
enumerates the classes `container`, `linux-vm`, `android` and `windows` and the job types `PAUSE_SANDBOX`, `SNAPSHOT_SANDBOX` and
`FORK_SANDBOX` ([`sandbox-class.enum.ts`](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/sandbox/enums/sandbox-class.enum.ts),
[`job-type.enum.ts`](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/sandbox/enums/job-type.enum.ts)), but the open Docker runner's
`pauseSandbox` returns "pause is not supported for sandbox type"
([`executor/sandbox.go` L105-107](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/runner/pkg/runner/v2/executor/sandbox.go#L105-L107)) and its job switch has no `FORK_SANDBOX` case
([`executor.go` L131-162](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/runner/pkg/runner/v2/executor/executor.go#L131-L162)) `DE`.

**Start path, request to running (read in v0.190.0 code).**
1. `createFromSnapshot` resolves the snapshot, checks quotas, and **if the sandbox is non-GPU, not linked and has
   no volumes, tries the warm pool first** ([`sandbox.service.ts` L572-597](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/sandbox/services/sandbox.service.ts#L572-L597)).
2. **Warm hit.** `fetchWarmPoolSandbox` looks up a pool row matching snapshot, region, cpu, mem, disk, gpu, os user
   and env, then selects started, unassigned sandboxes `ORDER BY RANDOM() LIMIT 300` (`WARM_POOL_CANDIDATE_LIMIT`,
   [`configuration.ts` L325-327](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/config/configuration.ts#L325-L327))
   and takes a 10-second Redis lock on one ([`sandbox-warm-pool.service.ts` L70-165](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/sandbox/services/sandbox-warm-pool.service.ts#L70-L165)).
   `assignWarmPoolSandbox` then rewrites organization, labels, name and intervals on that row, optionally calls the
   runner to apply network rules, and emits a state event ([`sandbox.service.ts` L762-848](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/sandbox/services/sandbox.service.ts#L762-L848)).
   **No container is created on a hit.** The docs add the conditions: same snapshot, same region, the snapshot's
   default resources, the default OS user, no custom env, volumes or secrets; pools "must be enabled for your
   organization" by contacting support; pool size counts against quota ([ASP-10]). A cron refills pools every 10
   seconds by creating sandboxes through the cold path ([`sandbox-warm-pool.service.ts` L167-215](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/sandbox/services/sandbox-warm-pool.service.ts#L167-L215),
   [`createForWarmPool` L390-458](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/sandbox/services/sandbox.service.ts#L390-L458)).
3. **Cold path.** The runner is chosen by `findAvailableRunners`: READY, schedulable, availability score above a
   threshold, holding the snapshot; sorted by score, **top 10, then uniformly random** ([`runner.service.ts` L297-362](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/sandbox/services/runner.service.ts#L297-L362),
   [L803-821](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/sandbox/services/runner.service.ts#L803-L821)).
   A row is inserted `pending = true` and a `CREATED` event runs `syncInstanceState` under a Redis state-change
   lock; a 10-second cron is the backstop, taking 250 unassigned and 50 per-runner pending sandboxes per tick with
   a concurrency of 10 ([`sandbox.manager.ts` L831-888, L1157-1163](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/sandbox/managers/sandbox.manager.ts#L831-L888)).
   If no READY runner has the snapshot, one is told to pull it and the API polls every 5 seconds (up to an hour),
   sleeping 3 seconds when no runner is available ([`sandbox-start.action.ts` L168-366](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/sandbox/managers/sandbox-actions/sandbox-start.action.ts#L168-L366)).
4. **Runner.** v2 runners **long-poll** the API (30 s timeout, up to 10 jobs; API side blocks on Redis `BRPOP` with a
   database fallback) and run each job in its own goroutine ([`poller.go` L56-123](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/runner/pkg/runner/v2/poller/poller.go#L56-L123),
   [`job.service.ts` L122-213](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/api/src/sandbox/services/job.service.ts#L122-L213),
   [`config.go` L52-53](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/runner/cmd/runner/config/config.go#L52-L53)). `Docker.Create` pulls the image
   whole through the Docker engine if it is not cached (skipped only for non-`latest` tags already present,
   [`image_pull.go` L22-70](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/runner/pkg/docker/image_pull.go#L22-L70)), calls `ContainerCreate` with CPU quota and memory limits, bind-mounts the Daytona daemon read-only, calls `ContainerStart`, polls
   container state every 10 ms, then polls the daemon's `/version` on port 2280 with a 5 ms sleep between attempts
   ([`create.go` L141-234](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/runner/pkg/docker/create.go#L141-L234),
   [`start.go` L66-156](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/runner/pkg/docker/start.go#L66-L156),
   [`daemon.go` L59-128](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/runner/pkg/docker/daemon.go#L59-L128)) `DE`.

**How "under 90 ms" is achieved.** The code shows one route with no per-sandbox start work: the warm-pool claim
(a database update of an already-started container), subject to the match conditions above. Daytona publishes no
clock-start/stop definition, no percentile and no concurrency for the figure [ASP-8][ASP-11]; Fly's competing
comparison page states the same ("none of the four vendors publishes p50 or p99 start latency", [ASP-51]). The
measured API-level numbers are 670 ms p50 / 1,025 ms max at 100-way burst, but 984 ms p50 with **p95 22.4 s and
max 38.6 s** one at a time on 2026-08-07 (raw, `DERIVED`; the site's trimmed p95 for that run is 21,465 ms)
[ASP-3][ASP-4], consistent with the code's cold path being a multi-hop, poll-driven pipeline whose slow branch
(snapshot pull) is bounded only by an hour-long poll. I did not determine which branch produced the tail.

**Snapshots, pause, fork.** For containers, a "snapshot" is an OCI image: `docker pause` the container, `docker
commit`, tag by content hash, push to the internal registry ([`snapshot_sandbox.go` L42-145](https://github.com/daytonaio/daytona/blob/v0.190.0/apps/runner/pkg/docker/snapshot_sandbox.go#L42-L145));
stop keeps the filesystem on the runner; archive moves it to object storage and "takes more time" to start,
scaling with size [ASP-11]; container filesystems stay on the runner while stopped, whereas VM sandboxes offload to
"nearby storage" on stop [ASP-14]. VM sandboxes add pause/resume ("under a second"), fork, and filesystem+memory
snapshots, hypervisor unnamed; the v0.190.0 docs and API already describe them for "VM-based runners", and the vendor
blog announcing VM sandboxes is dated 2026-07-10 [ASP-15][ASP-11][ASP-12]. No latency percentile for any of these is
published.

**Throughput and scale.** Published org caps are 300-600 creations per minute (5-10 per second) on Tiers 1-4,
Enterprise custom [ASP-13]. Daytona's own density study (2026-10-07) brought 1,000 sandboxes online in 10.8 s on
one NVIDIA Vera CPU (about 93 per second), 2,000 in 27.4 s (about 73 per second), versus 32.3 s and 88.4 s for 1,000
on AMD EPYC 9575F and 9755; sustained 13.3 completed agent jobs per second at peak, 12.7 at 2,000 concurrent
sandboxes (all `MV`; hardware core counts, sandbox size and workload image are not given beyond "the same coding-agent
task") [ASP-16]. It also states "on a typical day, Daytona spins up millions of sandboxes, and the median sandbox lives
for less than five minutes" `CL` [ASP-16].

**Self-hostable?** The frozen v0.190.0 tree is AGPL-3.0 and Docker-based, so its container class can be benchmarked
on our hardware (needs Postgres, Redis, a registry, S3-compatible storage [ASP-9 docs/architecture]); it is not
the product Daytona sells today (VM class, warm pools are not in it).

**What Hex must do to crush it.** (a) Make the warm claim a node-local compare-and-swap on a pre-armed box rather
than a Postgres+Redis round trip, and refuse with a typed error when the pool is empty. (b) The cold path should
involve no poll loops: Daytona's cold path crosses an API row, an event, a Redis lock, a runner pick, a job row,
a `BRPOP`, a Docker API create, a start, and two polls; Hex's OCI class already starts p50 14.4 ms with the image
present (n = 200; `matrix.md` "Hex today", [ASP-84]) against the 90 ms claim. (c) Do not describe a shared-kernel process
or OCI box as an untrusted-code boundary: the Copy Fail advisory is a documented example of the cross-tenant exposure
[ASP-18]. (d) Per-org creation caps of 5-10/s and about 93 starts/s per Vera socket set an easy throughput bar.

### 3.2 E2B

**What it is.** Open-source (Apache-2.0) Go backend of E2B Cloud: API, per-node orchestrator driving Firecracker,
in-VM agent `envd`, edge proxy, template builder [ASP-20]. The public repository was renamed from `e2b-dev/infra`
to `e2b-dev/runtime`; its commits carry `GitOrigin-RevId` trailers, i.e. it is a mirror of an internal repository,
so E2B Cloud's deployed code may differ [ASP-22]. Isolation: one Firecracker microVM per sandbox in its own cgroup
and network namespace with a per-sandbox nftables egress firewall; guest metadata through MMDS `DE`
[ASP-21 L305-323, L347-351].

**Start path (code read at commit `e266e2a`).**
1. SDK `POST /v2/sandboxes`; the API authenticates, resolves the template alias to a ready build, mints the envd
   access token, applies Redis-backed rate limits, and places the sandbox [ASP-21 L603-628].
2. **Placement is best-of-K.** Sample K = 3 ready nodes uniformly; score = max(CPU score, hugepage-pool score);
   CPU score = (requested + reserved + 0.5 x observed usage) / (4 x cores) with locally tracked in-flight
   reservations added; pick the lowest; up to 3 attempts; a node that refuses with `ResourceExhausted` stays
   eligible and the retry waits a full-jitter backoff from 1 ms to 1 s ([`placement_best_of_K.go` L15-75](https://github.com/e2b-dev/runtime/blob/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc/packages/api/internal/orchestrator/placement/placement_best_of_K.go#L15-L75),
   [`config.go`](https://github.com/e2b-dev/runtime/blob/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc/packages/api/internal/orchestrator/placement/config.go),
   [ASP-21 L164-189]) `DE`.
3. gRPC `SandboxService.Create` to that node's orchestrator. "Fresh creates are internally a *resume* of the
   template's base snapshot"; the API blocks on `Create`, which blocks on envd `/init`, so a returned sandbox is
   usable [ASP-21 L630-633].
4. `ResumeSandbox` ([`sandbox.go` L1534-2133](https://github.com/e2b-dev/runtime/blob/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc/packages/orchestrator/pkg/sandbox/sandbox.go#L1534-L2133)): create the cgroup; then, as parallel
   promises, (a) open the template memfile and start a **userfaultfd** handler, (b) start a **prefetcher** that replays
   the pages a resume is known to touch, (c) take a **pre-created network slot** (netns + veth + tap) from a pool of
   32 new and 100 reused slots with a 3 s return delay ([`pool.go` L27-33](https://github.com/e2b-dev/runtime/blob/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc/packages/orchestrator/pkg/sandbox/network/pool.go#L27-L33)),
   (d) build the **NBD rootfs overlay** (read-only template + per-sandbox COW cache), (e) serve memory.
5. `fc.NewProcess` starts a **fresh Firecracker process per sandbox** as `unshare -m -- bash -c <start script>` with
   `CLONE_INTO_CGROUP`; `Resume` waits for the FC API socket, the UFFD socket and the rootfs link, then calls the FC
   API to set the metrics FIFO, **`loadSnapshot` with a UFFD memory backend**, TX and drive rate limits, `resumeVM`,
   and MMDS metadata ([`process.go` L220-287, L723-892](https://github.com/e2b-dev/runtime/blob/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc/packages/orchestrator/pkg/sandbox/fc/process.go#L723-L892)).
6. The UFFD handler serves each fault with `UFFDIO_COPY` from the shared template cache through a worker pool of up
   to 4,096 in-flight requests per VM, at 4 KiB or 2 MiB page size ([`userfaultfd.go` L33-40, L250-277](https://github.com/e2b-dev/runtime/blob/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc/packages/orchestrator/pkg/sandbox/uffd/userfaultfd/userfaultfd.go#L33-L40)).
7. `WaitForEnvd` sends `/init` (env, token, CPU count), then `MarkRunning` publishes a Redis routing record
   ([`sandbox.go` L2087-2110](https://github.com/e2b-dev/runtime/blob/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc/packages/orchestrator/pkg/sandbox/sandbox.go#L2087-L2110)).

**How it gets fast (as designed).** Templates are pre-booted VM snapshots (memory + disk + VM state) in object storage;
memory is faulted in lazily; a template build ends with an "optimize" phase that records which pages a fresh resume
touches and stores them as prefetch hints; templates are cached on local disk, optionally on shared NFS or fetched
peer-to-peer; resume prefers the node that still caches the snapshot; guest memory comes from a 2 MiB hugepage pool
[ASP-20 "Why it's fast"][ASP-21 L324-346, L936-939, L1114-1118]; the orchestrator README requires `modprobe nbd
nbds_max=4096` and `nr_hugepages` [ASP-22]. What it still pays per sandbox is, by inspection of the code: a cgroup,
a `fork/exec` of `unshare` + `bash` + Firecracker, about six sequential HTTP calls over the FC unix socket, an NBD
device and a COW cache file, and an envd handshake. I did not time these steps; that this is the controllable fixed
cost is inference.

**Latency, claimed and measured.** Docs: "~150 ms from a snapshot or custom template" and snapshots load "in ~80ms"
`CL` [ASP-26]; the homepage and pricing markdown read on 2026-10-22 carry no start figure; E2B's own post of 2026-10-16
says the industry competes on "cold-start milliseconds" and that "sandboxes, as the industry defines them today, are
dead", pivoting to an "agent-driven private cloud" [ASP-27]. Independent: ComputeSDK p50 1,329 ms burst / 573 ms
sequential (section 2.1); DeltaBox's measurement of E2B restore, avg 900 ms per event (section 2.3).

**Pause, resume, fork.** Pause freezes the guest's workload cgroups through envd, pauses the VM, snapshots,
diffs memory by dirty-page tracking and rootfs COW blocks against the template, caches locally and uploads
asynchronously; there are optional deferred-rootfs-export and in-place checkpoint modes ([ASP-21 L894-1084]).
Docs: pause "approximately 4 seconds per 1 GiB of RAM", resume "approximately 1 second" `CL` [ASP-23]. Fork
checkpoints the source in place, then starts each fork as a new sandbox placed independently; the API caps one
request at 100 forks ([`sandbox_fork.go` L29-31](https://github.com/e2b-dev/runtime/blob/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc/packages/api/internal/handlers/sandbox_fork.go#L29-L31)), the SDK and CLI at 20 [ASP-25].
Idle sandboxes auto-pause; traffic to a paused sandbox resumes it through a single shared "resume flight" per
sandbox, a parking lot capped at 512 requests, and a typed 503 with `Retry-After` when the control plane cannot
resume right now [ASP-21 L407-458] (a worked example of typed refusal with a stated bound).

**Throughput and scale.** Plan limits only: 20 / 100-1,100 / 1,100+ concurrent; creation 1 per second (Hobby), 5 per
second (Pro) [ASP-24]. The repository ships `BenchmarkBaseImageLaunch` and `BenchmarkConcurrentResume`
(default levels 1-10 concurrent resumes, 2 vCPU, `e2bdev/base`, hugepages on; reports p50, p95, p99 and wall clock)
in [`packages/orchestrator/benchmarks/`](https://github.com/e2b-dev/runtime/tree/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc/packages/orchestrator/benchmarks); I found no published results.

**Self-hostable?** Yes. `embed/` runs the full stack with real Firecracker sandboxes on one Linux host with KVM via
Docker Compose; it is "one machine by design". This is the one competitor in the brief that we can benchmark
unmodified on our x86_64 hardware [ASP-20].

**What Hex must do to crush it.** (a) Keep the single snapshot-restore code path (matches CLAUDE.md §8) but pre-pay
the per-sandbox fixed cost: pre-forked VMM processes already in their cgroup and netns, waiting for `loadSnapshot`.
(b) Share template pages copy-on-write (`MAP_PRIVATE` of the memfile) instead of `UFFDIO_COPY` into private memory
[ASP-78 table 1 note][ASP-77]; keep userfaultfd for remote or compressed sources only. (c) Take `/init` out of the
critical path by capturing an initialised guest agent in the snapshot. (d) Pause is the weakest published number
(4 s per GiB): incremental dirty-page capture exists in E2B, so the bar is a pause that scales with dirty pages,
not guest memory (CubeSandbox's soft-dirty design is the reference). (e) E2B's tail is visible at n = 100 (max 3,520
ms, 2.2x the p99); Hex needs bounded queues and typed refusals on every wait, as E2B's own refusal/backoff does.

### 3.3 Modal Sandboxes

**What it is.** Hosted. Sandboxes run on **gVisor by default or a VM runtime** chosen per sandbox; GPU sandboxes only
on gVisor; nested virtualization is Enterprise-only [ASP-30]. Modal's security page says "Each container runs in its own
sandbox, isolated from the host, using primitives like gVisor or microVMs" [ASP-30, security page]. gVisor imposes the
syscall and file-open overheads quantified in section 2.4.

**Start path and why it is fast.** (1) *Container image loading* [ASP-33] (Belotti, 2024-09-08): images are not
unpacked; an image is a roughly 5 MB **index** loaded in 1-100 ms (memory, disk or network) and **FUSE-mounted in
about 2 ms**, with file data fetched lazily from a content-addressed tier (page cache, SSD at ~100 us and 4 GiB/s, AZ
cache server at ~1 ms, regional CDN at ~100 ms, blob store at ~200 ms); eager Docker/Kubernetes loading decompresses
single-threaded at ~80 MiB/s, "about a minute" for an 8 GB image; their tuned read-ahead is 32 MB, throughput
~2.5 GiB/s, and a 512 MiB safetensors file loads in 200 ms from disk cache or ~300 ms over the network (`MV`).
(2) *Scheduling* [ASP-32] (Weld and Adams, 2026-07-16): the original design queued sandboxes and wrote each
to Postgres with a second write on worker assignment; the **v2 design** (beta) removes every datastore from the creation
path: a fleet of scheduling servers holds cached worker state, **every worker is its own source of truth and publishes
state to a Redis stream**, the scheduler contacts a worker directly and the worker **accepts if it has free resources
or rejects**; metadata is written to durable storage asynchronously; "the sandbox creation path requires only two network hops
and one cheap CPU operation", scheduling "only takes tens of milliseconds now"; workers batch control messages.

**Numbers.** Vendor: 1,000,000 sandboxes created "in under a minute" with "the primary bottleneck" being the benchmark itself;
start time (client request to user code running) "less than half a second at the median", with a long tail "somewhat longer than we'd
like" attributed to kernel and network contention including **rtnl-lock contention in container networking setup, which took "tens
of seconds" until sandbox networking was changed**; today up to 50,000 concurrent per customer (`MV`/`CL`) [ASP-32].
No p99 or max is published; the 10,000-point scatter is a figure, not a table. Independent: 1,070 ms p50 with 3,561 ms p99 and
6,227 ms max at 100-way burst, p99 ranging 800-16,712 ms across five days (section 2.1).

**Snapshots.** Filesystem and directory snapshots are images (30-day default TTL). **Memory snapshots are alpha**:
the snapshot terminates the sandbox, TCP connections are closed, background processes from `exec` are not restored,
no GPU, 7-day expiry, and restore "can only be restored on the same exact instance type", which "can sometimes lead
to scheduling delays" [ASP-31]. No latency is published.

**Self-hostable?** No.

**What Hex must do to crush it.** (a) Match the v2 control-plane shape (stateless scheduler tier, worker-owned state,
accept/reject) and then beat its measured 1M-in-a-minute (about 16,700 creates per second at least) with a sharded
placement path; Hex's registry target for 1M placements is 120 s (AC-T7.11.2), which is slower than Modal's measured
creation of the same count. (b) Treat the rtnl finding as a requirement: pre-create netns/veth/tap off the hot path
and avoid any global kernel lock on it. (c) Memory snapshots must be hardware-portable or refused by name
(T-T7.2.1 already says so). (d) A gVisor default is a 2.2x-or-worse syscall tax per the 2019 study; microVM/process
boxes avoid it.

### 3.4 Morph Cloud (Infinibranch)

Hosted VM platform. Its docs state: "Infinibranch technology lets you snapshot, branch, and restore entire
computational environments in under 250ms", versus "2-3 minutes for typical VMs"; the 2024-11-19 launch post
credits "solving the core challenge of separating storage and compute for entire VMs", "<250 ms startup times",
OCI compatibility including Docker-in-Docker, and branch-level security `CL` [ASP-36][ASP-37]. Instances can be
paused with memory preserved ("effectively the same as if you had snapshotted"), started from a snapshot with
memory/process state restored, or cold-booted from a snapshot's disk [ASP-38]. **There is no stated method,
percentile, hardware or concurrency, no hypervisor named in the pages I read, and no independent measurement**
(Morph is not in ComputeSDK's lists). Self-hosting: no documentation found.
*To crush it:* publish the percentiles it omits. Morph's "<250 ms" is the same order as Hex's registry targets for the
snapshot class (AC-T7.2.3 warm start <= 300 ms; AC-T7.2.4 restore p99 <= 100 ms [ASP-84]), so those targets are not
4x better than the claim.

### 3.5 CodeSandbox SDK (Together AI)

The most detailed public engineering record of Firecracker memory snapshots in production. Canonical
`codesandbox.io` returned HTTP 403 to automated fetches, so the blog and docs were read from `codesandbox.stream`
pages that carry the same CodeSandbox branding and content [ASP-39][ASP-40][ASP-41][ASP-42].

- **Clone, 2022** [ASP-39]: Firecracker `create_snapshot` writes ~1 s per GB; the naive fork (pause ~16 ms, snapshot ~4 s,
  copy ~6 s, start ~300 ms) took ~10 s; eager writes of memory changes to disk plus a reflink copy reached 2 s. The 2023
  post [ASP-40] records the limit of that design: with 50+ VMs per node the 2 s mark was lost and "the clone time for a
  VM jump up to 14 seconds under heavy load" (disk strain from ~100,000 small random writes and reflink fragmentation).
- **Userfaultfd fork, 2023-05-09** [ASP-40]: a Rust `uffd-handler` registers the VM's memory (`memfd_create`), forks
  copy only a **page-source table**, one 8-byte pointer per 4 KiB page, so a 4 GiB VM's table is 4 MiB and copying it
  takes "~5-30ms"; sources are Uninitialized, another VM, a file, or a zstd-compressed file; the parent is
  write-protected so pages are copied to children before a post-fork write (this needed a Linux 6.1.8 kernel fix).
  Result (`MV`, production): **avg fork 921 ms, P01 336 ms, P99 1,833 ms** versus the filesystem fork's avg 1,954 ms,
  P01 701 ms, P99 17,519 ms.
- **Compressed lazy memory, 2024-05-29** [ASP-41]: 8 GiB snapshot -> 412 MiB with zstd (~20x) but a 4-10 s full
  decompress; instead **8 KiB chunks compressed with lz4 -> 745 MiB (~10x)**, decompressed on fault; sparse files
  were slow on btrfs, so chunks are concatenated with a manifest at the end; zero pages are not stored; Firecracker's
  memory balloon with **free page reporting** (CodeSandbox added the feature) lets freed pages be recorded as
  uninitialised and be reclaimed from the host; chunks stay raw when compression gains less than 50%. Compressed
  resume went from 4 s to ~1 s after de-sparsification and finally **avg 400 ms, P99 2 s, P01 150 ms** with ~2.5 million
  resumes per month on ~250,000 unique VMs per month; raw-snapshot resume was 300 ms (`MV`, production).
  Tried and abandoned: zstd dictionaries, predictive decompression (thread synchronisation cost).
- **SDK docs** `CL` [ASP-42]: create from template 1-3 s, resume from memory snapshot 0.5-2 s, disk snapshot 5-20 s, archive
  20-60 s; hosted only, "No, we do not offer self hosting at this time" [ASP-44].
- **Independent** [ASP-3][ASP-4]: p50 38,356 ms burst, 2,327 ms sequential with a 22.5 s max; the 1-3 s claim and the
  measured burst are 12-38x apart.

*To crush it:* its resume is limited by decompression and the filesystem on one NVMe tier (avg 400 ms; p99 2 s). Hex's
AC-T7.2.4 target (restore p99 <= 100 ms [ASP-84]) is 20x below CodeSandbox's published p99, but only if restore avoids
compression on the hot path (keep a hot-page set uncompressed, as the 50% rule and prefetching suggest).

### 3.6 Fly.io Machines and Sprites

- **Machines** (Firecracker microVMs from OCI images on dedicated servers [ASP-85]): "created" state fetches the
  image and builds a root filesystem, "maybe low double digit seconds"; "started" is "usually ... well under a second"
  `CL` [ASP-49]; creating "can take over a minute" per Fly's own Sprites post [ASP-45]. **Suspend** uses Firecracker snapshots
  of CPU, memory and file handles: resume "a few hundred ms" versus cold start "~2+ seconds"; not recommended above 2 GB of
  memory because suspend time grows; snapshots are discarded on deploy `CL` [ASP-48].
- **Scheduler (2023-02-01)** [ASP-50]: Fly replaced Nomad with `flyd`, a per-worker state machine over a local BoltDB
  ("every worker is its own source of truth"), a stateless broker `flaps` that collects capacity from workers in a
  region and runs "a quick best-fit ranking", and `Corrosion`, a SWIM-gossip SQLite replica for global state. Requests
  are "bids for resources" and workers "suppliers"; a request that finds no room is simply not filled, so there is **no
  pending state** ("jobs don't arrive and then sit on the book in a 'pending' state"), and the whole process can fail "the
  same way an immediate-or-cancel order does". Stated reasons: bin packing is wrong for platforms like Fly.io, and Nomad's
  asynchronous consensus scheduling is "no way to handle an HTTP request".
- **Sprites (2026-01-14)** [ASP-45]: Linux VMs created "in just a second or two" with a 100 GB durable disk. Decisions: no
  user container images (all Sprites start from one standard container, and "Every physical worker knows exactly what
  container the next Sprite is going to start with, so it's easy for us to keep pools of 'empty' Sprites standing by");
  the disk root is object storage with NVMe as a read-through cache, so Sprites "migrate (or recover from failed physicals)
  trivially"; "inside-out" orchestration inside the VM. Lifecycle [ASP-47]: after ~30 s idle the VM is **warm** (suspended,
  memory frozen; resume 100-500 ms with processes intact) then **cold** (stopped; wake 1-2 s, processes fresh); TCP
  connections do not survive. The storage layer was replaced on 2026-10-22: of 400 Sprites driven for about 5,000
  write/checkpoint/sleep/wake/restore cycles on the JuiceFS-based stack, "a nonzero number of them permanently lost the
  ability to checkpoint"; the **Sprites Block Device** presents a bucket as an NBD block device (ext4 on top; local cache;
  background upload), a checkpoint is a versioned block snapshot, and a restore "unmounts the filesystem, rewinds the volume
  in place and mounts it again", costing "about as much as a restart" [ASP-46]. Sprite checkpoints are filesystem-only
  [ASP-46][ASP-51].
- No independent latency measurement, no published percentile; self-hosting: no.

*To crush it:* Fly's scheduler shape (worker-owned state, broker with no pending queue, immediate typed refusal) is the
model CP-T7.3/CP-T7.4 should be at least as good as; its Sprite storage rewrite is evidence that metadata which must agree
with data (the post counts "three ledgers") loses state under crash, which Hex's crash-injection tests (AC-T7.3.3,
T-T7.1.2) are aimed at.

### 3.7 Vercel Sandbox

Firecracker microVM per sandbox on "Hive", Vercel's internal Firecracker orchestration platform ("processes over 2.7
million deployments per day" `CL`), generally available 2026-01-30 [ASP-55]. Persistent by default: on stop the SDK snapshots the
filesystem and resumes from it ("even faster than starting a fresh sandbox", no number) [ASP-53]. Default region `iad1`.
Quotas [ASP-54]: 10,000 concurrent (Pro/Enterprise; Hobby 10), 24 h sessions, up to 32 vCPUs depending on plan, **vCPU
allocation rate "increases with sustained usage": Pro starts at 150 vCPU/min and adds 500 vCPU/min each minute to a ceiling
of 5,000 vCPU/min**, back to the start after 10 idle minutes; 20 deletions per second. Vercel justifies Active-CPU billing
by "waiting for model inference accounts for 71% to 98% of runtime across five agent benchmarks" (it cites an arXiv paper
I did not open) [ASP-52]. Independent:
416 ms p50 / 710 ms max burst; 420 ms p50 / 1,764 ms max sequential [ASP-3][ASP-4]. Self-hosting: no. Internals of Hive,
snapshot restore latency and warm pools: not published.
*To crush it:* a ramped vCPU-allocation quota is an admission-control design to study (a typed, stated-bound refusal);
the measured 416-ms p50 is a hosted-API number and the bar for an on-node start is far lower.

### 3.8 Cloudflare Containers, Sandbox SDK, Dynamic Workers

Two sandbox environments [ASP-56]. **Containers**: each instance is a Firecracker microVM with its own kernel; images
are pre-fetched to locations, placement picks "the nearest location with a pre-fetched image", and "cold starts can
often be in the 1-3 second range" depending on image size [ASP-57]; fractional instance sizes (`lite` 1/16 vCPU, 256 MiB;
up to 4 vCPU / 12 GiB) with per-account defaults of 1,500 vCPU, 6 TiB memory and 30 TB disk (raisable) and filesystem
snapshots (30-day retention) [ASP-58]. **Dynamic Workers** (blog 2026-03-24, Varda, Pai, Gupta): V8 isolates created at
runtime; "an isolate takes a few milliseconds to start and uses a few megabytes of memory. That's around 100x faster and
10x-100x more memory efficient than a typical container" `CL`; "many container-based sandbox providers impose limits on
global concurrent sandboxes and rate of sandbox creation. Dynamic Worker Loader has no such limits"; one-off Dynamic Workers
usually run on the same machine, even the same thread, as the Worker that created them; the catch is that agents write
JavaScript (WASM and Python are possible but slower to load) and "security bugs in V8 are more common than security bugs in
typical hypervisors" [ASP-59]. Independent:
the `cloudflare` ComputeSDK provider measured 657 ms p50 / 2,010 ms max (burst), 1,995 ms p50 / 6,508 ms max (sequential);
Dax real workload 125.4 s, the slowest completing provider [ASP-3][ASP-4][ASP-7].
*To crush it:* the isolate numbers are the lightweight-class bar for a Hex process box (few ms, few MB) but only for
code that is JavaScript; native-code process boxes with cgroup limits need to match the start cost without the language
restriction, and say plainly that the kernel is shared.

### 3.9 Northflank, Blaxel, Tensorlake, Runloop, Beam, Freestyle, Isorun (hosted; claims plus ComputeSDK)

- **Northflank** [ASP-60][ASP-61]: Cloud Hypervisor microVMs (Kata) for CPU workloads, gVisor for GPUs, BYOC. Its page
  breaks a "Ready to run in 0.80s" run into schedule 0.12 s, **microVM and guest-kernel boot 0.41 s**, start cached container
  0.27 s: a cold boot, not a snapshot restore. The same product measured 83 ms p50 (sequential, 2026-08-07) and returned
  no successful iterations in the 2026-10-20 burst [ASP-3][ASP-4] (cause unknown). Internals: n/p.
- **Blaxel** [ASP-62][ASP-63]: microVMs persisted with filesystem and memory snapshots, auto-suspended after "a few seconds of
  network inactivity"; "Resume from standby in 25 milliseconds with full memory state restored" and, per the acquisition post,
  "suspend and resume in 25 milliseconds, up to 5x faster than other sandbox products" `CL`; "The root filesystem runs in
  memory"; "Scale to 100,000+ concurrent sandboxes" by tier quota; acquired by Baseten on 2026-09-10. Measured create is
  481 ms p50 / 539 ms max (burst); the resume figure is not covered by any independent measurement I found. Internals: n/p.
- **Tensorlake** [ASP-67][ASP-68]: Cloud Hypervisor microVMs; its own container scheduler (not Kubernetes) with a
  resource tracker, per-function pools with min/max/buffer, a placement engine with a "vacuum pass" that evicts lower-priority
  containers when nothing fits, and desired state pushed to dataplanes over persistent gRPC streams; claims creation "in
  single-digit milliseconds" and "hundreds" per second at peak. Measured 2,763 ms p50 burst (hundreds of times the claim). It
  documents the Kubernetes comparison explicitly: pod creation involves etcd writes, admission controllers, kubelet sync and
  image pulls, which "takes seconds at best".
- **Runloop** [ASP-69]: a "micro-VM*" environment with "Two layers of security, VM + Container" on a "custom bare-metal hypervisor"; 465 ms
  p50 sequential. No start-time figure in the pages read.
- **Beam** [ASP-70][ASP-71][ASP-72]: container sandboxes, open-source runtime beta9 ("Launch containers in under a second using a
  custom container runtime, scheduler, and embedded caching"); its sandbox page's heading says "Sub-second boot times" over
  the text "cold boot in one to three seconds"; memory snapshots and forks exist. Measured 105 ms p50 sequential, 920 ms p50 burst.
- **Freestyle** [ASP-64]: full Linux VMs up to 32 vCPU / 64 GB, nested virtualization, "VMs provision in about 65ms", live fork "without
  pausing it", wake "under 100ms" `CL`; measured 2,511 ms p50 burst (38x the claim).
- **Isorun** [ASP-65][ASP-66]: KVM microVMs from any OCI image (the first use of an image prepares an optimized cache, 10-60 s, kept per image and runner), fork,
  hibernate/resume, credential injection outside the guest; "ready in 10ms" `CL`; the fastest hosted measurement in ComputeSDK:
  **59 ms p50, 74 ms max at 100-way burst; 17 ms p50, 189 ms max sequential**. Public beta; internals n/p.
- **Microsandbox** [ASP-73]: local-first, embeddable microVMs (Apache-2.0; fork and snapshot of live sandboxes; no daemon). Its "under 100
  milliseconds" average boot is stated for "guest boot on an M1 machine" `CL`, which is not an API-level start; the ComputeSDK provider
  (a different, remote path) measured 2,360 ms p50 and 5,127 ms max at 100-way burst [ASP-3].

### 3.10 CubeSandbox (Tencent Cloud, open source)

The most reproducible self-hostable competitor. Apache-2.0; x86_64 Linux with KVM; E2B-SDK-compatible API; v0.1.0 on 2026-04-20,
v0.7.0 on 2026-08-28 [ASP-74]. **Architecture** [ASP-76]: `Cubelet` (Go node agent), `CubeShim` (Rust containerd shim v2), `CubeHypervisor`
(RustVMM + KVM VMM with virtio block, net, vsock, fs; seccomp-hardened), `CubeCoW` (Rust; thin volumes with O(1) snapshot and clone through
the `FICLONE` ioctl on XFS reflink, incremental memory snapshots by soft-dirty page tracking), `CubeVS` (eBPF data plane), `CubeEgress`
(L7 egress proxy with credential injection). Boot: Cubelet clones the template's rootfs and memory volumes with reflink "(O(1), no data
copy)" and CubeShim restores the VM from the memory snapshot. A pool of pre-created TAP devices (`tap_init_num`, default 500) means TAP creation is not paid per
sandbox [ASP-75].

**Vendor benchmark, with commands** [ASP-75] (`MV`; 2026-06-01): Tencent bare-metal BMI5, 2 sockets x 24 cores x 2 threads (Xeon Platinum
8255C @ 2.5 GHz, 96 logical CPUs), 375 GiB DDR4-2933, 3.84 TB NVMe on XFS; sandbox 2 vCPU / 2 GiB; E2B SDK against the Cube API; 3 warm-up
rounds discarded. Create from template (n, avg / min / p95 / max, throughput):

| Concurrency | n | avg | min | p95 | max | creates per second |
|---|---|---|---|---|---|---|
| 1 | 20 | 47.8 ms | 43.5 | 57.4 | 60.4 | 17.9 |
| 10 | 200 | 88.7 | 45.8 | 116.9 | 119.1 | 101.4 |
| 20 | 300 | 98.1 | 47.7 | 175.8 | 232.6 | 180.9 |
| 50 | 500 | 276.1 | 60.6 | 508.4 | 681.3 | 147.6 |

Other operations (avg unless noted): snapshot 49.8 ms (~7 MB dirty) rising to 486.9 ms at 1,136 MB dirty; create from snapshot 63.9 ms,
independent of snapshot size (60-85 ms); rollback 81.6 ms; clone 219.6 ms single, 100 clones at 50 concurrency in 540.9 ms wall (5.4 ms
amortized); pause 558.4 ms at 2 GiB (full-memory copy; soft-dirty incremental pause "expected" to reach ~60 ms, not yet shipped);
resume 41.8 ms (p95 65.1 ms). Density: amortized overhead measured by `free` as **~21.5 MB at 100 live, ~25.7 MB at 1,000 live** idle
sandboxes, 1,000 sandboxes using ~25 GiB of 375 GiB. The report's rounds are small (n = 3-5 for snapshot operations), so those p95 and
max values are not percentiles in a statistical sense.

**Inconsistencies inside Tencent's own material.** The README says "under 50 concurrent creations, avg 67ms, P95 90ms, P99 137ms" [ASP-74],
while the benchmark report's 50-concurrent row is avg 276 ms, p95 508 ms, max 681 ms [ASP-75]; the README says "<5MB" memory overhead
while the report measures ~21.5-25.7 MB amortized. Both documents are on the repository's `master` as read on 2026-10-22, and the README links the report as the source of its benchmark
claims; I did not determine which run produced the README's 67 ms figure (the repository also has a separate PVM cloud-VM report that I did not read).
DeltaBox cites CubeSandbox v0.3.0's snapshot 49.8 ms, create-from-snapshot 63.9 ms and rollback 81.6 ms and measures its fan-out as
10x or more slower than DeltaBox's, with 77-80% GPU occupation at N = 16-64 [ASP-78].

*To crush it:* this is the number to beat for a self-hostable microVM with a network and an agent: ~48 ms create, ~64 ms from snapshot, ~181
creates/s/node (96 threads), ~25 MB per idle VM. Every one of those has a visible fixed cost (a Go node agent, a Rust shim, containerd, TAP
devices, reflink metadata) that a single-binary Hex node agent can remove. Run `cube-bench` (Go, in the repo) on Hex hardware to get the
number on our machines.

### 3.11 Zeroboot (open-source prototype)

Created 2026-03 by Adam Miribyan; Apache-2.0; "working prototype ... not production-hardened" [ASP-77]. Method: boot a template under Firecracker once,
snapshot memory and CPU state; each fork "Creates a new KVM VM, maps snapshot memory as CoW, restores all CPU state" (its diagram names `mmap(MAP_PRIVATE)`). README
table (`MV`, hardware not stated): **spawn p50 0.79 ms, p99 1.74 ms; ~265 KB memory per sandbox; fork + exec Python ~8 ms; 1,000 concurrent forks
815 ms**. Limits it states: one vCPU, no networking (serial I/O only), shared CSPRNG state across forks, 15 s to re-snapshot a template. The competitor
columns of its table (E2B ~150 ms p50 / ~300 ms p99 / ~128 MB, Daytona ~27 ms / ~90 ms / ~50 MB, microsandbox ~200 ms) carry no source in the README
and are not used here. This is the lower bound for what a CoW-shared snapshot fork costs when the guest needs no network, devices or agent.

### 3.12 Google GKE Agent Sandbox, Agent Substrate, Kubernetes agent-sandbox (overlap with `substrates-and-sandboxes.md`)

Verified here: the Kubernetes SIG Apps post (2026-03-20) says "starting a new pod adds about a second of overhead" and that `SandboxWarmPool` + `SandboxClaim` over a
`SandboxTemplate` "effectively eliminating cold starts" with gVisor or Kata via RuntimeClass `CL` [ASP-83]. Google's 2026-05-20 post (GA of GKE Agent
Sandbox, and Agent Substrate): warm pool "enables GKE to allocate 300 sandboxes per second, per cluster, at sub second latency, with 90% of allocations
completing in 200 milliseconds" `CL`; pod snapshots resume idle agents "in seconds"; a "minimal control plane designed to bypass some of the limitations
of Kubernetes" for "the chatter of millions of sub-second tool calls" [ASP-80]. Substrate's README claims resume "less than 400ms" at "more than 1500 activations per
second" and "10x higher density than standard containers" `CL` [ASP-81]; its architecture document says "Much of this architecture is aspirational, and is
not yet implemented", maps many "actors" onto fewer pre-started "worker" pods, and sets north-star targets of **activation latency p95 100 ms, 1 billion
agents per cluster, 1,000 wakeup events per second** `DE` (targets) [ASP-82]. It names data locality for actor state, observability of multiplexed actors
and autoscaling as unsolved. These are the closest published statements of the problem Hex's CP-T7.11 addresses.

### 3.13 Research evidence: DeltaBox and AgentZip

DeltaBox [ASP-78] (Firecracker guest; Linux 6.8 overlayfs-based `DeltaFS`; CRIU plus a warm template process for `DeltaCR`): checkpoint 10.83 ms and restore 1.86 ms
weighted average (fast path 1.86 ms; CRIU lazy-pages slow path 9.29 ms), `fork()` of a ~15 MB agent heap p50 0.57 ms (N = 1), 0.78 (4), 1.67 (16), 5.47 ms (64),
p99 14.74 ms at 64, aggregate 165.9 forks/s at 64, 679 MB total RSS for 64 children (`MV`, authors' own prototype; 96-core host). Its argument: duplicate only the change
between consecutive checkpoints, hide the dump inside LLM wait time, and fork from a frozen template for rollback. AgentZip [ASP-79] (Li et al.,
arXiv:2609.11294, 2026-09-10): memory compression for high-fanout agent sandboxes that exploits template-relative and cross-sandbox page redundancy,
reducing sandbox-owned memory "by up to 8.7x, compared with 2.1x for the Linux configuration", with restore prefetching and agent-phase-aware scheduling cutting
the slowdown of aggressive compression from "as high as 3.1x to 1.40x".

---

## 4. Techniques worth stealing

Each item names the evidence; none is a Hex design decision until a `docs/design/` record cites it.

1. **One create path: restore a pre-booted snapshot** (E2B README "a sandbox is a resumed snapshot"; Cube; Zeroboot) [ASP-20][ASP-74][ASP-77]. Fresh creates,
   resumes and forks share code, which is also CLAUDE.md §8.
2. **Share snapshot pages copy-on-write instead of copying them** (`mmap(MAP_PRIVATE)` of the snapshot file: Zeroboot 0.79 ms p50 and 265 KB; reflink'd memory
   volume: Cube ~25 MB per 2 GiB VM; versus E2B's `UFFDIO_COPY` into private memory [ASP-78]). Reserve userfaultfd for remote, compressed or cross-VM sources
   (CodeSandbox's page-source table: an 8-byte pointer per page; fork of the table in ~5-30 ms) [ASP-40].
3. **Record the working set at build time and prefetch it at restore** (E2B's "optimize" phase and prefetcher) [ASP-21 L1114-1118][ASP-22 `sandbox.go` L1631-1718];
   CodeSandbox records the pages a resume reads and replays them in its snapshot-load benchmark tool [ASP-41].
4. **Store memory snapshots chunked, compressed per chunk, zero pages elided, with a manifest at the end** (CodeSandbox: 8 KiB lz4 chunks, 10x; stay raw below 50%
   gain; free-page-reporting balloon so freed memory costs nothing) [ASP-41]. Avoid sparse files on btrfs.
5. **Dirty-page (soft-dirty or write-protect) incremental snapshots** so pause cost scales with dirty pages, not guest size (Cube soft-dirty; E2B diff; DeltaBox
   delta layers). E2B's published "4 s per GiB" and Cube's 558 ms for a 2 GiB full copy are the weak points to beat [ASP-23][ASP-75].
6. **Copy-on-write root filesystems at O(1)**: reflink/`FICLONE` (Cube), per-sandbox COW cache over a read-only NBD template (E2B), layered overlay with a frozen
   writable layer and switch-on-rollback (DeltaBox) [ASP-76][ASP-22][ASP-78]; and **lazy, content-addressed image loading** for OCI boxes (Modal: a ~5 MB index mounted
   in ~2 ms versus single-threaded eager decompression at ~80 MiB/s) [ASP-33].
7. **Take every setup step that touches a global kernel lock off the create path.** E2B pools network slots (32 new / 100 reused); Cube pre-creates TAP devices (500);
   Modal measured creation stalling for "tens of seconds" on the rtnl lock when its scheduler became fast, and had to redesign sandbox networking [ASP-22][ASP-75][ASP-32].
8. **Control plane: stateless schedulers, worker-owned state, synchronous accept-or-refuse, nothing durable in the create path.** Modal v2 (2026) and Fly `flyd`/`flaps`
   (2023) converged on the same shape; Tensorlake and Daytona show the alternatives (a central desired-state tracker; a Postgres/Redis/cron reconciler with
   10 s ticks) [ASP-32][ASP-50][ASP-67][ASP-9]. E2B adds in-flight reservation accounting to best-of-K and full-jitter refusal backoff [ASP-22].
9. **Affinity for restore**: prefer the node that still caches the snapshot (E2B), or make storage location-independent (Fly's object-storage disks) so any node can resume;
   Substrate names data locality a first-class scheduler concern [ASP-21 L936-939][ASP-45][ASP-82].
10. **Wake-on-traffic with shared resume flights and bounded parking** (E2B client proxy: one resume per sandbox, parked requests capped at 512, typed 503 with `Retry-After`)
    [ASP-21 L407-458]. This is rule 2 and rule 3 of CLAUDE.md applied to a wait.
11. **Process-level fork from a frozen template for the lightest class** (DeltaBox: 0.57 ms p50 `fork()` of a warm agent, 1.86 ms restore) [ASP-78], and CoW-forked
    VMs for the strongest (Zeroboot) [ASP-77]; this is the "zygote" idea evaluated in `oci-runtimes-and-fast-start.md`.
12. **Credentials outside the sandbox** (Isorun, Cube, Blaxel, Substrate, E2B secrets as markers) is now table stakes for agent platforms [ASP-65][ASP-76][ASP-62][ASP-81][ASP-21]; relevant to
    the trust boundary Hex must state for each box class.
13. **Admission by allocation rate rather than only by count** (Vercel's ramped vCPU/min quota; E2B's creation-rate limits) [ASP-54][ASP-24].
14. **A shipped, reproducible benchmark harness** (E2B `BenchmarkConcurrentResume`; Cube `cube-bench` and scripted snapshot suite with the exact commands; ComputeSDK's open JSON): the
    pattern for Hex's own `bench/`, and the way to measure these systems on our hardware [ASP-22][ASP-75][ASP-3].

---

## 5. The bar the 4-5x rule implies (arithmetic on the evidence; not a requirement edit)

"Best competing implementation" differs by class, and the numbers come from different hardware, so each row is a lead until `bench/` re-measures the self-hostable systems on the same
machine. `DERIVED` = my arithmetic (best figure divided by 4 and by 5).

| Metric (class) | Best published or measured competitor | Kind | 4x / 5x better means |
|---|---|---|---|
| microVM create from template, on node, 1 at a time (avg / p95 / max) | CubeSandbox 47.8 / 57.4 / 60.4 ms (96-thread Xeon, 2 vCPU / 2 GiB, n = 20) [ASP-75] | `MV` | avg 12.0 / 9.6 ms; p95 14.4 / 11.5 ms; max 15.1 / 12.1 ms |
| microVM create from snapshot | Cube 63.9 ms avg (p95 66.1) [ASP-75] | `MV` | 16.0 / 12.8 ms |
| resume from memory snapshot | Cube 41.8 ms avg (p95 65.1) [ASP-75]; Blaxel "25 ms" [ASP-62]; CodeSandbox avg 400 ms / P99 2 s in production [ASP-41] | `MV`, `CL`, `MV` | 10.5 / 8.4 ms avg (Cube); 6.3 / 5.0 ms (Blaxel claim); against CodeSandbox's p99: 500 / 400 ms |
| pause with 2 GiB guest | Cube 558.4 ms (full copy) [ASP-75]; E2B ~8 s (4 s/GiB) [ASP-23] | `MV`, `CL` | 140 / 112 ms (Cube); 2.0 / 1.6 s (E2B) |
| single clone / fork with memory | Cube 219.6 ms [ASP-75]; CodeSandbox avg 921 ms, P99 1,833 ms [ASP-40] | `MV` | 54.9 / 43.9 ms (Cube); P99 458 / 367 ms (CodeSandbox) |
| per-fork cost when no network or devices are needed | Zeroboot 0.79 ms p50, 1.74 ms p99 [ASP-77]; DeltaBox 0.57 ms (N=1) to 5.47 ms (N=64) [ASP-78] | `MV` | 0.20 / 0.16 ms p50; 1.4 / 1.1 ms at N=64 (process-level class) |
| creates per second per node, microVM | Cube 180.9/s at 20 concurrent on 96 threads [ASP-75]; Zeroboot ~1,227/s (1,000 in 815 ms) [ASP-77] | `MV` | 724 / 905 per second per 96 threads (Cube-equivalent) |
| creates per second, cluster | Modal: 1,000,000 in under 60 s, at least 16,700/s [ASP-32]; GKE 300/s [ASP-80]; Substrate "more than 1500 activations per second" [ASP-81] | `MV`, `CL`, `CL` | 66,700 / 83,300 per second (Modal); 6,000 / 7,500 (Substrate claim) |
| memory per idle workload | Cube ~25.7 MB per 2 GiB VM at 1,000 live [ASP-75]; Zeroboot ~265 KB (1 vCPU, no net) [ASP-77]; Cloudflare isolate "a few MB" [ASP-59] | `MV`, `MV`, `CL` | 6.4 / 5.1 MB (Cube class) |
| hosted API create to first command (burst of 100, p50 / max) | Isorun 59 / 74 ms; sequential 17 / 189 ms [ASP-3][ASP-4] | `MI` | 14.8 / 11.8 ms p50 (burst); needs a client within a few ms of the API; network round trip is the floor |
| tail behaviour | Modal burst p99 3,561 ms and max 6,227 ms at p50 1,070 ms (3.3x and 5.8x p50) [ASP-3]; Daytona sequential max 38,552 ms at p50 984 ms [ASP-4] | `MI` | no vendor publishes p99.9; the competitor tail is simply a large multiple of p50, so the bar is a stated p99.9 / p50 ratio near 1 |

**Observations on the current registry (read-only; no file edited).** Thresholds in `docs/requirements.toml` [ASP-84] against this evidence, with definitions that may differ:
AC-T7.2.3 (warm from snapshot <= 300 ms) is 4.7x looser than Cube's 63.9 ms create-from-snapshot and 12x looser than Blaxel's 25 ms claim; AC-T7.2.4 (restore p99 <= 100 ms) is 1.5x looser than Cube's
66.1 ms p95; AC-T7.2.2 (Firecracker cold <= 1,500 ms) is about 2x looser than Northflank's 0.8 s cold demonstration; AC-T7.6.2 (agent start, warm pool hit, p50 <= 300 ms) is 1.5x looser than GKE's claim that 90% of allocations complete in 200 ms and about 6x looser than Cube's 48 ms; AC-T7.11.2 (place 1,000,000 pending workloads on 10,000 simulated nodes in <= 120 s, i.e. >= 8,333/s) is about 2x slower than Modal's measured
creation of 1,000,000 real sandboxes in under 60 s (a different operation, and Modal says the benchmark client was the bottleneck). AC-T7.1.3 (OCI start p50 <= 50 ms) is already beaten by the 14.4 ms p50 recorded in
`matrix.md` ("Hex today", `bench/hex_start/RESULT.md`), which is 6.3x below Daytona's 90 ms claim and 3.3x below Cube's microVM create, though for a different class.

### 5.1 Findings that matter most for the competitive baseline

1. **Every fast figure comes from pre-created state with nothing durable in the create path; what customers measure end to end is 7x to hundreds of times slower and has a fat tail.** Daytona's only start-free path
   is a warm-pool row reassignment [ASP-9]; E2B restores a snapshot with a fresh Firecracker process per sandbox [ASP-22]; Modal removed every datastore from the creation path in 2026 [ASP-32]. Yet through the public
   API at 100-way burst the p50s are Daytona 670 ms, E2B 1,329 ms, Modal 1,070 ms, with maxima of 1.0 s, 3.5 s and 6.2 s and day-to-day p99s as high as 16.7 s (Modal, 2026-03-20) [ASP-3][ASP-6].
2. **The self-hostable bar is CubeSandbox and Zeroboot, not E2B.** Reflink-cloned memory and rootfs plus pre-created TAP devices give 47.8 ms create, 63.9 ms from snapshot, 41.8 ms resume, ~181 creates/s per
   96-thread node and ~25 MB per idle 2 GiB VM [ASP-75]; `mmap(MAP_PRIVATE)` of a snapshot gives 0.79 ms p50 and ~265 KB per fork when no network is needed [ASP-77]. E2B's UFFD-copy restore measured 0.9 s per event in DeltaBox's setup
   and its pause is documented at 4 s per GiB [ASP-78][ASP-23]. Beating these by 4-5x on the same machine means roughly <= 12 ms create, <= 16 ms from snapshot, <= 10 ms resume, <= 6 MB per idle VM and
   >= 900 creates/s per 96 threads (section 5, `DERIVED`).
3. **Control-plane scale is a different bar from per-node start.** Modal (1,000,000 sandboxes in under a minute) and Fly (`flyd` and `flaps`, no pending state) converged on stateless schedulers, worker-owned state and synchronous
   accept-or-refuse; Daytona's Postgres + Redis + 10 s-cron reconciler sits behind plan limits of 5-10 creations per second per organization [ASP-32][ASP-50][ASP-13][ASP-9]. Google's Agent Substrate states the problem Hex addresses
   (activation p95 100 ms, 1,000 wakeups/s and 1 billion agents per cluster as targets; "more than 1500 activations per second" claimed) [ASP-82][ASP-81]. Hex's AC-T7.11.2 (1M placements in 120 s) is slower than Modal's
   measured creation of 1M real sandboxes.
4. **The tail is the open flank.** No platform publishes p99.9. At n = 100 the measured max ranges from 1.5x the p50 (Daytona, burst 2026-10-20) to 5.8x (Modal, same run) and 39x (Daytona, sequential 2026-08-07), and in the
   worst runs is 2-3x the p99; one platform's p99 can move a lot between days (Modal: 800-16,712 ms over five runs) [ASP-3][ASP-4][ASP-6]. Hex's p99.9 and max claims need n >= 10,000 per point and raw data kept, and will be
   uncontested if they hold.
5. **Isolation is converging on a microVM per sandbox, with isolates as the cheap end.** Daytona's own advisory records a cross-tenant cache-corruption primitive reachable from a shared-kernel sandbox [ASP-18]; Daytona and Modal now offer VM classes
   beside their container and gVisor defaults [ASP-12][ASP-30]; Cloudflare Containers are Firecracker microVMs, while its isolates start in "a few milliseconds" with "a few megabytes" for JavaScript only [ASP-57][ASP-59]. A Hex process box competes
   with the isolate on cost and with the microVM on nothing but cost, and must say which boundary it provides.

---

## 6. Open questions and gaps

1. **No vendor publishes p99.9 or max of start latency**, and ComputeSDK's n = 100 supports only p99 and max. Hex needs its own runs at n >= 10,000 per point (`matrix.md`'s "Hex today" rows already note that n = 200
   cannot estimate p99.9).
2. **Daytona's "<90 ms"**: clock start/stop, concurrency, warm-pool state and percentile are unpublished; the VM runner (hypervisor, pause/fork mechanics) is closed source; why the sequential run on
   2026-08-07 had p95 22 s is unknown.
3. **E2B's "~150 ms" and "~80 ms"**: method unpublished; no results for the shipped benchmarks. The public repository is a mirror (`GitOrigin-RevId`); E2B Cloud may run different code. Running
   `BenchmarkConcurrentResume` and Embed on Hex hardware is the way to close this.
4. **Modal**: no memory-snapshot or VM-runtime latency; the 10,000-point start-time scatter is a chart, not a table; the new scheduler is beta.
5. **Morph Cloud**: no method, percentile, hardware or hypervisor named; I found no independent measurement. **Fly Machines/Sprites**: no percentile; Machines API rate limits not found in the pages read.
6. **Vercel (Hive), Cloudflare Containers, Blaxel, Freestyle, Isorun, Runloop, Tensorlake, Northflank**: start paths, snapshot restore latency and warm-pool designs are unpublished. Vendor
   figures differ from ComputeSDK's by 7x (Daytona 90 ms vs 670 ms p50) through 9x (E2B ~150 ms vs 1,329 ms) and 39x (Freestyle 65 ms vs 2,511 ms) to several hundred times (Tensorlake "single-digit milliseconds" vs 2,763 ms); the cause (API and network, queueing, adapter
   defaults, unwarmed pools, or the claim measuring something else) is not established. Blaxel's 25 ms is a resume and ComputeSDK measures a create, so they do not conflict.
7. **ComputeSDK caveats**: unknown runner region and RTT floor; adapter and pool configuration per provider undocumented; displayed percentiles are trimmed; one run per day; its sequential and staggered tests are
   retired (last run 2026-08-07); daily burst files for 2026-07-20, 08-20, 09-20, 10-13 and 10-19 returned 404 under the earlier naming pattern and the directory listing I could read ended at 2026-06-16, so the time series is 2026-03-20, 04-20, 05-20, 06-16 plus the 2026-10-20 latest.
8. **CubeSandbox**: README and report disagree (c = 50: avg 67 ms vs 276 ms; "<5MB" vs 21.5-25.7 MB); snapshot-operation rounds are n = 3-5; whether CubeCoW's numbers hold off the vendor's bare-metal host is untested.
   DeltaBox's claim that CubeCoW needs a non-mainline host kernel was not verified against Cube's docs.
9. **DeltaBox's E2B figures** measure E2B's incremental VM pause/resume as a checkpoint/rollback substrate (the paper says so), not E2B's create path; no p99 or max for E2B is given.
10. **GitHub API rate limits** blocked a few repository-metadata lookups; code was read from tarballs at the commits named in the references.
11. **Measurements to run on Hex hardware** (the self-hostable systems): E2B Embed with the shipped `BenchmarkConcurrentResume` [ASP-22]; CubeSandbox with `cube-bench` and its snapshot scripts [ASP-75]; Zeroboot [ASP-77];
    Daytona `v0.190.0`'s container class [ASP-9]; Agent Substrate when it is implemented [ASP-82]. Hosted platforms (Modal, Morph, CodeSandbox, Fly, Vercel, Cloudflare, Blaxel, Freestyle, Isorun, Tensorlake, Northflank,
    Runloop) can be re-measured only through their APIs, with the network floor stated.
12. **Not covered**: Docker's microVM sandboxes, Kubernetes Pod Snapshots in detail, first-party sandboxes of model vendors, Cua and Arrakis-style projects, Runloop and Northflank engineering write-ups (none found), and
    the arXiv paper Vercel cites for the 71-98% inference-wait share (not opened).

---

## References

All accessed 2026-10-22. Code references name the commit; files are linked inline where read.

**Cross-system independent measurement**

- **[ASP-1]** ComputeSDK (Snelling, LLC), "Burst TTI Benchmarks" (leaderboard, last run 2026-10-20). https://www.computesdk.com/benchmarks/sandboxes/burst-tti/
- **[ASP-2]** ComputeSDK, "Benchmark Methodology" (`METHODOLOGY.md`, `master`). https://github.com/computesdk/benchmarks/blob/master/METHODOLOGY.md (raw: https://raw.githubusercontent.com/computesdk/benchmarks/master/METHODOLOGY.md)
- **[ASP-3]** ComputeSDK, raw results `results/burst_tti/latest.json`, timestamp 2026-10-20T22:26:20Z. https://raw.githubusercontent.com/computesdk/benchmarks/master/results/burst_tti/latest.json
- **[ASP-4]** ComputeSDK, raw results `results/sequential_tti/latest.json`, timestamp 2026-08-07T13:27:36Z. https://raw.githubusercontent.com/computesdk/benchmarks/master/results/sequential_tti/latest.json
- **[ASP-5]** ComputeSDK, raw results `results/staggered_tti/latest.json`, timestamp 2026-08-07T13:27:36Z (100 launches, 200 ms apart). https://raw.githubusercontent.com/computesdk/benchmarks/master/results/staggered_tti/latest.json
- **[ASP-6]** ComputeSDK, daily burst results 2026-03-20, 2026-04-20, 2026-05-20, 2026-06-16. https://raw.githubusercontent.com/computesdk/benchmarks/master/results/burst_tti/2026-03-20.json (and `2026-04-20.json`, `2026-05-20.json`, `2026-06-16.json` in the same directory)
- **[ASP-7]** ComputeSDK, "Dax Sandbox Benchmark" (last run 2026-10-20). https://www.computesdk.com/benchmarks/sandboxes/dax/

**Daytona**

- **[ASP-8]** Daytona Platforms, `README.md` on `main` at commit ec4c21b2d597091ac09ecc278f3bcc172575a987 (2026-06-25): maintenance notice, "<90ms". https://github.com/daytonaio/daytona/blob/ec4c21b2d597091ac09ecc278f3bcc172575a987/README.md
- **[ASP-9]** Daytona, source tree at tag `v0.190.0` (commit 01c502bb1f1ff8f2885d0cd490e043736083dca8, 2026-06-23), AGPL-3.0; files linked inline (`apps/api/src/sandbox/**`, `apps/runner/pkg/docker/**`, `apps/runner/pkg/runner/v2/**`, `apps/docs/src/content/docs/en/**`). https://github.com/daytonaio/daytona/tree/v0.190.0
- **[ASP-10]** Daytona docs, "Warm Pools". https://www.daytona.io/docs/en/warm-pools.md
- **[ASP-11]** Daytona docs, "Sandboxes" (classes, lifecycle, fork, pause). https://www.daytona.io/docs/en/sandboxes.md
- **[ASP-12]** Daytona docs, "Linux Sandboxes" (container vs Linux VM). https://www.daytona.io/docs/en/linux-sandboxes.md
- **[ASP-13]** Daytona docs, "Limits" (tiers, creation rate). https://www.daytona.io/docs/en/limits.md
- **[ASP-14]** Daytona docs, "Persistence". https://www.daytona.io/docs/en/persistence.md
- **[ASP-15]** J. Stefanic, Daytona, "VMs (Pause & Fork)", 2026-07-10. https://www.daytona.io/dotfiles/vms-pause-and-fork
- **[ASP-16]** D. T. Graviet, Daytona, "When 2,000 Agents Share One CPU", 2026-10-07. https://www.daytona.io/dotfiles/when-2000-agents-share-one-cpu
- **[ASP-17]** Daytona, "Daytona is going closed source. Here's why.", 2026-06-11. https://www.daytona.io/dotfiles/updates/daytona-is-going-closed-source
- **[ASP-18]** Daytona, "Security Update: CVE-2026-31431 ('Copy Fail')", 2026-04-30. https://www.daytona.io/dotfiles/updates/security-update-cve-2026-31431-copy-fail
- **[ASP-19]** Daytona docs at `v0.190.0`, "Security exhibit" (Sysbox) and "Bring your own compute" (`CONTAINER_RUNTIME` default `sysbox-runc`). https://github.com/daytonaio/daytona/blob/v0.190.0/apps/docs/src/content/docs/en/security-exhibit.mdx

**E2B**

- **[ASP-20]** E2B, `README.md`, repository `e2b-dev/runtime` (formerly `e2b-dev/infra`) at commit e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc (2026-10-22). https://github.com/e2b-dev/runtime/blob/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc/README.md
- **[ASP-21]** E2B, `docs/ARCHITECTURE.md` at the same commit (line references `L` are to this file). https://github.com/e2b-dev/runtime/blob/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc/docs/ARCHITECTURE.md
- **[ASP-22]** E2B, source at the same commit: `packages/orchestrator/pkg/sandbox/{sandbox.go,fc/process.go,network/pool.go,uffd/**}`, `packages/orchestrator/{README.md,benchmarks/}`, `packages/api/internal/orchestrator/placement/**`, `packages/api/internal/handlers/sandbox_fork.go`. https://github.com/e2b-dev/runtime/tree/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc ; commit: https://github.com/e2b-dev/runtime/commit/e266e2af25ce6f0fd6b14a2b7bc9f9570a7a45fc
- **[ASP-23]** E2B docs, "Sandbox persistence" (pause 4 s/GiB, resume ~1 s). https://docs.e2b.dev/sandbox/persistence.md
- **[ASP-24]** E2B docs, "Billing & limits" (concurrency, creation rate). https://docs.e2b.dev/billing.md
- **[ASP-25]** E2B docs, "Sandbox forking". https://docs.e2b.dev/sandbox/fork.md
- **[ASP-26]** E2B docs, full text (`llms-full.txt`): "Startup ~150 ms from a snapshot or custom template" (page `agents/eve`) and "load the sandbox in ~80ms" (page `template/how-it-works`). https://docs.e2b.dev/llms-full.txt
- **[ASP-27]** V. Mlejnsky, E2B, "Sandboxes are dead", 2026-10-16. https://e2b.dev/resources/sandboxes-are-dead.md
- **[ASP-28]** T. Srnka, E2B, "E2B Sandboxes Aren't Affected by Copy Fail (CVE-2026-31431). Here's why.", 2026-04-30. https://e2b.dev/resources/not-affected-by-copy-fail-heres-why.md
- **[ASP-29]** E2B, "Insights" resource listing ("2B+ sandboxes started"). https://e2b.dev/resources/insights.md

**Modal**

- **[ASP-30]** Modal docs, "Sandboxes" (runtimes gVisor and VM, probes, pools). https://modal.com/docs/guide/sandboxes.md ; security: https://modal.com/docs/guide/security.md
- **[ASP-31]** Modal docs, "Sandbox Snapshots" (memory snapshots alpha, TTLs, limitations). https://modal.com/docs/guide/sandbox-snapshots.md
- **[ASP-32]** C. Weld and C. Adams, Modal, "Scaling to 1 million concurrent sandboxes in seconds", 2026-07-16. https://modal.com/blog/scaling-to-1-million-concurrent-sandboxes-in-seconds
- **[ASP-33]** J. Belotti, Modal, "Fast, lazy container loading in Modal.com", 2024-09-08. https://modal.com/blog/jono-containers-talk
- **[ASP-34]** Modal docs, "Cold start performance" ("Containers boot in about one second"). https://modal.com/docs/guide/cold-start.md
- **[ASP-35]** E. G. Young, P. Zhu, T. Caraza-Harter, A. C. Arpaci-Dusseau, R. H. Arpaci-Dusseau, "The True Cost of Containing: A gVisor Case Study", HotCloud 2019. https://www.usenix.org/system/files/hotcloud19-paper-young.pdf

**Morph, CodeSandbox, Fly.io**

- **[ASP-36]** Morph, "Welcome to Morph Cloud" (docs home). https://cloud.morph.so/docs/developers
- **[ASP-37]** Morph Team, "Introducing Infinibranch", 2024-11-19. https://cloud.morph.so/docs/blog/developers
- **[ASP-38]** Morph docs, "Pause / Resume" and "Boot from Snapshot". https://cloud.morph.so/docs/documentation/instances/pause-resume ; https://cloud.morph.so/docs/documentation/instances/boot-from-snapshot
- **[ASP-39]** I. van Hoorne, CodeSandbox, "How we clone a running VM in 2 seconds", 2022 (date not shown on the page copy). https://codesandbox.stream/blog/how-we-clone-a-running-vm-in-2-seconds (canonical `codesandbox.io/blog/how-we-clone-a-running-vm-in-2-seconds` returned 403)
- **[ASP-40]** I. van Hoorne, CodeSandbox, "Cloning microVMs by sharing memory through userfaultfd", 2023-05-09. https://codesandbox.stream/blog/cloning-microvms-using-userfaultfd
- **[ASP-41]** I. van Hoorne, CodeSandbox, "How we scale our microVM infrastructure using low-latency memory decompression", 2024-05-29. https://codesandbox.stream/blog/how-we-scale-our-microvm-infrastructure-using-low-latency-memory-decompression
- **[ASP-42]** CodeSandbox docs, "SDK: Core Concepts" (create 1-3 s; resume 0.5-2 s / 5-20 s / 20-60 s). https://codesandbox.stream/docs/sdk/core-concepts
- **[ASP-43]** CodeSandbox, "CodeSandbox SDK" product page ("within 2 seconds", "millions of concurrent VMs"). https://codesandbox.stream/sdk
- **[ASP-44]** CodeSandbox docs, "SDK: FAQ" (rate limits, self-hosting). https://codesandbox.stream/docs/sdk/faq
- **[ASP-45]** T. Ptacek, Fly.io, "The Design & Implementation of Sprites", 2026-01-14. https://fly.io/blog/design-and-implementation/
- **[ASP-46]** D. Botha, Fly.io, "SBD: We Have a Filesystem at Home", 2026-10-22. https://fly.io/blog/sprites-block-device/
- **[ASP-47]** Fly.io docs, "Sprites: Lifecycle" (warm/cold wake). https://docs.fly.io/sprites/concepts/lifecycle
- **[ASP-48]** Fly.io docs, "Machine Suspend and Resume". https://docs.fly.io/reference/suspend-resume
- **[ASP-49]** Fly.io docs, "Fly Machines overview". https://docs.fly.io/machines/overview
- **[ASP-50]** T. Ptacek, Fly.io, "Carving the Scheduler Out of Our Orchestrator", 2023-02-01. https://fly.io/blog/carving-the-scheduler-out-of-our-orchestrator/
- **[ASP-51]** Fly.io (competitor-authored), "Sprites vs E2B vs Modal vs Daytona: Agent Sandboxes Compared", checked by Fly 2026-10-12. https://fly.io/learn/sprites-vs-e2b-vs-modal-vs-daytona/

**Vercel, Cloudflare**

- **[ASP-52]** Vercel docs, "Vercel Sandbox" (last updated 2026-09-22). https://vercel.com/docs/vercel-sandbox
- **[ASP-53]** Vercel docs, "Understanding Sandboxes" (2026-08-25). https://vercel.com/docs/sandbox/concepts
- **[ASP-54]** Vercel docs, "Vercel Sandbox pricing and quotas" (2026-09-10). https://vercel.com/docs/sandbox/pricing
- **[ASP-55]** H. Arora and D. Fein, Vercel, "Run untrusted code with Vercel Sandbox, now generally available", 2026-01-30. https://vercel.com/blog/vercel-sandbox-is-now-generally-available
- **[ASP-56]** Cloudflare docs, "Sandboxes on Cloudflare" (2026-09-30). https://developers.cloudflare.com/sandbox/index.md
- **[ASP-57]** Cloudflare docs, "Containers: Architecture" (cold starts, Firecracker). https://developers.cloudflare.com/containers/concepts/architecture/
- **[ASP-58]** Cloudflare docs, "Containers: Limits and Instance Types" (2026-10-21). https://developers.cloudflare.com/containers/platform/limits/
- **[ASP-59]** K. Varda, S. Pai and K. Gupta, Cloudflare, "Sandboxing AI agents, 100x faster", 2026-03-24. https://blog.cloudflare.com/dynamic-workers/

**Other platforms**

- **[ASP-60]** Northflank, "Sandboxes" product page ("Ready in 0.8 seconds" breakdown; Cloud Hypervisor; gVisor for GPUs). https://northflank.com/product/sandboxes
- **[ASP-61]** Northflank, "What is an AI sandbox?" and "MicroVM vs gVisor" (blog; "over 2 million isolated workloads monthly", in production since 2021). https://northflank.com/blog/what-is-an-ai-sandbox ; https://northflank.com/blog/microvm-vs-gvisor
- **[ASP-62]** Blaxel, "Sandboxes" product page. https://blaxel.ai/platform/sandboxes
- **[ASP-63]** N. Lecomte, Blaxel, "Blaxel is joining Baseten to build the future of agentic infrastructure", 2026-09-10. https://blaxel.ai/blog/blaxel-is-joining-baseten-to-build-the-future-of-agentic-infrastructure
- **[ASP-64]** Freestyle, "VMs" product page. https://www.freestyle.sh/products/vms
- **[ASP-65]** Isorun, Inc., product site. https://isorun.ai/
- **[ASP-66]** Isorun docs (introduction, lifecycle and hibernation, OCI images). https://docs.isorun.ai/ ; https://docs.isorun.ai/sandboxes/lifecycle ; https://docs.isorun.ai/sandboxes/any-oci-image
- **[ASP-67]** Tensorlake docs, "Architecture" (container scheduler, desired state, pools). https://docs.tensorlake.ai/applications/architecture.md
- **[ASP-68]** Tensorlake docs, "Sandboxes FAQ" and "Sandboxes introduction". https://docs.tensorlake.ai/faqs/sandboxes-faq.md ; https://docs.tensorlake.ai/sandboxes/introduction.md
- **[ASP-69]** Runloop, homepage ("2x faster vCPUs ... custom bare-metal hypervisor", two-layer isolation). https://runloop.ai/
- **[ASP-70]** Beam docs, "Sandbox overview". https://docs.beam.cloud/v2/sandbox/overview.md
- **[ASP-71]** Beam, "Sandbox" product page ("Sub-second boot times", "one to three seconds"). https://www.beam.cloud/sandbox
- **[ASP-72]** Beam, `beta9` README. https://github.com/beam-cloud/beta9/blob/main/README.md
- **[ASP-73]** Microsandbox, `README.md` (boot "under 100 milliseconds" on an M1). https://github.com/superradcompany/microsandbox/blob/main/README.md
- **[ASP-74]** Tencent Cloud, CubeSandbox `README.md` (`master`). https://github.com/TencentCloud/CubeSandbox/blob/master/README.md
- **[ASP-75]** Tencent Cloud, "CubeSandbox Core Operations Performance Benchmark Report", 2026-06-01. https://github.com/TencentCloud/CubeSandbox/blob/master/docs/blog/posts/2026-06-01-cubesandbox-perf-benchmark.md
- **[ASP-76]** Tencent Cloud, CubeSandbox "Architecture overview". https://github.com/TencentCloud/CubeSandbox/blob/master/docs/architecture/overview.md ; changelog v0.3.0 (CubeCoW): https://github.com/TencentCloud/CubeSandbox/blob/master/docs/changelog/v0.3.0.md
- **[ASP-77]** A. Miribyan, Zeroboot `README.md`. https://github.com/zerobootdev/zeroboot/blob/main/README.md
- **[ASP-78]** Y. Dong, J. He, S. Liu, Y. Hou, D. Du, Z. Xu, S. Yu, B. Yang, Y. Xia, H. Chen, "DeltaBox: Scaling Stateful AI Agents with Millisecond-Level Sandbox Checkpoint/Rollback", arXiv:2605.22781v2, 2026-06-08. https://arxiv.org/abs/2605.22781 (full text https://arxiv.org/html/2605.22781v2)
- **[ASP-79]** M. Li, C. Xu, Q. Zhang, J. Yu, X. Sun, H. Mai, Z. Xie, "Memory Compression for High-Fanout Agent Sandboxes" (AgentZip), arXiv:2609.11294, 2026-09-10 (abstract read). https://arxiv.org/abs/2609.11294
- **[ASP-80]** B. Royal and T. Hockin, Google Cloud, "Agent Sandbox on GKE is now available for everyone, and a first look at Agent Substrate", 2026-05-20. https://cloud.google.com/blog/products/containers-kubernetes/bringing-you-agent-sandbox-on-gke-and-agent-substrate
- **[ASP-81]** Agent Substrate authors, `README.md`. https://github.com/agent-substrate/substrate/blob/main/README.md
- **[ASP-82]** Agent Substrate authors, `docs/architecture.md` (north-star metrics; "aspirational"). https://github.com/agent-substrate/substrate/blob/main/docs/architecture.md
- **[ASP-83]** J. Kuo and J. Santa Barbara, Kubernetes blog, "Running Agents on Kubernetes with Agent Sandbox", 2026-03-20. https://kubernetes.io/blog/2026/03/20/running-agents-on-kubernetes-with-agent-sandbox/
- **[ASP-84]** Hex repository, `docs/requirements.toml` (plan version 2, 2026-10-20) and `docs/research/runtime/matrix.md` ("Hex today"). Internal.
- **[ASP-85]** Fly.io docs, "Architecture" ("Application code runs in Firecracker microVMs"). https://docs.fly.io/reference/architecture
