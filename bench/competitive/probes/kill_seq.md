# A cgroup `cgroup.kill` was written to kills every task later cloned into it (goal 5 extended, step 6)

`kill_seq.c` makes two cgroups under the one it's given, writes `cgroup.kill` to the second while it is empty, then
clones a child into each with `clone3(CLONE_INTO_CGROUP)` and reports whether the child lived to exit 7. Built with
`gcc -O1 -Wall`, run as the user in a delegated `systemd-run --user --scope -p Delegate=yes` on 2026-11-03:

| host | kernel | fresh cgroup | after `cgroup.kill` | after `cgroup.kill`, a second clone |
|---|---|---|---|---|
| adacachy | 7.2.9-2-cachyos | survived | killed by signal 9 | killed by signal 9 |
| fedora | 6.19.10-300.fc44.x86_64 | survived | killed by signal 9 | killed by signal 9 |

**Why** (Linux v6.19, `kernel/cgroup/cgroup.c`, read at the tag): `cgroup_css_set_fork` samples the clone's
`kill_seq` while `kargs->cgrp` is still unset, so for `CLONE_INTO_CGROUP` it takes the forking task's own cgroup's
(`:6702-6705`; `kargs->cgrp = dst_cgrp;` comes only at `:6768`). `cgroup_post_fork` compares that with the target
cgroup's own (`:6902`) and kills the child when they differ (`:6941`), and every `cgroup.kill` raises the target's
(`cgrp->kill_seq++;`, `:4186`). A cgroup once killed so differs from a forker's that never was, for good.

**What it means for Hex.** The seam's inits are cloned into their box's cgroup (decision 5's seeds), so a cgroup
`cgroup.kill` was ever written to can't be handed to another box: its init would die before its ready signal, as
fedora's OCI pass of 2026-11-03 01:0xZ saw before the guard (`the container init closed before its ready signal` at
concurrency 1, 8 and 32). The node removes such a slot with its box instead of keeping it idle, and counts it
(`Counts::discarded_killed`; `crates/hex-cli/tests/oci_reuse.rs`, test 3's killed case).
