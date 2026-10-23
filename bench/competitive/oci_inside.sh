#!/usr/bin/env bash
# bench/competitive: the OCI phase, inside the privileged bench container (started by host.sh).
# Builds the two bundles, vacates the cgroup root to a leaf so runtimes can make container cgroups,
# probes each runtime once, then runs oci_bench.py for every runtime in ROUNDS rounds, each round in a
# shuffled order so drift over the run does not favour one runtime.
# Env: RUNTIMES ("hex runc crun youki runsc"), ROUNDS (3), N (10000), WARMUP (100), N_OVERRIDE
# ("runsc:3000" lowers n for a slow runtime), LIFECYCLES (2000), CAP (1000), BUDGET_MIB (8192).
set -uo pipefail
RUNTIMES=${RUNTIMES:-"hex runc crun youki runsc"}; ROUNDS=${ROUNDS:-3}; N=${N:-10000}; WARMUP=${WARMUP:-100}
LIFECYCLES=${LIFECYCLES:-2000}; CAP=${CAP:-1000}; BUDGET_MIB=${BUDGET_MIB:-8192}; N_OVERRIDE=${N_OVERRIDE:-}
OUT=/opt/out; mkdir -p /run/err

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

# cgroup v2's no-internal-process rule: move everything to a leaf so the root can parent containers.
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
for c in cpu memory pids io; do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true; done

{ for rt in $RUNTIMES; do printf '%s\t' "$rt"; case $rt in hex) echo "hex (cp/t7-runtime release build)";; *) /opt/bin/$rt --version 2>&1 | head -1;; esac; done; } >> $OUT/runtime_versions.tsv

ok_runtimes=""
for rt in $RUNTIMES; do # one full lifecycle per runtime before timing anything
  if python3 /opt/lab/oci_bench.py "$rt" "$OUT/$rt.probe.json" 1 0 2 2 64 > "$OUT/$rt.probe.log" 2>&1 \
     && python3 -c "import json,sys; d=json.load(open(sys.argv[1])); sys.exit(1 if d['errors'] else 0)" "$OUT/$rt.probe.json"; then
    ok_runtimes="$ok_runtimes $rt"; echo "probe ok: $rt"
  else
    echo "probe FAILED: $rt (see $rt.probe.json / $rt.probe.log)"
  fi
done

for round in $(seq 1 "$ROUNDS"); do
  for rt in $(printf '%s\n' $ok_runtimes | shuf); do
    n=$N; for o in $N_OVERRIDE; do [ "${o%%:*}" = "$rt" ] && n=${o#*:}; done
    echo "round $round: $rt (n=$n)"
    python3 /opt/lab/oci_bench.py "$rt" "$OUT/$rt.r$round.json" "$n" "$WARMUP" "$LIFECYCLES" "$CAP" "$BUDGET_MIB"
  done
done
echo done > $OUT/oci.done
