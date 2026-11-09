# run_clone: what a process box's run costs in cgroup_mutex (runtime design goal 7)

Goal 5 extended's step 6 found that a clone into a cgroup takes `cgroup_mutex`, the one lock for the whole host.
`CLONE_INTO_CGROUP` takes it in `cgroup_css_set_fork` and holds it to `cgroup_css_set_put_fork`, about 0.024 ms of
an oci lifecycle's wall at concurrency 32 (`create_serial.md`, run 23). Goal 7's run clones every program into
its box's cgroup the same way. This probe measures what that costs a run of `true`, at concurrency 1 and 32.

**How.** `run_clone.sh` runs `run_clone_inside.sh` in a privileged fedora:44 container, as `create_serial.sh` does,
since bpftrace isn't on the host. One bpftrace program watches seam_box's runs of `true` through `Runtime::run`:
its latency phase for concurrency 1, and its throughput phase at 32 alone for concurrency 32. The runtime's threads
are `seam_box` and `hex-supervisor`. For them it times:

- each clone from `cgroup_can_fork` to `cgroup_post_fork`'s return, by whether it clones into a cgroup;
- each `cgroup_mkdir`, `cgroup_rmdir` and `cgroup_kill_write`, timed whole;
- each wait for `cgroup_mutex` alone, from `lock:contention_begin` to `contention_end`, the runtime's and
  everyone else's.

`run_clone.py` divides each by the runs, the clones into a cgroup, warm-up included.

## Run 1 (2026-11-04, fedora): b82c7ff

- **Run:** `~/.cache/hex-t7/g7/bench/out/run-clone-20261104T091838Z`, from 09:18:38Z.
- **Binary:** seam_box b82c7ff (sha256 9ccb4f07884a6868).
- **Host:** Intel Core i9-14900HX, Linux 6.19.10-300.fc44, the container on CPUs 4-15 (8g, oom-score-adj 1000),
  under the timed lock.
- **Load:** 27.33 at the start and 26.49 at the end, with about 25 other containers beside it (`env.txt`).
- **bpftrace:** v0.24.2.

Per run of `true`, µs:

| | concurrency 1 (2,020 runs) | concurrency 32 (20,000 runs, 2,310/s) |
|---|---|---|
| clone into the cgroup (the lock's wait and hold) | 51.9 | 262.7 |
| `cgroup_mkdir`, whole | 154.3 | 217.3 |
| `cgroup_rmdir`, whole | 52.0 | 100.7 |
| `cgroup_kill_write`, whole | 1.4 (0.08 a run, 17.6 each) | 1.9 (0.01 a run, 254.9 each) |
| the runtime's waits for the lock, within those | 139.8 (0.53 a run, 263.9 each) | 434.5 (1.67 a run, 259.4 each) |
| everyone else's waits for the lock, over the same time | 437.4 (3.43 a run) | 2,758.3 (10.30 a run) |

**What it shows:**

- **The lock is held at most what the takers take less their waits.** A run takes the lock three times: its
  cgroup's `mkdir`, its clone into it, and its `rmdir`. Those three take 258.2 µs at concurrency 1, of which
  139.8 are waits, so the lock is held at most about 118 µs a run. At 32 they take 580.7 µs, of which 434.5 are
  waits, so about 146 µs.
- **The clone is a small share of a run at 32.** Its span, wait included, is 0.263 ms of a run that takes
  32 / 2,310 s = 13.9 ms of a thread's wall, under 2%.
- **This host's waits are mostly other people's.** Everyone else waited on `cgroup_mutex` 3.4 times a run at
  concurrency 1 and 10.3 at 32, against the runtime's 0.53 and 1.67. That is the host's other cgroup work at load
  27: kiln's containers, other agents' runs.
- **A few runs end with `cgroup.kill`.** 8% at concurrency 1 and 0.7% at 32. In those, the run's removal still
  read its box populated once the program's pidfd said it had ended. On Linux 6.19 a task leaves its cgroup at its
  last switch, after its pidfd reads ended (decision 5's delete, `kernel/sched/core.c:5156`). So the removal ends
  whatever might be left, with the kill and its lock take, rather than wait on a count it can't tell from a stray.
