# Competitive baseline: results

The baseline goal is Hex against named competitors on every axis, with Hex's ratio to the best competitor.
This file records the runs of record. **The baseline is incomplete:** the sandbox platforms and the
other open-source competitors Ada asked for are being measured now. The hosted-only platforms are not
measured: Ada chose open-source alternatives in their place (see the end).

Ratios are the best competitor's value over Hex's for lower-is-better axes, and Hex's over the best
competitor's for higher-is-better ones. Above 1 means Hex is ahead; the research bar is 4-5x.

## Run of record (2026-10-23 and 24, fedora)

- **Host:** fedora.attlocal.net, Intel Core i9-14900HX (32 CPUs, the bench pinned to CPUs 4-31), 62 GiB,
  kernel 6.19.10-300.fc44.x86_64. Everything is in one run directory on fedora,
  `~/.cache/hex-bench/out/20261023T025023Z/`, written by `bench/competitive/host.sh` one pass at a time.
- **Passes:**
  - runc, crun and youki: 02:50-05:35Z (run 1).
  - gVisor runsc: 06:50-07:20Z (run 2).
  - Hex `oci`, with a cgroup per container that it is born in: 2026-10-24 03:35-03:51Z, start load
    1.32 (rounds 1-2, binary `hex-cg2-2f0c122c`, below). Hex's earlier rounds are kept on fedora:
    `hex.nocgroup.r*` (no cgroup per container, 2026-10-23 06:50Z) and `hex.cgmigrate.r*` (a cgroup
    per container, joined by migration, 2026-10-23 23:01Z).
  - Process boxes: 07:25-07:45Z (run 2).
  - Density with held memory (below), on the same binaries (`FETCH=0`, their sums checked against run 2's):
    runc, crun, youki and runsc at 17:22-17:34Z (rounds 3-4, start load 0.85), and the process boxes at
    18:52-18:58Z (rounds 5-6, start load 1.04). Hex's own rounds measure held memory too.
  - Each pass started only at a 1-minute load under 2 (CLAUDE.md §7). During passes it rose to 3-8 from
    other people's work on fedora, not confined to CPUs 0-3. **The tail figures (p99.9, max) carry that
    noise.** Within a pass, runtimes ran in shuffled order across rounds.
- **Memory, in every table, is what the cgroup holds, page cache aside** (`cgmem.py`): `memory.current`
  less file cache, keeping shmem, plus swap. The kernel fills and reclaims page cache on its own schedule.
  On this shared host a Kubernetes stack's `memory.current` read anywhere from 1.7 to 7.0 GiB in one
  afternoon, while its held memory read 1.31 GiB in each of three tries. Rounds 1-2 measured only
  `memory.current`, so the memory and density rows come from the density rounds. Best of N takes each
  axis from the rounds that measured it.
- **Hex:**
  - `oci`: release build of `cp/t7-runtime` at 05af083, built on fedora after all seven gates passed
    (`hex-cg2-2f0c122c`, sha256 2f0c122c9ddd19e2…). Its containers each get a cgroup with
    `linux.resources` and the device list in force (3319a85), and are born in it (05af083).
  - Process boxes: the release build with ed5758d's four fixes (described below; binary sha256
    523b88e2bffaa473…). Run 1's measurements of the faulty binary are kept on fedora as
    `hex.faulty.r*.json` and `box.faulty.r*.json`.
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
| create p50 (ms) | 9.79 | 27.54 | 2.99 | 21.94 | 19.01 | 0.31x vs crun |
| create p99 (ms) | 15.12 | 69.11 | 6.87 | 36.42 | 28.69 | 0.45x vs crun |
| create p99.9 (ms) | 31.72 | 141.71 | 8.98 | 126.16 | 35.40 | 0.28x vs crun |
| create max (ms) | 167.89 | 228.07 | 19.97 | 220.71 | 40.06 | 0.12x vs crun |
| start p50 (ms) | 7.24 | 3.53 | 0.51 | 5.31 | 8.89 | 0.07x vs crun |
| start p99 (ms) | 10.36 | 6.34 | 1.14 | 8.39 | 13.93 | 0.11x vs crun |
| start p99.9 (ms) | 19.97 | 10.84 | 1.43 | 10.87 | 18.18 | 0.07x vs crun |
| start max (ms) | 69.67 | 36.58 | 1.91 | 16.80 | 19.68 | 0.03x vs crun |
| exec p50 / p99 / p99.9 / max (ms) | n/a: Hex has no exec yet | 8.42 / 15.01 / 36.58 / 129.45 | 1.64 / 3.50 / 4.28 / 73.99 | 6.78 / 31.10 / 82.67 / 178.16 | 6.12 / 11.42 / 13.24 / 14.48 | n/a |
| delete p50 (ms) | 1.28 | 104.48 | 10.76 | 25.58 | 5.19 | 4.07x vs runsc |
| delete p99 (ms) | 3.12 | 107.67 | 11.84 | 28.55 | 9.03 | 2.89x vs runsc |
| delete p99.9 (ms) | 12.06 | 117.70 | 22.94 | 33.33 | 105.63 | 1.90x vs crun |
| delete max (ms) | 23.00 | 244.60 | 32.11 | 47.77 | 106.01 | 1.40x vs crun |
| create+start of `true` p50 (ms) | 16.48 | 30.83 | 3.34 | 20.08 | 27.99 | 0.20x vs crun |
| create+start of `true` p99 (ms) | 21.93 | 65.20 | 6.00 | 40.87 | 38.91 | 0.27x vs crun |
| create+start of `true` p99.9 (ms) | 50.72 | 129.01 | 8.88 | 124.15 | 52.43 | 0.18x vs crun |
| create+start of `true` max (ms) | 1,208.44 | 759.89 | 28.02 | 223.19 | 55.23 | 0.02x vs crun |
| throughput, concurrency 1 (lifecycles/s) | 54 | 7 | 87 | 38 | 31 | 0.62x vs crun |
| throughput, concurrency 8 (lifecycles/s) | 308 | 59 | 1,015 | 398 | 230 | 0.30x vs crun |
| throughput, concurrency 32 (lifecycles/s) | 1,029 | 214 | 1,038 | 732 | 346 | 0.99x vs crun |
| CPU per lifecycle, concurrency 1 (whole bench cgroup) | 8.62 ms | 23.09 ms | 4.68 ms | 18.43 ms | 52.90 ms | 0.54x vs crun |
| memory held per idle container | 0.63 MiB | 0.41 MiB | 0.39 MiB | 0.39 MiB | 12.49 MiB | 0.62x vs crun |
| density (idle containers per GiB held) | 1,633 | 2,514 | 2,640 | 2,633 | 82 | 0.62x vs crun |

