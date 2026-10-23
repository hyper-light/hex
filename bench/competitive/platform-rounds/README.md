# Platform rounds (fedora, 2026-10-23)

The scripts that ran the sandbox platforms' rounds on fedora, byte for byte as they ran, written by the
fedora worker the T7 agent directed. They hard-code that run's paths (`~/.cache/hex-bench/out/20261023T025023Z/`,
`~/.cache/hex-bench/t7/`) and the pid of the kiln agent host the worker ran under. The run's `platforms.txt`
records every gate, try and verdict, and which version of each script and of `platform_bench.py` ran
which try. When a script changed mid-run, git history here holds each version.

## Why a referee

fedora is shared: during the platform passes other people's work ran there. That included kiln reach
runs' builds and test binaries, and the calcom PR-monitor units' `tsc`, at up to 6 CPUs. A start gate
alone can't keep a 30-minute round clean, so every round is sampled while it runs, and judged on what
the samples saw.

- **`platform_gate.sh`**: the start gate. It has two conditions:
  - (a) No other benchmark unit is active, and no process outside the platform's stack runs at nice below
    10 over 50% CPU in a 5-second `top` sample. The stack is:
    - the labelled containers;
    - the host's dockerd and containerd;
    - any `STACK_EXTRA_CG` cgroups (`/e2b` for E2B);
    - from E2B on, the round's own bench unit.

    Processes at nice 10 or above are listed but don't block. Daytona's rounds ran with the third
    revision of the script, in which the bench unit could count as foreign. Their samples never caught
    it, because the Daytona client mostly waits on HTTP.
  - (b) The 1-minute load is under 2 plus the stack's own idle load, read after 5 idle minutes. CLAUDE.md
    §7's load bar holds for the bench; a platform's idle daemons are part of what it costs.

  With (a) holding and (b) not, it runs anyway after 45 minutes and says so.
- **`round_sampler.sh`**: while the round's unit runs, a 5-second `top` sample about every 10 s. A sample
  is *foreign* if it holds a process outside the stack at nice below 10 over 50% CPU. The kiln agent host is
  tallied apart: it is the worker's own parent, and its CPU is not in the stack's cgroups.
- **`run_refereed.sh`**: one round. Each try passes the gate, then runs the bench command as its own
  systemd user unit, with `STACK_CGROUPS` from `stack_cgroups.sh`, sampled throughout. The verdict:
  - A try counts if under 10% of its samples are foreign. Otherwise the next try runs, up to three.
  - If none of the three is clean, the try with the lowest foreign share is kept, labelled with that share.
  - The share goes into the round's JSON as `referee`, and `summarize.py` prints it under the table.
  - A try during which a stack container restarted is void, since its cgroup counters reset.
  - A try whose density phase found a sandbox's own processes outside `STACK_CGROUPS` stops the referee,
    so the list can be fixed.
- **`stack_cgroups.sh <platform>`**: `STACK_CGROUPS` for a platform. It lists:
  - the scopes of the stack's running containers;
  - `system.slice/docker.service` and `system.slice/containerd.service`;
  - `e2b` for E2B, whose orchestrator puts each sandbox's Firecracker in `/sys/fs/cgroup/e2b/<sandbox>`;
  - `hexbench.slice` for Docker, which holds the Docker adapter's sandboxes.
- **`daytona_up.sh`, `daytona_snapshot_wait.py`, `daytona_chain.sh`**: Daytona's bring-up, which records
  each image's digest in `platforms.txt`, then a wait for its default snapshot.
- **`daytona_cmd.txt`**: the bench command for each Daytona try: n = 300 lifecycles after 10 warm-ups,
  100 per concurrency level, density capped at 200.

The compose override that sets Daytona's admin quotas holds its admin key, so it is not here. The
scripts read the key from a mode-600 file at run time and never print it.
