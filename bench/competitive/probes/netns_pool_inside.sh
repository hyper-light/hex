#!/usr/bin/env bash
# bench/competitive/probes: netns_pool's rounds inside the bench container (privileged, a private cgroup namespace),
# as host.sh starts the OCI pass's, or the pass image on Docker Desktop (netns_pool.md). Builds netns_pool.c,
# moves every process to a leaf so the root's children may take the memory controller, and runs netns_pool.py.
# /opt/lab is bench/competitive; results go to /opt/out. Env: ROUNDS (2), N (500).
set -euo pipefail
OUT=/opt/out; ROUNDS=${ROUNDS:-2}; N=${N:-500}
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
echo "+memory" > /sys/fs/cgroup/cgroup.subtree_control
# ld.bfd by name: with the host's /usr and not its /etc, `ld` is a dangling alternatives link.
gcc -O2 -Wall -Wextra -Werror -fuse-ld=bfd -o /run/netns_pool /opt/lab/probes/netns_pool.c
echo "kernel $(uname -r) $(uname -m), $(nproc) cpus, load at start: $(cat /proc/loadavg)"
python3 -I /opt/lab/probes/netns_pool.py "$OUT/netns_pool.json" "$ROUNDS" "$N"
echo "load at end: $(cat /proc/loadavg)"
