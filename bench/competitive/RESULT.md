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
| throughput, concurrency 1 (lifecycles/s) | 57 | 7 | 90 | n/a | 29 | 0.63x vs crun |
| throughput, concurrency 8 (lifecycles/s) | 339 | 59 | 954 | n/a | 217 | 0.35x vs crun |
| throughput, concurrency 32 (lifecycles/s) | 1,146 | 221 | 1,711 | n/a | 311 | 0.67x vs crun |
| CPU per lifecycle, concurrency 1 (whole bench cgroup) | 7.02 ms | 23.09 ms | 4.68 ms | 18.43 ms | 52.90 ms | 0.67x vs crun |
| memory held per idle container | 0.63 MiB | 0.41 MiB | 0.39 MiB | 0.39 MiB | 12.49 MiB | 0.62x vs crun |
| density (idle containers per GiB held) | 1,636 | 2,514 | 2,640 | 2,633 | 82 | 0.62x vs crun |

Hex's create+start max (934 ms) is one of a few stalls in each round: round 2's next-slowest samples
are 706, 400 and 126 ms, and round 1's max is 1,114 ms, with 185 ms next.

**The OCI table's hex rows predate decisions 1 and 2** (binary c7fc78f, which fsynced its state and left
a shim per container), so its start p50 (6.74 ms) carries the ~6.6 ms fsync penalty and its held memory
(0.63 MiB) the shim. A pass on the no-fsync, no-shim binary (3e5444d8, adacachy, 2026-10-26, hex and crun
only, interleaved; LOAD_GATE=0, so not of record by §7, but the hex-vs-crun ratios cancel the shared host's
load) measures what those two decisions changed: **start p50 0.27 ms against crun's 0.25 (0.94x)** — the
fsync penalty is gone — and **held memory per idle container 0.42 MiB against crun's 0.40 (0.95x)**, down
from 0.63, with no process left beside any idle container (`probes/oci_breakdown.sh` step 3: hex leaves only
the workload, as crun does). Held memory was still ~20 KiB/container above crun's on that binary; plan step 2
closed it (below): 0.99x crun's on adacachy, and 0.98-0.99x of record on fedora, with Hex's loopback down. With
it up, as crun's is, Hex is at parity (below, "Loopback up"). The OCI table's other rows are still the earlier
binary's.

**What that residual was: a device program and two pipes per container** (runtime-architecture.md, decision 2).

The run: density only, Hex and crun interleaved, 3 rounds of 1,000 idle containers each. Run
`20261027T033009Z`, adacachy, binary 3e5444d8, bench code fa409c7, LOAD_GATE=0, 2026-10-27 03:30Z. Held memory
doesn't depend on load, but this run isn't of record by §7 all the same.

How it's measured: the density phase records the bench cgroup's memory.stat rise by entry, from the same two
readings as its held figure, and the bytes under each runtime's state root. Each reading waits until
memory.current has held still across two of the kernel's 2 s stat flushes (cgmem.quiet_snapshot); every
reading here was quiet within 2-6 windows. `probes/held_attribution.py` pairs round k of Hex with round k of
crun.

Hex less crun, in KiB per idle container:

| part | round 1 | round 2 | round 3 |
|---|---:|---:|---:|
| held | +11.36 | +12.64 | +12.88 |
| anon | -0.12 | -0.06 | +0.09 |
| shmem | 0.00 | 0.00 | 0.00 |
| sock, swap, zswap, hugepages | 0.00 | 0.00 | 0.00 |
| kernel | +11.48 | +12.69 | +12.78 |
| kernel: slab | +4.55 | +4.34 | +4.32 |
| kernel: vmalloc | +4.00 | +4.00 | +4.00 |
| kernel: percpu | +1.20 | +1.20 | +1.20 |
| kernel: pagetables | +0.16 | -0.26 | +0.03 |
| kernel: the rest | +1.58 | +3.41 | +3.23 |
| named entries' sum | +11.36 | +12.64 | +12.88 |
| left unattributed | 0.0% | 0.0% | 0.0% |
| state root allocated (Hex; crun) | 4.00; 8.00 | 4.00; 8.00 | 4.00; 8.00 |

- **The bar is met in every round.** The named entries leave none of the held difference unattributed in any of
  the three rounds. In each reading, memory.current stood 256 KiB above anon + file + kernel + sock, at the
  baseline and the peak alike, so the gap cancels in the rise.
- **Where the residual is.** It's kernel memory, not runtime state on a tmpfs: shmem rose by nothing for either
  runtime, and Hex's state root is half the size of crun's.
- **The largest named entry is slab**, +4.3 to +4.6 KiB per container.
- **The steady part.** Two entries are the same in every round, and both are what a loaded BPF program
  charges to its loader's cgroup (`kernel/bpf/core.c:102-118,149-159`):
  - vmalloc, exactly one page per container, where crun's rise is 0;
  - percpu, +1.20 KiB, the program's per-CPU `stats` and `active`.
- **Why only Hex loads a program.** The bench bundle has no `linux.resources`, so crun attaches no device program
  (`cgroup-resources.c:1383`: `if (need_bpf_dev && resources->devices_len)`). Hex gives the spec its
  deny-by-default policy and loaded one program per container.
- **The rest: two pipes per idle container.** `/proc/<pid>/fd` of an idle Hex container's `sleep` listed four
  pipe descriptors, both ends of the create handshake's two pipes; crun's listed 0, 1 and 2. The pipes were
  made without close-on-exec, so they went on through the container's exec. In a lab of 500 idle containers a
  round, the cgroup the runtime ran in held 4.1 KiB per Hex container of the kernel's rest, one pipe page, and
  none per crun container.

**What closed it.** Two changes (decision 2, "Nothing per container but the container"): containers with one
device policy share one loaded program, and the handshake's pipes are close-on-exec, so the program holds only
its stdio. Each was measured alone, in the same shape of run as above (adacachy, density only, Hex and crun
interleaved, 3 rounds of 1,000 idle containers, LOAD_GATE=0, 2026-10-27). Hex less crun, in KiB per idle
container, and Hex's held memory over crun's:

| binary | run | round 1 | round 2 | round 3 | held, Hex / crun |
|---|---|---:|---:|---:|---|
| no shim (3e5444d8) | `20261027T033009Z` | +11.36 | +12.64 | +12.88 | |
| the program shared (423797c7) | `20261027T034522Z` | +5.19 | +4.21 | +5.34 | |
| the pipes closed, the program not shared (57b21894) | `20261027T044129Z` | +3.76 | +2.90 | +2.25 | 1.0090 / 1.0070 / 1.0054 |
| both (1063a51f) | `20261027T043711Z` | -3.31 | -2.75 | -3.90 | 0.9920 / 0.9934 / 0.9906 |

- **Sharing** took vmalloc's rise per container from +4.00 KiB to 0 and percpu's from +1.20 to 0. Without it,
  both were back.
- **Either change alone** leaves Hex above crun; both bring it below, by 2.8-3.9 KiB per container, 422 KiB held
  against crun's 425-426.
- **Unattributed:** in `034522Z` and `043711Z`, at most 0.73% of any round's difference. In `044129Z`'s third
  round, 12.1%: 0.27 KiB per container in Hex's own readings, as some of Hex's pages went to swap during that
  round (anon -6.12 KiB, swap +6.21 against crun), which held counts either way. Every reading was quiet.

**Of record on fedora** (plan step 4): the bench box, now named kiln-a094a12d.attlocal.net after a reprovision
(Intel i9-14900HX, 32 CPUs pinned 4-31, 62 GiB, Linux 6.19.10-300.fc44). Density only, Hex and crun interleaved,
2 rounds of 1,000 idle containers, LOAD_GATE=0: held memory doesn't depend on load, and the ratio is same-run.
The host carried kiln CI throughout. Hex's binary is the static-glibc release build made on that host against
Fedora's glibc-static: a static build from adacachy dies there, since CachyOS builds its glibc with AVX-512.

| binary | run | load (start, end) | Hex less crun, KiB | held, Hex / crun |
|---|---|---|---:|---:|
| 92b3b91 (d4fd321b) | `20261027T061920Z` | 8.81, 18.05 | -5.96 / -4.44 | 0.9849 / 0.9887 |
| 80fa5e3 (d088b7eb) | `20261027T055504Z` | 11.54, 5.22 | -4.80 / -4.82 | 0.9878 / 0.9877 |

- **The parts:** vmalloc and percpu rose by 0 per Hex container, and slab by 5.4-6.5 KiB less than crun's.
  Unattributed: 1.1% and 6.1% in the 92b3b91 run.
- **Absolute:** Hex holds 396,853 and 397,464 bytes per idle container, against crun's 402,956 and 402,014.
- **Nothing beside the container:** `probes/oci_breakdown.sh` step 3 on the same binary
  (`breakdown-20261027T062402Z`) finds Hex's 5 idle containers' cgroups holding only `/usr/bin/sleep`, and no
  Hex process anywhere.
