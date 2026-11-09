# A box's `/dev` made fresh against handed on within one owner (goal 5 extended, step 6)

The lead's lever for step 3's `hex-devfs` worker (2026-11-03T17:47Z): keep a detached original of an owner's `/dev`,
give each of the owner's boxes a clone, and check and reset the original after the box, in place of making a fresh
tmpfs for every box. `docs/research/runtime/goal5-mounts.md` §9 says what Linux v6.19 allows and what a check must cover;
this measures what each costs.

`devfs_clone.c` runs both on one pinned thread, as root, each iteration timing its steps:
- **fresh**, as `init.rs`'s `prepare_dev` makes it and a box's teardown ends it: `fsopen("tmpfs")`, `fsconfig` of runc's
  `/dev` options (`mode=755`, `size=65536k`), `FSCONFIG_CMD_CREATE`, `fsmount` (nosuid, strictatime); the six default
  nodes (`mknodat`, then `fchmodat` to their mode); the five default links (`symlinkat`); the three mountpoints a box's
  spec makes in it (`pts`, `shm`, `mqueue`); then `close`, the mount's last reference, which tears its superblock down;
- **handed on**: `open_tree(original, OPEN_TREE_CLONE)`, the three mountpoints in the clone, `close` of the clone; then
  the check through the original: `getdents64` of the root, `statx` and `listxattrat` (Linux 6.13, so no node is
  opened) of every entry and of the root, `readlinkat` of each link, and the mountpoints removed
  (`unlinkat(AT_REMOVEDIR)`).

Built with `gcc -O2 -Wall -Wextra` (GCC 16.2.1; source sha256 0b80bf19bec19477, binary 08cc8534318e43ef) and run on
fedora, 6.19.10-300.fc44.x86_64, on 2026-11-03 at 18:52Z, in a privileged `fedora:44` container pinned to CPU 20 (an
E-core), 20,000 iterations each way: load 4.7, CPU pressure 0.15%, 63 other containers on the host, quiet.

| step | p50 us | p99 us | mean us |
|---|---|---|---|
| fresh: `fsopen` to `fsmount` | 13.4 | 24.1 | 15.2 |
| fresh: six nodes | 21.4 | 35.2 | 24.2 |
| fresh: five links | 11.8 | 23.6 | 13.8 |
| fresh: three mountpoints | 6.7 | 15.5 | 7.5 |
| fresh: close, the superblock's teardown | 18.5 | 161.1 | 27.2 |
| **fresh, whole** | **75.3** | **400.1** | **87.8** |
| handed on: `open_tree` clone | 2.7 | 6.7 | 3.1 |
| handed on: three mountpoints | 8.6 | 18.3 | 9.8 |
| handed on: close of the clone | 10.8 | 155.2 | 17.5 |
| handed on: the check | 23.0 | 33.2 | 25.4 |
| handed on: mountpoints removed | 6.0 | 14.0 | 7.0 |
| **handed on, whole** | **53.0** | **282.1** | **62.8** |

**What it means for Hex.**
- **The hand-on saves 22 us p50 a box, 25 us mean.** The check costs what the six nodes do, and the clone's close a
  little less than the superblock's teardown. That's about a quarter of the 97 to 101 us a lifecycle that run 17
  sampled on the worker's thread (`create_serial.md`).
- **It adds one exclusive hold of the host's mount lock a box.** The clone takes `namespace_sem` to write
  (`fs/namespace.c:3017`) and `fsmount` takes none (`:4269-4380`). The 2.7 us here is the lock uncontended. At concurrency
  32 it waits with the inits' attaches, where run 21 already has 0.206 ms a lifecycle waited.
- **The six nodes' modes are a cheaper cut on the fresh path:** each node's `fchmodat` is there because the agent's umask
  can't change for one thread without its own `fs_struct`.

**The pool's thread in a lifecycle** (2026-11-03T19:31Z, the unit `hex-g5x-s6-verify19-20261103T193123Z`): step 6's
`create_serial.sh` MODE=devfs (58720c4) on the merge of steps 2, 3 and 6 (mr12h, `hex` ff5cef383a844f85), one owner's
boxes, on the E-cores 16-31, load 2.6 to 6.7; run `create-serial-devfs-20261103T193124Z`. Per `/dev` the thread made:

| | concurrency 1 (5,027 made) | concurrency 32 (14,492 made) |
|---|---|---|
| on-CPU | 124.6 us | 77.0 us |
| futex calls (its waits and wakes) | 1.00 | 0.04 |
| `mknodat` x6, on-CPU | 36.8 us | 23.7 us |
| `symlinkat` x5 | 25.1 us | 17.4 us |
| `fsconfig` x4 (the superblock made at `FSCONFIG_CMD_CREATE`) | 22.1 us | 16.0 us |
| `fchmodat` x6 | 9.5 us | 6.4 us |
| `fsmount`, `fsopen` | 5.4, 4.9 us | 3.2, 1.8 us |

At concurrency 1 the pool's thread is woken by every take and makes one `/dev`, then waits again; at 32 it is never
idle and makes them back to back.

**The low mark's A B B A** (2026-11-03, 20:12Z to 20:15Z, the unit `hex-g5x-s6-verify21-20261103T201127Z`; the same mode
and CPUs, one-minute load 7.5 to 13.9): mr12h against mr13, which is mr12h with the pool's thread woken only at a key's
low mark and filling the key in one pass (`hex` 252eea5c30632778).

| run | binary | concurrency 1: on-CPU, futex calls a `/dev` | concurrency 32: on-CPU, futex calls a `/dev` |
|---|---|---|---|
| `create-serial-devfs-20261103T201214Z` | mr12h | 79.8 us, 1.00 | 78.2 us, 0.04 |
| `create-serial-devfs-20261103T201258Z` | mr13 | 73.0 us, 0.25 | 81.0 us, 0.03 |
| `create-serial-devfs-20261103T201334Z` | mr13 | 67.4 us, 0.25 | 79.7 us, 0.03 |
| `create-serial-devfs-20261103T201424Z` | mr12h | 74.9 us, 1.00 | 78.3 us, 0.04 |

The low mark takes concurrency 1's futex calls to a quarter, as it is built to, and its on-CPU from 77 us to 70 a
`/dev`; concurrency 32's doesn't move. **The 124.6 us above didn't repeat:** mr12h here is 75 to 80 us at concurrency 1,
so that figure was that run's alone, and what this record said of it (that the 48 us between the two columns is the
cost of a wake and a cold make a take) is withdrawn. mr13's mark was fixed, half a key's eight; the tree's is learned,
each key's own, from the takes that came during its fill passes (`cp/t7-port-g7`, 2026-11-09), and not measured here.

**What would falsify it.** The steps' order of size changing on another kernel or at another load: a check that costs
more than the nodes and links it replaces, or a clone whose hold of `namespace_sem` costs concurrency 32 more than the
CPU it saves beside concurrency 1.
