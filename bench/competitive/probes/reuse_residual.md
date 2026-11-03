# What an idle cgroup still has charged (goal 5 extended, step 6)

`RESIDUAL` (`crates/hex-node/src/cgroups.rs`) is the most memory an idle cgroup may still have charged and be handed
to its owner's next box (`hex_core::reuse::check`, research f27-f30). These are the readings it rests on. None is a
timed figure: each reads `memory.current` and `memory.stat` of a cgroup whose box has been deleted, its processes all
gone.

## What a box leaves

- **A box that only ran processes, all ended** (`crates/hex-cli/tests/oci_reuse.rs`, test 2: box A burned CPU through
  `sha256sum`, held 64 MiB of anonymous memory, had a program OOM-killed and forks refused, then was deleted; its idle
  cgroup read before box B was given it). adacachy, Linux 7.2.9-2-cachyos, rootless, 2026-11-03:

  | run | `memory.current` | anon | file | kernel | slab | percpu | shmem | file_dirty |
  |---|---|---|---|---|---|---|---|---|
  | 1 | 5,775,360 | 0 | 0 | 2,682,880 | 418,616 | 544 | 0 | 0 |
  | 2 | 7,471,104 | 0 | 0 | 3,067,904 | 434,728 | 3,264 | 0 | 0 |
  | 3 | 5,480,448 | 0 | 413,696 | 2,666,496 | 435,984 | 1,632 | 0 | 0 |
  | 4 | 3,719,168 | 0 | 0 | 749,568 | 235,608 | 544 | 0 | 0 |
  | 5 | 6,262,784 | 0 | 0 | 3,031,040 | 435,280 | 544 | 0 | 0 |
  | 6 | 5,087,232 | 0 | 270,336 | 2,580,480 | 422,104 | 1,088 | 0 | 0 |

  The same test on fedora, Linux 6.19.10-300.fc44, as root in the step's root lab (the fedora job of
  2026-11-03T01:16:09Z, its `rootlab.log`): 5,054,464 charged, anon 0, file 0, kernel 3,047,424, slab 503,008,
  percpu 3,264, shmem 0, file_dirty 0.

  Nothing the box held is left: anon, shmem and dirty file pages are 0. What `memory.stat` doesn't name, about half of
  `memory.current`, is most likely charge the kernel caches per CPU and drains lazily (inferred from research f30: a
  `memory.max` write, which drains those caches first, took an emptied box from 544,768 bytes to 49,152); the rest is
  kernel memory, slab among it.
- **A box that left a file in memory** (test 3's charged guard: A wrote a 64 MiB file into its root, then was deleted):
  its idle cgroup read 60 MiB or more charged, the file's page cache, which the next box would be charged for. The
  check refuses it (`Refused::Charged`) and the cgroup is removed.
- **The bench's lifecycle** (create, start and delete of a `true` container, one owner's boxes): on fedora (Linux
  6.19.10, `create_serial.md`'s run 7, `seam_oci`'s counts) 519 of 520 boxes at concurrency 1 and 4,968 of 5,000 at
  concurrency 32 were handed an idle cgroup, the rest given fresh ones; in the OCI pass (`reuse_pass.md`, run 1) 9,999
  and 10,000 of each sustained phase's 10,000. No idle cgroup was refused for what it had charged.
- **What the bench's idle cgroups held** (the lead, 2026-11-02T18:23Z: measure the bench's real lifecycle residual
  on fedora). In the OCI pass `~/.cache/hex-t7/g5x-s6/hex-bench/out/20261103T044133Z` (`reuse_pass.md`'s run 4,
  CPUs 4-15, at 610bba9), `seam_oci` read `memory.current` of every idle slot at the end of each phase (its
  `idle_residual_bytes`): 32 per phase, the throughput and sustained phases, each driver, both rounds.

  | driver, round | throughput: min, p50, max | sustained: min, p50, max |
  |---|---|---|
  | in process, 1 | 0, 86,016, 864,256 | 53,248, 337,920, 864,256 |
  | in process, 2 | 0, 219,136, 864,256 | 45,056, 294,912, 1,073,152 |
  | over the socket, 1 | 0, 139,264, 1,314,816 | 73,728, 311,296, 1,089,536 |
  | over the socket, 2 | 0, 75,776, 1,748,992 | 53,248, 370,688, 1,171,456 |

  Over all 256: 0 at least, 167,936 bytes at the median, 716,800 at the 90th percentile and 1,748,992 (1.7 MiB) at
  most.

## The threshold

16 MiB, above every clean reading and under every box that left files:
- the bench's lifecycle leaves at most 1,748,992 bytes, 1.7 MiB, so the threshold is 9.6 times that (run
  20261103T044133Z, above);
- test 2's heavier box, which burned CPU, held 64 MiB and had a program OOM-killed, leaves at most 7,471,104 bytes,
  7.1 MiB, and at least 3,719,168, 3.5 MiB, none of it the box's own memory, so the threshold is 2.2 times its most;
- a box that left files in memory leaves 64 MiB in test 3 and 85 MB in research f27's probe, four times the threshold.

A threshold set from the bench's readings alone, twice its most say, would refuse test 2's clean box. The check also
requires the threshold under the next box's `memory.max`, so a box bounded at 16 MiB or less always gets a fresh
cgroup.

What would show it wrong: boxes whose processes all ended refused as `Charged` (the reuse rate falling for nothing the
box held), or a box handed a cgroup whose charge it then can't fit under its bound.
