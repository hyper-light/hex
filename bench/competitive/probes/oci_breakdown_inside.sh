#!/usr/bin/env bash
# bench/competitive/probes: where an OCI runtime's time and memory go, inside the bench container
# (oci_breakdown.sh starts it as host.sh starts the OCI pass). The runtime design rests on these:
#   1. fsync: each runtime's lifecycle with its state root on the container's overlay, then on a tmpfs,
#      interleaved A B A B. Hex writes state with two fsyncs per write (state.rs); a root on tmpfs makes
#      them free, so the difference is what they cost.
#   2. the CLI floor: posix_spawn and wait of `true`, and of each runtime's `--version` and `state`, the
#      least any CLI-driven operation can cost.
#   3. processes per idle container: what each runtime leaves running beside the workload.
#   4. the throughput ceiling: create+start+delete at concurrency 1, 8 and 32 with the bundle's network
#      namespace and without it. Hex and crun both top out near 1,030 lifecycles/s at 32.
#   5. the driver's own ceiling: the same throughput phase with /usr/bin/true standing in for the runtime, so
#      every call is a spawn and an exit and nothing else: a ceiling the runtimes share could be the driver's.
# Env: N (1000), WARMUP (50), LIFECYCLES (1000), RUNTIMES ("hex crun"), STEPS ("1 2 3 4 5").
set -uo pipefail
N=${N:-1000}; WARMUP=${WARMUP:-50}; LIFECYCLES=${LIFECYCLES:-1000}; RUNTIMES=${RUNTIMES:-"hex crun"}
STEPS=${STEPS:-"1 2 3 4 5"}; step() { [[ " $STEPS " == *" $1 "* ]]; }
OUT=/opt/out; mkdir -p /run/err

bundle() { # bundle <dir> <args json> <namespaces json>
  local B=$1; mkdir -p "$B/rootfs"/{usr,proc,dev,tmp}
  ( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin )
  cat > "$B/config.json" <<JSON
{ "ociVersion":"1.0.2","root":{"path":"rootfs","readonly":false},"hostname":"b",
  "process":{"cwd":"/","args":$2,"env":["PATH=/usr/bin:/usr/sbin"],"user":{"uid":0,"gid":0}},
  "mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},
            {"destination":"/proc","type":"proc","source":"proc"},
            {"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],
  "linux":{"namespaces":$3} }
JSON
}
ALL='[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]'
NONET='[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}]'
bundle /run/bundle-sleep '["/usr/bin/sleep","infinity"]' "$ALL"
bundle /run/bundle-true '["/usr/bin/true"]' "$ALL"

# cgroup v2's no-internal-process rule, as oci_inside.sh: everything to a leaf so the root parents containers.
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
for c in cpu memory pids io; do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true; done

root_of() { case $1 in hex) echo /run/hex-oci ;; *) echo "/run/$1" ;; esac; }
{ echo "date: $(date -u +%FT%TZ)"; echo "kernel: $(uname -r)"; echo "load: $(cat /proc/loadavg)"
  for rt in $RUNTIMES; do mkdir -p "$(root_of "$rt")"; echo "$rt root: $(findmnt -n -o FSTYPE,SOURCE -T "$(root_of "$rt")")"; done
} > "$OUT/env.txt"

# 1. fsync: A (overlay) B (tmpfs) A B, n lifecycles each.
step 1 && for rt in $RUNTIMES; do
  R=$(root_of "$rt")
  for pass in a1 b1 a2 b2; do
    case $pass in b*) mount -t tmpfs tmpfs "$R" ;; esac
    HEXBENCH_PHASES=lifecycle python3 -I /opt/lab/oci_bench.py "$rt" "$OUT/fsync.$rt.$pass.json" "$N" "$WARMUP" 0 0 64 \
      > "$OUT/fsync.$rt.$pass.log" 2>&1
    case $pass in b*) umount "$R" ;; esac
  done
done

