# Competitive baseline: results

The baseline goal is Hex against named competitors on every axis, with Hex's ratio to the best competitor.
This file records the runs of record. **The baseline is incomplete:** the sandbox platforms are being
measured now, and the hosted platforms are not measured (see the end).

Ratios are the best competitor's value over Hex's for lower-is-better axes, and Hex's over the best
competitor's for higher-is-better ones. Above 1 means Hex is ahead; the research bar is 4-5x.

## Run of record (2026-10-23, fedora)

- **Host:** fedora.attlocal.net, Intel Core i9-14900HX (32 CPUs, the bench pinned to CPUs 4-31), 62 GiB,
  kernel 6.19.10-300.fc44.x86_64. Everything is in one run directory on fedora,
  `~/.cache/hex-bench/out/20261023T025023Z/`, written by `bench/competitive/host.sh` one pass at a time.
- **Passes:**
  - runc, crun and youki: 02:50-05:35Z (run 1).
  - Hex `oci` and gVisor runsc: 06:50-07:20Z (run 2).
  - Process boxes: 07:25-07:45Z (run 2).
  - Each pass started only at a 1-minute load under 2 (CLAUDE.md §7). During passes it rose to 3-8 from
    other people's work on fedora, not confined to CPUs 0-3. **The tail figures (p99.9, max) carry that
    noise.** Within a pass, runtimes ran in shuffled order across rounds.
- **Hex:** release build of `cp/t7-runtime` with ed5758d's four fixes (described below), built on fedora
  after all seven gates passed (binary sha256 523b88e2bffaa473…). Run 1's measurements of the faulty
  binary are kept on fedora as `hex.faulty.r*.json` and `box.faulty.r*.json`.
- **Competitors:**
  - runc v1.5.2 (sha256-verified), crun 1.30.1 (static release binary), youki v0.7.0 (from the release
    tarball).
  - gVisor runsc release-20261019.0 (sha512-verified tarball, with its `gvisor-bin/` Sentry).
  - systemd's transient scopes and services (`systemd-run --user`).
- **Parameters:**
  - Two rounds, best of 2 per metric. n = 10,000 per round after 100 warm-ups; runsc n = 2,000.
  - 1,000 lifecycles per concurrency level.
  - Density capped at 1,000 containers or 8 GiB (process boxes: 4 GiB).

### OCI class

| Axis | Hex | runc | crun | youki | runsc | Hex vs best |
|---|---|---|---|---|---|---|
| create p50 (ms) | 8.04 | 27.54 | 2.99 | 21.94 | 19.01 | 0.37x vs crun |
| create p99 (ms) | 12.06 | 69.11 | 6.87 | 36.42 | 28.69 | 0.57x vs crun |
| create p99.9 (ms) | 23.12 | 141.71 | 8.98 | 126.16 | 35.40 | 0.39x vs crun |
| create max (ms) | 64.02 | 228.07 | 19.97 | 220.71 | 40.06 | 0.31x vs crun |
| start p50 (ms) | 6.35 | 3.53 | 0.51 | 5.31 | 8.89 | 0.08x vs crun |
| start p99 (ms) | 10.08 | 6.34 | 1.14 | 8.39 | 13.93 | 0.11x vs crun |
| start p99.9 (ms) | 21.40 | 10.84 | 1.43 | 10.87 | 18.18 | 0.07x vs crun |
| start max (ms) | 26.73 | 36.58 | 1.91 | 16.80 | 19.68 | 0.07x vs crun |
| exec p50 / p99 / p99.9 / max (ms) | n/a: Hex has no exec yet | 8.42 / 15.01 / 36.58 / 129.45 | 1.64 / 3.50 / 4.28 / 73.99 | 6.78 / 31.10 / 82.67 / 178.16 | 6.12 / 11.42 / 13.24 / 14.48 | n/a |
| delete p50 / p99 / p99.9 / max (ms) | 0.80 / 2.11 / 2.67 / 4.83 | 104.48 / 107.67 / 117.70 / 244.60 | 10.76 / 11.84 / 22.94 / 32.11 | 25.58 / 28.55 / 33.33 / 47.77 | 5.19 / 9.03 / 105.63 / 106.01 | **not comparable** (below) |
| create+start of `true` p50 (ms) | 14.59 | 30.83 | 3.34 | 20.08 | 27.99 | 0.23x vs crun |
| create+start of `true` p99 (ms) | 26.32 | 65.20 | 6.00 | 40.87 | 38.91 | 0.23x vs crun |
| create+start of `true` p99.9 (ms) | 36.09 | 129.01 | 8.88 | 124.15 | 52.43 | 0.25x vs crun |
| create+start of `true` max (ms) | 63.85 | 759.89 | 28.02 | 223.19 | 55.23 | 0.44x vs crun |
| throughput, concurrency 1 (lifecycles/s) | 65 | 7 | 87 | 38 | 31 | 0.75x vs crun |
| throughput, concurrency 8 (lifecycles/s) | 353 | 59 | 1,015 | 398 | 230 | 0.35x vs crun |
| throughput, concurrency 32 (lifecycles/s) | 1,187 | 214 | 1,038 | 732 | 346 | 1.14x vs crun |
| CPU per lifecycle, concurrency 1 (whole bench cgroup) | 5.48 ms | 23.09 ms | 4.68 ms | 18.43 ms | 52.90 ms | 0.86x vs crun |
| memory per idle container | 0.54 MiB | 0.40 MiB | 0.37 MiB | 0.37 MiB | 12.43 MiB | 0.68x vs crun |
| density (idle containers per GiB) | 1,899 | 2,536 | 2,774 | 2,771 | 82 | 0.68x vs crun |

