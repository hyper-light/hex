# One owner's boxes through host.sh's OCI pass (goal 5 extended, step 6)

Goal 5 extended, step 6's second criterion: sustained lifecycles a second at concurrency 32 against throughput at
concurrency 1, in the same OCI pass, one owner's boxes, at least 8x (seam_oci's throughput and sustained phases through
host.sh on fedora).

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

- **The criterion is not met:** 4.21x to 4.81x against 8x.
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
  them. Both rates rose with them: the merge sustained 3,754 to 4,293 a second at concurrency 32 against step 6's
  3,110 to 3,205, and ran 819 to 1,028 at concurrency 1 against 575 to 694, a lifecycle alone taking 0.97 to 1.22 ms.
  At concurrency 1 the rate rose the more, so the ratio fell.
- **Every box was handed its owner's idle cgroup but one:** of the merge's sustained phases in process, 10,000 and
  9,999 of 10,000, and 10,000 and 9,999 ended through their init's pidfd.
- **What holds concurrency 32 with all three in** (`create_serial.md`, run 10): neither `cgroup_mutex` nor the mount
  namespace's lock, but RTNL, the host's one network lock, in making each box's network namespace. The pool that makes
  them ahead (decision 4) ran dry: 9,561 to 9,608 of each sustained phase's 10,000 creates made theirs on the path in
  the merge, against 5,078 to 5,359 for step 6 alone.
