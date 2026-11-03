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

  Nothing the box held is left: anon, shmem and dirty file pages are 0. What `memory.stat` doesn't name, about half of
  `memory.current`, is most likely charge the kernel caches per CPU and drains lazily (inferred from research f30: a
  `memory.max` write, which drains those caches first, took an emptied box from 544,768 bytes to 49,152); the rest is
  kernel memory, slab among it.
- **A box that left a file in memory** (test 3's charged guard: A wrote a 64 MiB file into its root, then was deleted):
  its idle cgroup read 60 MiB or more charged, the file's page cache, which the next box would be charged for. The
  check refuses it (`Refused::Charged`) and the cgroup is removed.
- **The bench's lifecycle** (create, start and delete of a `true` container, one owner's boxes): on fedora (Linux
  6.19.10, run `create-serial-locks-20261103T005157Z`, `seam_oci`'s counts) 519 of 520 boxes at concurrency 1 and 4,968
  of 5,000 at concurrency 32 were handed an idle cgroup, the rest given fresh ones, and no idle cgroup was refused for
  what it had charged.

## The threshold

16 MiB: above every reading of a box whose processes all ended (7.5 MiB at most, of which none is the box's own memory),
and under what a box that left files in memory leaves (64 MiB in test 3; 85 MB in research f27's probe). The check also
requires the threshold under the next box's `memory.max`, so a box bounded at 16 MiB or less always gets a fresh cgroup.

What would show it wrong: boxes whose processes all ended refused as `Charged` (the reuse rate falling for nothing the
box held), or a box handed a cgroup whose charge it then can't fit under its bound.
