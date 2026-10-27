# Hex research: the evidence behind the design

Every design decision in `docs/design/` cites these notes (CLAUDE.md §5). Hex's bar is set against the
best competing implementation: at least 4-5x better on throughput, latency (p50 through p99.9 and max)
and resource consumption — the floor, not the target. These notes are where "the best competing
implementation" and its numbers are established, before any design or code.

## Method

- **Primary sources first:** peer-reviewed papers, specifications, kernel documentation and commits,
  official docs, source code (linked to the file, ideally the line), and the projects' own engineering
  write-ups. Secondary articles are used only to find primary ones.
- **Every claim and every number is cited** with its title, URL, author or organisation, and date, and
  was opened and read when the note was written (accessed 2026-10-22 unless stated). Nothing is cited
  from memory; a source that couldn't be found is written as a gap, never filled in.
- **Every number has a kind:**
  - `MEASURED-INDEPENDENT` — a third party measured it; method, hardware and conditions are given.
  - `MEASURED-VENDOR` — the project measured itself.
  - `CLAIMED` — a figure with no stated method (marketing, a README line).
  - `DESIGN` — a fact from code or docs (how a thing works, not how fast).
- **Percentiles are quoted exactly as reported.** A mean is not a p50, "under 100 ms" is not a
  percentile, and most sources publish no p99.9 or max — the notes say so rather than infer them.
- **Our own measurements supersede claims.** The competitive baseline (`bench/`, CLAUDE.md §7) runs the
  self-hostable systems on our own hardware; until then a `CLAIMED` or `MEASURED-VENDOR` number is a
  lead, not a fact Hex designs against.

## Index

### Runtime (`runtime/`)

| Note | Covers |
|---|---|
| [`agent-sandbox-platforms.md`](runtime/agent-sandbox-platforms.md) | Daytona, E2B, Modal, Morph, CodeSandbox, Fly Machines, Vercel and Cloudflare sandboxes, and peers |
| [`substrates-and-sandboxes.md`](runtime/substrates-and-sandboxes.md) | Google AX and Agent Substrate, Kubernetes Agent Sandbox, gVisor, Kata, Firecracker (snapshots, warm pools, restore research), CRIU |
| [`oci-runtimes-and-fast-start.md`](runtime/oci-runtimes-and-fast-start.md) | runc, crun and youki start paths; zygotes and fork servers, clone3 with CLONE_INTO_CGROUP, pooled namespaces, serverless fast-start research |
| [`cgroup-v2-resource-control.md`](runtime/cgroup-v2-resource-control.md) | memory.min/low/high/max, cpu/io weights, io.latency and io.cost, PSI, TMO and oomd; machine protection |
| [`oci-spec-defaults.md`](runtime/oci-spec-defaults.md) | What `docker run`, containerd and runc's rootless mode give a container from an image; the kernel's overlayfs, mount and openat2 facts a structured spec's bundle rests on |
| [`matrix.md`](runtime/matrix.md) | The cross-system comparison: isolation, start latency, throughput, density, overhead — every cell cited |

### Scheduler (`scheduler/`)

| Note | Covers |
|---|---|
| [`cluster-managers-at-scale.md`](scheduler/cluster-managers-at-scale.md) | Kubernetes at extreme scale (kube-scheduler internals, limits, etcd/API-server bottlenecks, sampling, preemption, gangs, multi-cluster), Borg, Omega and the Borg traces, Meta's Twine, RAS and Shard Manager |
| [`placement-algorithms-and-fairness.md`](scheduler/placement-algorithms-and-fairness.md) | Sparrow, Firmament/Quincy, Apollo, Hydra, Gödel, Medea, TetriSched, POP, Sia, Gavel, Pollux, Themis, Tiresias, 3Sigma, Decima, Morpheus, DRF, Tetris, Kueue, Volcano |
| [`agentic-workloads.md`](scheduler/agentic-workloads.md) | Agent workload traces (Copilot, Claude Code, Bailian, M365, TraceLab): session shape, burstiness, idle time; LLM and agent serving (Autellix, Parrot, Llumnix, SAGA, SMetric, Continuum); provider rate limits and subscription windows; gateways; KV- and context-aware placement; model and effort routing; GPU exposure per runtime class |
| [`proximity-and-failure-containment.md`](scheduler/proximity-and-failure-containment.md) | Vivaldi coordinates (Dabek, Ledlie, Serf/Consul, hyper-swim), SWIM, Lifeguard, φ-accrual and hyper-raft's NFD-E detectors, gray failure and fail-slow (Panorama, IASO, Perseus, OmegaGen), containment (CPI², crash backoff, outlier ejection, shuffle sharding, retry budgets, metastable failures) |
| [`comparison.md`](scheduler/comparison.md) | How each system places, scales, prioritises, handles mixed hardware and contains failures, with the measured results each reports — every cell cited to a note |
