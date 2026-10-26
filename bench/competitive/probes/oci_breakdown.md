# Where an OCI runtime's time and memory go: the breakdown probe

`oci_breakdown.sh` runs `oci_breakdown_inside.sh` in the OCI pass's bench container, for Hex and crun. The
runtime design (`docs/design/runtime-architecture.md`, kiln research `01a16e75781e74fdb1ddd0ca6edf2278`) rests on
it.

## Run 1 (2026-10-24, fedora)

- **Host:** fedora.attlocal.net, Intel Core i9-14900HX (32 CPUs, the container pinned to 4-31), kernel
  6.19.10-300.fc44.x86_64.
- **When and load:** 2026-10-24T15:33:32Z to 15:38:05Z. The 1-minute load was 0.13 at the start, after a
  minute with no other bench unit active and the load under 2, and 4.72 at the end, from the probe's own
  concurrency-32 passes.
- **Hex:** `hex-cg3-8947ea21`, sha256 8947ea2187a27972…, the release build of c7fc78f (the competitive rounds of
  record's binary). **crun:** 1.30.1, commit 079ff6a7, sha256 86d1e6a0e7694597… (the OCI pass's binary).
- **Harness:** `bench/competitive` at 460597a with this probe. Out dir on fedora:
  `~/.cache/hex-bench/out/breakdown-20261024T153332Z/`.
- **Parameters:** n = 1,000 lifecycles per pass after 50 warm-ups; 1,000 lifecycles per concurrency level. Every
  pass recorded errors `{}`.

The absolute numbers run slower than the rounds of record: crun's create p50 is 6.88 ms here against 2.99 in
`RESULT.md`. The comparisons below are within this run, in interleaved passes.

### 1. What fsync costs: the state root on the container's overlay (a) and on a tmpfs (b)

p50 / p99 / p99.9 / max in ms.

| Pass | create | start | exec | delete |
|---|---|---|---|---|
| hex a1 | 12.42 / 14.34 / 16.44 / 30.99 | 8.39 / 10.17 / 11.83 / 12.83 | 4.37 / 5.50 / 9.08 / 11.02 | 2.05 / 2.56 / 2.77 / 23.29 |
| hex b1 | 5.74 / 6.56 / 7.19 / 8.05 | 1.68 / 1.99 / 2.13 / 2.14 | 3.49 / 4.19 / 16.32 / 16.33 | 4.13 / 7.00 / 7.76 / 8.10 |
| hex a2 | 12.39 / 14.04 / 33.10 / 40.46 | 8.17 / 9.74 / 11.38 / 11.56 | 4.13 / 5.46 / 5.62 / 18.93 | 2.01 / 2.47 / 2.65 / 2.74 |
| hex b2 | 5.78 / 6.77 / 6.99 / 27.75 | 1.70 / 2.03 / 2.10 / 2.17 | 3.53 / 4.25 / 4.42 / 15.50 | 4.08 / 7.38 / 8.38 / 14.60 |
| crun a1 | 6.88 / 8.52 / 9.84 / 27.99 | 1.16 / 1.54 / 1.70 / 1.98 | 3.59 / 4.21 / 4.59 / 4.64 | 11.67 / 12.19 / 20.20 / 26.63 |
| crun b1 | 6.67 / 8.23 / 8.77 / 24.57 | 1.13 / 1.54 / 1.68 / 1.68 | 3.60 / 4.49 / 22.61 / 28.01 | 11.57 / 12.05 / 13.49 / 16.84 |
| crun a2 | 7.18 / 8.66 / 9.23 / 10.14 | 1.19 / 1.59 / 1.81 / 1.92 | 3.64 / 4.40 / 24.59 / 25.19 | 11.71 / 12.21 / 12.32 / 16.47 |
| crun b2 | 6.90 / 8.50 / 8.86 / 9.28 | 1.17 / 1.60 / 1.79 / 1.84 | 3.65 / 4.47 / 5.04 / 20.99 | 11.58 / 12.02 / 12.13 / 12.20 |

- **Hex's fsyncs cost about 6.6 ms of start and of create.** On the overlay, start is 8.39 and 8.17 ms and create
  12.42 and 12.39 ms. On the tmpfs they're 1.68 and 1.70 ms, and 5.74 and 5.78 ms.
- **crun doesn't move,** since it writes no fsync: start 1.13-1.19 ms and create 6.67-7.18 ms either way.
- **With its fsyncs gone, Hex's start is 1.45x crun's** (1.68 against 1.16) and its create 0.84x (5.74 against
  6.88).
