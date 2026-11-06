# The census of the inputs Hex bounds as hostile

Ada's rule of 2026-11-06: no picked constants. The lead's call the same day: a bound on hostile input is the largest
legitimate input in a recorded census, times a stated margin of 2. This records the census `input_census.py` takes,
read-only, of each input such a bound holds:

- `cgroup`: the files `crates/hex-runtime/src/linux/cgroup.rs` reads whole through `MAX_CGROUP_FILE`, and
  `memory.stat`, the largest of each over every cgroup on the host. Hex reads no `memory.stat`; a box's
  `cgroup.procs` grows with its processes and has its own bound, derived (`MAX_PROCS_FILE`: `PID_MAX_LIMIT` pids
  of seven digits and a newline), so the census of it is no bound's.
- `net`: the `/proc/net` files `crates/hex-runtime/src/linux/oci/netcheck.rs` reads through `MAX_PROC_FILE`, and
  each of its five dumps answered whole, read in reads of 8 KiB as the check reads them, in the host's namespace
  (busy) and in a fresh one (`unshare -rn`, loopback up, as the pool makes them).

Run: `bash bench/competitive/probes/input_census.sh bench/competitive/probes/input_census.py <log>` (the probe in the host's namespace, then in a
fresh one).

## adacachy, 2026-11-06 16:10Z (not fedora: fedora didn't answer kiln from about 14:40Z)

Linux 7.2.9-2-cachyos, load average 39-43, 191 sessions, 653 cgroups, `pid_max` 4,194,304. Run by hand, not a
hex-slot unit (adacachy has none); log `~/.cache/hex-t7/g5rec/census-adacachy.log`.

Largest cgroup file, by name (bytes, and where):

| file | bytes | where |
| --- | ---: | --- |
| cgroup.events | 21 | sys-fs-fuse-connections.mount |
| cgroup.freeze | 2 | sys-fs-fuse-connections.mount |
| cgroup.max.descendants | 4 | / |
| cgroup.procs | 2,731 | / |
| cgroup.stat | 485 | / |
| cgroup.subtree_control | 49 | / |
| cgroup.type | 7 | sys-fs-fuse-connections.mount |
| cpu.max | 15 | cpu90 |
| cpu.stat | 196 | / |
| io.stat | 551 | / |
| memory.current | 12 | user.slice |
| memory.events | 88 | user.slice |
| memory.oom.group | 2 | sys-fs-fuse-connections.mount |
| memory.peak | 13 | user.slice |
| memory.stat | 1,498 | / |
| pids.current | 5 | user.slice |
| pids.events | 6 | sys-fs-fuse-connections.mount |

`/proc/net` files (bytes; `ip_tables_names` and `ip6_tables_names` absent in both):

| file | host | fresh |
| --- | ---: | ---: |
| dev | 3,501 | 323 |
| protocols | 2,647 | 2,647 |
| sockstat | 123 | 114 |

Dumps (bytes in all / reads / largest read):

| dump | host | fresh |
| --- | --- | --- |
| addresses, IPv4 | 1,604 / 2 / 1,584 | 96 / 2 / 76 |
| addresses, IPv6 | 1,420 / 1 / 1,420 | 100 / 1 / 100 |
| routes, IPv4 | 3,244 / 2 / 3,224 | 200 / 2 / 180 |
| routes, IPv6 | 6,072 / 3 / 3,732 | 136 / 2 / 116 |
| rules | 1,000 / 1 / 1,000 | 424 / 1 / 424 |

## fedora

Waits on fedora (unit `hex-g5rec-cB-census-20261106T144753Z` queued under hex-slot when it stopped answering). The
bounds are derived once its census, at the bench's load, is here: each the larger host's largest, times 2.
