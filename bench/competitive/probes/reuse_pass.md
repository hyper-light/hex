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

## Run 11 (2026-11-03, fedora): the create pool's wakes, A B B A on 4-15

- **Binaries:** mr10, run 10's merge (`hex` 59d490d19f583b50, `seam_oci` 83fd243409634180), against mr11, mr10 with
  508acde's change to the create pool ported to x-s3's pool: its starting thread woken only for a seed to start, its
  line only for a hand-off (`hex` ed7e76509fa73fb0, `seam_oci` 678990fb38e15538).
- **Host:** the unit `hex-g5x-s6-verify15b-20261103T180307Z`, 18:03 to 18:10Z, run 9's method, A B B A; load 7.7 at its
  start and 19.5 to 32.4 after, CPU pressure 1.0% at its start and 24.2 to 29.0% after, other tenants beside it.

Pooled over 40 pairs each (mean ratio, bootstrap SE; CPU a lifecycle and CPUs busy, the pairs' medians):

| tree | runs | in process | CPU a lifecycle, c1, c32 | CPUs busy, c1, c32 | over the socket |
|---|---|---|---|---|---|
| mr10 | 20261103T180310Z, T180505Z | 6.30x (0.41), 8,586/s over 1,316/s | 0.71, 1.06 ms | 1.17, 9.86 | 7.06x (0.31) |
| mr11 | 20261103T180349Z, T180427Z | 5.61x (0.18), 9,690/s over 1,733/s | 0.70, 1.05 ms | 1.28, 10.42 | 6.03x (0.18) |

- **The pool's change keeps more of concurrency 32's CPUs busy, not a higher ratio here:** in process those rose from
  9.86 to 10.42 at the same CPU a lifecycle, the waits on the pool's lock gone (`create_serial.md` run 21); over the
  socket concurrency 32's rate didn't move, 8,122 a second against 8,090. The ratio swung with concurrency 1's rate,
  which moved with the load: mr10's second pass ran at load 26 to 32, its concurrency 1 at 1,077 a second.
- **As the criterion reads it, for mr11:** on 4-15 5.15x in process (4.72x to 5.23x) and 5.16x over the socket (4.03x
  to 5.74x), run 20261103T180548Z; on 4-31 16.00x (10.86x to 20.20x) and 15.12x (10.90x to 15.33x), run
  20261103T180816Z, flattered as run 10's.

## Run 12 (2026-11-03, fedora): option 1, concurrency 1 on the P-cores and 32 on all, beside 4-15, in one interleaved pass

- **What it measures** (the lead, 2026-11-03T17:48Z, for Ada's choice of the bar): interleaved pairs with each pair's
  concurrency 1 pinned to 4-15 and its 32 to 4-31 (2ff615a: every thread of seam_oci and of its descendants re-pinned
  at each switch, and its CPUs read back), beside the 4-15-only interleaved ratio, the stricter measure and the method
  of record (the lead, 2026-11-03T17:47Z). Both reported; neither is called met until Ada picks the bar.
- **Binaries:** the merge, mr12h: mr11 with step 3's setsid first (cbe5673) and the harness (`hex` ff5cef383a844f85,
  `seam_oci` abf25aa454bc75ef); step 6 alone, tip9 (2ff615a; `hex` 72a5769b437bcfa4, `seam_oci` 40e9653e5c0fd8c4).
- **Host:** the unit `hex-g5x-s6-verify17-20261103T183130Z`, 18:32 to 18:45Z, A B C D D C B A; load 26.6 to 44.0, CPU
  pressure 2.1 to 29.9%, about 64 other tenants' containers.

Pooled over 40 pairs each:

| tree | measure | runs | in process | CPU a lifecycle, c1, c32 | CPUs busy, c1, c32 | over the socket |
|---|---|---|---|---|---|---|
| merge | option 1 | 20261103T183222Z, T184504Z | 7.55x (0.15), 11,199/s over 1,513/s | 0.81, 1.42 ms | 1.26, 16.19 | 7.16x (0.12) |
| merge | 4-15 | 20261103T183613Z, T184151Z | 4.78x (0.07), 8,632/s over 1,806/s | 0.68, 1.17 ms | 1.28, 10.22 | 4.92x (0.10) |
| step 6 alone | option 1 | 20261103T183303Z, T184226Z | 1.59x (0.03), 1,194/s over 750/s | 1.38, 5.48 ms | 1.04, 6.66 | 1.73x (0.04) |
| step 6 alone | 4-15 | 20261103T183701Z, T183931Z | 1.84x (0.02), 1,430/s over 779/s | 1.30, 3.70 ms | 1.05, 5.34 | 2.02x (0.03) |

- **Each half held its CPUs:** in every pair of every option-1 run, every task the pin reached held its half's CPUs
  alone, as its `Cpus_allowed_list` read back: 11 to 90 tasks at a switch.
- **The merge reads 7.55x as option 1 and 4.78x on 4-15 alone.** A lifecycle at concurrency 32 costs 1.42 ms on all of
  fedora's CPUs against 1.17 on 4-15: part of its work runs on the E-cores.
- **Step 6 alone fell to a fifth of run 10's rate at concurrency 32:** run 13 and `create_serial.md` run 22 show why.

## Run 13 (2026-11-03, fedora): step 6 alone, before and after the create pool's change, under about 7,500 host threads

- **Binaries:** tip8 (9693bfc; `hex` bcc6cd4556b7a9e2, `seam_oci` f103b11339f33b5e) against tip9 (2ff615a), A B B A,
  interleaved on the E-cores 16-31; the unit `hex-g5x-s6-verify18-20261103T185412Z`, 18:54 to 19:12Z; the host's
  threads 7,292 to 7,580 at each pass's start, load 23.2 to 46.7, CPU pressure 0.2 to 3.5%, kiln's pool containers
  beside it.

| run | tree | in process | CPU a lifecycle, c1, c32 | over the socket |
|---|---|---|---|---|
| 20261103T185413Z | tip8 | 2.67x (0.16), 715/s over 260/s | 2.86, 6.64 ms | 2.89x (0.17) |
| 20261103T190013Z | tip9 | 3.33x (0.13), 967/s over 292/s | 3.70, 6.40 ms | 3.82x (0.14) |
| 20261103T190431Z | tip9 | 3.45x (0.12), 973/s over 283/s | 3.81, 6.45 ms | 3.52x (0.15) |
| 20261103T190832Z | tip8 | 3.38x (0.15), 989/s over 293/s | 3.77, 6.40 ms | 2.95x (0.16) |

- **The fall is the host's, not the create pool's change or the protocols' count:** tip8 and tip9 cost the same, 2.9
  to 3.8 ms a lifecycle at concurrency 1 and 6.4 to 6.6 at 32, where run 10's tip8 cost 0.78 and 1.61 on 4-15.
  `create_serial.md` run 22 profiles both: a third of a lone lifecycle's CPU is pivot_root's walk of the host's threads.


## Run 14 (2026-11-03, fedora): the `/dev` pool's low mark, interleaved A B B A on 4-15

- **Binaries:** mr12h (`hex` ff5cef383a844f85), and mr13 (`hex` 252eea5c30632778), which is mr12h with the pool's thread
  woken only at a key's low mark and filling the key in one pass (`3260249`, on step 3's branch).
