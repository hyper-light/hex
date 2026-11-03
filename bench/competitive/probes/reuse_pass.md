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
