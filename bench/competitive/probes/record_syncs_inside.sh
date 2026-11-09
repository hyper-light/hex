#!/usr/bin/env bash
# bench/competitive/probes: the fsync family a seam create makes, inside the bench container (record_syncs.sh
# starts it with host.sh's OCI-pass flags, /run a tmpfs or the container's overlay as RUN says). seam_oci's
# lifecycle phase, N lifecycles of a `sleep` container (create, start, exec of `true`, delete) by each driver, in
# process and over the socket (its `hex node` a child, followed), under strace -f -c counting the fsync family
# and the renames: each box record write is a rename, and so is each container state write, so the renames show
# the writes the fsyncs would follow were made. The node agent runs as root here, so it keeps its containers'
# state under /run (decision 1); seam_oci keeps its records under $HOME/.cache, on the overlay.
# Env: RUN (tmpfs or overlay), N (5). Writes /opt/out/$RUN/{fs.txt,strace.<driver>.txt,seam.<driver>.json}.
set -euo pipefail
RUN=${RUN:?tmpfs or overlay}; N=${N:-5}; OUT=/opt/out/$RUN; mkdir -p "$OUT" "$HOME/.cache"
{ echo "/run: $(stat -f -c %T /run)"; echo "\$HOME/.cache (the records'): $(stat -f -c %T "$HOME/.cache")"; } | tee "$OUT/fs.txt"

# The OCI pass's bundles (oci_inside.sh).
bundle() { # bundle <dir> <args json>
  local B=$1; mkdir -p "$B/rootfs"/{usr,proc,dev,tmp}
  ( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin )
  cat > "$B/config.json" <<JSON
{ "ociVersion":"1.0.2","root":{"path":"rootfs","readonly":false},"hostname":"b",
  "process":{"cwd":"/","args":$2,"env":["PATH=/usr/bin:/usr/sbin"],"user":{"uid":0,"gid":0}},
  "mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},
            {"destination":"/proc","type":"proc","source":"proc"},
            {"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],
  "linux":{"namespaces":[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]} }
JSON
}
bundle /run/bundle-sleep '["/usr/bin/sleep","infinity"]'
bundle /run/bundle-true '["/usr/bin/true"]'

# cgroup v2's no-internal-process rule: everything moves to a leaf so the root can parent the boxes.
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
for c in cpu memory pids io; do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true; done

TRACE=fsync,fdatasync,syncfs,sync,sync_file_range,msync,rename,renameat,renameat2
for driver in inprocess socket; do
  HEXBENCH_DRIVER=$driver HEXBENCH_PHASES=lifecycle /opt/bin/strace -f -c -e trace="$TRACE" -o "$OUT/strace.$driver.txt" \
    /opt/bin/seam_oci "$OUT/seam.$driver.json" "$N" 0 1 /opt/bin/hex /run/bundle-sleep /run/bundle-true > "$OUT/seam.$driver.out" 2>&1
  echo "== /run on $RUN, $driver: $N lifecycles"; cat "$OUT/strace.$driver.txt"
done
