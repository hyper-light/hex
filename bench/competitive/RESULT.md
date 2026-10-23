# Competitive baseline: results

The baseline goal is Hex against named competitors on every axis, with Hex's ratio to the best competitor.
This file records each run of record. **The baseline is incomplete:** see "Not yet measured" below.

Ratios are the best competitor's value over Hex's for lower-is-better axes, and Hex's over the best
competitor's for higher-is-better ones. Above 1 means Hex is ahead; the research bar is 4-5x.

## Run 1: OCI runtimes and process boxes (2026-10-23)

- **Host:** fedora.attlocal.net, Intel Core i9-14900HX (32 CPUs, the bench pinned to CPUs 4-31), 62 GiB,
  kernel 6.19.10-300.fc44.x86_64. The run directory on fedora is
  `~/.cache/hex-bench/out/20261023T025023Z/`.
- **When:** 02:50:23Z to 05:35:17Z.
- **Load:** CLAUDE.md §7 asks for a load average under 2. It was 1.86 at the start (5- and 15-minute: 3.25,
  3.46). Mid-run it rose to 3.7-7.8 from other people's work (a `clippy-driver` build and KDE's Baloo
  indexer, not confined to CPUs 0-3), and it ended at 3.75. **The tail figures (p99.9, max) carry that
  noise**, and the runtimes were measured in shuffled order across two rounds so it isn't biased toward
  one of them.
- **Hex:** a release build of `cp/t7-runtime` at fb15d9d (binary sha256 a2558d2e13d906fe…). Its OCI
  runtime has a create race, fixed in a later commit (see "Hex's results are affected" below).
- **Competitors:** runc v1.5.2 (sha256-verified), crun 1.30.1 (static release binary), youki v0.7.0
  (from the release tarball). runsc's probe failed and it was not measured (see below).
- **Parameters:** 2 rounds, best of 2 per metric; n = 10,000 per round after 100 warm-ups; 1,000
  lifecycles per concurrency level; density capped at 1,000 containers or 8 GiB (process boxes: 4 GiB).

### OCI class

| Axis | Hex | runc | crun | youki | Hex vs best |
|---|---|---|---|---|---|
| create p50 (ms) | 8.41 | 27.54 | 2.99 | 21.94 | 0.36x vs crun |
| create p99 (ms) | 11.19 | 69.11 | 6.87 | 36.42 | 0.61x vs crun |
| create p99.9 (ms) | 16.04 | 141.71 | 8.98 | 126.16 | 0.56x vs crun |
| create max (ms) | 39.43 | 228.07 | 19.97 | 220.71 | 0.51x vs crun |
| start p50 (ms) | 6.55 | 3.53 | 0.51 | 5.31 | 0.08x vs crun |
| start p99 (ms) | 9.18 | 6.34 | 1.14 | 8.39 | 0.12x vs crun |
| start p99.9 (ms) | 12.31 | 10.84 | 1.43 | 10.87 | 0.12x vs crun |
| start max (ms) | 36.17 | 36.58 | 1.91 | 16.80 | 0.05x vs crun |
| exec p50 / p99 / p99.9 / max (ms) | n/a: Hex has no exec yet | 8.42 / 15.01 / 36.58 / 129.45 | 1.64 / 3.50 / 4.28 / 73.99 | 6.78 / 31.10 / 82.67 / 178.16 | n/a |
| delete p50 (ms) | 0.66 | 104.48 | 10.76 | 25.58 | 16.29x vs crun, **unverified** |
| delete p99 (ms) | 0.92 | 107.67 | 11.84 | 28.55 | 12.88x vs crun, **unverified** |
| delete p99.9 (ms) | 1.15 | 117.70 | 22.94 | 33.33 | 20.04x vs crun, **unverified** |
| delete max (ms) | 1.38 | 244.60 | 32.11 | 47.77 | 23.22x vs crun, **unverified** |
| create+start of `true` p50 (ms) | 14.72 | 30.83 | 3.34 | 20.08 | 0.23x vs crun |
| create+start of `true` p99 (ms) | 26.27 | 65.20 | 6.00 | 40.87 | 0.23x vs crun |
| create+start of `true` p99.9 (ms) | 40.07 | 129.01 | 8.88 | 124.15 | 0.22x vs crun |
| create+start of `true` max (ms) | 223.75 | 759.89 | 28.02 | 223.19 | 0.13x vs crun |
| throughput, concurrency 1 / 8 / 32 (lifecycles/s) | failed (create race) | 7 / 59 / 214 | 87 / 1,015 / 1,038 | 38 / 398 / 732 | n/a |
| CPU per lifecycle, concurrency 1 | failed (create race) | 23.09 ms | 4.68 ms | 18.43 ms | n/a |
| memory per idle container | 0.54 MiB | 0.40 MiB | 0.37 MiB | 0.37 MiB | 0.68x vs crun |
| density (idle containers per GiB) | 1,895 | 2,536 | 2,774 | 2,771 | 0.68x vs crun |