# 2. the CLI floor, spawned and waited as oci_bench.py spawns every call.
step 2 && python3 -I - "$OUT/floor.json" "$N" $RUNTIMES <<'EOF'
import json, os, sys, time
out, n, rts = sys.argv[1], int(sys.argv[2]), sys.argv[3:]
sys.path.insert(0, "/opt/lab")
import oci_bench as ob
def spawn(argv, env):
    actions = [(os.POSIX_SPAWN_OPEN, fd, os.devnull, os.O_RDWR, 0) for fd in (0, 1, 2)]
    t0 = time.perf_counter_ns()
    pid = os.posix_spawn(argv[0], argv, env, file_actions=actions)
    os.waitpid(pid, 0)
    return (time.perf_counter_ns() - t0) / 1e6
def dist(xs):
    xs = sorted(xs)
    return {q: xs[max(0, int(-(-q * len(xs) // 1)) - 1)] for q in (0.5, 0.99)} | {"max": xs[-1]}
res = {"true": dist([spawn(["/usr/bin/true"], dict(os.environ)) for _ in range(n)])}
for rt in rts:
    r = ob.Runtime(rt)
    res[f"{rt} --version"] = dist([spawn(r.argv[:1] + ["--version"], r.env) for _ in range(n)])
    cid = f"floor-{rt}"
    r.create("/run/bundle-sleep", cid)
    res[f"{rt} state"] = dist([spawn(r.argv + ["state", cid], r.env) for _ in range(n)])
    r.delete(cid)
json.dump(res, open(out, "w"), indent=1)
EOF

# 3. processes per idle container: five `sleep` containers each, then every process in the bench's cgroups.
step 3 && for rt in $RUNTIMES; do
  python3 -I - "$rt" "$OUT/procs.$rt.txt" <<'EOF'
import os, subprocess, sys
sys.path.insert(0, "/opt/lab")
import oci_bench as ob
rt, out = sys.argv[1], sys.argv[2]
r = ob.Runtime(rt)
ids = [f"procs-{rt}-{i}" for i in range(5)]
for cid in ids:
    r.create("/run/bundle-sleep", cid)
    r.start(cid)
ps = subprocess.run(["ps", "-e", "-o", "pid,ppid,rss,vsz,cgroup:60,args"], capture_output=True, text=True).stdout
open(out, "w").write(ps)
for cid in ids:
    r.delete(cid)
EOF
done

# 4. the throughput ceiling, with the network namespace and without it.
step 4 && for ns in net nonet; do
  case $ns in nonet) bundle /run/bundle-true '["/usr/bin/true"]' "$NONET" ;; *) bundle /run/bundle-true '["/usr/bin/true"]' "$ALL" ;; esac
  for rt in $RUNTIMES; do
    HEXBENCH_PHASES=throughput python3 -I /opt/lab/oci_bench.py "$rt" "$OUT/ceiling.$rt.$ns.json" 1 0 "$LIFECYCLES" 0 64 \
      > "$OUT/ceiling.$rt.$ns.log" 2>&1
  done
done
# 5. the driver's own ceiling: `crun` is /usr/bin/true, so create, start and delete each spawn and exit at once.
#    This tree's driver, then (OLD_DRIVER_REV) an older one's, back to back.
if step 5; then
  mkdir -p /run/standin && ln -sfn /usr/bin/true /run/standin/crun
  HEXBENCH_BIN=/run/standin HEXBENCH_PHASES=throughput python3 -I /opt/lab/oci_bench.py crun "$OUT/ceiling.standin.json" 1 0 \
    "$LIFECYCLES" 0 64 > "$OUT/ceiling.standin.log" 2>&1
  if [ -f "$OUT/old-driver/oci_bench.py" ]; then
    HEXBENCH_BIN=/run/standin HEXBENCH_PHASES=throughput python3 -I "$OUT/old-driver/oci_bench.py" crun \
      "$OUT/ceiling.standin.old.json" 1 0 "$LIFECYCLES" 0 64 > "$OUT/ceiling.standin.old.log" 2>&1
  fi
fi
echo "load at end: $(cat /proc/loadavg)" >> "$OUT/env.txt"