- **Method:** the unit `hex-g5x-s6-v23-20261103T220545Z`, 22:14Z to 22:17Z. Interleaved pairs on 4-15, 2 rounds of 10
  pairs a run, one owner's boxes, reuse on, one-minute load 19 to 39 with kiln's pool of about 65 containers beside it.

| run | binary | in process (SE) | rates, 32 over 1 | CPU a lifecycle at 1, 32 | CPUs busy at 1, 32 |
|---|---|---|---|---|---|
| `20261103T221421Z` | mr12h | 6.27x (0.19) | 7,721/s over 1,234/s | 1.01, 1.27 ms | 1.27, 9.96 |
| `20261103T221508Z` | mr13 | 6.12x (0.14) | 8,305/s over 1,354/s | 0.90, 1.23 ms | 1.26, 10.25 |
| `20261103T221550Z` | mr13 | 5.83x (0.13) | 8,146/s over 1,399/s | 0.88, 1.24 ms | 1.25, 10.27 |
| `20261103T221634Z` | mr12h | 4.74x (0.09) | 8,431/s over 1,776/s | 0.69, 1.20 ms | 1.27, 10.34 |

Pooled over 40 pairs each: mr12h 5.51x (SE 0.16), 8,060/s over 1,457/s; mr13 5.98x (SE 0.10), 8,225/s over 1,376/s. Over
the socket, 5.22x (SE 0.17) and 5.58x (SE 0.09).

**What it says: nothing for the low mark, and neither reaches 8x.** mr12h alone reads 6.27x and then 4.74x minutes apart,
as its concurrency 1 moves from 1.01 ms of CPU a lifecycle to 0.69, and mr13's two runs lie inside that. Concurrency 32's
pooled rate moves 2%, within its runs' spread. The low mark's own effect is on its thread (`devfs_clone.md`). What the
ratio swings with here is concurrency 1's CPU a lifecycle, 46% on one binary within three minutes, which is the host's
state and not this change. Its cause isn't named yet.