### Process class

| Axis | Hex `box run` | systemd scope | systemd service | Hex vs best |
|---|---|---|---|---|
| run of `true` p50 (ms) | 2.73 | 7.13 | 12.11 | 2.61x vs scope |
| run of `true` p99 (ms) | 6.06 | 9.16 | 35.49 | 1.51x vs scope |
| run of `true` p99.9 (ms) | 13.90 | 13.15 | 43.11 | 0.95x vs scope |
| run of `true` max (ms) | 28.30 | 34.54 | 50.55 | 1.22x vs scope |
| throughput, concurrency 1 (runs/s) | 314 | 145 | 76 | 2.17x vs scope |
| throughput, concurrency 8 (runs/s) | 759 | 195 | 143 | 3.89x vs scope |
| throughput, concurrency 32 (runs/s) | 783 | 160 | 192 | 4.08x vs service |
| CPU per run (the call, plus systemd's manager and bus) | 2.51 ms | 7.81 ms | 13.92 ms | 3.11x vs scope |
| memory per idle box | 0.70 MiB | 1.12 MiB | 0.42 MiB | 0.60x vs service |
| density (idle boxes per GiB) | 1,458 | 916 | 2,424 | 0.60x vs service |

## What the run shows

- **OCI class: Hex misses the bar, and crun is the competitor to beat.**
  - crun is 3-9x faster than runc and youki on create and start.
  - Hex is behind crun on create (0.37x), start (0.08x), create+start (0.23x), CPU (0.86x) and memory
    and density (0.68x).
  - Hex leads only on throughput at concurrency 32 (1.14x).
  - Reaching the 4x floor would mean Hex getting faster by a factor of 11 on create (4 × 8.04 / 2.99),
    50 on start and 17 on create+start.
  - Hex's own AC-T7.1.3 bar (start p50 under 50 ms) is met.
- **Hex's `start` is the largest gap: 6.35 ms against crun's 0.51 ms.** The container side is already the
  cheap shape: it signals ready, blocks on the exec FIFO, writes one byte and execs. The runtime side
  writes its state durably on every operation (temp file, fsync, rename, directory fsync). That is the
  leading suspect, to be confirmed by profiling before the runtime design (goal 4) changes it.
- **Process class: Hex is 2-4x ahead of systemd on latency, throughput and CPU**, and reaches the 4x bar
  only on throughput at concurrency 32. Its p99.9 is level with systemd scopes. Each idle box keeps its
  `hex box run` wrapper process alive, which a systemd service doesn't, so Hex is behind on memory
  (0.60x).
- **gVisor runsc** costs its user-space kernel on every axis: 12.4 MiB and 52.9 ms of CPU per container.

## Hex's oci class makes no per-container cgroup, so its create and delete ratios are not like for like

Hex's `oci` class parses `linux.cgroupsPath` but never uses it, and doesn't implement `linux.resources`
(`crates/hex-runtime/src/linux/oci/`). Its containers get no cgroup of their own and no resource
limits. runc, crun and youki each make a cgroup per container by default. Their `create` includes
making it, and their `delete --force` includes waiting for it to empty and removing it, which is most
of crun's 10.76 ms. So:

- Hex's delete (0.80 ms, now waiting for the container to exit) skips work the others do. It is not a
  lead.
- Hex's create is slower than crun's while doing less, so the true create gap is wider than 0.37x.
- It is a correctness gap, not only a benchmark caveat. An `oci` container with no bound breaks
  CLAUDE.md §2 ("every resource has a bound"). The runtime design must give each `oci` container its own
  cgroup, with `linux.resources` in force, before any `oci`-class ratio counts.

## Faults the bench found, and their fixes

- **In Hex (ed5758d):**
  1. `create` raced: the shim's pid message and the container's ready message come from two processes
     on one pipe, in either order, and the reader accepted only one.
  2. A failed `create` left its container and shim running.
  3. `delete --force` returned without waiting for the container to exit. Run 1 showed it 16x
     faster than crun's.
  4. A box's memory bound didn't cover swap. The gates caught this on fedora's zram: a 512 MiB hog
     finished in a 64 MiB box. Recorded in `docs/design/runtime.md`.

  Each fix has tests, and all seven gates passed on fedora.
- **In the bench:**
  - `hex box run` needs a fresh cgroup per run (66e985a).
  - gVisor's 2026 releases run the Sentry from `gvisor-bin/` beside `runsc` (66e985a).
  - Container ids must be fixed width: runsc refused an ambiguous prefix under concurrency 32 (50279b1).
    Its first pass is kept as `runsc.idclash.r*.json`.
  - A density phase that stops early mustn't produce a negative memory figure (912ef34).

## Not yet measured

- **Self-hosted platforms, running now:** Daytona, then Docker, E2B (approved by Ada on fedora, host
  changes included), Kubernetes (kind) with the Kubernetes Agent Sandbox, Agent Substrate and Google AX.
- **Hosted-only platforms** (E2B Cloud, Daytona Cloud, Modal, Morph, CodeSandbox, Fly Machines, Vercel,
  Cloudflare): no accounts or API keys exist for them, so they are not measured.
