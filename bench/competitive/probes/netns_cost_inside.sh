#!/usr/bin/env bash
# bench/competitive/probes: what a container's network namespace costs create and delete, Hex's oci class through
# the Runtime seam (seam_oci's lifecycle phase on a bundle with the namespace and on one without) beside libcrun in
# process (oci_bench.py's lifecycle and lifecycle_nonet), and a bare `unshare -n` against `unshare -u`; 2 rounds in
# a shuffled order, best of 2. Runs in the bench container as root, /opt/bin holding hex, seam_oci and
# libcrun-driver and /opt/lab bench/competitive (goal 4 of docs/design/runtime-architecture.md, decision 4).
# The OCI pass measures the same in its own rounds (seam_oci's lifecycle and lifecycle_nonet phases beside
# oci_bench.py's, goal 4 plan step 2); this stays the quick check, a few minutes with nothing else run.
# Env: N (500), W (50).
set -uo pipefail
mkdir -p /run/err; N=${N:-500}; W=${W:-50}; OUT=/opt/out
ALL='[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]'
NONET='[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}]'
bundle() { local B=$1 NS=$3; mkdir -p "$B/rootfs"/{usr,proc,dev,tmp}
  ( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin )
  cat > "$B/config.json" <<JSON
{ "ociVersion":"1.0.2","root":{"path":"rootfs","readonly":false},"hostname":"b",
  "process":{"cwd":"/","args":$2,"env":["PATH=/usr/bin:/usr/sbin"],"user":{"uid":0,"gid":0}},
  "mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},
            {"destination":"/proc","type":"proc","source":"proc"},
            {"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],
  "linux":{"namespaces":$NS} }
JSON
}
bundle /run/bundle-sleep '["/usr/bin/sleep","infinity"]' "$ALL"
bundle /run/bundle-sleep-nonet '["/usr/bin/sleep","infinity"]' "$NONET"
bundle /run/bundle-true '["/usr/bin/true"]' "$ALL"
# cgroup v2's no-internal-process rule, as oci_inside.sh: everything to a leaf so the root parents containers.
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
for c in cpu memory pids io; do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true; done
echo "kernel $(uname -r) $(uname -m), load at start: $(cat /proc/loadavg)"
for round in 1 2; do
  for item in $(printf '%s\n' seam seamnonet libcrun | shuf); do
    case $item in
      seam) HEXBENCH_PHASES=lifecycle /opt/bin/seam_oci "$OUT/seam.r$round.json" "$N" "$W" 0 /opt/bin/hex \
              /run/bundle-sleep /run/bundle-true > /dev/null 2>&1 ;;
      seamnonet) HEXBENCH_PHASES=lifecycle /opt/bin/seam_oci "$OUT/seamnonet.r$round.json" "$N" "$W" 0 /opt/bin/hex \
              /run/bundle-sleep-nonet /run/bundle-true > /dev/null 2>&1 ;;
      libcrun) HEXBENCH_PHASES="lifecycle lifecycle_nonet" python3 -I /opt/lab/oci_bench.py libcrun "$OUT/libcrun.r$round.json" \
              "$N" "$W" 0 0 64 > /dev/null 2>&1 ;;
    esac || echo "$item r$round FAILED"
  done
done
each() { local s e; s=$(date +%s%N); for _ in $(seq 300); do "$@"; done; e=$(date +%s%N); echo $(( (e - s) / 300000 )); }
echo "unshare -u true: $(each unshare -u /usr/bin/true) us each; unshare -n true: $(each unshare -n /usr/bin/true) us each; unshare -u true again: $(each unshare -u /usr/bin/true) us each"
echo "load at end: $(cat /proc/loadavg)"
python3 -I - <<'PY'
import glob, json, statistics
def ms(path, phase, op):
    return json.load(open(path)).get("phases", {}).get(phase, {}).get("ms", {}).get(op, [])
def best(pattern, phase, op):
    p50s = [statistics.median(v) for f in sorted(glob.glob(pattern)) if (v := ms(f, phase, op))]
    return min(p50s) if p50s else float("nan")
for rt, net, nonet in (("hex", ("/opt/out/seam.r*.json", "lifecycle"), ("/opt/out/seamnonet.r*.json", "lifecycle")),
                       ("libcrun", ("/opt/out/libcrun.r*.json", "lifecycle"), ("/opt/out/libcrun.r*.json", "lifecycle_nonet"))):
    for op in ("create", "delete"):
        a, b = best(*net, op), best(*nonet, op)
        print(f"{rt} {op} p50 best of 2: with a network namespace {a:.3f} ms, without {b:.3f} ms, the namespace {a - b:+.3f} ms ({a / b:.2f}x)")
PY