Hex's create+start max (1,208 ms) is a few stalls in round 2: its next-slowest samples are 240, 132
and 86 ms, and round 1's max is 67 ms.

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
| memory held per idle box | 0.71 MiB | 1.11 MiB (see below) | 0.36 MiB | 0.50x vs service |
| density (idle boxes per GiB held) | 1,434 | 919 | 2,884 | 0.50x vs service |

systemd-scope's figure counts the bench driver's cgroup, which forks each scope box. Watching only the
scope gave 0.01 MiB per box, and watching the driver too gave 1.11 MiB: far more than the task and kernel
stack a fork leaves behind (cgroup-v2.rst, "Memory Ownership"). `probes/scope_charge.py` will name the
charge by kind before this cell is final. It doesn't move the ratio, which is against systemd services.

## What the run shows

- **OCI class: Hex misses the bar, and crun is the competitor to beat.**
  - crun is 3-9x faster than runc and youki on create and start.
  - Hex is behind crun on create (0.31x), start (0.07x), create+start (0.20x), CPU (0.54x) and held
    memory and density (0.62x). It is level at concurrency 32 (0.99x).
  - Hex's delete is the fastest of all (1.28 ms p50, 8.4x crun's), now that it removes a real cgroup
    as the others do.
  - Reaching the 4x floor would mean Hex getting faster by a factor of 13 on create (4 × 9.79 / 2.99),
    57 on start and 20 on create+start.
  - Hex's own AC-T7.1.3 bar (start p50 under 50 ms) is met.
- **Hex's `start` is the largest gap: 7.24 ms against crun's 0.51 ms.** The container side is already
  the cheap shape: it signals ready, blocks on the exec FIFO, writes one byte and execs. The runtime
  side writes its state durably on every operation (temp file, fsync, rename, directory fsync). That
  is the leading suspect, to be confirmed by profiling before the runtime design (goal 4) changes it.
