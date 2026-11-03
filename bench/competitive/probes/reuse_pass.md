# One owner's boxes through host.sh's OCI pass (goal 5 extended, step 6)

Goal 5 extended, step 6's second criterion: sustained lifecycles a second at concurrency 32 against throughput at
concurrency 1, in the same OCI pass, one owner's boxes, at least 8x (seam_oci's throughput and sustained phases through
host.sh on fedora).

Runs 1 and 2 ran on CPUs 4-31, P- and E-cores, as host.sh runs by default. The criterion is a ratio within one pass,
so each stands as measured (the lead, 2026-11-03T00:48Z). Their absolute rates aren't comparable with the records on
4-15, the P-core threads, from create_breakdown.md's run 4 on. Run 3 repeats run 1 on 4-15, at f7c4065.

## Run 1 (2026-11-03, fedora)

- **Host and load:** fedora (Intel Core i9-14900HX, 32 threads, the pass's container pinned to 4-31), Linux
  6.19.10-300.fc44.x86_64; the 1-minute load 20.73 at the start and 21.85 at the end, one or two CI jobs beside it.
  `LOAD_GATE=0`: a ratio within one pass, so not of record by CLAUDE.md §7's load rule for absolute figures.
- **Binaries:** run 7's of `create_serial.md` (the code of `a6f32f1`): `hex` 1404d8835bb80926, `seam_oci`
  833f7025d1d9b056.
- **Run:** `~/.cache/hex-t7/g5x-s6/hex-bench/out/20261103T011948Z`: `PHASES=oci RUNTIMES=libcrun ROUNDS=2 N=200
  WARMUP=20 LIFECYCLES=2000 HEXBENCH_PHASES="throughput sustained" HEXBENCH_SUSTAINED=10000
  HEXBENCH_OWNER=bench-owner`, every box seam_oci made its owner's, the bundles' processes holding Docker's default
  capabilities, libcrun's alike.
- **What the node did** (in process, `seam_oci`'s counts): of each sustained phase's 10,000 lifecycles, 9,999 and
  10,000 boxes were handed an idle cgroup, the rest fresh; none refused; 9,999 and 10,000 ended through their init's
  pidfd, one by `cgroup.kill`.

| lifecycles a second | throughput, concurrency 1 | concurrency 8 | concurrency 32 | sustained, concurrency 32 | sustained over concurrency 1 |
|---|---|---|---|---|---|
| Hex's seam in process, round 1 | 606 | 2,516 | 2,746 | 2,616 | 4.32x |
| Hex's seam in process, round 2 | 668 | 2,985 | 2,590 | 2,814 | 4.21x |
| Hex's seam over its socket, round 1 | 574 | 2,719 | 2,467 | 2,535 | 4.41x |
| Hex's seam over its socket, round 2 | 609 | 2,703 | 3,013 | 2,932 | 4.81x |
| libcrun in process, round 1 | 73 | 627 | 1,670 | 1,653 | 22.60x |
| libcrun in process, round 2 | 74 | 633 | 1,607 | 1,626 | 22.09x |

- **The criterion is not met:** 4.21x to 4.81x against 8x (4-31, as host.sh runs it).
- **Both rates rose.** Hex's sustained rate at concurrency 32 is 2,535 to 2,932 a second. With every box's cgroup made
  and removed it was 2,108 and 2,095 in goal 5 step 2's pass of record (`RESULT.md`, "Goal 5, plan step 2", a load of
  4.18 to 15.09) and 1,429 under `create_serial.md`'s run 6 probes. `cgroup_mutex`, held 87.1% of the time at
  concurrency 32 by boxes of no owner on this host minutes before the pass (`create_serial.md`, run 8), is held 12.4%
  by one owner's (run 7). A lifecycle at concurrency 1 got faster too: 1.50 to 1.74 ms, against 2.54 in process and
  3.16 over the socket in the pass at ca45e1a (394 and 316 a second, `create_serial.md`), so 8x of it now asks 4,592 to
  5,344 a second.
- **libcrun's ratio is not like for like.** Its lifecycle at concurrency 1 sleeps in its own polling: 13.7 ms a
  lifecycle against 1.5 to 1.7 for Hex's (`create_serial.md`, run 1: 4.069 ms of polling sleeps a lifecycle). At
  concurrency 32 its rate is under two thirds of Hex's.
- **What holds concurrency 32 back now** (`create_serial.md`, run 9): the mount namespace's lock, one for the whole
  host, which the inits waited for 5.349 ms a lifecycle (bind mounts, `pivot_root` with its walk of every thread on
  the host, the mount table's read), then RTNL, 2.089 ms, in making the network namespaces the pool hadn't made ahead
  (3,751 and 4,034 of the in-process sustained phases' 10,000 here). `cgroup_mutex` is waited for 0.043 ms a
  lifecycle. Hex's CPU at concurrency 32 is 2.6 to 3.3 ms a lifecycle (`cpu_s_per_lifecycle`), 16.44% of its samples
  spinning to take an `rw_semaphore` to write.

## Run 2 (2026-11-03, fedora): an experiment, steps 2, 3 and 6 merged, beside step 6 alone

- **Why:** whether the plan's levers together reach 8x: step 2's root without `pivot_root` and step 3's mounts, each
  on its own branch, with this step's. Of record for no branch: the merge is no commit.
- **The merge:** `cp/t7-ext-s6` at a6f32f1 merged with `cp/t7-ext-s3` at 148d12e, which holds step 2's bf77298. Its
  one conflict in code is resolved by keeping both sides: `start_via_seed` takes step 2's pruned root and returns step
  6's `State`, and `start_seeded` returns the `State` too. The three probe scripts in conflict are step 3's (the pass
  runs none of them). Built on fedora as run 1's binaries were: `hex` 8268582ba6f124ea, `seam_oci` 58e1325a8a636b42.
- **Host and load:** run 1's host. The merge's pass from 2026-11-03T01:40:11Z, the 1-minute load 7.11 at its start and
  17.14 at its end; then step 6's alone (run 1's binaries) from 01:42:05Z, 17.14 and 20.72. `LOAD_GATE=0`.
- **Runs:** `~/.cache/hex-t7/g5x-s6/hex-bench/out/20261103T014011Z` (the merge) and `20261103T014205Z` (step 6 alone),
  each with run 1's parameters, every box `bench-owner`'s.

| lifecycles a second | throughput, concurrency 1 | concurrency 8 | concurrency 32 | sustained, concurrency 32 | sustained over concurrency 1 |
|---|---|---|---|---|---|
| Hex's seam in process, the merge, round 1 | 819 | 3,629 | 4,070 | 3,961 | 4.83x |
| Hex's seam in process, the merge, round 2 | 926 | 3,490 | 3,879 | 3,754 | 4.05x |
| Hex's seam over its socket, the merge, round 1 | 897 | 3,750 | 4,253 | 4,055 | 4.52x |
| Hex's seam over its socket, the merge, round 2 | 1,028 | 3,908 | 3,789 | 4,293 | 4.18x |
| libcrun in process, beside the merge, round 1 | 73 | 632 | 1,652 | 1,646 | 22.51x |
| libcrun in process, beside the merge, round 2 | 73 | 634 | 1,436 | 1,745 | 24.01x |
| Hex's seam in process, step 6 alone, round 1 | 612 | 3,018 | 2,882 | 3,138 | 5.12x |
| Hex's seam in process, step 6 alone, round 2 | 671 | 3,020 | 3,247 | 3,191 | 4.76x |
| Hex's seam over its socket, step 6 alone, round 1 | 575 | 2,778 | 3,044 | 3,110 | 5.41x |
| Hex's seam over its socket, step 6 alone, round 2 | 694 | 2,922 | 3,257 | 3,205 | 4.62x |
| libcrun in process, beside step 6 alone, round 1 | 73 | 594 | 1,487 | 1,517 | 20.67x |
| libcrun in process, beside step 6 alone, round 2 | 73 | 634 | 1,676 | 1,700 | 23.33x |

- **The three steps' levers don't reach 8x either:** 4.05x to 4.83x, against 4.62x to 5.41x for step 6 alone beside
  them (4-31, as host.sh runs it). Both rates rose with them: the merge sustained 3,754 to 4,293 a second at
  concurrency 32 against step 6's 3,110 to 3,205, and ran 819 to 1,028 at concurrency 1 against 575 to 694, a
  lifecycle alone taking 0.97 to 1.22 ms. At concurrency 1 the rate rose the more, so the ratio fell.
- **Every box was handed its owner's idle cgroup but one:** of the merge's sustained phases in process, 10,000 and
  9,999 of 10,000, and 10,000 and 9,999 ended through their init's pidfd.
- **What holds concurrency 32 with all three in** (`create_serial.md`, run 10): neither `cgroup_mutex` nor the mount
  namespace's lock, but RTNL, the host's one network lock, in making each box's network namespace. The pool that makes
  them ahead (decision 4) ran dry: 9,561 to 9,608 of each sustained phase's 10,000 creates made theirs on the path in
  the merge, against 5,078 to 5,359 for step 6 alone.

## Run 3 (2026-11-03, fedora): on CPUs 4-15, at f7c4065: of record for the second criterion

- **Host and load:** fedora, Linux 6.19.10-300.fc44.x86_64, 2026-11-03T02:36:17Z, the pass's container pinned to
  4-15 and capped (`--memory 8g`, `--oom-score-adj 1000`); the 1-minute load 19.80 at the start and 32.60 at the end,
  from the host's other work beside it. `LOAD_GATE=0`.
- **Binaries:** create_serial.md's run 11's, built at f7c4065, the step on the delete flake's fix: `hex`
  729934ce2c45a292, `seam_oci` 92b8a36ae1c6c74e.
- **Run:** `~/.cache/hex-t7/g5x-s6/hex-bench/out/20261103T023617Z`, run 1's parameters, every box `bench-owner`'s; the
  fedora unit `hex-g5x-s6-verify2b-20261103T023358Z`.
- **What the node did** (in process): of each sustained phase's 10,000 lifecycles, 10,000 and 9,999 boxes were handed
  an idle cgroup, the other fresh; 10,000 and 9,999 ended through their init's pidfd, one by `cgroup.kill`.

| lifecycles a second | throughput, concurrency 1 | concurrency 8 | concurrency 32 | sustained, concurrency 32 | sustained over concurrency 1 |
|---|---|---|---|---|---|
| Hex's seam in process, round 1 | 724 | 2,122 | 2,231 | 2,152 | 2.97x |
| Hex's seam in process, round 2 | 593 | 1,924 | 1,787 | 1,850 | 3.12x |
| Hex's seam over its socket, round 1 | 679 | 2,000 | 2,110 | 2,250 | 3.32x |
| Hex's seam over its socket, round 2 | 675 | 1,936 | 1,915 | 1,948 | 2.89x |
| libcrun in process, round 1 | 75 | 549 | 1,170 | 1,121 | 14.92x |
| libcrun in process, round 2 | 74 | 558 | 1,100 | 1,065 | 14.44x |

- **The criterion is not met:** 2.89x to 3.32x against 8x. On twelve CPUs Hex's rate at concurrency 32 is 1,850 to
  2,250 a second, against 2,535 to 2,932 on run 1's 28; the rate at concurrency 1 is about the same, 593 to 724.
- **What holds concurrency 32 is what run 1 met:** the mount namespace's lock, which create_serial.md's run 11 also
  finds on these CPUs, and the network namespaces made on the path (1,613 and 228 of the in-process sustained phases'
  10,000 here, 2,007 and 734 over the socket).


## Run 4 (2026-11-03, fedora): on CPUs 4-15, at 610bba9, the idle cgroups' residual read at each phase's end

- **Host and load:** kiln-a094a12d, the host fedora was named after its reset at 02:59Z, Linux
  6.19.10-300.fc44.x86_64, 2026-11-03T04:41:33Z, the pass's container pinned to 4-15 and capped (`--memory 8g`,
  `--oom-score-adj 1000`). The 1-minute load was 14.05 at the start and 23.65 at the end, after an hour near 170 from
  other tenants' builds.
- **Binaries:** built on fedora at 610bba9, the step with the lead's `cpu.max` ruling: `hex` 765e809d32ae1a07,
  `seam_oci` 93bd371b2e2b5499. The `seam_oci` of 25dd262 on records each phase's `idle_residual_bytes`
  (`reuse_residual.md`).
- **Run:** `~/.cache/hex-t7/g5x-s6/hex-bench/out/20261103T044133Z`, run 1's parameters; the fedora unit
  `hex-g5x-s6-verify3c-20261103T044132Z`.
- **What the node did** (in process): every one of each sustained phase's 10,000 boxes was handed an idle cgroup
  and ended through its init's pidfd, none by `cgroup.kill`.

| lifecycles a second | throughput, concurrency 1 | concurrency 8 | concurrency 32 | sustained, concurrency 32 | sustained over concurrency 1 |
|---|---|---|---|---|---|
| Hex's seam in process, round 1 | 832 | 2,463 | 2,362 | 2,278 | 2.74x |
| Hex's seam in process, round 2 | 843 | 2,108 | 2,443 | 2,238 | 2.66x |
| Hex's seam over its socket, round 1 | 953 | 2,485 | 2,339 | 2,351 | 2.47x |
| Hex's seam over its socket, round 2 | 896 | 2,796 | 2,576 | 2,563 | 2.86x |
| libcrun in process, round 1 | 77 | 601 | 1,260 | 1,364 | 17.65x |
| libcrun in process, round 2 | 77 | 640 | 1,381 | 1,364 | 17.78x |

- **The criterion is not met:** 2.47x to 2.86x against 8x, as run 3's 2.89x to 3.32x on these CPUs. The rate at
  concurrency 1 is higher than run 3's (832 to 953 against 593 to 724) on a quieter host, and at concurrency 32 about
  the same (2,238 to 2,563 against 1,850 to 2,250).
- **Network namespaces made on the path:** 2,277 and 2,938 of the in-process sustained phases' 10,000, 2,978 and
  2,985 over the socket.

## Run 5 (2026-11-03, fedora): an experiment, steps 2, 3 and 6 merged, with network namespaces and without

- **Why:** Ada asked how close taking the network namespace's making off the lifecycle gets to 8x, before deciding on
  reusing it (2026-11-03). Of record for no branch: the merge is no commit.
- **The merge:** run 2's, on step 6 at f7c4065: `cp/t7-ext-s6` at f7c4065 with `cp/t7-ext-s3` at 148d12e merged in the
  working tree, built on fedora as run 1's binaries were: `hex` 622aae3633815203, `seam_oci` 62b728b537684184.
- **Host and load:** kiln-a094a12d, Linux 6.19.10-300.fc44.x86_64, the pass's container pinned to 4-15 and capped
  (`--memory 8g`, `--oom-score-adj 1000`), `LOAD_GATE=0`. `NETNS=0` from 2026-11-03T05:13:01Z, the 1-minute load
  26.81 at its start and 35.13 at its end; `NETNS=1` from 05:17:31Z, 29.63 and 40.90, with kiln CI jobs beside it.
- **Runs:** `~/.cache/hex-t7/g5x-s6/hex-bench/out/20261103T051301Z` (`NETNS=0`: every bundle without a network
  namespace) and `20261103T051731Z` (`NETNS=1`), run 1's parameters, every box `bench-owner`'s; the fedora unit
  `hex-g5x-s6-verify3c-20261103T044132Z`. The bundles had `/proc/sys` read-only (`readonlyPaths`): the
  oci_inside.sh of this step's network-namespace reuse, synced uncommitted into the tree the pass ran from. A
  read-only path is a bind mount more in each create, in both passes alike.

| lifecycles a second | throughput, concurrency 1 | concurrency 8 | concurrency 32 | sustained, concurrency 32 | sustained over concurrency 1 |
|---|---|---|---|---|---|
| Hex's seam in process, no network namespace, round 1 | 468 | 2,439 | 3,609 | 4,784 | 10.23x |
| Hex's seam in process, no network namespace, round 2 | 215 | 1,479 | 3,744 | 4,407 | 20.54x |
| Hex's seam over its socket, no network namespace, round 1 | 122 | 969 | 2,427 | 2,579 | 21.23x |
| Hex's seam over its socket, no network namespace, round 2 | 166 | 1,446 | 3,062 | 3,182 | 19.16x |
| Hex's seam in process, network namespaces, round 1 | 181 | 665 | 865 | 902 | 5.00x |
| Hex's seam in process, network namespaces, round 2 | 388 | 1,279 | 1,215 | 1,136 | 2.93x |
| Hex's seam over its socket, network namespaces, round 1 | 256 | 1,106 | 1,057 | 929 | 3.63x |
| Hex's seam over its socket, network namespaces, round 2 | 244 | 1,089 | 1,306 | 1,334 | 5.46x |

- **The making caps concurrency 32:** in the same binary, minutes apart, Hex sustained 4,407 to 4,784 a second in
  process without network namespaces and 902 to 1,136 with them, 85.9 to 87.5% of whose creates made theirs on the
  path, the pool dry.
- **Whether that reaches 8x isn't shown:** the rate at concurrency 1 was a fifth to a half of what run 2's merge
  (step 6 at a6f32f1) ran (819 to 1,028) and step 6 alone in run 4 (832 to 953), on a host at load 26 to 41. With run 4's 832,
  the 4,784 would be 5.75x. A lifecycle alone is what the host's load slows most, so the ratio here rises with the
  load; it needs concurrency 1 measured on a quieter host, or beside concurrency 32 in each round.

## Run 6 (2026-11-03, fedora): concurrency 1 and 32 interleaved, steps 2, 3 and 6 merged, the network namespace reused

- **Why:** the second criterion, 8x a minimum (Ada, via the lead, 2026-11-03), measured as the lead set it: concurrency 1
  and 32 in turn in each round (seam_oci's `interleaved` phase: 10 pairs a round of 500 lifecycles at 1 then 4,000 at
  32), each adjacent pair's ratio, pooled with a bootstrap standard error (`interleaved_pool.py`), the host's load and CPU
  pressure beside each pair. The same for reuse on and off (`HEXBENCH_REUSE=0`: the owner opts out, its bundles
  unchanged), with network namespaces and without (`NETNS`), and for step 6 alone.
- **The merge:** run 2's resolution on `cp/t7-ext-s6` at 4e9ef6a with `cp/t7-ext-s3` at **148d12e**, uncommitted. That base
  lacks step 3's propagation fix (e2dd76c), so these figures are of record for no branch and are marked with it; later
  merges are on 16e52a9 (the lead, 2026-11-03).
- **Binaries:** the merge `hex` 42bd32d4d8a88d65 with `seam_oci` 9e5f8f386309aff0 (the first unit) and 42304065482ac3e3
  (the second, which also records each half's CPU: c9c0cf7); step 6 alone, `hex` 02f441a98f6eb8dd with `seam_oci`
  e1e01b3bd503a858 and 897dc7f829890263.
- **Host:** kiln-a094a12d, Linux 6.19.10-300.fc44.x86_64, each pass's container pinned to CPUs 4-15 (6 P-cores, 2 threads
  each) and capped (`--memory 8g`, `--oom-score-adj 1000`), `LOAD_GATE=0`, 2 rounds, every box `bench-owner`'s. The
  fedora units `hex-g5x-s6-verify4-20261103T062500Z` and `hex-g5x-s6-verify5-20261103T063629Z`.
- **Two passes are contaminated, kept only as such:** `20261103T062725Z` (reuse on: 4.72x and 4.64x) and
  `20261103T062813Z` (reuse off: 2.17x and 2.38x) met the lead's trace unit on the same CPUs, 06:27:30 to 06:28:34Z.
  They were run again in the second unit.

| run | what | load | in process: c1, c32 (lifecycles/s) | ratio (SE) | over the socket: c1, c32 | ratio (SE) |
|---|---|---|---|---|---|---|
| 20261103T064217Z | merge, reuse on | 4.8 to 15.8 | 1,923, 7,178 | 3.74x (0.06) | 1,388, 7,211 | 5.23x (0.29), 10 pairs |
| 20261103T064252Z | merge, reuse off | 15.8 to 37.4 | 1,252, 2,605 | 2.08x (0.05) | 1,148, 2,574 | 2.24x (0.03) |
| 20261103T064414Z | merge, no network namespace | 37.4 to 39.0 | 1,806, 9,855 | 5.45x (0.13) | 1,582, 8,163 | 5.17x (0.12) |
| 20261103T064447Z | step 6 alone, reuse on | 39.0 to 30.1 | 1,117, 3,204 | 2.87x (0.05) | 1,007, 3,164 | 3.14x (0.06) |
| 20261103T062941Z | merge, no network namespace (first unit) | 22 to 35 | 1,716, 9,425 | 5.49x (0.15) | 1,464, 7,748 | 5.32x (0.16) |
| 20261103T063016Z | step 6 alone, reuse on (first unit) | 35 to 24 | 968, 3,034 | 3.13x (0.10) | 997, 3,187 | 3.19x (0.08) |

The pairs' medians of each half's CPU (the second unit's, in process; over the socket alike):

| run | CPU a lifecycle, c1 | c32 | CPUs busy, c1 | c32 |
|---|---|---|---|---|
| 20261103T064217Z, reuse on | 0.64 ms | 1.19 ms | 1.24 | 8.60 |
| 20261103T064252Z, reuse off | 1.02 ms | 1.77 ms | 1.33 | 4.64 |
| 20261103T064414Z, no network namespace | 0.54 ms | 1.04 ms | 1.03 | 10.29 |
| 20261103T064447Z, step 6 alone | 0.94 ms | 1.92 ms | 1.10 | 6.16 |

- **Reuse works under load:** with it on, 98.4 to 99.6% of each round's 45,000 boxes were handed a kept namespace, none
  refused and none closed unchecked; the rest were given a fresh one when none of their owner's was idle, as the one
  checker thread fell behind (`create_serial.md`, run 12).
- **The ratio is the CPUs kept busy times the CPU per lifecycle:** c32 over c1 is (CPUs busy at 32 over at 1) times (CPU a
  lifecycle at 1 over at 32): for reuse on, 6.94 times 0.54, 3.73x. Reuse off keeps 4.64 of 12 CPUs busy at
  concurrency 32: it waits (run 10's RTNL). Reuse on keeps 8.60 busy, and its waits are run 12's.
- **With no network namespace at all,** concurrency 32 kept 10.29 of the 12 CPUs busy, and each lifecycle cost 1.93
  times the CPU it cost alone. Two threads share each core's units there, and 6 cores run at a lower clock than 1, so a
  lifecycle held by CPU can't be run 32 at once on these CPUs much past 6x what it runs alone.
- **One start failed:** the socket driver's second round of `20261103T064217Z`, at its seventh pair, with "start
  i006b003224: reading the exec fifo failed: Resource temporarily unavailable". So that row is over 10 pairs. Since
  explained: the start's poll reported a hang-up as the init opened its end of the FIFO, before its byte, a race in
  the kernel's FIFO poll (`fifo_hup.md`), and 756072f has the start poll again.

## Run 7 (2026-11-03, fedora): run 6's method on 16e52a9, with the cgroup files opened through their directory

- **The merge:** `cp/t7-ext-s6-x-s3`'s resolution (e9f58a0 with `cp/t7-ext-s3` at 16e52a9, step 3's propagation fix
  in), with step 6's tip through 9cf1562 merged on it, uncommitted (of record for no branch): the cgroup files opened
  through their held directory (3ea3ffa) and the checker's time counted (9cf1562). `hex` beef4e8e13cb4680, `seam_oci`
  5e692e576c1daf7c.
- **Host:** run 6's, CPUs 4-15, the unit `hex-g5x-s6-verify6b-20261103T070138Z`, 2 rounds of 10 pairs.

| run | what | load | in process: c1, c32 | ratio (SE) | CPU a lifecycle, c1, c32 | CPUs busy, c1, c32 | over the socket: ratio (SE) |
|---|---|---|---|---|---|---|---|
| 20261103T070533Z | reuse on | 9.4 to 23.1 | 1,453, 8,364 | 5.76x (0.12) | 0.81, 1.21 ms | 1.19, 10.17 | 6.19x (0.27) |
| 20261103T070613Z | reuse off | 23.1 to 33.0 | 1,215, 2,596 | 2.14x (0.05) | 1.11, 1.73 ms | 1.31, 4.56 | 2.54x (0.07) |

- **The cgroup files' walks were run 6's cap:** with them opened by name through the held directory, reuse on keeps
  10.17 of 12 CPUs busy at concurrency 32, against run 6's 8.60, and the ratio is 5.76x against 3.74x. The base moved
  too, 148d12e to 16e52a9, so the lever's own share is the same pass on the branch without it.
- **The next cap is the one checker:** it took 127.2 and 130.8 us a namespace, so it checked at most about 7,700 a
  second while concurrency 32 gave back 8,364. 5,508 and 5,248 of each round's 45,000 boxes found none of their
  owner's idle and were given a fresh namespace (12%), and 2,448 and 2,827 were closed past the owner's bound: each
  made and torn down under RTNL.
- **The extra CPU a lifecycle costs at concurrency 32 isn't spinning** (`create-serial-offcpu-20261103T070739Z`, the
  same binaries, on-CPU samples at 999 Hz): 0.509 ms a lifecycle at concurrency 1, 1.016 at 32. Lock spinning is
  0.064 of it (0.004 at 1). The rest rises across code that takes no lock: user code 0.102 to 0.186, page faults
  0.036 to 0.069, process teardown 0.016 to 0.049, fork 0.022 to 0.038, syscall entry 0.056 to 0.080. Two threads
  share each core on these CPUs.
- No start failed in either pass, over 90,000 lifecycles each.

## Run 8 (2026-11-03, fedora): the four checkers, and the cores' shared threads

- **The merge:** `cp/t7-ext-s6-x-s3` at e9fb28f with step 6's tip through fcc002d merged on it, uncommitted (of record
  for no branch): run 7's, and the four checkers each reading through its own held `/proc/thread-self/net` (893ea05).
  `hex` 63fd1465f9f87837, built on fedora. The unit `hex-g5x-s6-verify7-20261103T071725Z`, 2 rounds of 10 pairs.