- **runtime-tools** on the same binary: `bench/oci-validation/SUMMARY-hex.txt` (26 against crun's 28; the two
  differences are Hex's refusals of a mount label and an AppArmor profile it can't apply, kept by Ada's call).

**Loopback up: parity, not a lead** (2026-10-27). Every run above left each Hex container's loopback down:
until 3bb57d3 the init never brought `lo` up, so 127.0.0.1 was unreachable in a container with its own network
namespace (goal 3's plan step 5 found it; `oci/net.rs` now brings it up with one rtnetlink request, as runc and
crun do). crun brings it up, so part of Hex's lead was loopback state Hex didn't make. On 3bb57d3 (adacachy, run
`20261027T165449Z`, 3 interleaved rounds of 1,000, LOAD_GATE=0), Hex holds 0.9981, 1.0018 and 1.0002 of crun's
memory per idle container (-0.80, +0.75 and +0.08 KiB), its slab now 0.39 to 0.96 KiB above crun's where it
was 5.4 to 6.5 below. Round 2 left 45.7% of a 0.75 KiB difference unattributed, swap moving during it. Goal 2's
"at most crun's" stands at parity, two rounds a hair above, and wants a fedora run with loopback up.

**Also on adas-mac-studio-2** (Ada's call, 2026-10-27: the Mac Studios too). Docker Desktop's Linux VM
(LinuxKit 6.12.65, aarch64, 14 CPUs, 33.6 GB) runs the containers. The bench containers' `/usr` is the
hexbench/mac:44 image's, Fedora 44 with python3, since the Mac's `/usr` is macOS's; otherwise `oci_inside.sh`
runs as host.sh runs it. Hex is 92b3b91, built static-glibc for aarch64 in that image (60c29228); crun is
1.30.1's arm64 release. The VM's load was 2.94 at the start and 3.40 at the end.

| binary | rounds | Hex less crun, KiB | held, Hex / crun |
|---|---|---:|---:|
| 92b3b91 (60c29228, aarch64) | 2 of 1,000 | -1.39 / -13.74 | 0.9975 / 0.9754 |

vmalloc and percpu rose by 0 per Hex container, and nothing was unattributed in either round. Step 3 there
finds only the workload in Hex's container cgroups and no Hex process anywhere. runtime-tools there gives Hex 23
and crun 25, and the same two differences, Hex's refusals (`bench/oci-validation/SUMMARY-hex.txt`).
- **The earlier run.** Run `20261027T031736Z` (bench code 4cf9b28) read memory.stat without waiting for a flush.
  It left 6.8%, 18.7% and 8.7% unattributed. Each runtime's own named-entry rise there was 0.6-3.3 KiB per
  container off its held rise, because memory.stat may trail by up to 64 pages a CPU between flushes
  (`mm/memcontrol.c:743-756`). Its attribution was the same.

The three throughput rows were re-measured on 2026-10-25 through the worker-process-per-slot driver (the
2026-10-25 run of record below); every other row is the 2026-10-23/24 run's. The old lock-serialized driver
capped concurrent spawns, most at concurrency 32 (old rows: hex 52/325/1,030, runc 7/59/214, crun
87/1,015/1,038, youki 38/398/732, runsc 31/230/346 lifecycles/s at concurrency 1/8/32;
`probes/oci_breakdown.md`, run 2). crun's concurrency-32 rate rose the most, 1,038 → 1,711/s, which is why
Hex is no longer level there. **youki is n/a:** under the new driver its delete raced at concurrency 32 (it
lost a container's state when the init exited before it read `/proc/<pid>/stat`, and the next round's reused
id then collided), so neither round completed youki's concurrency-32 lifecycles; the old driver's lock
serialized the spawns and masked it. CPU per lifecycle is still the 2026-10-23/24 run's (concurrency 1, which
the old driver did not cap).

### Process class

| Axis | Hex `box run` | Hex seam | systemd scope | systemd service | Hex vs best |
|---|---|---|---|---|---|
| run of `true` p50 (ms) | 2.73 | 0.87 | 7.13 | 12.11 | 2.61x vs scope |
| run of `true` p99 (ms) | 6.06 | 1.67 | 9.16 | 35.49 | 1.51x vs scope |
| run of `true` p99.9 (ms) | 13.90 | 2.44 | 13.15 | 43.11 | 0.95x vs scope |
| run of `true` max (ms) | 28.30 | 22.60 | 34.54 | 50.55 | 1.22x vs scope |
| throughput, concurrency 1 (runs/s) | 314 | 1,306 | 145 | 76 | 2.17x vs scope |
| throughput, concurrency 8 (runs/s) | 759 | 3,664 | 195 | 143 | 3.89x vs scope |
| throughput, concurrency 32 (runs/s) | 783 | 2,910 | 160 | 192 | 4.08x vs service |
| CPU per run (the call, plus systemd's manager and bus) | 2.51 ms | 0.89 ms | 7.81 ms | 13.92 ms | 3.11x vs scope |
| memory held per idle box | 0.74 MiB | 0.27 MiB | 0.28 MiB | 0.39 MiB | 0.38x vs scope |
| density (idle boxes per GiB held) | 1,388 | 3,811 | 3,615 | 2,607 | 0.38x vs scope |

The density rounds before these, 5 and 6, gave systemd scopes 1.11 MiB per box. That was the bench, not
systemd. They spawned every box, slept 3 s and read memory. At 1,000 boxes the user manager hadn't
started a single scope by then, so the reading was 1,000 `systemd-run` clients still waiting
(`probes/scope_charge.py`, fedora 2026-10-24). Now each box is up, its `sleep` running in its own cgroup,
before the next starts. Rounds 5 and 6 are kept on fedora as `box.settle3s.r*`. Per box, a scope holds 31
KiB in the driver's cgroup (the kernel stack and slab a fork leaves), about 255 KiB in its own (the
process, its page tables and the cgroup's per-CPU counters), and about 90-150 KiB in the user manager.

## Run of record (2026-10-25, fedora): the in-process driver, the ceiling, the seam, and re-measured throughput

Goal 0's bench gained libcrun driven in process (the competitor of Hex's `Runtime` seam), a sustained and a
no-network-namespace phase, a stand-in ceiling, and the worker-process-per-slot throughput driver. This run
records them on the same host and competitors as the run above, in three load-gated passes into one
directory (`~/.cache/hex-bench/out/ror-step5-20261025T100031Z`), each started at a 1-minute load under 2
(`env.txt`: libcrun pass 0.89, OCI-CLI-throughput pass 1.98, seam pass 1.89). The host is shared, so load
rose during the passes; best of 2 takes each metric from its quieter round, and the tails (p99.9, max) and
throughput carry that noise, as on the run above.

- **libcrun** is built in process from crun 1.30.1's pinned release (tarball sha256 99d036d9…), driver
  `bench/competitive/libcrun/driver.c`. It is the competitor of Hex's seam, so it is outside the command
  line's ratio (`docs/design/runtime-architecture.md`).
- **hex (command line)** is `hex-cg3` (c7fc78f, sha256 8947ea21…), the binary of the OCI table above; only
  its throughput is re-measured here.
- **seam_box** (the process class through the `Runtime` seam) is HEAD of `cp/t7-runtime` (sha256 0000dda1…),
  which carries decisions 1, 2 and 7 (no state fsync, no shim, the box cloned into its cgroup).

**The driver's own ceiling** (`/usr/bin/true` standing in for the runtime, best of 4 rounds): **1,014 /
6,862 / 13,577** lifecycles/s at concurrency 1 / 8 / 32. At concurrency 32 that is 13,577/s, far above the
4,152/s the design's targets need a bench to drive, and 9.7x the old lock-serialized driver's ~1,394/s
(`probes/oci_breakdown.md`, run 2). So the driver no longer caps any runtime's concurrency-32 rate.

### In-process (the `Runtime` seam's layer): libcrun, against Hex's seam

libcrun's calls run in the driver's own process; the one difference from crun's command line is the process
crun spawns per call. Hex's `oci`-class seam column came with goal 3 (the oci class behind the seam, `seam_oci`),
from a run of its own beside libcrun: **"Goal 3: the oci class through the seam"** below.

| Axis | libcrun | Hex seam (`oci`) |
|---|---|---|
| create p50 (ms) | 3.59 | waiting (goal 3) |
| create p99 (ms) | 7.04 | waiting (goal 3) |
| create p99.9 (ms) | 7.58 | waiting (goal 3) |
| create max (ms) | 12.49 | waiting (goal 3) |
| start p50 (ms) | 0.10 | waiting (goal 3) |
| start p99 (ms) | 0.28 | waiting (goal 3) |
| start p99.9 (ms) | 0.38 | waiting (goal 3) |
| start max (ms) | 0.54 | waiting (goal 3) |
| exec p50 (ms) | 2.42 | waiting (goal 3) |
| exec p99 (ms) | 4.75 | waiting (goal 3) |
| exec p99.9 (ms) | 14.30 | waiting (goal 3) |
| exec max (ms) | 22.78 | waiting (goal 3) |
| delete p50 (ms) | 10.46 | waiting (goal 3) |
| delete p99 (ms) | 10.98 | waiting (goal 3) |
| delete p99.9 (ms) | 25.74 | waiting (goal 3) |
| delete max (ms) | 35.98 | waiting (goal 3) |
| create+start of `true` p50 (ms) | 2.79 | waiting (goal 3) |
| create+start of `true` p99 (ms) | 38.16 | waiting (goal 3) |
| create+start of `true` p99.9 (ms) | 82.66 | waiting (goal 3) |
| create+start of `true` max (ms) | 163.78 | waiting (goal 3) |
| throughput, concurrency 1 (lifecycles/s) | 71 | waiting (goal 3) |
| throughput, concurrency 8 (lifecycles/s) | 617 | waiting (goal 3) |
| throughput, concurrency 32 (lifecycles/s) | 1,927 | waiting (goal 3) |
| CPU per lifecycle, concurrency 1 (whole bench cgroup) | 3.21 ms | waiting (goal 3) |

libcrun without a network namespace (lifecycle_nonet): create p50 2.36 ms, delete p50 10.34 ms. Its sustained
rate at concurrency 32 (best round): 1,903/s, the last 1,000 lifecycles 0.89x the first's (windows 2,075,
1,949, 2,007, 1,997, 2,000, 1,500, 1,757, 1,996, 2,066, 1,854).

### The process class through the seam (the Hex seam column above)

The process table above gains a **Hex seam** column from `seam_box` (the node agent in the bench's own
process, each call a `Runtime` method). On the HEAD binary its run of `true` is 0.87 ms p50: decision 1 (no
state fsync) removes the seven fsyncs that made it 37 ms when the seam was first measured (2026-10-24), and
with no command-line spawn and no wrapper process it is below both `hex box run` (2.73 ms) and a systemd
scope (7.13 ms). It holds 0.27 MiB per idle box, below a scope's 0.28. Goal 7 judges these against the scope;
its own ratio column comes then.

## Goal 3: the oci class through the seam (2026-10-29, fedora)

host.sh's OCI pass, `PHASES=oci RUNTIMES=libcrun HEXBENCH_PHASES=lifecycle`, with `SEAM_OCI_BIN`: libcrun in
process beside Hex's `oci` class through the `Runtime` seam, in process and over the node agent's socket, each round
in turn, 2 rounds of 1,000 lifecycles after 100 untimed, best of 2. Run `~/.cache/hex-bench/out/20261029T001409Z`:
fedora (`kiln-a094a12d`, Intel Core i9-14900HX, Linux 6.19.10-300.fc44, pinned to CPUs 4-31), load 0.86 at its start
and 3.58 at its end, no load gate (`LOAD_GATE=0`). `hex` is 8224f97 on `cp/t7-runtime`, a static-glibc release
build (sha256 749fd88368188ade…); `seam_oci` the same commit's (sha256 f4b4c60b87b6cabf…); libcrun 1.30.1 as above.