- **Process class: Hex is 2-4x ahead of systemd on latency, throughput and CPU**, and reaches the 4x bar
  only on throughput at concurrency 32. Its p99.9 is level with systemd scopes. Each idle box keeps its
  `hex box run` wrapper process alive, which a systemd service doesn't, so Hex is behind on memory
  (0.50x: 0.71 MiB held per box against a service's 0.36).
- **gVisor runsc** costs its user-space kernel on every axis: 12.5 MiB held and 52.9 ms of CPU per
  container.

## A cgroup per container, and what joining one cost

The bench found that Hex's `oci` class made no cgroup per container. It parsed `linux.cgroupsPath`
but never used it, and ignored `linux.resources`, so its containers had no bound (CLAUDE.md §2) and
its delete skipped work every other runtime does. Since 3319a85 each container gets a cgroup with
its resources and device list in force (`docs/design/runtime.md`), and Hex's delete is comparable.

The first version joined the container to its cgroup by migration (a write to `cgroup.procs`), and
create's p50 rose from 8.04 ms to 25.14 ms. A migration waits out a full RCU grace period when it
comes after a quiet spell: `probes/join_cost.py` measured 0.1 ms within 5 ms of the last migration
and about 10 ms (p90 14-19 ms) after 20 ms or more. runc and youki join the same way (27.5 and 21.9 ms
creates); crun clones its init straight into the cgroup (3.0 ms). Since 05af083 Hex does too, with
`clone3(CLONE_INTO_CGROUP)`, and create is 9.79 ms: about 2 ms over having no cgroup at all.

## Faults the bench found, and their fixes

- **In Hex:** each `oci` container got a cgroup with its resources and devices in force (3319a85), and
  is born in it (05af083), as described above. Before those, **ed5758d**:
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
  - Memory was `memory.current`, which counts page cache the kernel reclaims at will. Now it's what the
    cgroup holds, page cache aside, everywhere (79137b8).
  - The box bench watched the whole user manager for systemd's density, so other people's user units
    were in it. Now it watches the boxes' own slice, the manager and the bus (79137b8), plus the driver's
    cgroup for every method, since forks are charged there (ec1d841). That slice is stopped when done
    (84f7321).
  - Re-running only some phases needs `ROUND_FIRST`, `HEXBENCH_PHASES` and `FETCH=0`. Without `FETCH=0`,
    `fetch.py` would resolve each competitor's latest release again (79137b8, fff1f6d).
  - hex_start hung on fedora because the tree there predated 4c76c13, the harness fix for exactly that
    hang (a detached create's init holds the caller's pipes).
  - The platform harness's own faults are listed with the platform results.

## Sandbox platforms, so far

Each platform end to end through its own API, against Hex `oci` (create+start for "create until it
runs a command"). Each round is refereed for other people's load (`platform-rounds/`); CPU and memory
are the stack's own cgroups'. Docker's earlier rounds predate the referee and the stack-cgroup method,
so they're kept on fedora as `unrefereed.*` and its column waits for its refereed rounds.

| Axis | Hex | agentsandbox | agentsandbox-warm | daytona | k8s | Hex vs best |
|---|---|---|---|---|---|---|
| create until it runs a command p50 (ms) | 16.96 | 434.86 | 48.96 | 407.92 | 408.92 | 2.89x vs agentsandbox-warm |
| create until it runs a command p99 (ms) | 24.36 | 544.33 | 465.19 | 488.21 | 501.13 | 19.09x vs agentsandbox-warm |
| create until it runs a command p99.9 (ms) | 47.26 | 689.10 | 763.29 | 502.48 | 559.45 | 10.63x vs daytona |
| create until it runs a command max (ms) | 172.87 | 689.10 | 763.29 | 502.48 | 559.45 | 2.91x vs daytona |
| exec of `true` p50 / p99 / p99.9 / max (ms) | n/a: Hex has no exec yet | 42.43 / 54.68 / 97.31 / 97.31 | 32.82 / 90.60 / 125.57 / 125.57 | 12.54 / 25.11 / 27.95 / 27.95 | 41.63 / 53.13 / 60.44 / 60.44 | n/a |
| delete p50 (ms) | 1.28 | 30.54 | 43.16 | 237.01 | 27.32 | 21.42x vs k8s |
| delete p99 (ms) | 3.12 | 61.91 | 155.86 | 293.34 | 47.87 | 15.33x vs k8s |
| delete p99.9 (ms) | 12.06 | 87.34 | 176.57 | 350.24 | 79.13 | 6.56x vs k8s |
| delete max (ms) | 23.00 | 87.34 | 176.57 | 350.24 | 79.13 | 3.44x vs k8s |
| lifecycles/s at concurrency 1 | 54 | 2 | 6 | 2 | 2 | 9.48x vs agentsandbox-warm |
| lifecycles/s at concurrency 8 | 308 | 6 | 6 | 7 | 6 | 42.39x vs daytona |
| lifecycles/s at concurrency 32 | 1,029 | 6 | 6 | 8 | 7 | 124.53x vs daytona |
| CPU per lifecycle at concurrency 1 (platforms: what a lifecycle adds to their stack's CPU) | 8.62 ms | 881.71 ms | 535.41 ms | 511.92 ms | 806.63 ms | 59.42x vs daytona |
| memory held per idle sandbox | 0.63 MiB | 4.94 MiB | 5.18 MiB | 30.86 MiB | 7.74 MiB | 7.87x vs agentsandbox |
| density (idle sandboxes per GiB held) | 1,633 | 207 | 198 | 33 | 132 | 7.87x vs agentsandbox |
| the stack's own CPU at idle (Hex `oci` runs no daemon) | n/a | 0.136 CPUs | 0.192 CPUs | 0.046 CPUs | 0.120 CPUs | n/a |
| the stack's own memory held at idle (Hex `oci` runs no daemon) | n/a | 1,305.79 MiB | 2,096.97 MiB | 1,228.50 MiB | 1,307.31 MiB | n/a |

Where density stopped: Daytona at the cap of 200 in each of four rounds (its held memory from round 4,
with no foreign load in 18 samples; round 3's 94.7% foreign share is recorded, as a density-only round
is never retried). Kubernetes pods and agentsandbox stopped at 99-100, each refused there by the
cluster ("not ready in 300s"), and the warm pool at its cap of 30.

agentsandbox is kubernetes-sigs/agent-sandbox, a Sandbox per lifecycle; agentsandbox-warm claims one
from a warm pool of 32. k8s is a Pod through the Kubernetes API on kind.

Faults the platform harness had, each fixed before the rounds above:
- CPU and memory were host-wide, so they charged a platform with whatever else fedora ran. They are now
  the stack's own cgroups, less the stack's idle CPU, plus the client's rusage; kernel threads in the
  root cgroup don't count as outside the stack.
- Daytona's SDK has no blocking delete, so delete is timed until the sandbox's state says it's gone.
- The Kubernetes client raced when one exec client was shared across threads (now one per thread), and a
  pod that timed out coming up leaked (now removed).
- One failing lifecycle lost a whole throughput phase; each concurrency level now keeps its result.
- The referee's start gate took the stack's idle load from the load average, which remembers everyone's
  last minutes; it now reads the stack's cgroups (`platform-rounds/`).

## E2B, self-hosted, against Hex on adacachy (not of record)

fedora couldn't reserve the 4 GiB of contiguous hugepages E2B's host setup needs: other people's units held
most of its memory, and compaction and a cache drop freed 24 of 2,048 pages. So by Ada's decision
(2026-10-24), E2B ran on adacachy, and Hex's `oci` rounds ran there too, for a same-hardware ratio. adacachy
runs production at a 1-minute load of 60-113, so **these rows are not of record by CLAUDE.md §7**, and are
unrefereed. Hex's tail rows carry that load most: 10,000 samples per round against E2B's 300.

- **E2B:** e2b-dev/runtime at bca4b33, `embed/compose`, unchanged, with its client e2b 2.46.0. It ran inside
  one privileged container with its own Docker daemon (`e2b-dind/`). adacachy's UFW drops the hop from each
  sandbox to its orchestrator's egress proxy on the host side of the sandbox's veth, and in the container
  that hop stays inside it, so the host's firewall was never changed. Two settings for the bench: 9,216 2
  MiB hugepages (18 GiB, enough for 32 concurrent creates of a 512 MiB sandbox), and its seeded tier's
  limit of 20 concurrent sandboxes raised to 1,000, as Daytona's admin quotas were. Each round's density
  stopped at 35 sandboxes, the hugepage pool: each idle sandbox maps its whole 512 MiB guest from it.
- **Hex:** `hex-adacachy-d2c8636a`, built on adacachy from the same source as fedora's `hex-cg2` (05af083's
  crates), 2026-10-24 05:57-06:46Z.
- **Memory counts hugepages.** E2B's guest memory is hugepages, which `memory.current` leaves out unless
  the hierarchy is mounted with `memory_hugetlb_accounting`. Held memory now adds each cgroup's
  `hugetlb.*.current` (`cgmem.py`). A first round that left them out (6.7 MiB per sandbox), and whose
  density stopped at the tier limit of 20, is kept apart as `limit20.platform.e2b.r1.json`.

| Axis | Hex | E2B | Hex vs E2B |
|---|---|---|---|
| create until it runs a command p50 (ms) | 43.12 | 61.83 | 1.43x |
| create until it runs a command p99 (ms) | 258.44 | 111.36 | 0.43x |
| create until it runs a command p99.9 (ms) | 611.60 | 114.43 | 0.19x |
| create until it runs a command max (ms) | 1,945.13 | 114.43 | 0.06x |
| exec of `true` p50 / p99 / p99.9 / max (ms) | n/a: Hex has no exec yet | 6.70 / 20.58 / 23.39 / 23.39 | n/a |
| delete p50 (ms) | 0.83 | 4.63 | 5.57x |
| delete p99 (ms) | 42.16 | 13.88 | 0.33x |
| delete p99.9 (ms) | 144.59 | 19.35 | 0.13x |
| delete max (ms) | 1,131.83 | 19.35 | 0.02x |
| lifecycles/s at concurrency 1 | 14 | 12 | 1.18x |
| lifecycles/s at concurrency 8 | 165 | 21 | 7.71x |
| lifecycles/s at concurrency 32 | 154 | 25 | 6.28x |
| CPU per lifecycle at concurrency 1 (E2B: what a lifecycle adds to its stack's CPU) | 8.47 ms | 260.15 ms | 30.73x |
| memory held per idle sandbox | 0.66 MiB | 674.53 MiB | 1,024.50x |
| density (idle sandboxes per GiB held) | 1,555 | 2 | 1,024.50x |
| the stack's own CPU at idle (Hex `oci` runs no daemon) | n/a | 0.101 CPUs | n/a |
| the stack's own memory held at idle (Hex `oci` runs no daemon) | n/a | 2,686.19 MiB | n/a |

E2B's create restores a sandbox from its template's memory snapshot rather than booting one, and its p50
(62 ms) is that restore. When the rounds were done, everything of E2B's was removed from adacachy: the
container and its volumes, the hugepages (back to 0) and the nbd module.

## Not yet measured

- **Platforms still to run, self-hosted:** Agent Substrate and Google AX, and Docker, refereed (its round
  is waiting at the referee's gate on fedora).
- **The other open-source competitors Ada asked for (2026-10-23, "do all"),** each with its harness
  written and in trials on fedora:
  - process sandboxes against `hex box run` (`sandbox_bench.py`): bubblewrap, nsjail, firejail,
    minijail, systemd-nspawn, Anthropic's sandbox-runtime and Codex's sandbox;
  - microVM monitors run directly (`vm_bench.py`), the baseline for CP-T7.2's microVM classes:
    Firecracker and Cloud Hypervisor;
  - crun with libkrun, in the OCI table;
  - container engines and runtimes through their own CLIs: Podman, containerd with nerdctl, and Kata
    (QEMU and Cloud Hypervisor) through containerd;
  - sandbox platforms: Microsandbox, Zeroboot, Beam beta9, and CubeSandbox (which Ada will install).
- **Hosted-only platforms** (E2B Cloud, Daytona Cloud, Modal, Morph, CodeSandbox, Fly Machines, Vercel,
  Cloudflare) are not measured, by Ada's decision (2026-10-23, confirmed 2026-10-24). Rather than open
  accounts, she chose the open-source alternatives above, which run on the same hardware as Hex. The
  harness can still run a hosted round, labelled hosted, with its network round trip recorded
  (`platform_bench.py`, HOSTED=1).