| run | CPUs | what | load | in process: c1, c32 | ratio (SE) | CPU a lifecycle, c1, c32 | CPUs busy, c1, c32 | over the socket: ratio (SE) |
|---|---|---|---|---|---|---|---|---|
| 20261103T075855Z | 4-15 | reuse on | 6.9 to 19.4 | 1,832, 8,914 | 4.87x (0.10) | 0.71, 1.15 ms | 1.35, 10.32 | 5.11x (0.12) |
| 20261103T075932Z | 4-15 | reuse off | 19.4 to 34.6 | 1,178, 2,593 | 2.20x (0.04) | 1.16, 1.72 ms | 1.41, 4.48 | 2.51x (0.05) |
| 20261103T080056Z | 4,6,8,10,12,14 | no network namespace | 34.6 to 32.6 | 1,771, 7,057 | 3.98x (0.08) | 0.61, 0.72 ms | 1.10, 5.18 | 4.25x (0.13) |
| 20261103T080138Z | 4,6,8,10,12,14 | reuse on | 32.6 to 30.1 | 1,417, 6,003 | 4.24x (0.14) | 0.81, 0.86 ms | 1.19, 5.13 | 4.29x (0.14) |
| 20261103T080227Z | 4-31 | reuse on | 30.1 to 30.9 | 1,005, 12,334 | 12.32x (0.21) | 1.25, 1.43 ms | 1.28, 18.01 | 11.46x (0.25) |