## Run 15 (2026-11-03, fedora): the held files, the descriptor sweep and the FIFO's two ends, A B B A on 4-15

- **Binaries:** mr13 (`hex` 252eea5c30632778), and mr17 (`hex` fc991e45d9cbb5ce, `seam_oci` 5c607a9069d096a2; `334aee5`,
  merged for measurement only): x-s3's series and step 6's, so mr13's low mark with a kept namespace's `/proc/net` files
  held (`f37032d`), every descriptor from 3 up marked close-on-exec before foreign code, a start holding both FIFO
  ends, and the `/dev` pool's thread with a umask of 0 (held for Ada).
- **Under the old raising code:** mr17's node raised the process's descriptor limit to all it might hold (f37032d,
  before the held files' bound, b49e81e), so every kept namespace held its `/proc/net` files; mr13 predates them. A
  node now keeps the limit it is given, and at a container's default of 1024 holds none.
- **Method:** the unit `hex-g5x-s6-v24-20261103T225919Z`, 23:03Z to 23:06:30Z, run noisy on the lead's order: interleaved
  pairs on 4-15, 2 rounds of 10 pairs a run, one owner's boxes, reuse on, one-minute load 35 to 52, the host's threads
  about 7,100 to 7,350, kiln's pool beside it, `create_serial.md` run 23 just before. Step 4's timed unit
  `hex-s4-job3-20261103T225951Z` (a sustained pass, then a create breakdown) ran on 4-15 too until about 23:05Z, beside
  the first two runs.

| run | binary | in process (SE) | rates, 32 over 1 | CPU a lifecycle at 1, 32 | CPUs busy at 1, 32 | over the socket (SE) |
|---|---|---|---|---|---|---|
| `20261103T230320Z` | mr13 | 4.82x (0.12) | 7,451/s over 1,546/s | 0.76, 1.26 ms | 1.21, 9.49 | 5.74x (0.11) |
| `20261103T230409Z` | mr17 | 5.64x (0.35) | 6,512/s over 1,162/s | 0.96, 1.26 ms | 1.13, 9.10 | 5.44x (0.17) |
| `20261103T230500Z` | mr17 | 5.29x (0.18) | 8,046/s over 1,527/s | 0.77, 1.22 ms | 1.19, 9.80 | 5.06x (0.11) |
| `20261103T230545Z` | mr13 | 5.18x (0.13) | 8,299/s over 1,603/s | 0.75, 1.21 ms | 1.24, 10.33 | 5.21x (0.24) |

Pooled over 40 pairs each: mr13 5.00x (SE 0.09), 7,852/s over 1,574/s; mr17 5.46x (SE 0.20), 7,198/s over 1,320/s. Over
the socket, mr13 5.47x (SE 0.14) and mr17 5.25x (SE 0.11).

**What it says: no gain to claim, and neither reaches 8x.** In process mr17 reads 0.46x higher, about two standard
errors, and over the socket 0.22x lower. Its higher ratio comes from its first run's slower concurrency 1 (0.96 ms of CPU
a lifecycle against 0.75 to 0.77 in the other three), the host's swing run 14 found; its second run lies beside mr13's. The two runs after step 4's unit ended read
mr17 5.29x (SE 0.18) and mr13 5.18x (SE 0.13), inside each other's spread.
Concurrency 32's CPU a lifecycle is 1.21 to 1.26 ms in all four. The held files' own saving is on the checker's thread
and is still to be measured apart.

## Run 16 (2026-11-04, fedora): the final ratio's merge against step 3's a600107, A B B A on 4-15, and option 1

- **Binaries:** the final merge, `cp/t7-ext-s6-x-s3-final` at ca6dc80 (x-s3 c912114 with step 6's b84bbaf, then step 3's
  final a600107; `hex` a7706c0bf68bad85, `seam_oci` 55d4dd6ab5f6ae91), against mr17 (run 15's: `hex` fc991e45d9cbb5ce,
  `seam_oci` 5c607a9069d096a2). The merge's gates (018867e, ca6dc80) and its root lab passed in the same unit first.
- **Under the old raising code:** its node raised the process's descriptor limit to all it might hold (f37032d,
  before the held files' bound, b49e81e), so every kept namespace held its `/proc/net` files; a node now keeps the
  limit it is given, and at a container's default of 1024 holds none (`What criterion 2 needs on 4-15`).
- **Method:** the unit `hex-g5x-s6-fdh25-20261104T054328Z`, run 9's method, A B B A from 05:47:38Z to 05:50:07Z, then one
  option-1 pass to 05:50:33Z; one owner's boxes, reuse on, interleaved pairs on 4-15, 2 rounds of 10 pairs a run; kiln's
  pool of about 103 containers beside it and no other unit of Hex's, and the session-lab agent's kiln gate unit
  `kiln-gate-9c245a2959aa-1793770896` (from 05:41Z, CPUWeight 1, Nice 19, its tests starting Docker containers),
  pinned to 16-31 only from 05:50Z, so on 4-15 beside the first two runs. CPU pressure rose under the second run.

