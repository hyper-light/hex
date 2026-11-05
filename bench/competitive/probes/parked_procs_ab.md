# Parked procs: the A/B of record on fedora's host (decision 8, parked procs; AC-T7.1.3)

**Run.** fedora (Linux 6.19.10), 2026-11-05 14:38:38Z-14:42:35Z, under `fedora-timed.lock`. One release build of
cp/t7-g5-parkproc 9e13b95: hex sha256 65f6100e612bebd8, seam_oci d13c5005b54a2793 (each cell's `env.txt`). The
harness is g6's ab4c cell (bench/competitive/probes/create_breakdown.sh and seedstart_extract.py at cp/t7-g6
a646981, exported without git, so `env.txt` names no harness commit), with one change:
create_breakdown.sh's `docker run` passes `-e HEXBENCH_PARKED_PROCS`. Body and logs:
`~/.cache/hex-t7/g5tail/ab-pp-20261105T143755Z` on fedora; each cell's out dir is in its `exits`.
- A: parked procs off (`HEXBENCH_PARKED_PROCS` unset). B: on (`=1`). Both have parked inits on: owner bundles, reuse
  on.
- The under-load bar's conditions: the mounts trace of part 19, 2,000 creates at 8 in flight, 8,000 idle threads,
  Docker's paths, the /dev pool on, 6 or 12 busy loops on CPUs 4-15 at nice 0. No priority change for Hex, and no
  co-tenant.
- 4 rounds, interleaved: A then B in odd rounds, B then A in even. Load at start 9.12, at end 13.76. Noisy host, as
  run.

**Part 19 and the whole create** (µs unless marked; one row per cell, rounds 1-4):

| L | arm | part 19 p50 | part 19 p99 | part 19 unpreempted p99 | create p99 (ms) | mount-lock waits a create |
|---|-----|-------------|-------------|-------------------------|-----------------|---------------------------|
| 6 | A | 204, 224, 229, 223 | 1093, 1466, 1232, 1369 | 504, 787, 510, 635 | 3.274, 3.051, 3.529, 3.269 | 4.50, 5.24, 4.36, 4.93 |
| 6 | B | 37, 39, 36, 36 | 586, 584, 688, 524 | 214, 154, 383, 247 | 3.236, 2.755, 3.131, 2.587 | 0.79, 0.68, 0.78, 0.75 |
| 12 | A | 197, 207, 231, 200 | 1260, 1569, 1777, 1240 | 397, 447, 415, 275 | 6.903, 6.511, 5.198, 4.543 | 2.00, 2.49, 2.81, 1.91 |
| 12 | B | 38, 36, 40, 37 | 815, 1001, 341, 369 | 296, 428, 63, 71 | 4.396, 4.189, 3.622, 4.973 | 0.47, 0.47, 0.29, 0.27 |

**Mount-lock write holds per box** (`nssem_holds_by_holder`, holds / 2,000; every cell alike, give or take 0.2):
- The create's init holds it 31.0 times a create with B off, 5.1 with B on. Its calls fall from 104 to 17.4 a create,
  and its time in them from 175-219 µs to 31-49 µs.
- The node (the in-process seam_oci) holds it 3.0 times a box with B off, 34.0 with B on: the proc and its masks,
  now made at park time, off the create's path. So a box's holds in all go from about 35.7 to about 40.9. They move
  off the create's path; they aren't removed.
- The delete pays that work (reuse on: the park, then the proc). This harness doesn't time deletes, so the delete's
  cost isn't measured here.

