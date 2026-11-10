# What a reused cgroup's bound writes cost, and a read of cpu.max (goal 5 extended, step 6)

On reuse every bound is written again but `cpu.max`, which is read first and written only when its values differ
(`hex_runtime::linux::cgroup::BoxRoot::rebound`, the lead's ruling of 2026-11-03T01:19Z). Its write takes the host-wide
`cfs_constraints_mutex` and walks every task group on the host (Linux v6.19 `tg_set_cfs_bandwidth`,
`kernel/sched/core.c:9461-9462`; `__cfs_schedulable`, `:9624`), while its read takes no lock (`cpu_max_show`,
`:10104`). `cpu_max_write.sh` times 2,000 open-write-close cycles of each file and 2,000 reads of `cpu.max`, in a cgroup
of its own.

## Run 1 (2026-11-03, fedora)

- **Host and load:** kiln-a094a12d, the host fedora was named after its reset at 02:59Z (Intel Core i9-14900HX, 32
  threads), Linux 6.19.10-300.fc44.x86_64, 2026-11-03T03:33:46Z, pinned to CPUs 4-15. The 1-minute load was 169.52 at
  the start, from other tenants' builds, with 279 cgroups on the host. Up beside it: issue556oss-control-plane, two
  buildx_buildkit_kiln-w08 builders, buildx_buildkit_kagent-builder-v0.23.00, kind-registry and kiln-t10-minio.
- **Run:** `~/.cache/hex-t7/g5x-s6/hex-bench/out/cpu-max-write-20261103T033346Z`, the harness at 610bba9; the fedora
  unit `hex-g5x-s6-verify3b-20261103T030643Z`.

| 2,000 each, microseconds | p50 | p90 | p99 | max |
|---|---|---|---|---|
| `cpu.max` write, `max 100000` | 6.6 | 9.5 | 18.7 | 84.6 |
| `cpu.max` read | 2.8 | 2.9 | 4.2 | 9.5 |
| `cpu.weight` write, `100` | 2.6 | 2.7 | 4.3 | 28.3 |
| `pids.max` write, `max` | 2.5 | 4.4 | 5.3 | 18.8 |

- **A `cpu.max` write costs 2.5 times the others' here**, 6.6 against 2.5 to 2.6 us p50, and a read 2.8. When the
  values already hold, the read stands in for the write, 3.8 us less a reuse on this host; when they differ, the
  read adds 2.8.
- **What it costs grows with the host's task groups.** An earlier reading on adacachy, not of record (the same loop
  by hand, 2026-11-03, 3,348 cgroups on the host), took 68.5 us p50 for the write against 1.5 for `pids.max`. A host
  with many cgroups pays for every reused box's write, which is what the skip spares.