### Process class

| Axis | Hex `box run` | systemd scope | systemd service | Hex vs best |
|---|---|---|---|---|
| box run of `true` p50 / p99 / p99.9 / max (ms) | failed (harness, see below) | 6.56 / 8.22 / 10.14 / 27.94 | 11.79 / 36.75 / 43.67 / 49.48 | n/a |
| throughput, concurrency 1 / 8 / 32 (runs/s) | failed (harness) | 156 / 239 / 187 | 84 / 147 / 202 | n/a |
| CPU per run (the call, plus systemd's manager and bus) | failed (harness) | 7.08 ms | 13.65 ms | n/a |
| memory per idle box | 0.72 MiB | 1.17 MiB | 0.44 MiB | 0.62x vs systemd service |
| density (idle boxes per GiB) | 1,429 | 878 | 2,308 | 0.62x vs systemd service |

### What this run shows

- **Hex is behind crun on the start path.** Its `create` takes 2.8x crun's time, its `start` 12.8x, and
  create+start of a `true` container 4.4x. Hex's own AC-T7.1.3 bar (start p50 under 50 ms) is met,
  but the research bar is missed. Reaching even the 4x floor means Hex getting faster by a factor of 11
  on create (4 × 8.41 / 2.99), 51 on start and 18 on create+start. crun is the competitor to beat: 3-9x faster than runc and youki on create and start.
- **Hex's `start` is the largest gap: 6.55 ms against crun's 0.51 ms.** `start` only opens the exec FIFO
  that the container blocks on, so the time is the container's own wake-up and setup after the FIFO
  opens. This is the first thing the runtime design (goal 4) must fix.
- **Memory and density:** Hex uses 0.54 MiB per idle container against crun's 0.37, mostly its shim
  process (one per container; crun leaves none).
- **Hex's delete is unverified.** It measures 0.66 ms against crun's 10.76 ms, but crun's `delete --force`
  kills the container and waits for it to exit. Hex's figure must be checked against the same semantics
  before it counts as a lead.

### Hex's results are affected by two faults

1. **Hex's OCI create race** (a Hex bug). Under load, `create` failed with "ready before the container
   pid". The shim's pid message and the container's ready message come from two processes on one pipe,
   in either order, and the reader only accepted one order. A failed create also left its container and
   shim running. It broke Hex's throughput and CPU phases in both rounds, its create+start phase in round
   1, and its lifecycle phase in round 2. The numbers above are from the phases that completed. The fix
   (an order-independent reader, cleanup on failure, unit tests for every message order) is being
   gated on fedora, and Hex will be measured again with it.
2. **The process-box harness** (a bench bug). `hex box run` takes over the cgroup it starts in and parents
   its box there, after which cgroup v2 admits no new process to it. The harness reused one cgroup per
   worker, so every Hex run after the first exited 127. Fixed in 66e985a (a fresh cgroup per run); the
   density phase already used one per box, which is why its numbers stand.

### Not yet measured

- **runsc:** gVisor's 2026 releases run the Sentry from `gvisor-bin/` beside `runsc`, and the first
  fetch installed `runsc` alone. Fixed in 66e985a; runsc runs next.
- **Self-hosted platforms:** Daytona, E2B (approved by Ada on fedora, host changes included), Docker,
  Kubernetes (kind) with the Kubernetes Agent Sandbox, Agent Substrate and Google AX.
- **Hosted-only platforms** (E2B Cloud, Daytona Cloud, Modal, Morph, CodeSandbox, Fly Machines, Vercel,
  Cloudflare): no accounts or API keys exist for them, so they are not measured.
