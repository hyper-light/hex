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
