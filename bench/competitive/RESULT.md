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
  - Hex `oci`, with `hex oci exec`: 2026-10-24 14:09-14:25Z, start load 1.27, 4.99 at the end (rounds
    1-2, binary `hex-cg3-8947ea21`, below). Hex's earlier rounds are kept on fedora: `hex.noexec.r*`
    (`hex-cg2-2f0c122c`, born in its cgroups but without exec, 2026-10-24 03:35Z), `hex.nocgroup.r*` (no
    cgroup per container, 2026-10-23 06:50Z) and `hex.cgmigrate.r*` (a cgroup per container, joined by
    migration, 2026-10-23 23:01Z).
  - Process boxes: 07:25-07:45Z (run 2).
  - Density with held memory (below), on the same binaries (`FETCH=0`, their sums checked against run 2's):
    runc, crun, youki and runsc at 17:22-17:34Z (rounds 3-4, start load 0.85). Hex's own rounds measure
    held memory too. The process boxes' density: 2026-10-24 08:42-08:44Z (rounds 7-8, density only, so
    without the load gate: held memory doesn't depend on other people's CPU load), on `hex-cg2-2f0c122c`,
    whose process class is unchanged from the box rounds' binary.
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
  - `oci`: release build of `cp/t7-runtime` at c7fc78f, with `hex oci exec`, built on fedora after all
    seven gates passed (`hex-cg3-8947ea21`, sha256 8947ea2187a27972…). Its containers each get a cgroup
    with `linux.resources` and the device list in force (3319a85), and are born in it (05af083).
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
| create p50 (ms) | 9.13 | 27.54 | 2.99 | 21.94 | 19.01 | 0.33x vs crun |
| create p99 (ms) | 15.60 | 69.11 | 6.87 | 36.42 | 28.69 | 0.44x vs crun |
| create p99.9 (ms) | 38.40 | 141.71 | 8.98 | 126.16 | 35.40 | 0.23x vs crun |
| create max (ms) | 126.28 | 228.07 | 19.97 | 220.71 | 40.06 | 0.16x vs crun |
| start p50 (ms) | 6.74 | 3.53 | 0.51 | 5.31 | 8.89 | 0.08x vs crun |
| start p99 (ms) | 11.63 | 6.34 | 1.14 | 8.39 | 13.93 | 0.10x vs crun |
| start p99.9 (ms) | 24.06 | 10.84 | 1.43 | 10.87 | 18.18 | 0.06x vs crun |
| start max (ms) | 140.20 | 36.58 | 1.91 | 16.80 | 19.68 | 0.01x vs crun |
| exec p50 (ms) | 1.85 | 8.42 | 1.64 | 6.78 | 6.12 | 0.89x vs crun |
| exec p99 (ms) | 3.35 | 15.01 | 3.50 | 31.10 | 11.42 | 1.05x vs crun |
| exec p99.9 (ms) | 11.07 | 36.58 | 4.28 | 82.67 | 13.24 | 0.39x vs crun |
| exec max (ms) | 22.98 | 129.45 | 73.99 | 178.16 | 14.48 | 0.63x vs runsc |
| delete p50 (ms) | 0.97 | 104.48 | 10.76 | 25.58 | 5.19 | 5.33x vs runsc |
| delete p99 (ms) | 1.71 | 107.67 | 11.84 | 28.55 | 9.03 | 5.29x vs runsc |
| delete p99.9 (ms) | 4.42 | 117.70 | 22.94 | 33.33 | 105.63 | 5.19x vs crun |
| delete max (ms) | 20.63 | 244.60 | 32.11 | 47.77 | 106.01 | 1.56x vs crun |
| create+start of `true` p50 (ms) | 16.26 | 30.83 | 3.34 | 20.08 | 27.99 | 0.21x vs crun |
| create+start of `true` p99 (ms) | 29.30 | 65.20 | 6.00 | 40.87 | 38.91 | 0.20x vs crun |
| create+start of `true` p99.9 (ms) | 68.88 | 129.01 | 8.88 | 124.15 | 52.43 | 0.13x vs crun |
| create+start of `true` max (ms) | 934.36 | 759.89 | 28.02 | 223.19 | 55.23 | 0.03x vs crun |
| throughput, concurrency 1 (lifecycles/s) | 52 | 7 | 87 | 38 | 31 | 0.60x vs crun |
| throughput, concurrency 8 (lifecycles/s) | 325 | 59 | 1,015 | 398 | 230 | 0.32x vs crun |
| throughput, concurrency 32 (lifecycles/s) | 1,030 | 214 | 1,038 | 732 | 346 | 0.99x vs crun |
| CPU per lifecycle, concurrency 1 (whole bench cgroup) | 7.02 ms | 23.09 ms | 4.68 ms | 18.43 ms | 52.90 ms | 0.67x vs crun |
| memory held per idle container | 0.63 MiB | 0.41 MiB | 0.39 MiB | 0.39 MiB | 12.49 MiB | 0.62x vs crun |
| density (idle containers per GiB held) | 1,636 | 2,514 | 2,640 | 2,633 | 82 | 0.62x vs crun |

