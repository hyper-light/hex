# Goal 5's added levers: the init's clone by namespace, and a record write by filesystem

`levers.sh` runs `levers.c` as root in the OCI pass's privileged container (`hexbench/pass:44`, `--cgroupns=private`,
a tmpfs `/run`), on fedora. It times, n=2000 each:
- clone3's parent-side call with each namespace flag alone and all together, with and without `CLONE_INTO_CGROUP`;
- the child's exit and reap after it;
- a child's setns into an ipc and a uts namespace made ahead by a holder that unshared them;
- a record-sized write (a 400-byte temp file written, closed and renamed into place) on the container's tmpfs `/run`,
  its overlay root, and fedora's btrfs.

Not of record: no load gate, the load and the host's thread count printed with each run. The goal-5 extension plan
(`01a190c51f07757ab54b09f1aada633c`) rests its steps on these as findings f88-f93 of research
`01a190c51f137042a9697949c8fe2c7a`.

## Run 1 (2026-10-31T19:12:53Z, fedora, Linux 6.19.10, host at 2,850 threads, load 15.68 24.58 45.78)

levers.c sha256 a9bc5fd101b8b3e0 (the clone and write modes).

| | p50 (us) | p90 | p99 |
|---|---|---|---|
| clone, no new namespace | 44.9 | 77.0 | 140.8 |
| clone, CLONE_NEWNS | 59.1 | 96.7 | 193.3 |
| clone, CLONE_NEWPID | 50.7 | 85.9 | 195.5 |
| clone, CLONE_NEWIPC | 75.3 | 122.9 | 289.1 |
| clone, CLONE_NEWUTS | 52.5 | 83.5 | 182.1 |
| clone, all four | 100.5 | 152.5 | 369.9 |
| clone, CLONE_INTO_CGROUP | 48.9 | 80.5 | 211.6 |
| clone, all four into a cgroup | 100.3 | 152.9 | 287.3 |
| exit and reap, no new namespace | 75.0 | 313.0 | 758.6 |
| exit and reap, CLONE_NEWNS | 145.0 | 427.5 | 915.4 |
| exit and reap, all four | 163.8 | 526.2 | 1156.4 |
| a record write on tmpfs | 6.7 | 7.5 | 10.8 |
| a record write on overlayfs | 49.6 | 83.3 | 403.9 |
| a record write on btrfs | 52.4 | 72.0 | 182.6 |

## Run 2 (2026-10-31T19:14:03Z, fedora, host at 2,758 threads, load 36.95 29.15 45.83)

levers.c sha256 ecb69f3cbc89a125 (with the setns mode). Busier: no core was idle when it ended.

| | p50 (us) | p90 | p99 |
|---|---|---|---|
| clone, no new namespace | 38.4 | 66.7 | 134.7 |
| clone, CLONE_NEWIPC | 73.7 | 97.0 | 207.6 |
| clone, CLONE_NEWUTS | 46.0 | 77.1 | 157.7 |
| clone, all four | 86.2 | 140.1 | 277.3 |
| setns into an ipc namespace made ahead | 3.9 | 6.9 | 12.7 |
| setns into a uts namespace made ahead | 2.2 | 3.7 | 5.7 |
| a record write on tmpfs | 5.6 | 8.5 | 42.1 |
| a record write on overlayfs | 51.0 | 115.5 | 893.8 |
| a record write on btrfs | 48.9 | 84.1 | 1037.7 |

## What they find

- **An IPC namespace is the costliest a clone makes**, 30-35 us of it, because making one makes an internal mqueue
  mount (Linux v6.19 `ipc/namespace.c:73`, `ipc/mqueue.c:1637`). UTS adds 7-8 us. Joining both made ahead by setns
  costs 6 us, so an init that joins an ipc and a uts namespace made ahead off create's path saves about 35-40 us of
  its clone.
- **A record write is about 50 us on disk or the overlay and 6 us on tmpfs**, so a record whose box's directories all
  lie on tmpfs, kept on tmpfs, saves about 43 us a write.
- **A mount namespace costs most at its end**: the child's exit and reap rose from 75 to 145 us with one, a delete's
  cost rather than create's.
