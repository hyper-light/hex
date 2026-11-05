# Parked procs: the A/B of record on fedora's host (decision 8, parked procs; AC-T7.1.3)

**Run.** fedora (Linux 6.19.10), 2026-11-05 14:38:38Z-14:42:35Z, under `fedora-timed.lock`. One release build of
cp/t7-g5-parkproc 9e13b95: hex sha256 65f6100e612bebd8, seam_oci d13c5005b54a2793 (each cell's `env.txt`). The
harness is g6's ab4c cell (bench/competitive/probes/create_breakdown.sh and seedstart_extract.py at cp/t7-g6
a646981, exported without git, so `env.txt` names no harness commit), with one change:
create_breakdown.sh's `docker run` passes `-e HEXBENCH_PARKED_PROCS`, applied by hand. That is the line cp/t7-g6
4c25532 adds to a646981 (with `HEXBENCH_AHEAD`); later runs build from 4c25532 by git archive. Body and logs:
`~/.cache/hex-t7/g5tail/ab-pp-20261105T143755Z` on fedora; each cell's out dir is in its `exits`.

**Gate 6 at this tree (9e13b95):** unit hex-g5tail-ppfull-20261105T143517Z, gates 7/7 exit 0 and the root lab 7 of 7
test binaries, in one unit. The first 7.2 guest section below ran the same binaries.

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

**Gate 6 at this tree:** unit hex-g5tail-ppfull-20261105T150018Z, gates 7/7 exit 0 and the root lab 7 of 7 test binaries, in one unit.

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

**Gate 6 at this tree:** none. The nearest gated tree is cf0c831, which differs only in bench/'s two scripts: unit hex-g5tail-ppfull-20261105T153014Z, gates 7/7 exit 0, root lab 7 of 7, guest 2 of 2.

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

## Of record at 1e5e956: the proc made by the park's own thread, counted by thread (the lead's 15:40Z option (1))

**Run.** The same cell, at cp/t7-g5-parkproc 1e5e956. The harness is cp/t7-g6 f8933bd, taken by git archive with no
patch: its create_breakdown.sh hands the bench `HEXBENCH_PARKED_PROCS` and `HEXBENCH_AHEAD`. Binaries: hex sha256
9c93a282586eb89c, seam_oci c1c3520ce43272e7. fedora, 2026-11-05 17:05:26Z-17:12:32Z, under the timed lock. The 1-minute
load was 43.0 at the start and 53.0 at the end, the host far busier than at 2caebe2. Body:
`~/.cache/hex-t7/g5tail/ab-pp-20261105T170438Z`.

**Gate 6 at this tree:** unit hex-g5tail-ppfull-20261105T170215Z: gates 7/7 exit 0, the root lab 7 of 7 test binaries,
and the 7.2 guest lab 2 of 2, in one unit.

| L | arm | part 19 p50 / p99 / unpreempted p99 (µs) | create p50 / p99 (ms) | delete p50 / p99 (ms) | lifecycles a second |
|---|-----|------------------------------------------|-----------------------|-----------------------|---------------------|
| 6 | A | 247/1357/660, 257/1391/682, 251/1847/612, 254/2366/1644 | 1.217/3.886, 1.484/7.334, 1.528/6.949, 1.416/5.995 | 2.567/6.300, 2.534/9.406, 2.131/7.789, 2.131/16.986 | 1861, 1560, 1715, 1654 |
| 6 | B | 43/549/88, 45/841/69, 47/867/278, 44/552/75 | 1.114/4.346, 1.134/5.246, 1.303/5.128, 1.304/4.666 | 2.278/6.000, 2.880/9.320, 3.321/10.062, 2.118/5.574 | 1951, 1642, 1445, 1945 |
| 12 | A | 240/2068/470, 250/2332/502, 247/2019/605, 248/1972/988 | 1.578/9.295, 1.749/8.425, 1.659/6.321, 1.687/7.448 | 4.503/11.016, 2.871/11.921, 2.630/8.182, 2.592/17.963 | 1126, 1377, 1397, 1411 |
| 12 | B | 46/508/66, 45/917/84, 43/770/58, 42/1373/812 | 1.420/8.229, 1.353/9.425, 1.338/5.406, 1.395/7.528 | 4.914/10.482, 3.868/33.336, 3.173/32.943, 3.710/20.321 | 1099, 985, 1203, 1233 |

- Every B cell: 1,992 hits, 0 misses, 0 dry. All 2,000 of its procs were made at a delete (`procs_made_at_delete`), none
  at the refill. Every A cell made none.