Hex's create+start max (934 ms) is one of a few stalls in each round: round 2's next-slowest samples
are 706, 400 and 126 ms, and round 1's max is 1,114 ms, with 185 ms next.

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
| memory held per idle box | 0.74 MiB | 0.28 MiB | 0.39 MiB | 0.38x vs scope |
| density (idle boxes per GiB held) | 1,388 | 3,615 | 2,607 | 0.38x vs scope |

The density rounds before these, 5 and 6, gave systemd scopes 1.11 MiB per box. That was the bench, not
systemd. They spawned every box, slept 3 s and read memory. At 1,000 boxes the user manager hadn't
started a single scope by then, so the reading was 1,000 `systemd-run` clients still waiting
(`probes/scope_charge.py`, fedora 2026-10-24). Now each box is up, its `sleep` running in its own cgroup,
before the next starts. Rounds 5 and 6 are kept on fedora as `box.settle3s.r*`. Per box, a scope holds 31
KiB in the driver's cgroup (the kernel stack and slab a fork leaves), about 255 KiB in its own (the
process, its page tables and the cgroup's per-CPU counters), and about 90-150 KiB in the user manager.

## What the run shows

- **OCI class: Hex misses the bar, and crun is the competitor to beat.**
  - crun is 3-9x faster than runc and youki on create and start.
  - Hex is behind crun on create (0.33x), start (0.08x), create+start (0.21x), CPU (0.67x) and held
    memory and density (0.62x). It is level at concurrency 32 (0.99x).
  - Hex's exec is close behind crun's at p50 (1.85 ms against 1.64, 0.89x), just ahead at p99
    (1.05x), and behind at p99.9 (0.39x). At p50 it is 3-5x faster than runc's, youki's and runsc's.
  - Hex's delete is the fastest of all (0.97 ms p50, 11x crun's), now that it removes a real cgroup
    as the others do. It is the one axis that clears the 4x bar, from p50 through p99.9 (5.2-5.3x).
  - Reaching the 4x floor would mean Hex getting faster by a factor of 12 on create (4 × 9.13 / 2.99),
    53 on start, 19 on create+start and 4.5 on exec.
  - Hex's own AC-T7.1.3 bar (start p50 under 50 ms) is met.
- **Hex's `start` is the largest gap: 6.74 ms against crun's 0.51 ms.** The container side is already
  the cheap shape: it signals ready, blocks on the exec FIFO, writes one byte and execs. The runtime
  side writes its state durably on every operation (temp file, fsync, rename, directory fsync). That
  is the leading suspect, to be confirmed by profiling before the runtime design (goal 4) changes it.
