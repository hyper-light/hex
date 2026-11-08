#!/usr/bin/env bash
# Inside waits_census.sh's container: the bundles as css_cost_inside.sh makes them, then seam_oci over the node's
# socket with the waits census on, in three arms, each its own census file: the node's state on tmpfs (/run) and on the
# host's filesystem (/opt/disk), under the bar's no-owner conditions (no HEXBENCH_OWNER, the node's defaults otherwise),
# with seam_oci's list, counts, hellos and restart phases; an owner arm, whose kept namespaces the network check checks;
# and, where /opt/bin holds debug builds (hex-debug, seam_oci-debug), a debug arm: the tmpfs arm's phases on them, the
# record a debug build's waits are seeded from (waits_census.sh runs the suites beside it, for their load).
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
# The owner arm's bundles, as oci_inside.sh's with an owner: Docker's default capabilities and /proc/sys read-only, so a
# box's network namespace is kept for its owner's next box and checked as it is handed on.
CAPS='["CAP_CHOWN","CAP_DAC_OVERRIDE","CAP_FSETID","CAP_FOWNER","CAP_MKNOD","CAP_NET_RAW","CAP_SETGID","CAP_SETUID","CAP_SETFCAP","CAP_SETPCAP","CAP_NET_BIND_SERVICE","CAP_SYS_CHROOT","CAP_KILL","CAP_AUDIT_WRITE"]'
for b in sleep true; do
  cp -r /run/bundle-$b /run/bundle-owner-$b
  python3 -I - /run/bundle-owner-$b/config.json "$CAPS" <<'PY'
import json, sys
p, caps = sys.argv[1], json.loads(sys.argv[2])
c = json.load(open(p))
c["process"]["capabilities"] = {k: caps for k in ("bounding", "effective", "permitted")}
c["linux"]["readonlyPaths"] = ["/proc/sys"]
json.dump(c, open(p, "w"))
PY
done
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
for c in cpu memory pids io; do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true; done
echo "inside: kernel $(uname -r), load $(cat /proc/loadavg), /opt/disk on $(stat -f -c %T /opt/disk)" >> "$OUT/env.txt"
# Every wait's answers, at most this many lines an arm: N lifecycles of the phases below, each a handful of waits, and
# the list arm's lists of their parts.
export HEX_WAITS_CENSUS_MAX=$((N * 128))
# Each row at least the 1,000 answers its seed needs (waits_census.py): 1,000 lists, N connections, N counts calls, and
# 16 restarts of 1,000 boxes (a restart's recovery lists about 65 to 130 times, run 1, at 10,000 boxes or fewer).
export HEXBENCH_LIST_BOXES=${LIST_BOXES:-$N} HEXBENCH_LISTS=${LISTS:-1000} HEXBENCH_RESTART_BOXES=${RESTART_BOXES:-1000}
export HEXBENCH_RESTARTS=${RESTARTS:-16}
# Each arm: its name, the node's home (its state under it), the owner (none: the bar's no-owner conditions), its phases.
# tmpfs and disk drive every no-owner row (list: serve.reads; restart: node.listing); owner drives the network check's
# rows (kept namespaces checked as their boxes go).
ARMS=("tmpfs /run/home - - lifecycle sustained rtt list counts hellos restart"
      "disk /opt/disk/home - - lifecycle sustained rtt list counts hellos restart"
      "owner /run/home-owner bench owner- lifecycle sustained")
[ -x /opt/bin/hex-debug ] && [ -x /opt/bin/seam_oci-debug ] \
  && ARMS+=("debug /run/home-debug - -debug lifecycle sustained rtt list counts hellos restart")
for arm in "${ARMS[@]}"; do
  set -- $arm; name=$1 home=$2 owner=$3 flavour=$4; shift 4; phases="$*"
  # Which binaries and bundles: the release builds and the bar's bundles; the owner's bundles; the debug builds.
  bin=""; bundles=""; case $flavour in -debug) bin=-debug ;; owner-) bundles=owner- ;; esac
  mkdir -p "$home/.cache/hex-t7/ociseam"
  if [ "$owner" = - ]; then unset HEXBENCH_OWNER; else export HEXBENCH_OWNER=$owner; fi
  HOME=$home HEX_WAITS_CENSUS="$OUT/census-$name.txt" HEXBENCH_DRIVER=socket \
    HEXBENCH_PHASES="$phases" HEXBENCH_SUSTAINED="$N" \
    /opt/bin/seam_oci$bin "$OUT/seam-$name.json" "$N" 20 0 /opt/bin/hex$bin /run/bundle-${bundles}sleep /run/bundle-${bundles}true \
    > "$OUT/seam-$name.log" 2>&1 || echo "seam_oci ($name) failed: $(tail -c 300 "$OUT/seam-$name.log")"
  echo "census ($name: $phases, owner $owner): $(wc -l < "$OUT/census-$name.txt" 2>/dev/null || echo 0) lines" >> "$OUT/env.txt"
done
