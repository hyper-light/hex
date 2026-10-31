# What a native macOS box can bound with public interfaces: measured

`box_probe.c` on adas-mac-studio-3 (Apple M4 Max, 14 cores, 36 GiB, macOS 26.2 25C56, Apple clang 21.0.0, `-O2`),
2026-10-31. These are the native-platforms research's §7 measurements (`docs/research/runtime/native-platforms.md`).
Records (each three passes of every row):

- `records/box-probe-374bdbf-20261031T233823Z.txt`: commit 374bdbf, kiln run r30, 23:38:23-23:38:49Z, load 1.86-2.46.
- `records/box-probe-9694770-rt-20261031T233931Z.txt`: commit 9694770, the watcher real-time, 23:39:31-23:39:45Z,
  load 3.74-4.01.

The bound is 256 MiB. Each row is 20 runs, and the figure given is the worst of three passes.

## A box is listed and read cheaply

| Box | read p50 | read p99 | per process |
|---|---|---|---|
| 1 process | 5.4-5.5 µs | 5.7 µs | 5.4 µs |
| 16 | 10.3-10.4 µs | 10.7-12.3 µs | 0.65 µs |
| 128 | 49.3-49.7 µs | 53.8-54.3 µs | 0.39 µs |

A read is the box's process group listed (`proc_listpids`) and each member's footprint read (`proc_pid_rusage`). A
same-user child reads at every flavor (`check`).

## A member that daemonizes leaves its group and parent, not its user

`escape`, every pass: `ppid=1 in_box_pgrp=0 child_of_leader=0 same_uid_listed=1`.

## A plain sleep is no timer to hold a bound with

One member allocating at full speed (23-25 GiB/s here: the probe prints MiB/s), polled every T:

| T | `usleep` overshoot p50 / max | kqueue `NOTE_CRITICAL` p50 / max |
|---|---|---|
| 1 ms | 164-186 / 187-204 MiB | 9.7-22.5 / 17.8-25.5 MiB |
| 10 ms | 689-1,137 / 1,540-2,007 MiB | 15.2-244 / 226-251 MiB |

`usleep` ran about six times long, at every QoS the probe tried. A one-shot kqueue timer marked `NOTE_CRITICAL`
(`sys/event.h`: "system does minimal timer coalescing") ran as asked.

## The bound held: poll at the headroom's pace, from a real-time watcher

Polled every (bound - footprint) / 256 GiB/s, at least 250 µs, on `NOTE_CRITICAL` timers, with the members at utility
QoS. The rate is above the 164 GiB/s a box's members together reached here.

| Members | box allocates | watcher at user-interactive QoS: overshoot max | watcher real-time: overshoot max |
|---|---|---|---|
| 1 | 24-26 GiB/s | 6.5-7.5 MiB | 6.6-7.4 MiB |
| 4 | 83-97 GiB/s | 21.4-52.4 MiB | 10.9-28.3 MiB |
| 8 | 146-153 GiB/s | 36.2-50.3 MiB | 30.7-41.5 MiB |
| 14 (every core) | 150-161 GiB/s | 334-482 MiB | 26.0-36.9 MiB |
| 28 (twice the cores) | 162-164 GiB/s (real-time record) | 846-1,458 MiB | 34.1-41.5 MiB |

The real-time watcher is a Mach time-constraint thread (`THREAD_TIME_CONSTRAINT_POLICY`: 250 µs period, 50 µs
computation). Once a box's members fill every core, a watcher at user-interactive QoS reads late; a real-time one
doesn't.

## What it costs

A box held at 95% of its bound (`hold`, real-time watcher):

| Floor | reads/s | the watcher's CPU |
|---|---|---|
| 250 µs | 3,618-3,639 | 4.7-5.5 ms/s (≤ 0.55% of a core) |
| 1 ms | 973-974 | 2.0-2.4 ms/s (≤ 0.24%) |

A box far from its bound is read every 100 ms (the ceiling), and costs next to nothing.

## What this says

- On this machine, a native macOS box holds a 256 MiB memory bound to within 42 MiB, against members allocating up to
  164 GiB/s, twice as many as the cores included. The overshoot is the box's rate times the floor, plus a read.
- The rate (256 GiB/s) is a tuning constant measured here. Another machine needs its own measurement (CLAUDE.md §5).
- This isn't a kernel cap. A box can pass its bound by the overshoot before it ends. On Linux and Windows the kernel
  refuses the allocation itself.
