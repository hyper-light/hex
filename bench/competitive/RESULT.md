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