| Axis | libcrun | Hex seam (`oci`) | Hex seam over the socket | seam over libcrun |
|---|---|---|---|---|
| create p50 (ms) | 5.89 | 19.19 | 19.54 | 3.26x |
| create p99 (ms) | 7.76 | 23.71 | 25.08 | 3.06x |
| create p99.9 (ms) | 14.67 | 34.32 | 49.88 | 2.34x |
| create max (ms) | 17.25 | 43.54 | 55.45 | 2.52x |
| start p50 (ms) | 0.196 | 0.197 | 0.488 | **1.005x** |
| start p99 (ms) | 0.27 | 0.38 | 0.81 | 1.39x |
| start p99.9 (ms) | 0.36 | 0.51 | 0.87 | 1.42x |
| start max (ms) | 0.41 | 0.68 | 1.39 | 1.66x |
| exec p50 (ms) | 3.756 | 1.086 | 1.372 | **0.289x** |
| exec p99 (ms) | 4.99 | 2.34 | 2.97 | 0.47x |
| exec p99.9 (ms) | 23.81 | 16.02 | 18.60 | 0.67x |
| exec max (ms) | 31.16 | 26.73 | 22.25 | 0.86x |
| delete p50 (ms) | 10.68 | 7.80 | 8.09 | 0.73x |
| delete p99 (ms) | 11.84 | 10.96 | 11.93 | 0.93x |
| delete p99.9 (ms) | 22.64 | 22.44 | 24.84 | 0.99x |
| delete max (ms) | 33.44 | 29.36 | 41.03 | 0.88x |

Goal 3's bars (plan step 6): start p50 through the seam at most 1.1 of libcrun's, **met** (1.005); exec p50 at most
0.25 of libcrun's, **not met** in this run (0.289). The socket costs 0.29-0.35 ms a call at p50 (create 0.350, start
0.291, exec 0.286, delete 0.297). Create is 3.3x libcrun's: goal 3's bars don't cover it; it is open in `STATUS.md`.

