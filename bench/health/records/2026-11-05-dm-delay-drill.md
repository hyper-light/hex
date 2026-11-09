# AC-T7.10.3 dm-delay drill, 2026-11-05T12:27:46Z

- Commit: db90a60 (dirty files: 5: the nemesis's detector-off mutant, committed after as its own
  commit, and the run's scratch scripts; hex-node's disk probe and `disk_drill` are db90a60's)
- Host: fedora, 6.19.10-300.fc44.x86_64; Intel(R) Core(TM) i9-14900HX
- Run: `bench/health/dm_delay_drill.sh` in fedora unit `hex-sched-g5-probe-drill1-20261105T122307Z`
  (CPUs 16-31 for the build; the drill's container on CPUs 4-15 under `fedora-timed.lock`)
- Load: load average 23.53, 21.42, 23.93 (noisy, as run)
- Co-tenants: 17 kiln CI burst jobs, issue556oss-control-plane, 3 buildx builders, kind-registry,
  kiln-t10-minio
- Device: dm-delay over a 512 MiB loop file, ext4; stall: every I/O delayed 60 s
- Work: 8 units, a 4 KiB write and fdatasync each 50 ms, each write due in 10,000 ms
- Probe: a 4 KiB record written and fdatasync'd every 100 ms; RFC 6298 timeout (1,000 ms at the
  stall, its floor); `Suspect` past one timeout, `Drain` past three (F3)

```
drill probe=on units=8 deadline_ms=10000 period_ms=100 timeout_at_stall_ms=1000 marked_ms=1509 drained_ms=3502 moved=8 writes=3216 failed=0
drill probe=off units=8 deadline_ms=10000 period_ms=100 timeout_at_stall_ms=0 marked_ms=none drained_ms=none moved=0 writes=816 failed=8
```

Probe on: marked 1,509 ms and drained 3,502 ms after the stall, all 8 units moved, failed=0
(deadline 10,000 ms). Probe off: failed=8 (non-vacuity: the same stall fails every unit).
