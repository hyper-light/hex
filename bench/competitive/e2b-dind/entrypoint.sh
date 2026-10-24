#!/bin/sh
# cgroup v2's no-internal-processes rule: this namespace's root hands controllers down only once it holds no process,
# so everything here moves to a leaf first (moby's hack/dind does the same), then dockerd starts.
set -eu
mkdir -p /sys/fs/cgroup/init
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/init/cgroup.procs 2>/dev/null || true; done
sed -e 's/ / +/g' -e 's/^/+/' < /sys/fs/cgroup/cgroup.controllers > /sys/fs/cgroup/cgroup.subtree_control
exec dockerd --host=unix:///var/run/docker.sock
