# Learned waits: twice the slowest answer, and the limits beside them

Source notes for decision 9 of `docs/design/runtime-architecture.md` (Ada's rule of 2026-11-06: no picked
constants; values learned at runtime, derived from a named record, or cited limits; and the lead's failure-bound
rule of the same day).

## The failure-bound rule (the lead, 2026-11-06)

A wait lasts twice the slowest answer recorded on this node, a running maximum; a node with no samples makes one
calibration read at start before arming the bound, as goal 5's disk probe does; no picked floor or initial value.
A timeout then needs an answer more than twice slower than any seen. RFC 6298's estimator was the first draft's
basis; the lead ruled its floor (1 s) and its constants (K = 4, alpha 1/8, beta 1/4) were chosen for TCP's clocks
and paths and don't carry over, so it is not cited as a basis.

Karn's point is kept from Karn and Partridge ("Improving Round-Trip Time Estimates in Reliable Transport
Protocols", SIGCOMM '87; RFC 6298 §3 states it for TCP): a sample is taken only from an answer whose wait is
unambiguous. Here: no sample from a wait that ran out, nor from a seed that died or was taken for hung.

## What Hex's own records say about the spread a running maximum must cover

`bench/competitive/probes/seed_path.md`, run 2 (fedora, concurrency 1): a seed's clone of an init p50 0.209 ms,
p90 0.262, p99 0.856, max 0.989 ms; run 1: max 15.778 ms against a p50 of 0.256. A bound calibrated on one p50-like
read (2 × 0.209 = 0.418 ms) sits under that run's p99; it is the slowest answer seen that sets the bound, so the
calibration read's own place in the distribution decides how soon the bound is wide enough.

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
