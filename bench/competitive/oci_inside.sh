#!/usr/bin/env bash
# bench/competitive: the OCI phase, inside the privileged bench container (started by host.sh).
# Builds the two bundles, vacates the cgroup root to a leaf so runtimes can make container cgroups,
# probes each runtime once, then runs oci_bench.py for every runtime in ROUNDS rounds, each round in a
# shuffled order so drift over the run does not favour one runtime.
# Env: RUNTIMES ("hex runc crun youki runsc libcrun"), ROUNDS (3), ROUND_FIRST (1), N (10000), WARMUP (100),
# N_OVERRIDE ("runsc:3000" lowers n for a slow runtime), LIFECYCLES (2000), CAP (1000), BUDGET_MIB (8192),
# HEXBENCH_PHASES (oci_bench.py's and seam_oci's phases, all by default), HEXBENCH_SUSTAINED (the sustained phase's
# lifecycles, 10000, read by both), STANDIN (1: the driver's own ceiling each round).
set -uo pipefail
RUNTIMES=${RUNTIMES:-"hex runc crun youki runsc libcrun"}; ROUNDS=${ROUNDS:-3}; ROUND_FIRST=${ROUND_FIRST:-1}; N=${N:-10000}; WARMUP=${WARMUP:-100}
LIFECYCLES=${LIFECYCLES:-2000}; CAP=${CAP:-1000}; BUDGET_MIB=${BUDGET_MIB:-8192}; N_OVERRIDE=${N_OVERRIDE:-}
OUT=/opt/out; mkdir -p /run/err

ALL_NS='[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]'
bundle() { # bundle <dir> <args json> [namespaces json, all five by default]
  local B=$1 NS=${3:-$ALL_NS}; mkdir -p "$B/rootfs"/{usr,proc,dev,tmp}
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
bundle /run/bundle-sleep '["/usr/bin/sleep","infinity"]'
bundle /run/bundle-true '["/usr/bin/true"]'
# lifecycle_nonet's: the same container without a network namespace of its own.
bundle /run/bundle-sleep-nonet '["/usr/bin/sleep","infinity"]' '[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}]'

# cgroup v2's no-internal-process rule: move everything to a leaf so the root can parent containers.
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
for c in cpu memory pids io; do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true; done

{ for rt in $RUNTIMES; do printf '%s\t' "$rt"; case $rt in hex) echo "hex (cp/t7-runtime release build)";; libcrun) /opt/bin/libcrun-driver --version 2>&1;; *) /opt/bin/$rt --version 2>&1 | head -1;; esac; done; } >> $OUT/runtime_versions.tsv

ok_runtimes=""
for rt in $RUNTIMES; do # one full lifecycle per runtime before timing anything
  if python3 /opt/lab/oci_bench.py "$rt" "$OUT/$rt.probe.json" 1 0 2 2 64 > "$OUT/$rt.probe.log" 2>&1 \
     && python3 -c "import json,sys; d=json.load(open(sys.argv[1])); sys.exit(1 if d['errors'] else 0)" "$OUT/$rt.probe.json"; then
    ok_runtimes="$ok_runtimes $rt"; echo "probe ok: $rt"
  else
    echo "probe FAILED: $rt (see $rt.probe.json / $rt.probe.log)"
  fi
done

# libcrun in process: a broken config.json is refused with libcrun's own error, and the same driver goes on to
# serve the next call; otherwise the driver is wrong, and libcrun runs no round.
if [[ " $ok_runtimes " == *" libcrun "* ]]; then
  mkdir -p /run/bundle-broken && ln -sfn /run/bundle-true/rootfs /run/bundle-broken/rootfs
  head -c 40 /run/bundle-true/config.json > /run/bundle-broken/config.json
  if ! python3 - "$OUT/libcrun.broken.json" <<'PY'
import json, sys
sys.path.insert(0, "/opt/lab")
import oci_bench as ob
rt, out = ob.Runtime("libcrun"), {}
try:
    rt.create("/run/bundle-broken", "libcrun-broken")
    out["broken"] = "created"