| run | binary | in process (SE) | rates, 32 over 1 | CPU a lifecycle at 1, 32 | CPUs busy at 1, 32 | over the socket (SE) | load, CPU pressure |
|---|---|---|---|---|---|---|---|
| `20261104T054739Z` | mr17 | 5.77x (0.19) | 9,137/s over 1,569/s | 0.75, 1.19 ms | 1.22, 10.14 | 5.07x (0.10) | 8.6 to 12.5, 0.4 to 8% |
| `20261104T054815Z` | final | 6.63x (0.20) | 8,026/s over 1,206/s | 0.94, 1.29 ms | 1.15, 9.61 | 6.83x (0.38) | 24 to 28, 35 to 38% |
| `20261104T054901Z` | final | 4.74x (0.11) | 9,238/s over 1,944/s | 0.71, 1.20 ms | 1.27, 10.28 | 4.69x (0.06) | 27 to 28, 16 to 19% |
| `20261104T054934Z` | mr17 | 4.62x (0.07) | 9,428/s over 2,036/s | 0.70, 1.19 ms | 1.24, 10.21 | 4.89x (0.06) | 25 to 27, 18 to 19% |
| `20261104T055007Z` | final, option 1 | 7.52x (0.16) | 15,926/s over 2,125/s | 0.65, 1.39 ms | 1.28, 20.07 | 7.28x (0.15) | 23, 16 to 17% |

Pooled over 40 pairs each, on 4-15: mr17 5.20x (SE 0.13), 9,281/s over 1,772/s; the final merge 5.69x (SE 0.19),
8,589/s over 1,489/s. Over the socket, 4.98x (SE 0.06) and 5.76x (SE 0.27).

**What it says: the final merge is mr17 on this path, and neither reaches 8x; option 1 reads 7.52x.** The final merge's
higher pooled figure is its first run's: CPU pressure stood at 35 to 38% through it, and its concurrency 1 ran at 0.94 ms
of CPU a lifecycle against 0.70 to 0.75 in the other three, so 1,206 a second. Its second run, at 4.74x, lies beside
mr17's second, at 4.62x, and concurrency 32 ran 9,137 to 9,428 a second in the three steady runs. Step 3's changes since
16e52a9 are its classic init's (`e1b85f4`, `933d56f`), which the pruned path the bench takes doesn't run.

## What criterion 2 needs on 4-15, from these runs

The ratio is the CPUs each concurrency keeps busy times the CPU a lifecycle costs, (busy32 / busy1) x (cpu1 / cpu32).
Run 16's two steady runs: (10.25 / 1.25) x (0.705 / 1.195) = 4.84x, against the 4.62x and 4.74x measured.

- **cpu32 / cpu1 has a floor from the cores, not from Hex:** 4-15 is six cores' two threads each. On one thread of each
  core concurrency 32 cost 1.18 times concurrency 1's CPU a lifecycle (run 8), on both 1.36 to 1.44 (run 9, CPU pressure
  25%), so sharing a core adds about 1.19 times; run 16, with other tenants on the same cores, 1.69. At 1.19 alone, with
  all 12 CPUs busy and today's 1.25 at concurrency 1: 12 / 1.25 / 1.19 = 8.07x, the bar with nothing to spare.
- **busy1 is above 1 by the work beside concurrency 1's path:** the namespace checker and the `/dev` pool (about 0.16 ms
  a lifecycle in `create_serial.md` run 23's threads, before the held files) and the overlap of the agent's and the
  init's steps. Work cut from the path speeds concurrency 1 as much as 32; work cut from beside it speeds 32 alone.
- **So 8x on 4-15 needs all three at once:** concurrency 32's CPUs all busy (10.2 to 10.4 of 12 today), its CPU a
  lifecycle within the cores' sharing of concurrency 1's (1.69 times today, under load), and next to no work beside
  concurrency 1's path. With every CPU busy and none beside the path, today's 1.69 still caps it at 12 / 1.0 / 1.69 =
  7.1x. What run 16 has left beside the path: the checker's netlink dumps (about 30 us a check), the `/dev` pool's tmpfs
  and nodes (about 70 us a box, `devfs_clone.md`).
- **Option 1 at 7.52x** puts concurrency 32 on 4-31's 28 CPUs, 20.1 of them busy: there the ratio moves with how many
  of the 28 a lifecycle's waits leave idle as much as with the CPU a lifecycle.