**The predicted key** (seam_oci's `parked` counts, throughput phase, B):
- 7 of the 8 cells: 2,000 procs made, 1,992 hits, 0 misses, 0 dry, 0 failed. 8 are held at the end: the idle slots'.
- B-L6-r3: 1,978 hits, 14 misses, 0 dry, 0 failed. Its ipc and uts pair pool ran dry 18 times (`ipc_uts.made_on_path`
  18), and those creates make fresh ipc and uts namespaces. Their flags then differ from the parked init's, so the
  init is ended and the create clones fresh. Those 14 are the misses, and `claimed` is 1,978 there. That is decision
  8's flag match, not the proc. No A cell's pair pool ran dry.

**Not of record** for the whole create against libcrun: this run has no libcrun arm. Its create p99 is the A/B's own
comparison.

## Again at 506a263 (the review's off-path drop)

**Run.** The same body and harness, at cp/t7-g5-parkproc 506a263 (adds `ProcSink`, an unattached tree dropped by
the node's `hex-proc-drop` thread). fedora, 2026-11-05 15:02:19Z-15:07:53Z, under the timed lock. Load rose from
32.7 to 141 during the run (another tenant's), so this host was far busier than the first run's. Body:
`~/.cache/hex-t7/g5tail/ab-pp-20261105T150137Z`.

| L | arm | part 19 p50 | part 19 p99 | unpreempted p99 | create p99 (ms) | mount-lock waits a create |
|---|-----|-------------|-------------|-----------------|-----------------|---------------------------|
| 6 | A | 239, 252, 256, 254 | 1301, 1265, 1260, 1459 | 819, 628, 787, 620 | 3.519, 4.306, 3.812, 3.741 | 4.70, 5.01, 5.21, 4.97 |
| 6 | B | 40, 42, 42, 45 | 405, 401, 523, 1133 | 197, 116, 254, 895 | 3.278, 3.482, 4.857, 4.204 | 0.69, 0.66, 0.60, 1.35 |
| 12 | A | 229, 242, 239, 245 | 1698, 1741, 2380, 2312 | 842, 443, 754, 855 | 7.037, 8.107, 6.913, 11.033 | 2.54, 2.13, 2.40, 2.70 |
| 12 | B | 42, 44, 41, 42 | 1440, 788, 775, 883 | 275, 176, 149, 274 | 6.643, 6.000, 5.818, 5.111 | 0.48, 0.49, 0.43, 0.47 |

- Every B cell: 1,992 hits, 0 misses, 0 dry, 0 failed. The phase's `procs_dropped` is 0: its 8 held procs are
  dropped after the phase, with the node.
- Holds as before: the init's are 31.0 a create off and 5.1 on; the node's are 1.8-3.0 a box off and 34.0 on.
- Part 19's p50 falls about 6x and its p99 falls in every L12 cell. At L6, B's create p99 is under A's in 2 of 4
  rounds: at this load the whole create's p99 at L6 doesn't separate the arms. At L12 every B cell is under every A
  cell.

## Again at 195e86a: the proc made by one worker thread, after the park (the lead's 14:15Z (b))

**Run.** The same body and harness, at cp/t7-g5-parkproc 195e86a. The delete or refill only queues a job, and the
node's one `hex-parked-procs` thread makes the proc (queue bound `PER_NODE`). seam_oci sha256 8fa181e71654f2f4.
fedora, 2026-11-05 15:33:18Z-15:37:21Z, under the timed lock; load fell from 15.6 to 12.5.
Body: `~/.cache/hex-t7/g5tail/ab-pp-20261105T153245Z`.

| L | arm | part 19 p50 | part 19 p99 | create p99 (ms) | hits | dry | skipped | orphaned |
|---|-----|-------------|-------------|-----------------|------|-----|---------|----------|
| 6 | A | 214, 214, 216, 234 | 1177, 1135, 1173, 1276 | 3.116, 3.191, 2.894, 3.275 | - | - | - | - |
| 6 | B | 212, 217, 219, 220 | 1200, 1187, 1069, 1296 | 2.942, 3.651, 2.928, 3.471 | 427, 428, 440, 409 | 1565, 1564, 1552, 1583 | 956, 901, 852, 930 | 609, 663, 700, 653 |
| 12 | A | 202, 203, 203, 198 | 1208, 1566, 1860, 757 | 4.000, 4.770, 7.554, 3.377 | - | - | - | - |
| 12 | B | 194, 198, 200, 199 | 1941, 1828, 1487, 2207 | 4.643, 5.180, 4.450, 5.060 | 663, 659, 602, 637 | 1329, 1333, 1390, 1355 | 909, 941, 994, 918 | 420, 392, 396, 437 |

- The one worker doesn't keep up with 8 creates in flight. Each cell skipped about 900 parks, its queue full, and
  made about 400-700 procs only after their init was claimed (orphaned, then dropped). So 67-80% of claims found no
  proc (dry): the init mounted its own, and part 19 is A's again. B's init holds a create are 22.4-25.7, against 5.1
  with the proc made on the park's own thread (506a263).
- No misses and no failures.
- With the proc made on the park's own thread (the delete, or the refill: 9e13b95 and 506a263, above), every claim
  was a hit, 1,992 of 1,992. This run is what falsifies one worker thread, not the parked proc.

## The 7.2 guest, indicative (NOT OF RECORD: a VM's numbers)

**Run.** Fedora's kernel 7.2.15-200.fc44 in vm7_lab.sh's guest on fedora: 12 vCPUs on CPUs 4-15, 6G. 2026-11-05
14:47:42Z-14:51:31Z, under `fedora-timed.lock`. The same binaries (hex 65f6100e612bebd8, seam_oci d13c5005b54a2793).
- Harness: g6's guest cell, vm7_breakdown.sh at a646981, with HEXBENCH_PARKED_PROCS added to the env it hands the
  guest. Body: `~/.cache/hex-t7/g5tail/vm-pp-20261105T144742Z` on fedora.
- The Copy way (`HEX_OCI_MNTNS=copy`), since this build parks on Copy only. Otherwise as on the host: 2,000 creates
  at 8 in flight, 8,000 idle guest threads, Docker's paths, the /dev pool on, owner bundles with reuse on, 6 or 12
  busy loops in the guest.
- 2 rounds: AB, then BA. fedora's load rose from 25.7 to 52.6 over the run, with no core idle by its end.

| L | arm | part 19 p50 | part 19 p99 | unpreempted p99 | create p99 (ms) | mount-lock waits a create |
|---|-----|-------------|-------------|-----------------|-----------------|---------------------------|
| 6 | A | 215, 226 | 1324, 2999 | 1014, 1515 | 7.082, 9.890 | 4.64, 3.89 |
| 6 | B | 47, 44 | 4706, 998 | 2844, 656 | 15.288, 9.162 | 0.30, 0.59 |
| 12 | A | 202, 220 | 1313, 2052 | 508, 1043 | 8.359, 7.071 | 1.66, 1.64 |
| 12 | B | 43, 45 | 317, 1060 | 86, 226 | 8.278, 14.421 | 0.27, 0.20 |

- Every B cell: 2,000 procs made, 1,992 hits, 0 misses, 0 dry, 0 failed. The init's holds are 28.9 a create off and
  5.0 on; the node's are 3.0 a box off and 32.0 on.
- Part 19's p50 falls about 5x, as on the host. Its p99 and the whole create's p99 don't fall consistently: B-L6-r1
  and B-L12-r2 are the slowest cells of their L. With the host this loaded, the guest's tails mostly reflect the
  host, so they settle nothing either way.

## The 7.2 guest on the tree way, indicative (NOT OF RECORD: a VM's numbers)

**Run.** As the guest run above, but at cp/t7-g5-parkproc 8c35cf9, where a park clones with its way's flags. The
mount namespaces are made from the tree (`HEX_OCI_MNTNS=tree`, 7.x's default). Binaries: hex c2cb3634627c093e,
seam_oci 9638d84d168ba926. 2026-11-05 15:18:50Z-15:22:13Z, under the timed lock; fedora's load fell from 30.2 to 25.0.
Body: `~/.cache/hex-t7/g5tail/vm-pp-20261105T151759Z`.

| L | arm | part 19 p50 | part 19 p99 | unpreempted p99 | create p99 (ms) | mount-lock waits a create |
|---|-----|-------------|-------------|-----------------|-----------------|---------------------------|
| 6 | A | 225, 198 | 2118, 1345 | 1246, 686 | 6.745, 4.713 | 2.95, 3.93 |
| 6 | B | 43, 41 | 747, 608 | 410, 257 | 3.552, 5.717 | 0.66, 0.67 |
| 12 | A | 190, 202 | 1528, 1449 | 354, 357 | 13.255, 5.796 | 1.57, 1.53 |
| 12 | B | 41, 36 | 350, 227 | 93, 71 | 4.194, 9.053 | 0.22, 0.30 |

- Every cell: all 2,000 mount namespaces made from the tree, and 1,992 creates claimed a parked init. Before
  8c35cf9's parent, 81082bd, a create on this way claimed none.
- In the B cells: 1,992 hits, 0 misses, 0 dry, 0 failed. The proc pool served 0 to 7 creates a cell, the ones no
  parked init was ready for. A create takes one or the other, never both.
- The init's holds are 29.0 a create off and 5.0 on; the node's are 2.0 a box off and 31.0 on.
- Part 19's p50 falls about 5x, and its p99 falls in all 4 pairs. The whole create's p99 doesn't settle: B is lower
  in 2 of 4 pairs, and B-L6-r2 (5.72 ms) and B-L12-r2 (9.05 ms) are over their pairs' A.