except ob.OpError as e:
    out["broken"] = str(e)
try:
    rt.create("/run/bundle-true", "libcrun-after")
    rt.start("libcrun-after")
    rt.delete("libcrun-after")
    out["after"] = "ok"
except ob.OpError as e:
    out["after"] = str(e)
rt.close()
json.dump(out, open(sys.argv[1], "w"))
ok = out["broken"] != "created" and out["after"] == "ok"
print(f"probe {'ok' if ok else 'FAILED'}: libcrun refuses a broken config.json, then serves the next call: {json.dumps(out)}")
sys.exit(0 if ok else 1)
PY
  then ok_runtimes=$(printf '%s\n' $ok_runtimes | grep -vx libcrun | tr '\n' ' '); fi
fi

# The driver's own ceiling (probes/oci_breakdown.md, step 5): its throughput phase with /usr/bin/true standing
# in for the runtime, so every call is a spawn and an exit and nothing else. Once a round, in the shuffle with
# the runtimes, so it shares their conditions.
mkdir -p /run/standin && ln -sfn /usr/bin/true /run/standin/crun
# The oci class through the Runtime seam (seam_oci; SEAM_OCI names its binary in /opt/bin), by each of its
# drivers: in process (hex-seam), and over the node agent's socket (hex-seam-socket: a `hex node` process
# seam_oci starts). The node agent runs in this container, as root, with its box root beside the runtimes'
# container cgroups, so its create, start, exec and delete are compared with libcrun's in-process calls.
# Each driver runs once a round, in the shuffle, as the runtimes do. Their phases are oci_bench.py's lifecycle,
# lifecycle_nonet (on the bundle without a network namespace), throughput and sustained, and rtt (inspect of a
# running box, whose difference between the drivers is the socket's cost per call); around each, seam_oci reads
# the node's network namespace counts (decision 4), so the sustained phase says how often its creates found the
# namespace pool dry.
SEAM=""; [ -n "${SEAM_OCI:-}" ] && [ -x "$SEAM_OCI" ] && SEAM="hex-seam hex-seam-socket"
for round in $(seq "$ROUND_FIRST" $((ROUND_FIRST + ROUNDS - 1))); do
  for rt in $(printf '%s\n' $ok_runtimes $([ "${STANDIN:-1}" = 1 ] && echo standin) $SEAM | shuf); do
    if [ "$rt" = standin ]; then
      echo "round $round: the stand-in (/usr/bin/true as crun)"
      HEXBENCH_BIN=/run/standin HEXBENCH_PHASES=throughput python3 /opt/lab/oci_bench.py crun "$OUT/ceiling.standin.r$round.json" \
        1 0 "$LIFECYCLES" 0 64
      continue
    fi
    if [ "$rt" = hex-seam ] || [ "$rt" = hex-seam-socket ]; then
      if [ "$rt" = hex-seam ]; then driver=inprocess; file=ociseam; else driver=socket; file=ociseamsock; fi
      echo "round $round: the oci class through the Runtime seam (seam_oci, $driver)"
      HEXBENCH_DRIVER=$driver "$SEAM_OCI" "$OUT/$file.r$round.json" "$N" "$WARMUP" "$LIFECYCLES" /opt/bin/hex \
        /run/bundle-sleep /run/bundle-true /run/bundle-sleep-nonet 2>&1 | tee -a "$OUT/$file.log" \
        || echo "$file round $round exited $?"
      continue
    fi
    n=$N; for o in $N_OVERRIDE; do [ "${o%%:*}" = "$rt" ] && n=${o#*:}; done
    echo "round $round: $rt (n=$n)"
    python3 /opt/lab/oci_bench.py "$rt" "$OUT/$rt.r$round.json" "$n" "$WARMUP" "$LIFECYCLES" "$CAP" "$BUDGET_MIB"
  done
done
echo done > $OUT/oci.done
