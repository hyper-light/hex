#!/usr/bin/env bash
# Inside waits_census.sh's container: the bundles as css_cost_inside.sh makes them, then seam_oci over the node's
# socket with the waits census on, its node's state first on tmpfs (/run), then on the host's filesystem (/opt/disk),
# each run's census in its own file. The bar's no-owner conditions: no HEXBENCH_OWNER, the node's defaults otherwise.
# A second node start over the first run's state times recovery's listings (node.listing).
set -uo pipefail
N=${N:-10000}; OUT=/opt/out
ALL_NS='[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]'
B=/run/bundle-sleep; mkdir -p "$B/rootfs"/{usr,proc,dev,tmp}
( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin )
cat > "$B/config.json" <<JSON
{ "ociVersion":"1.0.2","root":{"path":"rootfs","readonly":false},"hostname":"b",
  "process":{"cwd":"/","args":["/usr/bin/sleep","infinity"],"env":["PATH=/usr/bin:/usr/sbin"],"user":{"uid":0,"gid":0}},
  "mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},
            {"destination":"/proc","type":"proc","source":"proc"},
            {"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],
  "linux":{"namespaces":$ALL_NS} }
JSON
cp -r /run/bundle-sleep /run/bundle-true; sed -i 's|"/usr/bin/sleep","infinity"|"/usr/bin/true"|' /run/bundle-true/config.json
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
for c in cpu memory pids io; do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true; done
echo "inside: kernel $(uname -r), load $(cat /proc/loadavg), /opt/disk on $(stat -f -c %T /opt/disk)" >> "$OUT/env.txt"
# Every wait's answers, at most this many lines a run: N lifecycles of the phases below, each a handful of waits.
export HEX_WAITS_CENSUS_MAX=$((N * 64))
for place in tmpfs disk; do
  case $place in tmpfs) home=/run/home ;; disk) home=/opt/disk/home ;; esac
  mkdir -p "$home/.cache/hex-t7/ociseam"
  HOME=$home HEX_WAITS_CENSUS="$OUT/census-$place.txt" HEXBENCH_DRIVER=socket \
    HEXBENCH_PHASES="lifecycle sustained rtt" HEXBENCH_SUSTAINED="$N" \
    /opt/bin/seam_oci "$OUT/seam-$place.json" "$N" 20 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true \
    > "$OUT/seam-$place.log" 2>&1 || echo "seam_oci ($place) failed: $(tail -c 300 "$OUT/seam-$place.log")"
  echo "census ($place): $(wc -l < "$OUT/census-$place.txt" 2>/dev/null || echo 0) lines" >> "$OUT/env.txt"
done