- Holds: the init's 31.0 a create off and 5.1 on; the node's 2.1-3.0 a box off and 30.4-34.0 on.
- Part 19's p50 falls about 5.6x (240-257 to 42-47 µs). The under-load bar (part 19 p99 0.5 ms) is approached, not
  met: B's p99 is 549-867 µs at L6 and 508-1,373 µs at L12. Its p99 falls in every pair (L6 549-867 against 1357-2366;
  L12 508-1373 against 1972-2332), and so does its unpreempted p99.
- The create's p50 falls in every pair, by 0.10-0.40 ms.
- The delete's p50 rises in 6 of 8 pairs, and its p99 reaches 33 ms in two L12 B cells. At this load the delete is
  noisy in both arms (A's own p99 reaches 18 ms).
- The lifecycle rate is mixed: at L6, B is higher in 3 of 4 pairs; at L12, lower in all 4 (985-1,233 a second against
  1,126-1,411). Against 2caebe2's run at a quarter of the load, this one says the same, more loudly.

## The delete-cost pass of record at 1e5e956: host.sh lifecycle with the sustained c32 rate (the lead's 15:40Z)

**Run.** One static-glibc build of 1e5e956 (hex sha256 52ec4855987fb7b7, seam_oci d13aad09db93301c), the same unit as the
mounts run above, after it. host.sh OCI passes (lifecycle, start_true, and sustained at concurrency 32 for 10,000
lifecycles; 2 rounds of 10,000), one owner with reuse on, interleaved A B B A, each under the timed lock on CPUs 4-15.
fedora, 2026-11-05 17:13:29Z-17:57:49Z. Load at each pass's start: 56.1, 26.7, 18.6, 17.9. Out dirs:
`~/.cache/hex-bench/out/` 20261105T171329Z (A), 20261105T172511Z (B), 20261105T173557Z (B), 20261105T174629Z (A).
Gate 6 at this tree: unit hex-g5tail-ppfull-20261105T170215Z.

In process (seam_oci's in-process driver; over the socket in brackets, where both arms ran with parked procs off),
each pass's `table.md`:

| pass | create p50 / p99 (ms) | delete p50 / p99 / p99.9 (ms) | sustained c32 (lifecycles a second) |
|------|-----------------------|-------------------------------|-------------------------------------|
| A 1 | 0.32 / 0.53 | 0.28 / 0.53 / 1.29 (0.30 / 0.52) | 7,667 (6,825) |
| B 1 | 0.33 / 0.59 | 0.34 / 0.61 / 1.17 (0.29 / 0.48) | 7,599 (6,845) |
| B 2 | 0.28 / 0.43 | 0.32 / 0.51 / 0.83 (0.27 / 0.45) | 8,831 (7,798) |
| A 2 | 0.32 / 0.86 | 0.33 / 0.72 / 1.03 (0.29 / 0.57) | 9,104 (8,025) |

- Every B claim took its proc: 10,099 of 10,099 a lifecycle round, 10,100 of 10,100 a start_true round, and 9,772-9,872
  of the same number claimed in the sustained rounds. All B procs were made at a delete; no A round made one.
- The delete, B against the A beside it (A 1 with B 1, B 2 with A 2): in process, p50 +0.06 ms and -0.01 ms, p99 +0.08
  ms and -0.21 ms. (The socket column is parked procs off in both arms: its `hex node` doesn't take the setting. See
  the correction above.) At this tree, serially, the delete doesn't grow materially.
- The sustained c32 rate, B against the A beside it: -0.9% and -3.0% in process. Over the socket, +0.3% and -2.8%,
  but that is off against off.
  A 1 ran at load 56, so its pair is the weaker comparison.
- In B's sustained phase in process, the network-namespace pool ran dry 19 and 12 times in 10,000 (0.2%, 0.1%); never
  in A's. That's the netns pool at c32, not the parked proc; recorded, not chased here.
- Hex's delete stays 30-36x libcrun's at p50 in process.

## Experiment: 8 workers making the procs (side branch cp/t7-g5-parkproc-workers 6dcd731; the lead's 15:09Z (b))

**Run.** The same body and harness, at 6dcd731: the one-worker build (e9a6e66) on f15393c's tree, with `PROC_WORKERS` = 8
threads on one queue bounded at `PER_NODE`. fedora, 2026-11-05 16:53:15Z-16:59:03Z, under the timed lock. Load rose from
45 to 133 during it (another tenant's), so this host was much busier than for the other runs. Body:
`~/.cache/hex-t7/g5tail/ab-pp-20261105T165218Z`.

| L | arm | part 19 p50 | create p50 / p99 (ms) | delete p50 / p99 (ms) | hits | dry | skipped | orphaned |
|---|-----|-------------|-----------------------|-----------------------|------|-----|---------|----------|
| 6 | A | 239, 251, 250, 258 | 1.29/9.91, 1.42/5.75, 1.39/4.92, 1.49/5.08 | 1.56/5.95, 1.83/6.73, 1.53/7.79, 1.61/5.77 | - | - | - | - |
| 6 | B | 228, 241, 233, 239 | 1.35/14.89, 1.39/4.56, 1.35/4.29, 1.37/4.52 | 1.82/5.80, 1.97/7.05, 1.87/15.47, 1.89/7.16 | 665, 643, 619, 680 | 1327, 1349, 1373, 1312 | 754, 738, 807, 735 | 573, 611, 566, 577 |
| 12 | A | 241, 238, 246, 240 | 1.69/10.48, 1.67/7.95, 1.68/7.60, 1.61/5.15 | 3.06/14.55, 2.83/20.28, 2.69/7.03, 2.65/7.59 | - | - | - | - |
| 12 | B | 221, 227, 228, 229 | 1.75/6.84, 1.62/6.12, 1.74/10.08, 1.71/8.53 | 3.16/15.00, 2.60/11.09, 3.83/28.28, 3.79/20.27 | 853, 771, 782, 868 | 1139, 1221, 1210, 1124 | 743, 799, 799, 738 | 396, 422, 411, 386 |

- Eight workers fare little better than one: 31-44% of claims hit, about 750 parks a cell find the queue full, and
  390-610 procs are made after their init was claimed. Part 19 stays at A's numbers.
- The delete still grows: B's p50 is 1.82-1.97 ms at L6 against A's 1.53-1.83. Taking the proc off the delete's
  thread doesn't take its cost off the delete.
- A proc's 34 mount-lock holds are writes to the host's one `namespace_sem`, so more threads queue on the same lock
  (DESIGN, read from part 19's trace; not measured apart here).
- On this host's load, the experiment falsifies making the proc on workers, at 1 or 8 threads. Making it on the
  parking thread is what gets the hits (2caebe2, below).

## Of record at 2caebe2: the proc made on the parking thread, with the delete timed (the lead's 14:23Z)

**Run.** The same body and harness, at cp/t7-g5-parkproc 2caebe2. The proc is made on the parking thread again
(fae7f43); the namespace is opened through the pidfd and unattached trees are dropped by `hex-proc-drop`. seam_oci
(fc5dc85) times each delete beside each create (`delete_ms`). Binaries: hex sha256 5cfd361d02faf7e9, seam_oci
97f5fcdd712cc7ae. fedora, 2026-11-05 15:45:20Z-15:49:04Z, under the timed lock; load rose from 5.8 to 16.3.
Gates and labs at this tree in one unit, hex-g5tail-ppfull-20261105T154329Z: gates 7/7 exit 0, root lab 7 of 7
test binaries, the 7.2 guest lab 2 of 2. Body: `~/.cache/hex-t7/g5tail/ab-pp-20261105T154447Z`.

| L | arm | part 19 p50 / p99 / unpreempted p99 (µs) | create p50 / p99 (ms) | delete p50 / p99 (ms) | lifecycles a second |
|---|-----|------------------------------------------|-----------------------|-----------------------|---------------------|
| 6 | A | 210/1114/551, 214/1272/800, 215/1028/427, 215/1018/623 | 1.019/2.971, 1.055/3.232, 1.025/2.929, 1.106/3.055 | 1.059/3.548, 1.089/3.320, 1.062/6.321, 1.064/3.226 | 3050, 2992, 2879, 2961 |
| 6 | B | 37/730/438, 36/529/334, 37/553/259, 38/517/253 | 0.864/3.105, 0.853/2.860, 0.841/2.611, 0.877/2.725 | 1.344/3.428, 1.403/3.596, 1.349/9.295, 1.414/3.546 | 2948, 2917, 2726, 2861 |
| 12 | A | 204/1574/624, 215/1694/531, 211/1670/549, 208/2132/796 | 1.234/4.982, 1.328/5.079, 1.253/5.265, 1.291/6.447 | 2.317/6.589, 2.502/5.985, 2.562/5.853, 2.504/7.542 | 1848, 1708, 1741, 1678 |
| 12 | B | 36/674/99, 35/761/189, 36/475/71, 37/351/72 | 1.051/3.972, 1.109/4.324, 0.952/4.065, 0.861/5.910 | 2.788/6.740, 2.859/6.923, 2.976/10.686, 3.041/12.604 | 1773, 1697, 1164, 1080 |

- Hits: 1,992 of 1,992 in 7 cells. B-L12-r1 had 1,984 hits and 8 misses, with its ipc and uts pool dry 4 times.
  No cell was dry and none failed.
- Holds: the init's 31.0 a create off and 5.1-5.2 on; the node's 2.7-3.0 a box off and 32.4-34.0 on.
- **The delete grows.** Its p50 is +0.29 to +0.35 ms at L6 and +0.36 to +0.54 ms at L12. Its p99 is higher in 7 of
  8 pairs, by up to 5.1 ms (B-L12-r4). What the create saves, the delete pays and more: the create's p50 falls by
  0.16-0.23 ms at L6 and 0.18-0.43 ms at L12. At L6 the lifecycle rate is 2.5-5.3% lower with B. At L12 two B rounds
  (r3, r4) ran at 1,164 and 1,080 a second, against A's 1,678-1,848, as load rose; the other two match A.
- So the parked proc takes about 0.2-0.4 ms and most of part 19's mount-lock holds off the create, and puts them
  on the delete.

## The delete-cost pass: host.sh lifecycle, A against B (the lead's 14:53Z step 5)

**Run.** One static-glibc build of cp/t7-g5-parkproc 35c5787, as host.sh's passes ship hex (hex sha256 06d7e93c59663441,
seam_oci f6a6bf3fca9a12a3). 35c5787 is fbe1352's tree, plus host.sh handing seam_oci `HEXBENCH_PARKED_PROCS`.
fbe1352 is gated: unit hex-g5tail-ppfull-20261105T155901Z, gates 7/7 exit 0, root lab 7 of 7, guest 2 of 2.
- host.sh OCI passes: lifecycle and start_true, 2 rounds of 10,000 each, one owner with reuse on, so every delete parks
  an init and, with B, makes its proc. Serial: one create at a time. host.sh's own bundles, whose proc has one
  read-only path (`/proc/sys`), not Docker's paths. libcrun in process is the competitor.
- Interleaved A B B A: A has `HEXBENCH_PARKED_PROCS` unset, B has it at 1. Each pass ran under the timed lock on CPUs
  4-15. fedora, 2026-11-05 16:04:01Z-16:48:40Z. Load at each pass's start: 13.3, 36.6, 33.7, 40.2.
- Body: `~/.cache/hex-t7/g5tail/del-pp-20261105T160258Z`. Out dirs: `~/.cache/hex-bench/out/` 20261105T160401Z (A),
  20261105T161458Z (B), 20261105T162617Z (B), 20261105T163758Z (A).

In process (seam_oci's in-process driver), ms, each pass's `table.md`:

| pass | create p50 | create p99 | create p99.9 | delete p50 | delete p99 | delete p99.9 | delete max |
|------|-----------:|-----------:|-------------:|-----------:|-----------:|-------------:|-----------:|
| A 1 | 0.34 | 0.52 | 0.72 | 0.29 | 0.50 | 0.93 | 14.28 |
| B 1 | 0.33 | 0.50 | 0.98 | 0.34 | 0.56 | 3.91 | 24.70 |
| B 2 | 0.42 | 0.71 | 1.02 | 0.41 | 0.78 | 1.65 | 18.26 |
| A 2 | 0.34 | 0.57 | 1.16 | 0.31 | 0.54 | 1.67 | 17.38 |

Over the socket, delete p50 is 0.30, 0.34, 0.30, 0.31 and p99 is 0.54, 0.68, 0.53, 0.72, in the same order. **Correction
(2026-11-05):** the socket driver's `hex node` is a process of its own, which seam_oci starts without parked procs, so
every socket figure here is parked procs off in both arms. Only the in-process figures compare A with B.

- Every B lifecycle's create claimed its parked init with its proc: 10,099 hits of 10,099 claims a round, 0 misses,
  0 dry, 0 failed. Every A round made no proc.
- **The delete grows with B**: in process, p50 +0.03 to +0.12 ms (0.34 and 0.41 against A's 0.29 and 0.31), p99 up
  to +0.28 ms. (The socket figures are off against off; see the correction above.)
- Serially and with one masked path, the create doesn't fall with B: p50 0.33 and 0.42 against A's 0.34 and 0.34.
  Part 19's work in host.sh's bundle is small (one read-only path), so it has little to save. The mounts runs above,
  with Docker's 15 paths at 8 in flight, are where it saves.
- The four passes ran at different loads (13 to 40). The second B pass and the second A pass ran hottest, so
  compare within the pairs, not across them.
- Hex's delete stays 25-35x libcrun's at p50 in every pass (in process 34.88x, 30.39x, 25.22x, 33.44x).

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

**Gate 6 at this tree:** unit hex-g5tail-ppfull-20261105T151557Z, gates 7/7 exit 0, root lab 7 of 7, the guest lab 2 of 2, in one unit.

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