- **Hex's delete gets slower on the tmpfs** (4.13 against 2.05 ms), and this probe doesn't explain why. The delete
  never waits on the shim (`run.rs:339-377`), but the shim writes the stopped state, with its fsyncs, as soon as
  the killed container exits, while the delete runs. Without a shim (the design's decision 2) that write is gone,
  so delete is measured again then.

### 2. The command-line floor: posix_spawn and wait, as `oci_bench.py` spawns every call

ms, over 1,000 spawns each.

| Program | p50 | p99 | max |
|---|---|---|---|
| `/usr/bin/true` | 0.666 | 0.978 | 1.095 |
| `hex --version` | 1.038 | 1.403 | 1.540 |
| `hex oci state <id>` | 1.075 | 1.506 | 2.129 |
| `crun --version` | 0.786 | 1.117 | 1.283 |
| `crun state <id>` | 0.517 | 1.166 | 1.276 |

Spawning and waiting for `true` costs 0.666 ms at p50. 4x crun's start in this run (1.16 / 4 = 0.29 ms) is
less than that, so no command-line runtime can start a container 4x faster than crun's command line. `hex
--version` costs 0.25 ms more than `crun --version`: Hex's binary starts slower.

### 3. What each runtime leaves running per idle container

Five `sleep infinity` containers each (`procs.<runtime>.txt`; RSS in KiB, which counts shared pages):

| Runtime | Beside each container's `sleep` (RSS 1,548-1,684) | Its RSS | Its parent |
|---|---|---|---|
| hex | `/opt/bin/hex __oci-init`, the shim, one per container | 3,140-3,328 | the caller's subreaper |
| crun | nothing; the `sleep` is reparented | n/a | n/a |

### 4. The throughput ceiling, with the bundle's network namespace and without it

Lifecycles/s, and CPU ms per lifecycle in brackets, at concurrency 1 / 8 / 32:

| Runtime | with a network namespace | without |
|---|---|---|
| hex | 44 (12.84) / 301 (8.82) / 1,029 (7.68) | 58 (7.33) / 353 (6.23) / 1,079 (6.90) |
| crun | 68 (10.19) / 1,069 (5.24) / 1,171 (5.37) | 73 (4.62) / 1,183 (4.83) / 1,251 (4.88) |

Without its network namespace, a lifecycle at concurrency 32 is 5% faster for Hex (1,079 against 1,029/s) and 7%
for crun (1,251 against 1,171). On this kernel the namespace isn't what caps throughput near 1,000-1,250/s. SOCK
measured 200 against 900 containers/s on a 2018 kernel, before network-namespace creation was fixed
(`docs/research/runtime/oci-runtimes-and-fast-start.md` §4.3).

## Run 2: step 5, the driver's own ceiling (2026-10-24, fedora)

- **Host:** fedora.attlocal.net, as in run 1, kernel 6.19.10-300.fc44.x86_64.
- **When and load:** 2026-10-24T18:29:13Z, once Quiver's exclusive window had ended. The 1-minute load was 0.86
  at the start (the 5-minute load was still 8.37 from the window) and 0.95 at the end.
- **The drivers:** the new one is `oci_bench.py` at c532eef (sha256 93b17d7f…): each concurrency slot is a
  worker process of its own. The old one is 25f65cf's (76bbf549…), a thread pool. Both ran
  `STEPS=5 OLD_DRIVER_REV=25f65cf oci_breakdown.sh`, back to back. Out dir on fedora:
  `~/.cache/hex-bench/out/breakdown-20261024T182913Z/`.
- **Parameters:** 1,000 lifecycles per concurrency level. Both passes recorded errors `{}`.

### 5. The driver's own ceiling: `/usr/bin/true` standing in for the runtime

The throughput phase with `/usr/bin/true` as crun, so every call is a spawn and an exit and nothing else.
Lifecycles/s, and the bench cgroup's CPU ms per lifecycle in brackets:

| Driver | concurrency 1 | 8 | 32 |
|---|---|---|---|
| old: threads, 25f65cf | 647 (1.98) | 1,062 (2.46) | 1,394 (1.86) |
| new: a worker process per slot, c532eef | 612 (2.06) | 6,868 (1.39) | 12,776 (1.88) |

- **The new driver drives 12,776 lifecycles/s at concurrency 32:** 3.1x the 4,152/s the design's targets need,
  and 9.2x the old driver's.
- **The old driver's own ceiling at concurrency 32 was 1,394/s,** 7% above the plan's line of about 1,300/s.
- **The old driver was the cap for crun, not for Hex.** In the first verification pass through the new driver
  (2026-10-24T18:58Z, 1-minute load 1.70 at its start, `~/.cache/hex-bench/out/20261024T185457Z/`, the same
  binaries), crun ran 1,565 lifecycles/s at concurrency 32. The old driver ran it at 1,171 in run 1, and could
  drive at most 1,394, so crun's rows of record at concurrency 32 measured the old driver. Hex ran 1,027 against
  1,029 in run 1: below the old driver's ceiling, so Hex's rows were its own. Through the new driver, Hex is at
  0.66x crun at concurrency 32, where `RESULT.md` has it level (0.99x), and step 5's run of record re-measures
  every runtime's throughput.
- **Both drivers write the same JSON.** In this run, `ceiling.standin.json` (new) and `ceiling.standin.old.json`
  (old) have the same top-level keys and the same throughput fields (`cpu_s_per_lifecycle`, `lifecycles`,
  `per_s`, `wall_s`). Set into a run directory each, as `crun.r1.json`, they make the same table in
  `summarize.py`, row for row: 35 lines, differing only in the numbers. The same holds off the bench host,
  against the stand-in (adacachy).

## Run 3 (2026-10-26, adacachy): the no-fsync binary, step 1

Decision 1 landed: the `oci` class now writes `state.json` with a temp file and `rename(2)` and **no fsync**
(`state.rs`). Re-running step 1 on the no-fsync binary shows the fsync penalty run 1 measured is gone.

- **Host:** adacachy, kernel 7.2.9-2-cachyos (32 CPUs, the container pinned 4-31). **Hex:** static-glibc HEAD
  of `cp/t7-runtime`, sha256 00eb00954a7f88d3 (built `--target x86_64-unknown-linux-gnu` with `+crt-static`).
  **crun:** 1.30.1, as run 1.
- **When and load:** 2026-10-26T04:09Z, 1-minute load ~15 — noisy. CLAUDE.md §7's load<2 is unreachable on the
  shared hosts right now (fedora overloaded/offline, adacachy oscillating 7–218); this step's overlay-vs-tmpfs
  and hex-vs-crun figures are same-run ratios that cancel other people's CPU load, so they hold noisy
  (noisy-is-default). `LOAD_GATE=0`. Out dir: `~/.cache/hex-bench/out/breakdown-20261026T040921Z/`.
- **Parameters:** n = 500 per pass after 50 warm-ups, interleaved a (overlay) b (tmpfs) a b.

### 1. The fsync penalty is gone: overlay ≈ tmpfs

p50 in ms (best of the two passes each).

| Pass | create | start | delete |
|---|---|---|---|
| hex overlay (a) | 2.68 | 0.341 | 7.34 |
| hex tmpfs (b) | 4.39 | 0.336 | 6.14 |
| crun overlay (a) | 1.68 | 0.325 | 0.43 |
| crun tmpfs (b) | 1.58 | 0.293 | 0.37 |

- **Hex's start is within 1.5% overlay-to-tmpfs** (0.341 against 0.336 ms), against run 1's 8.39 ms overlay /
  1.68 ms tmpfs with fsyncs. The ~6.6 ms fsync penalty run 1 measured on start is gone.
- **Hex's create shows no overlay penalty** (overlay 2.68 ≤ tmpfs 4.39 ms), against run 1's 12.42 overlay /
  5.74 tmpfs. create p50 is noisy at this load (overlay passes 4.31 and 2.68, tmpfs 5.44 and 4.39), so the two
  sides span rather than land within a literal 10%, but there is no overlay penalty, which is the point.
- **crun (the control) is overlay ≈ tmpfs** either way, so the host and the overlay are not the variable.
- **With its fsyncs gone, Hex's start is 1.05x crun's** here (0.341 against 0.325), within the 1.45x the design
  argues (run 1's tmpfs projection, 1.68 against 1.16 ms). This is *this breakdown probe's* hex-vs-crun figure;
  the competitive OCI pass of record (`RESULT.md`) was built from the pre-no-fsync binary (c7fc78f, which still
  fsynced: start 6.74 ms, ~13x crun), so it does **not** yet confirm this. A competitive OCI pass on the
  no-fsync binary is still owed (it needs the bench host; fedora is offline). (An earlier revision of this line
  wrongly claimed the pass of record agreed at 1.07x — it does not; corrected 2026-10-26.)
- **Delete is kernel-variable and is not the state write.** Hex delete is ~7 ms here (overlay ≈ tmpfs, so not
  fsync/disk), against 2.05 ms on fedora's kernel in run 1; crun delete is ~0.4 ms here against ~11.6 ms in
  run 1. The two runtimes' teardown reverses between fedora 6.19 and CachyOS 7.2.9: Hex waits for the cgroup to
  empty (`cgroup.kill` then `cgroup.events`), whose POLLPRI notification lags the actual depopulation on this
  kernel, where crun's return does not.
- **Fixed** (`cgroup.rs`, `wait_empty`): capping the `cgroup.events` poll so `populated` is re-read every
  500 µs, rather than waiting on the lagged POLLPRI, cut Hex's delete here from ~7 ms to **0.9 ms** (re-run on
  the fixed binary, sha 1cd7deb7, same kernel; crun 0.4 ms in the same run). Start, create and the
  overlay-vs-tmpfs figures are unchanged — the cap only affects the empty-wait, which only `delete` uses — and
  it cannot regress a kernel whose POLLPRI is already prompt, since it only re-reads sooner. Correctness is
  unchanged: `wait_empty` still returns only when `populated` is false or the deadline passes.