**What the exec bar met here: fedora at rest.** The run met fedora with nothing else running, and fedora at rest
runs a short burst of work slowly. A 100,000-step multiply-add chain with no system call, timed every 2 ms, took 0.336
ms at p50 on a P-core and 0.500 on an E-core with fedora idle, and 0.104 and 0.161 with one busy loop on CPU 0
(outside the bench's CPUs) holding the package awake (`bench/competitive/probes/exec_ab.md`, "The host's state"). A
short call slows more than a long one there, since a long one ramps the clock itself: two `exec_ab.sh` runs of the
same build against libcrun, a minute apart, measured (ms, p50, best of 2; not of record, `N` 600):

| fedora | libcrun exec | Hex seam exec | over libcrun | libcrun start | Hex seam start |
|---|---|---|---|---|---|
| held awake, `exec-ab-20261029T002311Z` | 1.834 | 0.372 | 0.203 | 0.079 | 0.078 |
| at rest, `exec-ab-20261029T002359Z` | 3.831 | 0.996 | 0.260 | 0.198 | 0.174 |

So whether this bar is met on fedora depends on how the pass holds the host, which is the bench method's to state
(CLAUDE.md §7 asks for load under 2, the state in which fedora's clock is down).

**A second fedora pass, the package awake but the host contended** (`~/.cache/hex-bench/out/20261029T022447Z`,
2026-10-29T02:24Z, the same binaries byte for byte, rebuilt from e67e9ac, whose crates and harness are main's ed81e3b;
load 18.84 at its start and 20.68 at its end, from two CI jobs on fedora, 4 of 32 cores idle). libcrun's exec ran at its
awake speed, 1.823 ms (1.915 and 1.823 by round); Hex's seam exec 0.548 ms (0.552 and 0.548), **0.301x**; start 0.093
against 0.107 ms, 0.864x; over the socket exec 0.621 and start 0.166. With few cores idle, Hex's short exec waits for
a CPU at each of its wakeups (the clone's child is pinned to the agent thread's CPU, which other work may hold), where
libcrun's long one doesn't move: 0.548 ms here against 0.372 held awake on a quiet host.

**The same pass on other machines** (Ada's call, 2026-10-29: "test on literally any of the other machines we have").
`bench/competitive/docker_pass.sh` runs host.sh's OCI pass where the containers run in Docker Desktop's Linux VM:
hex and `seam_oci` at 3db2173 (8224f97's code; static-glibc hex), and the libcrun driver from crun 1.30.1's tarball
(sha256 99d036d95113552b…, the one fedora's driver is built from), all built in a Fedora 44 image whose `/usr` stands
in for the host's; then `oci_inside.sh` as host.sh runs it, `RUNTIMES=libcrun`, `SEAM_OCI`, lifecycle, 2 rounds of
1,000 after 100, the VM's CPUs past an eighth of them, no load gate, the VM's load recorded. Best of 2, p50 (ms):

| Machine | Linux | VM load | libcrun exec | seam exec | over libcrun | libcrun start | seam start | over libcrun |
|---|---|---|---|---|---|---|---|---|
| fedora, Intel Core i9-14900HX (the run above) | bare metal, 6.19.10 | 0.86-3.58 | 3.756 | 1.086 | **0.289** | 0.196 | 0.197 | 1.005 |
| jupiter, AMD Ryzen AI MAX+ 395 | WSL2, 6.18.54, 32 CPUs | 0.26-3.19 | 2.363 | 0.579 | **0.245** | 0.148 | 0.121 | 0.815 |
| adas-mac-studio-3, Apple M4 Max | LinuxKit, 6.12.65, 14 CPUs | 3.70-5.84 | 1.271 | 0.148 | **0.117** | 0.063 | 0.032 | 0.500 |

Runs: jupiter `C:\Users\corph\.cache\hexpass\jupiter-20261029T004519Z` (hex sha256 4679dec395fcc047…, seam_oci
13a940f36c5f936e…, driver 11b59748e8212008…); adas-mac-studio-3 `~/.cache/hexpass/studio3-20261029T003900Z` (hex
73a1355aa81447cf…, aarch64, seam_oci 92f1da0e98159cde…, driver 2ec536e732650bce…). Over the socket, exec 0.797 ms on
jupiter and 0.202 on the Mac Studio, start 0.287 and 0.083. Create is 2.81x libcrun's on jupiter (10.869 ms against
3.862) and 1.00x on the Mac Studio (1.969 against 1.975); delete 0.44x and 0.16x. Not run: adas-mac-studio-2, whose
Docker couldn't pull the base image (its credential helper hung) while another job's builds had been stuck there for
18 hours; adalundhe-macbook-pro, whose Docker Desktop asks to be signed in; adas-macbook-pro-2, Docker not running and
load 20.

## Goal 4, plan step 2: the seam with and without a network namespace, and sustained churn (2026-10-29, small N)

seam_oci's lifecycle_nonet and sustained phases (2404ec7) by both drivers, beside libcrun's, through `docker_pass.sh`
on fedora and adas-mac-studio-3: `RUNTIMES=libcrun`, `HEXBENCH_PHASES="lifecycle lifecycle_nonet sustained"`, 2 rounds
of 200 lifecycles after 20, the sustained phase 2,000 lifecycles at concurrency 32, no load gate, the load recorded. It
checks that the phases and the node's counts work end to end; the measurement of record is step 5's, with the pool.
Best of 2, p50 (ms):

| Machine | | create with / without | over | delete with / without | over | sustained /s (last window over first) | made on the path, of creates |
|---|---|---|---|---|---|---|---|
| fedora (Intel Core i9-14900HX, Linux 6.19.10, load 10.06-10.46) | Hex's seam, in process | 14.682 / 14.379 | 1.02x | 6.470 / 6.459 | 1.00x | 53 (1.05) | 2,000 of 2,000 |
| | Hex's seam, over the socket | 14.233 / 14.389 | 0.99x | 6.286 / 6.351 | 0.99x | 53 (0.99) | 2,000 of 2,000 |
| | libcrun in process | 2.945 / 2.413 | 1.22x | 10.394 / 10.381 | 1.00x | 1,644 (1.03) | |
| adas-mac-studio-3 (Apple M4 Max, LinuxKit 6.12.65, VM load 4.36-6.22) | Hex's seam, in process | 1.897 / 1.794 | 1.06x | 1.923 / 2.042 | 0.94x | 530 (0.99) | 2,000 of 2,000 |
| | Hex's seam, over the socket | 1.954 / 1.713 | 1.14x | 1.822 / 1.983 | 0.92x | 524 (1.05) | 2,000 of 2,000 |
| | libcrun in process | 1.916 / 1.633 | 1.17x | 11.569 / 11.877 | 0.97x | 1,439 (0.98) | |

Runs: fedora `~/.cache/hexpass/netns-step2-fedora-20261029T061904Z` (hex sha256 42dd7c68cb4ae797…, x86_64,
static-glibc; seam_oci cad5a780db23cf93…; libcrun driver 10ac19ada6d8035a…; one CI job beside it, CPUs 4-31);
adas-mac-studio-3 `~/.cache/hexpass/netns-step2-20261029T040340Z` (hex 3a5f6482f8ff12d8…, aarch64; seam_oci
7c6c9c7997298b0a…; driver 09d07ec1da05f497…); each driver from crun-1.30.1.tar.gz (99d036d95113552b…). No phase
recorded an error. The node keeps no pool yet, so each namespace a create made was made on its path, in every round,
by both drivers, on both machines: 220 of 220 creates in the lifecycle phase, none in lifecycle_nonet, 2,000 of 2,000
in the sustained phase, none taken from a pool or made ahead (the plan's test for a node with no pool). At this N the
namespace's cost on create moves between rounds (in process on fedora 0.30 and 0.63 ms, on the Mac Studio 0.34 and
0.10); the probe's 2 rounds of 500 put it at 0.571 and 0.544 ms (decision 4). Hex's seam sustains 53 lifecycles/s at
concurrency 32 on fedora against libcrun's 1,644, and 530 against 1,439 on the Mac Studio: on each machine about one
create's time per lifecycle (fedora's 14.7 ms, about 68 a second; the Mac Studio's 1.9 ms, about 526), as if its
creates ran one at a time, which its throughput at concurrency 1, 8 and 32 also showed (`probes/socket_cost.md`: 43,
55 and 52 a second in process, where libcrun's grew 63, 308 and 359). What holds them to that isn't measured here, and
is open. Each driver's best sustained round held level over its two windows (0.99-1.05); the Mac Studio's others moved
0.80-1.27, two windows being too few to read decay from. Fedora's kiln runner was offline from about 03:30Z to 05:50Z,
and the box rebooted at about 06:13Z, ending a first fedora attempt (05:51Z) after its phases had run but before its
summary, so the pass was run again. On the Mac Studio the pass image's base pull hung in Docker Desktop's credential
helper (`docker-credential-desktop get`, 16 minutes), so the pass ran with an empty Docker config: the image is public
and needs no credentials.

## Goal 4 of record (plan step 5): a container's network namespace from the pool, and sustained churn (2026-10-29)

seam_oci's lifecycle, lifecycle_nonet and sustained phases by both drivers, beside libcrun's, with the node's pool of
network namespaces made ahead (goal 4 plan step 4, 5257407; its production code is the same at 378b1d2, the tree the
passes built): on fedora by host.sh (`PHASES=oci RUNTIMES=libcrun SEAM_OCI_BIN=...`), on adas-mac-studio-3 by
`docker_pass.sh` and, at 6f73f9a (main: the same production code but for the reap race's fix, 6446160, which doesn't
touch create), on jupiter by `docker_pass.sh`; `HEXBENCH_PHASES="lifecycle lifecycle_nonet sustained"`, 2 rounds of
1,000 lifecycles after 100, the sustained phase 10,000 lifecycles at concurrency 32, no load gate (Ada's rule for a busy
bench host: the load recorded, not waited out). Best of 2, p50 (ms):

| Machine | | create with / without | over | delete with / without | over | sustained /s (last window over first) | made on the path, of the sustained phase's creates |
|---|---|---|---|---|---|---|---|
| fedora (Intel Core i9-14900HX, Linux 6.19.10, load 16.84 to 9.44, CI beside it) | Hex's seam, in process | 11.323 / 23.545 | 0.48x | 4.991 / 10.912 | 0.46x | 26 (1.06) | 0 of 10,000 |
| | Hex's seam, over the socket | 14.143 / 14.580 | 0.97x | 6.334 / 6.710 | 0.94x | 51 (1.14) | 0 of 10,000 |
| | libcrun in process | 3.057 / 2.406 | 1.27x | 10.413 / 10.387 | 1.00x | 1,294 (1.04) | |
| adas-mac-studio-3 (Apple M4 Max, LinuxKit 6.12.65, VM load 3.84 to 6.30) | Hex's seam, in process | 1.699 / 1.717 | 0.99x | 2.097 / 2.075 | 1.01x | 553 (0.94) | 0 of 10,000 |
| | Hex's seam, over the socket | 1.725 / 1.813 | 0.95x | 1.936 / 1.949 | 0.99x | 352 (0.48) | 0 of 10,000 |
| | libcrun in process | 1.970 / 1.629 | 1.21x | 11.600 / 11.846 | 0.98x | 1,429 (1.01) | |
| jupiter (AMD Ryzen AI Max+ 395, WSL2 6.18.54, VM load 1.71 to 8.44) | Hex's seam, in process | 8.590 / 8.298 | 1.04x | 3.734 / 3.625 | 1.03x | 108 (1.03) | 0 of 10,000 |
| | Hex's seam, over the socket | 12.714 / 12.661 | 1.00x | 5.395 / 5.391 | 1.00x | 66 (0.98) | 0 of 10,000 |
| | libcrun in process | 3.935 / 3.405 | 1.16x | 10.497 / 10.494 | 1.00x | 959 (0.96) | |

Runs: fedora `~/.cache/hex-bench/out/20261029T064153Z` (hex sha256 d5a123672795bbb5…, x86_64, static-glibc, built in the
pass image's Fedora 44 for its glibc-static; seam_oci 4e9ef2d68593572b…; libcrun 1.30.1, commit 079ff6a7…, in process;
CPUs 4-31), the host's clock before and after it (`probes/host_clock.sh`): a short burst's p50 0.108 and 0.115 ms on the
P-cores and 0.186 and 0.177 ms on the E-cores at rest, 0.107 and 0.116, 0.182 and 0.189 held awake; adas-mac-studio-3
`~/.cache/hexpass/g4-record-mac-20261029T063927Z` (hex 192629d1bb070de4…, aarch64, static-glibc, the same binary step
4's Mac Studio run measured; seam_oci 81e396ed990f0a77…; libcrun driver 2b1061455a7ce4dd…); jupiter
`C:\Users\corph\.cache\hexpass\g4-record-jupiter-20261029T112903Z` (hex 5e928bc423907648…, x86_64, static-glibc;
seam_oci ac201a2754a027d4…; libcrun driver 832260f4aa1732f6…); the Mac Studio's and jupiter's libcrun drivers from
crun-1.30.1.tar.gz (99d036d95113552b…). No phase recorded an error. On all three machines, by both drivers, in both
rounds, every create of the lifecycle phase took its namespace from the pool (1,100 of 1,100, the warm-up's with them;
1,131 or 1,132 made ahead, the 31 or 32 left in the pool at the phase's end) and so did every create of the sustained
phase (10,000 of 10,000), none made on the path and no make failed: one refill thread keeps up with 553 lifecycles a
second, and decision 4's second falsifier (more than 1% of a sustained phase's creates finding the pool dry) isn't met.

On the Mac Studio, whose quiet VM resolves it, a create from the pool costs what a create without a namespace costs
(0.99x in process, 0.95x over the socket), where libcrun's namespace costs it 1.21x and Hex's cost 1.23x before the pool
(`probes/netns_pool.md`); delete is unchanged (1.01x, 0.99x). Fedora's figures are the same pass under CI's load, which
moved Hex's create between rounds by more than the namespace costs: in process its p50 with the namespace was 11.323 ms
in round 1 and 24.644 in round 2, without it 24.195 and 23.545, so its best-of-2 ratio, 0.48x, pairs a quiet moment with
a busy one; each round's own ratio was 0.47x and 1.05x. Over the socket, where the rounds held steadier, they were 0.97x
and 0.98x. On fedora, too, Hex's create is 5-10x libcrun's either way (goal 5's gap), so a namespace is a small share of
it there. On jupiter (VM load 1.71 to 8.44) a create from the pool costs about what one without costs: 1.04x in process,
both best figures from its quieter first round (the second ran slower, 16.116 against 19.356 ms), and 1.00x over the
socket, where libcrun's namespace costs it 1.16x; delete is unchanged (1.03x, 1.00x).

Two things the sustained phase shows, neither the pool's: over the socket on the Mac Studio the rate falls by half over
the run, in both rounds (579 then 576, 560, 542, 459, 412, 269, 245, 188 and 279 lifecycles a second over its ten
windows of 1,000; the other round 577 down to 188-270), while in process it holds (570 to 536; the other round 491-599),
so something on the socket path slows with the lifecycles it has served; on fedora and jupiter, at about a tenth of that
rate, the socket's held (47-64 and 64-67 a second). And Hex's seam sustains 26 and 51 lifecycles a second on fedora
against libcrun's 1,294, 553 and 352 against 1,429 on the Mac Studio, and 108 and 66 against 959 on jupiter, as step 2's
passes found: about one create's time per lifecycle at concurrency 32. Both are open (`STATUS.md`).

## Goal 5, plan step 2: a box's record off create's path (2026-10-30, fedora)

seam_oci's lifecycle and sustained phases by both drivers, beside libcrun's, at eb6cae7 (built on cp/t7-g5-s2; 6f7ba39
here): each container's state on tmpfs under the node's runtime directory, a record whose directories all lie on tmpfs
written without its fsyncs, and no lock another box's create takes held across a record write (goal 5 plan step 2:
defda14, 1f4babe). host.sh (`PHASES=oci RUNTIMES=libcrun SEAM_OCI_BIN=...`), `HEXBENCH_PHASES="lifecycle sustained"`, 2
rounds of 1,000 lifecycles after 100, the sustained phase 10,000 lifecycles at concurrency 32, no load gate (the load
recorded).

**A change of method from this pass on.** The OCI pass's container mounts a tmpfs at `/run` (6f7ba39, host.sh and
docker_pass.sh), as a host has one, where the runtimes keep their state: libcrun's and crun's and runc's, and a root Hex
node's `/run/hex-node`. Every pass before it, goal 4's of record among them, kept every runtime's state on the
container's overlayfs. A ratio within one pass compares like setups; one taken across the change compares unlike ones,
and the sustained comparison with goal 4's pass below is one: libcrun's own rate went from 1,294 to 1,765 a second.

Best of 2 (each round's in brackets), ms:

| Machine | | create p50 | create p99.9 | delete p50 | sustained /s (last window over first) | made on the path, of the sustained phase's creates |
|---|---|---|---|---|---|---|
| fedora (Intel Core i9-14900HX, Linux 6.19.10, load 4.18 to 15.09, no other bench) | Hex's seam, in process | 1.928 (2.490 / 1.928) | 3.03 | 0.81 | 2,108 (0.94) | 248 of 10,000 |
| | Hex's seam, over the socket | 2.002 (2.002 / 2.080) | 3.01 | 0.88 | 2,095 (0.97) | 265 of 10,000 |
| | libcrun in process | 2.515 (2.786 / 2.515) | 4.10 | 10.30 | 1,765 (0.96) | |

- **Create, against libcrun's in the same run: 0.77x in process, 0.80x over the socket** (1.928 and 2.002 against 2.515
  ms; each round's own, 0.89x and 0.77x in process, 0.72x and 0.83x over the socket). Step 2's bar was at most 2x. In
  goal 4's pass of record it was 3.7x and 4.6x, nearly all of it the record's four fsyncs (12.187 of 18.297 ms in goal 5
  plan step 1's breakdown, `create-breakdown-20261030T000822Z`).
- **Sustained at concurrency 32: 2,108 and 2,095 lifecycles a second**, against goal 4's pass of record's 26 and 51:
  +8,008% and +4,008%, 81x and 41x (step 2's bar, +400%). That comparison crosses the change of method; within each
  pass, Hex's seam over libcrun went from 0.02x and 0.04x (26 and 51 against 1,294) to 1.19x and 1.19x (2,108 and 2,095
  against 1,765). The seam no longer runs about one create at a time: its rate held across the ten windows of 1,000
  (2,017-2,191 in process, 1,976-2,164 over the socket).
- **Decision 4's second falsifier is met: one thread refilling the network-namespace pool to 32 doesn't keep up at
  about 2,100 creates a second.** In each sustained phase 181-279 of the 10,000 creates (1.8-2.8%) found the pool dry,
  past the falsifier's 1%. Each made its namespace on its create's path, counted, and none failed; every create of the
  lifecycle phase, one at a time, took one from the pool (1,100 of 1,100). The pool held at goal 4's rates, 26 to 553
  a second, and fails at about 40x that. This run doesn't time a dry create (the sustained phase times its windows, not
  its creates); a namespace made on fedora costs 0.152-0.161 ms p50 and 0.333-0.401 ms p99 (`probes/netns_pool.md`,
  one thread, unloaded). At 2.5% of creates the dry ones sit in the tail above a create's p97.5, so a p99 or p99.9
  taken under concurrency reads them, as goal 5's p99.9 criterion (plan step 6) will.

Run: fedora `~/.cache/hex-bench/out/20261030T004533Z` (hex sha256 226b9315661e7a3b…, x86_64, static-glibc, built at
eb6cae7 in the pass image's Fedora 44; seam_oci 5de18d17af2cfcd4…; libcrun 1.30.1, commit 079ff6a7…, in process, driver
5c356f2139adc232…; CPUs 4-31), the host's clock before and after it (`probes/host_clock.sh`): a short burst's p50 0.320
and 0.114 ms on the P-cores and 0.501 and 0.163 ms on the E-cores at rest (the idle host clocks short bursts down before
the pass, not after), 0.114 and 0.102, 0.165 and 0.181 held awake. No phase recorded an error. A pass begun at 00:42:58Z
(`20261030T004258Z`) was stopped before any phase, since step 1's breakdown was measuring then; it is marked `ABORTED`.

## Goal 5 of record (plan step 6): create and create+start of `true` against a quarter of libcrun's (2026-11-05, fedora)

The bar (runtime design goal 5, decision 5): Hex's create p50 and create+start-of-`true` p50, and both p99.9, through
the seam at most 0.25x libcrun's in-process ones in the same run, by host.sh's OCI pass (`PHASES=oci RUNTIMES=libcrun
SEAM_OCI_BIN=...`), best of 2. The seam is both its columns, in process and over the node agent's socket, so a cell
holds only where both are under (the lead, 2026-11-05T03:51Z); in process is the binding one. host.sh's pass runs with
no owner unless `HEXBENCH_OWNER` names one, so the bar as written is the pass with none: no box's cgroup or network
namespace is handed on (decision 5, step 6's reuse within one owner). The pass with an owner beside it.

The tree: `cp/t7-g5-record` 70376d7, goal 5 extended's integrated tree (cp/t7-g5x-integrate b22a981) with goal 6
(cp/t7-g6 fde0ec1), the state directories in shards (cp/t7-ext-s6-shards 7179907) and the tail cut, inits parked
ahead in their cgroups so a create takes no `cgroup_mutex` (cp/t7-g5-tail 6b95c65, decision 8), with the merges' fixes.
Gates 7/7, 360 tests, and the root lab 8/8 there (unit hex-g5rec-gates-20261105T133811Z). Unit
hex-g5rec-pass-20261105T134425Z built hex (static-glibc, sha256 2c9e4b58aa55c980) and seam_oci (d6330e7307fafb75) at
it; libcrun 1.30.1 in process. N=10000 after 100, phases `lifecycle start_true`, CPUs 4-15 under the shared timed lock,
noisy (no load gate; about 23 containers of kiln CI beside it), `/run` a tmpfs. Every round's file holds its 10,000 in
each phase with no error, and no round crashed (no `crashed.txt`; host.sh's crashed-round check, 0892f3c).
Every pass in this section (fedora's, the Mac Studio's and jupiter's) ran at 70376d7, built before procs made ahead
for parked inits came into this tree (off by default at c6a3ac4, on by default for a root node at the merge of
cp/t7-g5-parkproc-default 3402c55): no node of these passes had them, so none of its figures rests on that setting.
The two columns differ in one setting: with `HEXBENCH_NOFILE` unset, the in-process node runs at the container's
soft descriptor limit, dockerd's 1,024, where it holds no `/proc/net` files (`oci_inside.sh:18-24`), while the
socket's `hex node` raises its own to the hard limit as it starts (`raise_descriptor_limit`,
`crates/hex-cli/src/node.rs:168`). The owner and its reuse reach both alike, in each box's spec.

Best of 2, ms (Hex over libcrun in the same pass):

| pass | | create p50 | create p99.9 | create+start of `true` p50 | create+start of `true` p99.9 |
|---|---|---|---|---|---|
| no owner, out/20261105T134534Z (load 16.30 to 16.82) | Hex in process | 0.36, 0.189x | 2.33, 0.463x | 0.33, 0.160x | 2.31, 0.406x |
| | Hex over the socket | 0.38, 0.200x | 2.12, 0.421x | 0.41, 0.199x | 2.53, 0.445x |
| | libcrun | 1.90 | 5.03 | 2.06 | 5.69 |
| reuse on, `HEXBENCH_OWNER=g5rec-owner`, out/20261105T135855Z (load 14.03 to 11.38) | Hex in process | 0.30, 0.152x | 0.92, 0.060x | 0.28, 0.133x | 0.85, 0.064x |
| | Hex over the socket | 0.31, 0.157x | 0.92, 0.060x | 0.33, 0.156x | 0.92, 0.069x |
| | libcrun | 1.98 | 15.38 | 2.11 | 13.36 |

- **With reuse on, every cell is under the bar in both columns.** Hex's own create p99.9 is under a millisecond in
  each round (0.93 and 0.71 in process, 0.86 and 0.92 over the socket, read from each round's file at sample
  floor(0.999 n)), so it holds against a quiet libcrun too: against the no-owner pass's 5.03 it would be 0.185x.
  libcrun's tail in this pass was the noisier (create p99.9 18.07 and 6.03 by round).
- **With no owner, the bar as written: both p50s are under in both columns, every p99.9 is over** (0.406x to 0.463x),
  Hex's at about 2.3 ms against a quiet libcrun's 5.03 and 5.69. Not met.
- **The tail cut is in every create.** In process, 10,099 of 10,107 and 10,098 of 10,106 lifecycle creates took an
  init parked ahead (none failed, none ended unclaimed), and 10,100 of 10,100 in `start_true`; with reuse on, 10,099 of
  10,100 in each round. No in-process create of either pass took over 8 ms, and the no-owner create's max fell to 3.08
  ms (r1 3.00, r2 3.08), where at 15a3323, before the cut, it was 18.78; there, with reuse on, 7 and 19 creates a
  round in process took over 8 ms (as 9 and 7 did at 2a91371), and none did here.
- **What holds the no-owner p99.9 is mostly Hex's own lock, not the cgroup lock the tail cut removed, and no other
  tenant's work** (`probes/goal16_ab.md` on cp/t7-g16-memslot acf45f9, "Whose work the no-owner tail waits on"): in
  its slowest 0.1% the create's lookups of its cgroup's files queue behind Hex's own refill's `mkdir` (1.377 ms),
  itself waiting on the host-wide cgroup-notification work; then the teardown of the pass's own boxes (0.3 to 0.85
  ms). Reuse within one owner makes no cgroup on the create's path and tears none down, which is the gap between the
  passes. This line read "the teardown of the box before it" until 2026-11-06 (superseded: below).
- **That lock wait is gone, and what holds the tail now is the kernel's teardown of the pass's own earlier boxes**
  (`probes/goal16_ab.md`, "No cgroupfs lookup on a no-owner create's path"; cp/t7-g16-prefetch 1190a2c, merged here at
  309e62b). The refill now opens the slot's files when it makes the cgroup ahead, so a no-owner create looks up
  nothing in cgroupfs (oci_reuse counted 20 lookups on the creating thread before, 0 after), and nothing a box sees
  changes. Its A/B at the bar's conditions (host.sh, no owner, 2 rounds of 10,000, A B B A, fedora, noisy) moved
  Hex's create p99.9 from a mean of 1.735 to 1.705 ms (2%) and create+start's from 2.54 to 2.45 (3.5%), both within
  the passes' spread: 0.36x and 0.40x of libcrun's 4.50 and 4.58 ms in the fixed arm's passes, not 0.25x. Traced, the
  fixed tree's slowest 0.1% have no cgroupfs lock wait; 1.40 ms of their mean is `css_killed_work_fn` preempting the
  create to tear down the run's own boxes (1.26 ms in the unfixed arm's trace), the rest Hex running (0.53) and RCU.
  So the bar as written stays unmet on fedora; that is now the kernel's teardown cost, 1.3 to 1.4 ms of the slowest
  0.1%, which only fewer cgroups torn down (reuse) removes. No pass of record followed the fix.

**Goal 16's finding: the kernel path that delays the slowest 0.1% of no-owner creates** (2026-11-06, fedora,
Linux 6.19.10; not of record: the tracers cost some). It is CPU preemption by the deleted box's memory-cgroup offline,
run by a per-CPU kworker on the next create's own CPU; not a lock the create waits on. Unit
hex-g5rec-g16trace-20261106T130219Z at a186d35 (the prefetch fix in, parked procs on): `probes/create_tail.sh` at the
bar's conditions (no owner, 2 x 10,000, CPUs 4-15 under the lock; out create-tail-20261106T130528Z, traced create p99.9
1.926 ms), with a bpftrace program beside it in a privileged container on CPUs 16-31 (fedora has no host perf or
bpftrace; the program and its maps under `~/.cache/hex-t7/g5rec/g16/` there). The path, in Linux v6.19:
1. An earlier delete's `rmdir` runs on a create CPU: `cgroup_rmdir` → `kill_css` → `percpu_ref_kill_and_confirm`, which
   calls `call_rcu_hurry` on that CPU (`lib/percpu-refcount.c:233`).
2. After the grace period its RCU callback runs in softirq on the same CPU. Traced, every `css_killed_work_fn` item was
   queued by `ksoftirqd/N` on CPUs 4-15 (11,900 to 14,300 a CPU on the even ones, 3,100 to 5,000 on the odd).
3. `css_killed_ref_fn` queues the offline with `queue_work(cgroup_offline_wq)` (`kernel/cgroup/cgroup.c:6062-6070`), a
   per-CPU workqueue (`alloc_workqueue("cgroup_offline", WQ_PERCPU, 1)`, `:6457`): a kworker bound to that create CPU.
4. `css_killed_work_fn` (`:6044-6059`) takes `cgroup_mutex` and takes each controller offline. The memory controller's
   `mem_cgroup_css_offline` is the cost: 128 to 256 us most often, 64 us to 1 ms, about 20,000 calls; cpu, blkcg and
   lru_gen offline 0 to 1 us each. It kept each create CPU busy 0.25 to 0.94 s over the run.
5. That kworker preempts the next create's thread (seam_oci, the seed, the init) on that CPU: 110 preemptions while it
   ran, 256 us to 2 ms (most 512 us to 1 ms), against 2 to 16 us for most other preemptions.
6. `create_tail_attr.py`'s per-create walk agrees: the slowest 0.1% add +1.368 ms of `css_*_work_fn` preemption over the
   middle band, +0.271 ms run queue, +0.086 ms Hex's own runs, and nothing waiting on `cgroup_mutex`.

**What's left of goal 16's tail, and why** (2026-11-06, fedora, Linux 6.19.10, and the 7.2.15 guest; not of record:
kprobes and bpftrace cost each callback some, so the figures compare runs, they don't time them). `probes/css_cost.sh`
at 53ac40d (2,000 boxes each, no owner, CPUs 4-15 under the lock), the same binaries throughout (pass g16-a186d35),
with bpftrace beside it (`~/.cache/hex-t7/g5rec/g16/` on fedora: units hex-g5rec-g16sb-20261106T132322Z and
hex-g5rec-g16c-20261106T135913Z).
- The memory offline is the kernel's walk of every memcg-aware list_lru on the host. `mem_cgroup_css_offline` is 0.20 to
  0.34 ms a box across four runs (load 8 to 24), `memcg_reparent_list_lrus` 85 to 87% of it,
  `reparent_shrinker_deferred` 0.019 to 0.024 ms, the rest about 0.02. Linux visits each list_lru under
  `list_lrus_mutex`, inside `css_killed_work_fn`'s `cgroup_mutex`: 6.19 takes the xarray's IRQ lock and stores for each
  (`mm/list_lru.c` v6.19.10:467-503), 7.2 loads it lockless (v7.2:525-563, 98733f3f0bec). The box's own entries aren't
  the cost: walks whose memcg held an entry in no list_lru took 355 us on average, those with 2 to 16 took 193 to 613
  (the walk's `kvfree_call_rcu` calls counted, one per list_lru that held an entry).
- Hex's share of those list_lrus is at most 2.5%. Fedora had 1,923 to 1,979 superblocks mounted (two memcg-aware
  list_lrus each, so about 3,900) across the runs; Hex's pooled ones are inside that run-to-run spread. Each box makes and
  destroys three (its proc, its `/dev` tmpfs, its ipc namespace's mqueue), the mounts its spec asks for: 12,204 to
  12,222 list_lru inits and 12,218 to 12,252 destroys per 2,000 boxes, with Hex's pools on or off. With them off the
  create's own thread waits on `list_lrus_mutex` behind other memcgs' walks (464 to 483 waits a run, 88 to 98 us each);
  with them on only the pools' threads do. The A/B's walk times (0.17 ms off, 0.21 to 0.29 on) follow the host's load
  (8 to 13 against 19 to 24), not the pools.
- The cost per list_lru per offline: 43 to 75 ns on fedora's 6.19 (0.168 to 0.291 ms over about 3,900), rising with
  load; 56 ns in the 7.2 guest, whose walk is 0.013 ms with its own 27 mounts and 0.230 ms with 1,950 tmpfs superblocks
  added (fedora's count; offline 0.025 to 0.256 ms a box). So 7.2 is no cheaper at the same count: an upgrade alone
  doesn't move this tail. No posted or merged kernel work (v7.2, master, mm-unstable, linux-next, to 2026-11-06) walks
  only the dying memcg's lists; Waiman Long's skip of empty lists (405cc51fc104, 2022) was reverted over races
  (b33e1044475a).
- The release: `css_release_work_fn` 0.216 ms a box, of which `css_rstat_flush` 0.069 (about 11.5 calls a box) and the
  rest its wait for `cgroup_mutex`; `cgroup_bpf_release` 0.058 ms a box beside it.
- Re-split, the trace above's slowest 0.1% are 1.10 ms preempted by these offlines and 0.27 ms by these releases, all
  of the pass's own boxes. What Hex could cut there is at most about 0.4 ms; 0.25x needs about 0.6 to 0.7 ms off. The
  rest is the walk, sized by every tenant's superblocks.

`cgroup_mutex`, which `css_killed_work_fn` holds across the memcg offline, is contended too: Hex's threads waited 925
ms (seam_oci), 478 ms (the seed and inits) and 336 ms (the refill) in all, most 64 to 256 us, in `clone3`
(`CLONE_INTO_CGROUP`, `cgroup_css_set_fork`, `:6694`), `cgroup_rmdir` and the refill's `cgroup_mkdir`: parks, execs,
deletes and the refill, not the create, whose init is parked. `cgroup_threadgroup_rwsem`: 0.35 ms in all.

**What the slowest 0.1% of no-owner creates wait on: the first reading, superseded** (the lead, 2026-11-05T14:20Z;
not of record: the tracer costs each switch some). Its figures stand; its conclusion doesn't. `create_tail_attr.py`
charged only the runtime thread's own state, so the runtime's wait in `ppoll` for its init (+0.552 ms below) hid what
the init was waiting on. The tail agent's trace of 2026-11-06 followed that wait into the container's process
(`probes/goal16_ab.md`, cp/t7-g16-memslot acf45f9, "Whose work the no-owner tail waits on"): the create's cgroupfs
lookups queued behind Hex's own refill's `mkdir`, 1.377 ms of the slowest 0.1%, with the css teardown of the pass's
own boxes 0.3 to 0.85 ms and no other tenant's. The first reading follows as it was. The tail agent's `probes/create_tail.sh` on this pass's own binaries (hex 2c9e4b58aa55c980, seam_oci
d6330e7307fafb75), no owner, N=10000, 2 rounds, CPUs 4-15 under the lock, load 6.93 to 7.08: unit
hex-g5rec-tailattr-20261105T144322Z, fedora `~/.cache/hex-bench/out/create-tail-20261105T144322Z`, 10,000 creates traced,
each one's charges summing to its span. Traced creates p50 0.316, p99 1.039, p99.9 1.670, max 2.771 ms (the quieter
host than the pass's). `create_tail_attr.py`'s grouping, the top 0.1% over the middle band (p45-p55), ms:

| kind | top 1% over mid | top 0.1% over mid |
|---|---|---|
| preemption by the cgroup teardown work items (`css_*_work_fn`) | +0.557 | +0.786 |
| other: the runtime blocked in `ppoll` until its init said it was ready | +0.221 | +0.552 |
| the run queue, woken and not yet run | +0.127 | +0.315 |
| the runtime's own runs | +0.047 | +0.048 |
| RCU softirq | +0.027 | +0.040 |

Of the ten slowest creates (1.670 to 2.771 ms), five were preempted on their CPU by `css_killed_work_fn` for 1.240 to
1.940 ms, three waited 1.419 to 2.059 ms on their init (its own delay, seen from the runtime), and two sat 1.452 and
1.700 ms on the run queue. The runtime's own work is about 0.3 ms in every band (0.286 mid, 0.334 in the top 0.1%).
The slowest starts are the same: their top 0.1% is +0.414 and +0.210 ms of `css_killed_work_fn` preemption. Its
conclusion then (superseded, above): "what holds the no-owner p99.9 is the teardown of the boxes deleted before, run on
the CPUs the next creates use".

The host's clock (`probes/host_clock.sh`) before: load 16.50, a short burst's p50 0.096 ms on the P-cores and 0.159 on
the E-cores at rest, 0.107 and 0.172 held awake; after: load 11.27, 0.131 and 0.230 at rest, 0.120 and 0.171 held.

**The same on the Mac Studio, through `docker_pass.sh`** (goal 5 plan step 6, 2026-11-06). Ada's Mac Studio (3)
(kiln runner `adas-mac-studio-3.local`), Apple M4 Max, its containers in Docker Desktop's Linux VM (kernel
6.12.65-linuxkit, aarch64, 14 CPUs, the pass pinned to 2-13, 31 GiB). docker_pass.sh (ade0653, 5c3aa9b: the owner
and reuse handed through, crun's tarball given since that Mac has no route out, checked against the pinned sha256)
built hex (static-glibc, sha256 9562fabc74ed450d) and seam_oci at 70376d7, the fedora pass's tree, in the pass image.
A one-shot launchd job (hex.g5rec.pass.20261106T040214Z), no owner and then `HEXBENCH_OWNER=g5rec-owner
HEXBENCH_REUSE=1`, phases `lifecycle start_true`, N=10000 after 100, best of 2. Every round's file holds its 10,000
in each phase with no error, no round crashed, and the parked inits were claimed 10,099 times a round in process
(none failed). Results on that Mac: `~/.cache/hexpass/g5rec-20261106T040214Z/{noowner,reuse}`.

| pass | | create p50 | create p99.9 | create+start of `true` p50 | create+start of `true` p99.9 |
|---|---|---|---|---|---|
| no owner (VM load 3.41 to 5.68) | Hex in process | 0.20, 0.107x | 0.48, 0.127x | 0.25, 0.102x | 0.64, 0.103x |
| | Hex over the socket | 0.28, 0.150x | 0.66, 0.175x | 0.39, 0.160x | 0.81, 0.131x |
| | libcrun | 1.87 | 3.77 | 2.44 | 6.19 |
| reuse on (VM load 3.26 to 6.30) | Hex in process | 0.20, 0.107x | 0.48, 0.122x | 0.24, 0.099x | 0.61, 0.108x |
| | Hex over the socket | 0.26, 0.139x | 0.59, 0.151x | 0.37, 0.152x | 0.76, 0.134x |
| | libcrun | 1.87 | 3.92 | 2.43 | 5.66 |

- **On the Mac Studio every cell is under the bar, in both columns and both modes**, the bar as written (no owner)
  included, with libcrun's tail quiet (create p99.9 3.77 and 3.92 ms). Hex's own create p99.9 there is about half a
  millisecond (0.58 and 0.49 by round in process with no owner, read at sample floor(0.999 n)), against fedora's 2.33
  with no owner. Why isn't measured here (no trace ran on the Mac). Fedora's no-owner tail is mostly the create's
  cgroupfs lookups queued behind Hex's own refill's `mkdir`, that `mkdir` waiting on the host-wide cgroup-notification
  work (above); what differs is the host around it: the VM ran nothing else (load 3.26 to 6.30 in 14 CPUs, against fedora's 16 to 17
  with kiln CI beside the pass), and its kernel is 6.12, not 6.19.
- The Mac's own load was 2.38 at the job's start and 2.73 at its end.

**And on jupiter** (2026-11-06): Windows, AMD Ryzen AI MAX+ 395, Docker Desktop's WSL2 VM (kernel 6.18.54.1, x86_64,
32 CPUs, the pass pinned to 4-31, 78 GiB), docker_pass.sh building hex (static-glibc, sha256 568e107e06b2d915) and
seam_oci at 70376d7 in the pass image, N=10000, best of 2. Amphora's benchmark chain there last wrote on 2026-11-01
and ran nothing; step 4's own Windows-container dockerd ran beside Docker Desktop, on its own pipe, and one container
(`w28host`) not of this pass ran in Docker Desktop's engine.

| pass | | create p50 | create p99.9 | create+start of `true` p50 | create+start of `true` p99.9 |
|---|---|---|---|---|---|
| no owner (VM load 3.16 to 3.51) | Hex in process | 0.81, 0.200x | 2.00, 0.310x | 0.89, 0.208x | 2.81, 0.342x |
| | Hex over the socket | 0.98, 0.242x | 3.16, 0.489x | 1.23, 0.287x | 2.71, 0.330x |
| | libcrun | 4.05 | 6.46 | 4.28 | 8.22 |
| reuse on (VM load 1.27 to 2.47) | Hex in process | 0.80, 0.200x | 1.42, 0.182x | 0.88, 0.200x | 1.47, 0.166x |
| | Hex over the socket | 0.94, 0.236x | 1.67, 0.214x | 1.17, 0.265x | 2.02, 0.228x |
| | libcrun | 3.99 | 7.81 | 4.41 | 8.85 |

Every round's file holds its 10,000 in each phase with no error and none crashed (checked with PowerShell, jupiter
having no Python). With no owner, jupiter is as fedora is: both p50s under in process, both p99.9s over (and over the
socket the create+start p50 too). With reuse on, every cell is under in process, and over the socket all but the
create+start p50 (0.265x). The reuse-on pass ran at 09:15Z (hex sha256 568e107e06b2d915 again): the first try, at
06:00Z, died when jupiter's C: drive, which holds Docker Desktop's VM disk, filled from other writers (67 MB free at
06:27Z) and Docker Desktop stopped; its disk wasn't freed. Once the lead had freed room (34.5 GB at 09:12Z, idle build
folders compressed in place), Docker Desktop was started again and the pass ran with C: at 36 to 38 GB free.

The passes before it, on the way (same method, fedora, noisy; each round read whole): at 6eb38bd (goal 6 merged) no
owner out/20261105T030924Z 0.286x, 0.191x, 0.255x, 0.251x (libcrun create p99.9 22.15) and reuse on
out/20261105T034248Z 0.187x, 0.071x, 0.160x, 0.035x (the socket's create p99.9 0.898x); at 2a91371 no owner
out/20261105T082351Z 0.219x, 0.043x, 0.236x, 0.126x (libcrun 65.19) and reuse on out/20261105T084842Z 0.212x, 0.369x,
0.150x, 0.136x; at 15a3323 (the shards) no owner out/20261105T124235Z 0.245x, 0.502x, 0.221x, 0.635x (libcrun 5.62)
and reuse on out/20261105T125319Z 0.190x, 3.20x, 0.166x, 0.659x. Each in the column order above, in process. The
p99.9 ratios move with libcrun's own tail (5 to 65 ms across them) while Hex's held at 2 to 4 ms until the tail cut.

## Goal 6 of record: mount namespaces without copying (2026-11-09, adacachy2)

The bar (runtime design goal 6, decision 6; the lead's goal 17): on Linux 7.0 and later create builds the container's
mount namespace from its tree (`OPEN_TREE_NAMESPACE`), and on 7.1 empty (`CLONE_EMPTY_MNTNS`), detected at runtime,
the classic path remaining for older kernels, create-time hooks and user namespaces. Done when (1) goal 0's seam driver
on a 7.0+ bench host shows create p50 on the new path below the classic path's in the same run; (2) the faster path is
the default; (3) runtime-tools passes the 28 tests crun passes; (4) Ada's under-load bar: part 19's p99 at most 0.5 ms
at 8 in flight with 8,000 idle threads, 6 and 12 busy loops at nice 0 on the same cores, Docker's default masked and
read-only paths, the whole create's p99 improving with it, and no priority, nice, RT or slice change.

The record: `mntns_record.sh host` at cp/t7-g6 0c29b3ec on adacachy2 (Linux 7.3.3-1-cachyos, both calls native; AMD
Ryzen 9 9950X3D2; the cells on CPUs 8-15,24-31, the L3 domain without CPU 0), run mntns-host-20261109T171050Z, load1
13.7-28 over its cells. In process through seam_oci's serve phase: rounds.py interleaves the arms round by round, each
round's order rotated and the host's load read, 299 rounds after one warmup a cell (Ada's rule for a p99 claim at 95%),
8 lifecycles at 8 in flight a burst, 8,000 idle threads, owner bundles with reuse on and Docker's masked and read-only
paths, untraced and then traced (part 19 to the init's chdir). Two phases, each best of 2 at 6 and 12 loops:

- **A**, the classic root and the three ways interleaved, for the verdicts against the classic root. The classic arm
  is a root node built by hex-node's test control `Node::with_classic_roots`, its creates counted under
  `ClassicReason::Baseline`. Its seeds keep the agent's mount namespace, so each init copies it, binds the rootfs and
  its mounts, `pivot_root`s, detaches the old root, and makes its own `/dev` and masked directories, as the classic
  path does. The record refuses an arm whose creates weren't all that.
- **B**, the three ways alone, for part 19's bar and the default. The first record with the classic arm, rec17
  (mntns-host-20261109T165122Z at 3c19be23, every arm in one phase), had every way's part 19 p99 at 2-6x the c45b03f
  record's (empty 104/135 µs at 12 loops became 805/717). A probe over its traces put the slowest 1% at 12 loops 80%
  blocked in `move_mount` and `mount_setattr` (switched out not runnable, where those wait on the mount lock, whose
  holders the trace can't name: `kptr_restrict` hides `namespace_sem`), preempted 15%, on a CPU 3%. The classic arm's
  whole-namespace copies and detached old roots on that lock were the suspect, so the bar and the default are read
  without it. In this record the ways' part 19 p99 at 12 loops is 178-459 µs in phase A too, at the same load: rec17's
  tails were mostly the host at the time.

| phase | loads | arm | create p50, ms (p1 / p2) | create p99, ms (p1 / p2) | part 19 p99, µs (p1 / p2) |
|---|---|---|---|---|---|
| B | 6 | copy | 0.325 / 0.320 | 0.817 / 0.692 | 185 / 121 |
| B | 6 | empty | 0.324 / 0.318 | 0.749 / 0.746 | 130 / 144 |
| B | 6 | tree | 0.327 / 0.326 | 0.763 / 0.767 | 159 / 140 |
| B | 12 | copy | 0.471 / 0.440 | 2.390 / 1.973 | 183 / 211 |
| B | 12 | empty | 0.472 / 0.436 | 2.668 / 1.782 | 243 / 294 |
| B | 12 | tree | 0.492 / 0.445 | 2.780 / 1.611 | 457 / 246 |
| A | 6 | copy | 0.315 / 0.334 | 0.775 / 0.850 | 343 / 155 |
| A | 6 | classic | 1.696 / 1.828 | 2.766 / 3.113 | 22,595 / 3,056 |
| A | 12 | copy | 0.445 / 0.725 | 2.305 / 5.440 | 323 / 178 |
| A | 12 | classic | 2.368 / 3.822 | 10.813 / 17.168 | 8,941 / 16,147 |

- **(1) met.** In phase A the default (copy) against the classic root: create p50 0.315 / 0.334 ms against 1.696 /
  1.828 at 6 loops, and 0.445 / 0.725 against 2.368 / 3.822 at 12, below it in 298, 298, 299 and 292 of 299 rounds.
- **(2) met, by the default rule** (the lead's, 2026-11-09): p50 decides only where a one-sided sign test on the rounds
  below, over every timed round of phase B, shows a difference at 95%; otherwise part 19 p99 at the heaviest load does,
  then create p99 there. No way's p50 is below each other's (copy below tree in 658 of 1,196 rounds, below empty in 604,
  tree below empty in 533; 627 needed). The ways tie at p50, as every run before had them: tree and empty below the copy
  in 124-161 of 299 rounds, cell by cell, a coin flip that had picked the default by noise. Part 19 p99 at 12 loops,
  best of 2: copy 183 µs, empty 243, tree 457 (the other pass 211, 294, 246). So the default is the copy, then empty,
  then tree (mntns_default.txt; HEX-MNTNS-9, HEX-MNTNS-1, with `mntns::order` taking a measured record in its own
  order). The new calls stay detected and a node's to ask for (`Node::with_mount_namespace`).
- **(3) met.** runtime-tools: hex passes 28 of the 28 tests crun 1.27 passes.
- **(4) met.** Phase B, the default (copy): part 19 p99 185 µs (the other pass 121) at 6 loops and 183 (211) at 12,
  under the 500 µs bar; every way at most 457 µs at 12 loops. Against the classic root the whole create's p99 improved
  in all four cells of phase A. No priority, nice, RT or slice change.
- **The caveat.** Classic against the default measures goal 5's prune, with the node's `/dev` and masked-directory
  pools the classic init makes its own of, and goal 6's ways together: it is the comparison the goal's text names. Goal
  6's own record of the ways is each new way against the copy in phase B: tree's create p50 below the copy's in 0 of 2
  passes at each load, empty's in 2 of 2 at 6 loops and 1 of 2 at 12, inside the tie the sign test reads. Phase A's
  second pass at 12 loops met load1 25-27, the ways' create p99 at 5.4-7.3 ms there and the classic root's at 17.2.

On the way:
- **L1, taken** (red caddac84, green 9bee9c97). A parked init leads its session (`setsid`) once it has said it is
  parked, before its claim, so neither the create that claims it nor the delete that parked it takes the reschedule its
  move to a new autogroup forces (kernel/sched/core.c `sched_move_task`). In the c45b03f record's traces at 12 busy
  loops it was a mean of 128.8 µs of every claimed create (p99 1,618), mostly preempted.
- **Pre-faulting at park, measured and not taken.** A claimed init takes 23-26 page faults between its run and its
  ready: 15-17 on text, which fork leaves unmapped in the child, 4-6 copy-on-write and 2-3 first touches, about 0.87 µs
  each. `MADV_POPULATE_READ` of the binary's 587 text pages costs 28.4 µs median in a fresh process on adacachy2,
  against the 10-14 µs the claim's text faults cost. That is CPU at park traded for latency at create, which loses
  throughput (the lead, 2026-11-09).
- **The init's second parse, measured and held in reserve.** bench spec_parse (46c95d30, release, 299 fresh processes,
  the record's 1,567-byte spec; unit hex-g6-specparse-20261109T165100Z): the first parse and check 15.8 µs median (21.4
  p99) with 9 faults, warm 8.2; `Spec::check` alone 0.08 µs; the claim's decode 0.67 µs first. A handoff the init reads
  without JSON would save an estimated 8-10 µs a create. It needs a format beside Hex's own wire format and must survive
  an upgrade between a node and a seed of another build, which JSON does by construction, so it waits (the lead,
  2026-11-09).

## Goal 7: the process class through the seam with no wrapper (2026-11-04, fedora)

Implementation goal 7 (decision 7). A run of `true` through the `Runtime` seam is now one call:
`Runtime::run`, then a wait for its end, which comes once the node agent has removed the box. The program is
cloned vfork-class into its box's cgroup. host.sh ran box_bench.py (`hex box run`, a systemd scope, a systemd
service) and seam_box in the same run, then the fork-class seam_box, round by round (`timed.sh` below). The binaries
measured are `cp/t7-g7`'s (b82c7ff, and 8660a5e for the fork-class clone), from before goal 7 was carried onto the one
client core and wire (`cp/t7-port-g7`, 2026-11-09), which is not measured here.

- **Where.** fedora (kiln-a094a12d, Intel Core i9-14900HX, Linux 6.19.10-300.fc44), every method pinned to
  CPUs 4-15, under the shared timed lock (`fedora-timed.lock`), 2026-11-04T08:54:41Z to 09:18:38Z.
- **Noisy, by design.** Run with LOAD_GATE=0, as Ada asks of every bench (bars hold under load). The
  1-minute load was 11.06, 10.62 and 15.93 at the three rounds' starts and 27.33 at the end. About 25 other
  containers ran beside it the whole time (`cotenants.txt`).
- **Binaries.** `hex` b82c7ff (sha256 e84fe970608bb352); seam_box b82c7ff (9ccb4f07884a6868); the
  fork-class base, seam_box 8660a5e (e0ce1b07fd17cdc1).
- **Runs.** `~/.cache/hex-t7/g7/bench/out/g7-20261104T085441Z` and `…-forkclone`. Each round: N 10,000
  (100 warm-up), 2,000 lifecycles at each concurrency, density to 1,000 boxes. Best of 3.

| Axis | Hex seam | systemd scope (same run) | systemd service | `hex box run` | Hex seam vs scope | Goal 7's bar |
|---|---|---|---|---|---|---|
| run of `true` p50 (ms) | 0.41 | 5.82 | 11.26 | 3.91 | 14.10x | at most a quarter of the scope's: **met** (also against 7.13 of record: 17.4x) |
| run of `true` p99 / p99.9 / max (ms) | 2.78 / 4.86 / 24.93 | 8.33 / 12.75 / 30.77 | 35.78 / 49.59 / 133.74 | 7.03 / 9.16 / 13.81 | 3.00x / 2.62x / 1.23x | (decision 8's) |
| runs/s at 1 / 8 / 32 | 1,683 / 2,526 / 2,583 | 197 / 170 / 157 | 79 / 148 / 179 | n/a (below) | 8.52x / 14.88x / 14.39x (vs the service at 32) | |
| CPU per run (ms) | 0.55 | 6.39 | 12.97 | 3.53 | 11.59x | at most a quarter of the scope's: **met** (also against 7.81 of record: 14.2x) |
| memory held per idle box (MiB) | 0.27 | 0.30 | 0.33 | 0.80 | 1.12x | at most the scope's: **met** (also against 0.28 of record) |
| density (idle boxes per GiB held) | 3,800 | 3,384 | 3,100 | 1,282 | 1.12x | |

By round, the seam's held memory per idle box was 0.2694, 0.2711 and 0.2730 MiB over 1,000 boxes. The scope's
was 0.3037, 0.3026 and 0.3029.

**The vfork-class clone, A/B in the same rounds** (seam_box 8660a5e is the same run path with the fork-class
clone; round by round, vfork / fork):

| Round (load at start) | run of `true` p50 (ms) | CPU per run, median (ms) | runs/s at 32 |
|---|---|---|---|
| 1 (11.06) | 0.413 / 0.471 | 0.447 / 0.515 | 2,583 / 1,993 |
| 2 (10.62) | 0.441 / 0.529 | 0.475 / 0.579 | 2,294 / 1,638 |
| 3 (15.93) | 0.794 / 0.862 | 0.834 / 0.958 | 1,444 / 1,385 |

**cgroup_mutex, per run of `true`** (`probes/run_clone.sh`, `run-clone-20261104T091838Z`, b82c7ff, CPUs 4-15,
load 27.3 at its start, right after the timed run; µs; `probes/run_clone.md` reads it):

| | concurrency 1 (2,020 runs) | concurrency 32 (20,000 runs, 2,310/s) |
|---|---|---|
| clone into the cgroup, `cgroup_can_fork` to `cgroup_post_fork`'s return (the lock's wait and hold) | 51.9 | 262.7 |
| `cgroup_mkdir`, whole | 154.3 | 217.3 |
| `cgroup_rmdir`, whole | 52.0 | 100.7 |
| `cgroup_kill_write`, whole (8% of runs at 1, 0.7% at 32) | 1.4 | 1.9 |
| the runtime's own waits for the lock, within those | 139.8 (0.53 waits) | 434.5 (1.67 waits) |
| everyone else's waits for it, over the same time | 437.4 (3.43 waits) | 2,758.3 (10.30 waits) |

**`hex box run` at concurrency 32.** box_bench.py's own removal of a worker's cgroup failed with EBUSY at
`.hexbox-run` in each round, so the CLI wrapper's throughput has no reading here. Goal 7 didn't change that
path. The cause was the driver's: it removed the cgroup as soon as wait4 reaped the call. On Linux 6.19 a
reaped task leaves its cgroup only at its final switch (`check_box_removal.py`), so the rmdir sometimes came
first. Fixed on `cp/t7-boxrun-ebusy` (869712d): remove_tree waits for `cgroup.events` to read `populated 0`.

The throughput phase again, with that fix (run `~/.cache/hex-t7/g7/bench/out/20261104T142342Z`, 2026-11-04T14:23:42Z,
CPUs 4-15, under the timed lock, noisy: load 37.27 at the start and 28.90 at the end; `hex` b82c7ff, sha256
e84fe970608bb352; 3 rounds of 2,000 lifecycles, best of 3; no phase error in any round):

| runs/s at 1 / 8 / 32 | `hex box run` | systemd scope | systemd service |
|---|---|---|---|
| best of 3 | 260 / 585 / 529 | 143 / 145 / 113 | 67 / 149 / 164 |
| by round, at 32 | 514, 466, 529 | 106, 113, 111 | 164, 162, 149 |

**timed.sh** (on fedora, `~/.cache/hex-t7/g7/timed.sh`): for each of 3 rounds,
`OUT_DIR=… ROUND_FIRST=r PHASES="box boxseam" SEAM_BIN=<seam_box b82c7ff> bash host.sh <hex b82c7ff>`, then
`HEXBENCH_PHASES="latency throughput" OUT_DIR=…-forkclone ROUND_FIRST=r PHASES=boxseam SEAM_BIN=<seam_box
8660a5e> bash host.sh <hex b82c7ff>`, with `CPUSET=4-15 LOAD_GATE=0 FETCH=0 N=10000 WARMUP=100 LIFECYCLES=2000
CAP=1000 BOX_BUDGET_MIB=4096 ROUNDS=1`. The run's load and co-tenants were logged before each round.

## What the run shows

- **OCI class: Hex misses the bar, and crun is the competitor to beat.**
  - crun is 3-9x faster than runc and youki on create and start.
  - Hex is behind crun on create (0.33x), start (0.08x), create+start (0.21x), CPU (0.67x) and held
    memory and density (0.62x). At concurrency 32 its throughput is 0.67x crun's (1,146 against 1,711/s),
    re-measured 2026-10-25 through the new driver; the old lock-serialized driver had capped crun's rate
    there and made Hex look level (0.99x).
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