- **Process class: Hex is 2-4x ahead of systemd on latency, throughput and CPU**, and reaches the 4x bar
  only on throughput at concurrency 32. Its p99.9 is level with systemd scopes. Each idle box keeps its
  `hex box run` wrapper process alive, which a systemd scope doesn't, so Hex is behind on memory
  (0.38x: 0.74 MiB held per box against a scope's 0.28).
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
`clone3(CLONE_INTO_CGROUP)`, and create is 9.13 ms (9.79 ms on the binary before `hex oci exec`):
about 1-2 ms over having no cgroup at all.

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
are the stack's own cgroups'. Docker's column is its refereed rounds 1-2. Neither met the referee's bar
of under 10% of samples with foreign load in any of its three tries, so each keeps its quietest (12.9%
and 18.1%). Its earlier rounds predate the referee and the stack-cgroup method, and are kept on fedora as
`unrefereed.*`.

| Axis | Hex | agentsandbox | agentsandbox-warm | daytona | docker | k8s | Hex vs best |
|---|---|---|---|---|---|---|---|
| create until it runs a command p50 (ms) | 15.90 | 434.86 | 48.96 | 407.92 | 238.92 | 408.92 | 3.08x vs agentsandbox-warm |
| create until it runs a command p99 (ms) | 27.14 | 544.33 | 465.19 | 488.21 | 449.35 | 501.13 | 16.56x vs docker |
| create until it runs a command p99.9 (ms) | 54.97 | 689.10 | 763.29 | 502.48 | 917.72 | 559.45 | 9.14x vs daytona |
| create until it runs a command max (ms) | 266.47 | 689.10 | 763.29 | 502.48 | 2,115.19 | 559.45 | 1.89x vs daytona |
| exec of `true` p50 (ms) | 1.85 | 42.43 | 32.82 | 12.54 | 44.90 | 41.63 | 6.77x vs daytona |
| exec of `true` p99 (ms) | 3.35 | 54.68 | 90.60 | 25.11 | 75.61 | 53.13 | 7.50x vs daytona |
| exec of `true` p99.9 (ms) | 11.07 | 97.31 | 125.57 | 27.95 | 80.33 | 60.44 | 2.53x vs daytona |
| exec of `true` max (ms) | 22.98 | 97.31 | 125.57 | 27.95 | 94.48 | 60.44 | 1.22x vs daytona |
| delete p50 (ms) | 0.97 | 30.54 | 43.16 | 237.01 | 150.13 | 27.32 | 28.07x vs k8s |
| delete p99 (ms) | 1.71 | 61.91 | 155.86 | 293.34 | 321.78 | 47.87 | 28.02x vs k8s |
| delete p99.9 (ms) | 4.42 | 87.34 | 176.57 | 350.24 | 639.35 | 79.13 | 17.89x vs k8s |
| delete max (ms) | 20.63 | 87.34 | 176.57 | 350.24 | 949.26 | 79.13 | 3.84x vs k8s |
| lifecycles/s at concurrency 1 | 52 | 2 | 6 | 2 | 2 | 2 | 9.13x vs agentsandbox-warm |
| lifecycles/s at concurrency 8 | 325 | 6 | 6 | 7 | 6 | 6 | 44.70x vs daytona |
| lifecycles/s at concurrency 32 | 1,030 | 6 | 6 | 8 | 7 | 7 | 124.57x vs daytona |
| CPU per lifecycle at concurrency 1 (platforms: what a lifecycle adds to their stack's CPU) | 7.02 ms | 881.71 ms | 535.41 ms | 511.92 ms | 385.09 ms | 806.63 ms | 54.85x vs docker |
| memory held per idle sandbox | 0.63 MiB | 4.94 MiB | 5.18 MiB | 30.86 MiB | 4.94 MiB | 7.74 MiB | 7.89x vs agentsandbox |
| density (idle sandboxes per GiB held) | 1,636 | 207 | 198 | 33 | 207 | 132 | 7.89x vs agentsandbox |
| the stack's own CPU at idle (Hex `oci` runs no daemon) | n/a | 0.136 CPUs | 0.192 CPUs | 0.046 CPUs | 0.003 CPUs | 0.120 CPUs | n/a |
| the stack's own memory held at idle (Hex `oci` runs no daemon) | n/a | 1,305.79 MiB | 2,096.97 MiB | 1,228.50 MiB | 222.86 MiB | 1,307.31 MiB | n/a |

Where density stopped: Docker at the cap of 1,000 in both rounds; Daytona at the cap of 200 in each of
four rounds (its held memory from round 4, with no foreign load in 18 samples; round 3's 94.7% foreign
share is recorded, as a density-only round is never retried). Kubernetes pods and agentsandbox stopped at
99-100, each refused there by the cluster ("not ready in 300s"), and the warm pool at its cap of 30.

agentsandbox is kubernetes-sigs/agent-sandbox, a Sandbox per lifecycle; agentsandbox-warm claims one
from a warm pool of 32. docker is Docker Engine (dockerd, containerd and runc) through its CLI. k8s is a
Pod through the Kubernetes API on kind. Docker's held memory per container (5,180,916 bytes, over 1,000)
and agentsandbox's per Sandbox (5,178,122 bytes, over 100) only round alike.

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

fedora couldn't reserve the 4 GiB of contiguous hugepages E2B's host setup needs: other people's units
held most of its memory, and compaction and a cache drop freed 24 of 2,048 pages. So by Ada's decision
(2026-10-24), E2B ran on adacachy, and Hex's `oci` rounds ran there too, for a same-hardware ratio.
adacachy runs production, at a 1-minute load of 25-95 during these rounds, so **these rows are not of
record by CLAUDE.md §7**, and are unrefereed. Hex's tail rows carry that load most: 10,000 samples per
round against E2B's 300.

- **E2B:** e2b-dev/runtime at bca4b33, `embed/compose`, unchanged, with its client e2b 2.46.0. It ran inside
  one privileged container with its own Docker daemon (`e2b-dind/`). adacachy's UFW drops the hop from each
  sandbox to its orchestrator's egress proxy on the host side of the sandbox's veth, and in the container
  that hop stays inside it, so the host's firewall was never changed. Two settings for the bench: 9,216 2
  MiB hugepages (18 GiB, enough for 32 concurrent creates of a 512 MiB sandbox), and its seeded tier's
  limit of 20 concurrent sandboxes raised to 1,000, as Daytona's admin quotas were. Each round's density
  stopped at 35-36 sandboxes, the hugepage pool: each sandbox reserves its whole 512 MiB guest from it.
- **Hex:** `hex-adacachy-exec-842f9d60` (sha256 842f9d606cfb0510…), built on adacachy from c7fc78f, the
  source of fedora's `hex-cg3` (with `hex oci exec`), 2026-10-24 13:50-14:39Z, at a 1-minute load of 74 at
  the start and 25 at the end. Its rounds before exec, on `hex-adacachy-d2c8636a` (05af083's crates,
  05:57-06:46Z), are kept on adacachy as `hex.noexec.r*`.
- **Memory counts hugepages, once.** E2B's guest memory is hugepages, which `memory.current` leaves out
  unless the hierarchy is mounted with `memory_hugetlb_accounting`. So held memory adds each cgroup's
  faulted hugepages, `hugetlb.<size>.current` (`cgmem.py`). The reservations, `hugetlb.<size>.rsvd.current`,
  are recorded apart. A faulted page stays charged there too, so adding both counts it twice.
  - The rows below take memory from density-only rounds 3 and 4.
  - Rounds 1 and 2 (lifecycle, throughput) added both and read 675 MiB per sandbox.
  - A first round left hugepages out (6.7 MiB per sandbox) and stopped at the tier limit of 20. It's kept
    apart as `limit20.platform.e2b.r1.json`.
- **Each sandbox reserves 512 MiB of hugepages** from the pool: exactly its guest's size, faulted or not.
  So E2B fits 2 sandboxes per GiB of hugepage pool, whatever they hold. The density row is per GiB held,
  like every other row.

| Axis | Hex | E2B | Hex vs E2B |
|---|---|---|---|
| create until it runs a command p50 (ms) | 46.41 | 61.83 | 1.33x |
| create until it runs a command p99 (ms) | 272.62 | 111.36 | 0.41x |
| create until it runs a command p99.9 (ms) | 659.78 | 114.43 | 0.17x |
| create until it runs a command max (ms) | 1,352.05 | 114.43 | 0.08x |
| exec of `true` p50 (ms) | 1.48 | 6.70 | 4.54x |
| exec of `true` p99 (ms) | 12.00 | 20.58 | 1.71x |
| exec of `true` p99.9 (ms) | 22.70 | 23.39 | 1.03x |
| exec of `true` max (ms) | 47.60 | 23.39 | 0.49x |
| delete p50 (ms) | 0.77 | 4.63 | 5.99x |
| delete p99 (ms) | 43.17 | 13.88 | 0.32x |
| delete p99.9 (ms) | 102.02 | 19.35 | 0.19x |
| delete max (ms) | 180.27 | 19.35 | 0.11x |
| lifecycles/s at concurrency 1 | 23 | 12 | 1.95x |
| lifecycles/s at concurrency 8 | 183 | 21 | 8.56x |
| lifecycles/s at concurrency 32 | 470 | 25 | 19.15x |
| CPU per lifecycle at concurrency 1 (E2B: what a lifecycle adds to its stack's CPU) | 5.61 ms | 260.15 ms | 46.40x |
| memory held per idle sandbox | 0.66 MiB | 180.89 MiB | 274.24x |
| density (idle sandboxes per GiB held) | 1,552 | 6 | 274.24x |
| the stack's own CPU at idle (Hex `oci` runs no daemon) | n/a | 0.091 CPUs | n/a |
| the stack's own memory held at idle (Hex `oci` runs no daemon) | n/a | 1,915.69 MiB | n/a |

E2B's create restores a sandbox from its template's memory snapshot rather than booting one, and its p50
(62 ms) is that restore. Its exec is a request through its SDK (`commands.run`) to the daemon in the
sandbox, envd; Hex's is a process cloned into the container's namespaces and cgroup. When the rounds were
done, everything of E2B's was removed from adacachy: the container and its volumes, the hugepages (back
to 0) and the nbd module. Rounds 1-2 ran 2026-10-24 08:42-08:47Z at load 64-95, and the density rounds
3-4 10:20-10:24Z at load 34-46.

## Not yet measured

- **Platforms still to run, self-hosted:** Agent Substrate and Google AX.
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
