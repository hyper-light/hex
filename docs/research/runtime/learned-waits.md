# Learned waits: twice the slowest answer, and the limits beside them

Source notes for decision 9 of `docs/design/runtime-architecture.md` (Ada's rule of 2026-11-06: no picked
constants; values learned at runtime, derived from a named record, or cited limits; and the lead's failure-bound
rule of the same day).

## The failure-bound rule (the lead, 2026-11-06, final)

Before any sample on a node, a wait's bound is 2 × the slowest answer in that wait's recorded distribution for the
host class, from a named record; after, 2 × max(the record's slowest, the node's slowest), only rising. A wait that
ran out is no sample, nor a dead or hung seed. No caller failure where an alternative exists. Where no record
exists, the wait is MEASURING with the record to come named, and no value is picked. RFC 6298's estimator (the
first draft) and calibration reads (the second) were ruled out: the RFC's floor and constants are TCP's clocks', and
a calibration read leaves the first wait unbounded.

Karn's point is from Karn and Partridge ("Improving Round-Trip Time Estimates in Reliable Transport Protocols",
SIGCOMM '87; RFC 6298 §3 states it for TCP): a sample is taken only from an answer whose wait is unambiguous.

## The records the seeds come from, checked in the files

- `bench/competitive/probes/seed_path.md`, run 3 (fedora, 2026-10-30T06:06:13Z, load 25.5 to 29.6): a seed's
  clone of an init, request to the init's first run, p50 0.234, p90 0.278, p99 1.050, max 1.918 ms, n=320. Run 2:
  request to child max 0.989 ms. Run 1's max of 15.778 ms is its first create's, timed from the seed's own start
  (the record says so), not a clone. `seeds::CLONE_SLOWEST` is run 3's 1.918 ms, the slowest clone recorded.
- `bench/competitive/probes/levers.md`, runs 1 and 2: a 400-byte record write and rename p99 1,037.7 µs on btrfs,
  893.8 on overlayfs, 42.1 on tmpfs; no fsync, no max. Not the store's durable write's distribution, so the store's
  wait is MEASURING.
- `bench/competitive/probes/input_census.md` (adacachy, 2026-11-06): sizes of `/proc/net` files and netlink dumps,
  no times. The network check's waits are MEASURING.
- No record times a box's emptying from its kill, a listing per entry, a turn's hold, a list part's drain or a
  socket call's answer by kind: each is MEASURING, its record to come named at the wait.

## Linux: pid_max, /proc/self/fd, RLIMIT_NOFILE

Documentation/admin-guide/sysctl/kernel.rst, "pid_max": the PID allocation wrap value; no task holds a pid at or
past it, so the threads a box's cgroups list are at most `pid_max` (read at runtime from
`/proc/sys/kernel/pid_max`). proc(5): `/proc/self/fd` holds one entry per open descriptor of the process.
getrlimit(2): `RLIMIT_NOFILE`, one more than the largest descriptor number a process may open.

## systemd: DefaultLimitNOFILE=

systemd-system.conf(5), "DefaultLimitNOFILE=": the default descriptor limit for services, 1024 soft and 524288 hard
since systemd v240 (the soft limit kept at 1024 for select(2)'s sake). The netns tests' "systemd's default" is
that soft half.

## Balls in bins: Dubhashi and Ranjan, "Balls and Bins: A Study in Negative Dependence"

Random Structures & Algorithms 13(2), 1998. The indicators that each bin is empty after independent throws are
negatively associated, so Chernoff-Hoeffding bounds apply to their sum. The store's shard test uses it: 256 ids
over 64 shards miss mu = 64·(63/64)^256 ≈ 1.14 shards on average, and P(misses ≥ 14) ≤ 2^-32 for a uniform hash.