- **The checkers keep up:** with four, 1.5% and 1.7% of each round's boxes were given a fresh namespace on 4-15, against
  run 7's 12%, and concurrency 32 ran 8,914 a second against 8,364. Each check's wall time rose to 208.2 and 212.9 us
  from run 7's 127.2 and 130.8: four checkers among 32 lifecycles on 12 CPUs.
- **The ratio fell from run 7's 5.76x though concurrency 32 rose,** because concurrency 1 rose more on a quieter host:
  1,832 a second against 1,453.
- **The CPU a lifecycle cost at concurrency 32 doubled in this pass, and the ceiling drawn from it is withdrawn (run
  9):** on one thread of each of the same 6 cores, with no network namespace, it cost 1.18 times its CPU alone (0.72
  against 0.61 ms), where on both threads of each it cost 1.93 times (run 6's 20261103T064414Z: 1.04 against 0.54).
  This run took that for the cores' shared threads and drew from it a ceiling on CPUs 4-15 of about 12 CPUs over 1.9
  times a lifecycle's CPU alone, about 12,100 a second, 6.6 times concurrency 1. Run 9 measured 1.36 to 1.44 times on
  both threads of each core at CPU pressure 25%: the 1.93 was run 6's load, 37.4 to 39.0, not the threads, and 6.6x
  is no ceiling.
- **On 4-31 the ratio is 12.32x, but concurrency 1 is slower there:** a lone lifecycle runs on the E-cores too, at 1.25
  ms of CPU against 0.71 on the P-cores, so 1,005 a second against 1,832. Concurrency 32 ran 12,334 a second there.
- **No start failed** in these passes, nor in 1,200,000 lifecycles at concurrency 32 sustained (3 rounds of 100,000 a
  driver on each of this merge and the 148d12e one with the FIFO diagnostics: `20261103T080305Z`, `20261103T080909Z`).

## Run 9 (2026-11-03, fedora): the checks' three changes, interleaved on 4-15

- **The binaries:** run 8's merge (four checkers) as mr4f, against the same tree with step 6's three changes to the
  checks, 8753abe (held buffers), 4c54e58 (8 KiB dump reads) and ad6f333 (the namespace's own netlink socket kept,
  sockstat's count first; on `cp/t7-ext-s6-kept-socket`), as mr9; each with the exec FIFO's fix (756072f's run.rs),
  as one of `create_serial.md` run 18's passes ended at that race without it. Uncommitted, of record for no branch.
  `hex` dca1aaf3bb8d527d and 2245470c0ac31f07, `seam_oci` 250bcce2ff44e489 and da929fde2f21a1ee.
- **Host:** CPUs 4-15, the unit `hex-g5x-s6-verify11-20261103T101353Z`, A B B A, each pass 2 rounds of 10 pairs,
  one owner's boxes, reuse on, a network namespace each; other tenants' load beside it, CPU pressure 24 to 26% at
  the passes' ends.

| run | what | load | in process: c1, c32 | ratio (SE) | CPU a lifecycle, c1, c32 | CPUs busy, c1, c32 | over the socket: ratio (SE) |
|---|---|---|---|---|---|---|---|
| 20261103T101506Z | mr4f | 5.6 to 14.1 | 1,774, 9,600 | 5.41x (0.08) | 0.75, 1.07 ms | 1.36, 10.33 | 6.79x (0.35) |
| 20261103T101543Z | mr9 | 14.1 to 23.1 | 1,850, 10,774 | 5.82x (0.15) | 0.69, 0.94 ms | 1.28, 10.30 | 8.08x (0.39) |
| 20261103T101618Z | mr9 | 23.1 to 19.5 | 1,581, 10,299 | 6.52x (0.31) | 0.69, 0.97 ms | 1.22, 10.14 | 6.30x (0.31) |
| 20261103T101652Z | mr4f | 19.5 to 17.4 | 1,779, 10,026 | 5.64x (0.20) | 0.71, 1.02 ms | 1.36, 10.37 | 7.48x (0.44) |

- **Pooled over each binary's 40 pairs, in process:** mr4f 5.53x (SE 0.11), concurrency 32 at 9,808 a second;
  mr9 6.17x (SE 0.19), at 10,531. Over the socket 7.14x (SE 0.30) and 7.19x (SE 0.29).
- **The checks' changes take 8% off a lifecycle's CPU at both concurrencies,** 0.75 to 0.69 ms at 1 and 1.04 to 0.96
  at 32 (the pairs' medians), and concurrency 1 keeps fewer CPUs busy beside its own path, 1.36 to 1.27: the work
  they took off was beside it. Concurrency 32 rose 7%, the ratio 12%.
- **Over the socket one pass passed 8x, 8.08x, and its twin read 6.30x:** that driver's concurrency 1 waits on the
  agent's socket as well, so its ratio swings with the host more than the in-process one.
- **Run 8's ceiling was its load's:** a lifecycle's CPU at concurrency 32 is 1.36 to 1.44 times its CPU at 1 here (each
  pass's medians), where run 6's pass without a network namespace (20261103T064414Z, which run 8 cites), at load 37.4 to
  39.0, gave 1.93. How much more a lifecycle costs 32 at once moves with the host's load, so run 8's "about 6.6 times
  concurrency 1, with all of its work off its own path gone" is withdrawn. On this run's figures, that work gone (17% of
  concurrency 1's CPU, `create_serial.md` run 17) puts concurrency 32 at about 7.2 times concurrency 1, and its CPUs
  kept busy as well, 10.2 of 12 now, at about 8.4 times.

## Run 10 (2026-11-03, fedora): criterion 2 as its text reads it, on host.sh's CPU set and on 4-15

- **What the criterion reads:** "sustained lifecycles a second at concurrency 32 against throughput at concurrency 1
  in the same OCI pass, one owner's boxes, at least 8x, measured by seam_oci's throughput and sustained phases
  through host.sh on fedora". It names no CPU set, so host.sh's own, 4-31, governs, with 4-15 beside it (the lead,
  2026-11-03T16:50Z); latency parts stay on 4-15.
- **The binaries:** step 6 alone at its tip, tip8 (`cp/t7-ext-s6-kept-socket` at 9693bfc, b82bf7f's code but for one
  line's formatting; `hex` bcc6cd4556b7a9e2, `seam_oci` f103b11339f33b5e), and the merge, mr10 (`cp/t7-ext-s6-x-s3`'s
  e9fb28f with step 6's checks' three changes, the exec FIFO's fix and the runtime records' shards, uncommitted, of
  record for no branch; `hex` 59d490d19f583b50, `seam_oci` 83fd243409634180).
- **Host:** the unit `hex-g5x-s6-verify13b-20261103T165417Z`, one owner's boxes, reuse on, a network namespace each;
  host.sh with its throughput and sustained phases, 3 rounds, `LIFECYCLES` 2,000 and `HEXBENCH_SUSTAINED` 10,000;
  then the interleaved phase, 2 rounds of 10 pairs; load 3.6 to 24.1 over the runs, other tenants beside them.

As the criterion reads it, each round's sustained rate over its throughput phase's at concurrency 1 (each pass's 3
rounds: the median, least to most):

| run | tree | CPUs | in process | over the socket |
|---|---|---|---|---|
| 20261103T165526Z | merge | 4-31 | 25.12x (24.88 to 26.48) | 23.55x (20.87 to 23.62) |
| 20261103T170058Z | step 6 alone | 4-31 | 8.29x (7.93 to 9.32) | 8.01x (7.87 to 9.10) |
| 20261103T165826Z | merge | 4-15 | 7.74x (5.87 to 9.09) | 7.80x (6.79 to 8.42) |
| 20261103T170431Z | step 6 alone | 4-15 | 6.44x (6.42 to 7.33) | 5.95x (3.35 to 6.23) |

Interleaved, the cross-check, pooled over 20 pairs (mean ratio, bootstrap SE; each half's CPU, the pairs' medians):

| run | tree | CPUs | in process | CPU a lifecycle, c1, c32 | CPUs busy, c1, c32 | over the socket |
|---|---|---|---|---|---|---|
| 20261103T170721Z | merge | 4-31 | 18.96x (0.27) | 1.18, 1.22 ms | 1.10, 21.49 | 19.94x (1.09) |
| 20261103T170834Z | step 6 alone | 4-31 | 5.89x (0.07) | 1.55, 2.36 ms | 0.98, 8.83 | 7.80x (0.41) |
| 20261103T170802Z | merge | 4-15 | 5.11x (0.19) | 0.58, 1.02 ms | 1.28, 10.28 | 4.96x (0.15) |
| 20261103T171011Z | step 6 alone | 4-15 | 3.09x (0.14) | 0.78, 1.61 ms | 1.09, 6.62 | 3.10x (0.07) |

- **On host.sh's 4-31 the merge passes 8x by both measures,** 25.12x as the criterion reads it and 18.96x
  interleaved; step 6 alone reads 8.29x as written, its least round 7.93x, and 5.89x interleaved.
- **On 4-31 a lone lifecycle runs slower,** at more CPU than one of 32 at once (the merge's throughput phase: 1.57 to
  1.74 ms a lifecycle at 1 against 1.20 to 1.24 at 32): there one runs on the E-cores too, or at their clocks, as
  run 8 found, which lifts the ratio. On 4-15, two threads share each core: the ratio is the CPUs kept busy times the
  CPU a lifecycle, (busy32/busy1) x (cpu1/cpu32), for the merge's interleaved pass (10.28/1.28) x (0.58/1.02),
  about 4.6x, its CPU a lifecycle 1.76 times its CPU alone under that pass's load (run 9's: 1.36 to 1.44).
- No start or create was refused in any of the eight passes.
