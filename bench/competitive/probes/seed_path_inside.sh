#!/usr/bin/env bash
# bench/competitive/probes: the seam's create through its seeds (goal 5 plan step 4), inside the bench container
# (seed_path.sh). Its readings, each over seam_oci's lifecycle phase but the last: the oci class through the seam in
# process, as root, on the OCI pass's bundles (oci_inside.sh's shape), each create through a seed (decision 5).
#   clone   A trace instance of this run's own records three sched events for the processes named hex (the seeds,
#           and each init until it execs its program): sched_waking, sched_process_fork and sched_switch to them.
#           seed_path.py takes, for each init a seed clones, from the seed's first wakeup by the node agent since its
#           last clone (the node's request arriving on its socket) to the init's birth (the fork event the seed's
#           clone3 fires in the seed) and to its first run: the seed's clone of an init, request to child. sched
#           events only: they cost a create least, and the syscall events of an instance recorded none on fedora when
#           this was written (133 forks, 0 syscall events, the same run).
#   execs   strace -f -e trace=execve over the whole process tree for EXECS creates after 5 warm-up ones: each exec of
#           the hex binary by its first argument (__oci-seed, a seed's start; __oci-init, the shim; __oci-exec, the
#           exec helper), and each program exec'd.
#   bpf     strace -f -y -e trace=bpf over the same: each bpf call by its command, and each attach by the cgroup it
#           targets (goal 5 extended, step 1). The bundles' default policy is the program the node's start puts on its
#           oci boxes' root, so their creates make no call; the node's start queries that root and, on a fresh one,
#           loads the default and attaches it there.
#   memory  Each seed's Pss (smaps_rollup) and VmRSS, sampled every 50 ms while the node serves MEMORY_N creates: each
#           `hex __oci-seed` in this pid namespace alone, since an init its seed cloned keeps the seed's arguments
#           until it execs, in a pid namespace of its own.
#   seeds   seam_oci's sustained phase (SUSTAINED lifecycles at concurrency 32) and the node's create seeds' counts
#           over it (seam_oci's `seeds`): the creates that waited for a seed, the seeds started and the most busy at
#           once, by which decision 5's bound of 32 is judged.
# Writes summary.txt and trace.txt to /opt/out.
# Env: N (300), WARMUP (20), EXECS (50), MEMORY_N (400), SUSTAINED (10000).
set -uo pipefail
N=${N:-300}; WARMUP=${WARMUP:-20}; EXECS=${EXECS:-50}; MEMORY_N=${MEMORY_N:-400}; SUSTAINED=${SUSTAINED:-10000}
OUT=/opt/out; mkdir -p /run/err
ALL_NS='[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]'
bundle() { # bundle <dir> <args json>
  local B=$1; mkdir -p "$B/rootfs"/{usr,proc,dev,tmp}
  ( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin )
  cat > "$B/config.json" <<JSON
{ "ociVersion":"1.0.2","root":{"path":"rootfs","readonly":false},"hostname":"b",
  "process":{"cwd":"/","args":$2,"env":["PATH=/usr/bin:/usr/sbin"],"user":{"uid":0,"gid":0}},
  "mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},
            {"destination":"/proc","type":"proc","source":"proc"},
            {"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],
  "linux":{"namespaces":$ALL_NS} }
JSON
}
bundle /run/bundle-sleep '["/usr/bin/sleep","infinity"]'
bundle /run/bundle-true '["/usr/bin/true"]'
# cgroup v2's no-internal-process rule, as oci_inside.sh: everything to a leaf so the root parents containers.
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
for c in cpu memory pids io; do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null || true; done
seam() { # seam <out json> <n> <warmup> [wrapper...]: seam_oci's lifecycle phase
  local out=$1 n=$2 w=$3; shift 3
  HEXBENCH_PHASES=lifecycle "$@" /opt/bin/seam_oci "$out" "$n" "$w" 0 /opt/bin/hex /run/bundle-sleep /run/bundle-true
}
{
  # The clone.
  T=/sys/kernel/tracing
  mountpoint -q "$T" || mount -t tracefs nodev "$T" || { echo "refusing: no tracefs at $T"; exit 4; }
  I="$T/instances/hexseed-$$"; mkdir "$I" || { echo "refusing: no trace instance at $I"; exit 4; }
  echo mono > "$I/trace_clock"; echo 32768 > "$I/buffer_size_kb"; echo 0 > "$I/tracing_on"
  for ev in sched/sched_process_fork sched/sched_waking; do
    echo 'comm == "hex"' > "$I/events/$ev/filter"; echo 1 > "$I/events/$ev/enable"
  done
  echo 'next_comm == "hex"' > "$I/events/sched/sched_switch/filter"; echo 1 > "$I/events/sched/sched_switch/enable"
  echo 1 > "$I/tracing_on"
  seam /run/out.clone.json "$N" "$WARMUP" > /run/clone.log 2>&1 || echo "the clone's run failed: $(tail -3 /run/clone.log)"
  echo 0 > "$I/tracing_on"; cat "$I/trace" > "$OUT/trace.txt"
  echo "clone: trace overruns $(grep -h overrun "$I"/per_cpu/cpu*/stats | awk -F': ' '{s+=$2} END{print s+0}'), $((N + WARMUP)) creates"
  python3 -I /opt/lab/probes/seed_path.py "$OUT/trace.txt"
  rmdir "$I" 2>/dev/null || true
  # The execs.
  seam /run/out.execs.json "$EXECS" 5 /opt/strace/usr/bin/strace -f -e trace=execve -qq -o /run/execs.txt \
    > /run/execs.log 2>&1 || echo "the execs' run failed: $(tail -3 /run/execs.log)"
  hexes() { grep 'execve("/opt/bin/hex"' /run/execs.txt | grep -c "$1"; }
  echo "execs over $((EXECS + 5)) creates: $(grep -c 'execve(' /run/execs.txt) in all; of the hex binary" \
    "$(grep -c 'execve("/opt/bin/hex"' /run/execs.txt): __oci-seed $(hexes __oci-seed), __oci-init $(hexes __oci-init)," \
    "__oci-exec $(hexes __oci-exec); /usr/bin/sleep $(grep -c 'execve("/usr/bin/sleep"' /run/execs.txt)," \
    "/usr/bin/true $(grep -c 'execve("/usr/bin/true"' /run/execs.txt)"
  # The device programs.
  seam /run/out.bpf.json "$EXECS" 5 /opt/strace/usr/bin/strace -f -y -e trace=bpf -qq -o /run/bpf.txt \
    > /run/bpf.log 2>&1 || echo "the bpf run failed: $(tail -3 /run/bpf.log)"
  cp /run/bpf.txt "$OUT/bpf.txt"
  echo "bpf over $((EXECS + 5)) creates, the node's start with them:" \
    "$(grep -o 'bpf(BPF_[A-Z_]*' /run/bpf.txt | sort | uniq -c | awk '{printf "%s %s, ", substr($2, 5), $1}')" \
    "attaches by target: $(grep 'bpf(BPF_PROG_ATTACH' /run/bpf.txt | grep -o 'target_fd=[0-9]*<[^>]*>' \
      | sed 's/^target_fd=[0-9]*//' | sort | uniq -c | xargs)"
  # The memory.
  seam /run/out.memory.json "$MEMORY_N" 20 > /run/memory.log 2>&1 &
  bench=$!
  while kill -0 $bench 2>/dev/null; do
    for p in $(pgrep -f '__oci-seed'); do
      [ "$(awk '/^NSpid:/{print NF}' "/proc/$p/status" 2>/dev/null)" = 2 ] || continue
      pss=$(awk '/^Pss:/{print $2}' "/proc/$p/smaps_rollup" 2>/dev/null)
      rss=$(awk '/^VmRSS:/{print $2}' "/proc/$p/status" 2>/dev/null)
      [ -n "$pss" ] && [ -n "$rss" ] && echo "$p $pss $rss"
    done
    sleep 0.05
  done > /run/memory.samples
  wait $bench || echo "the memory's run failed: $(tail -3 /run/memory.log)"
  awk '{seeds[$1]++; p[NR]=$2; r[NR]=$3} END {
    if (!NR) { print "memory: no samples"; exit }
    n = asort(p); asort(r); m = int((NR + 1) / 2); k = 0; for (s in seeds) k++
    printf "memory: %d samples of %d seeds, Pss KiB min %d median %d max %d; VmRSS KiB min %d median %d max %d\n", NR, k, p[1], p[m], p[NR], r[1], r[m], r[NR] }' /run/memory.samples
  # The seeds.
  HEXBENCH_PHASES=sustained HEXBENCH_SUSTAINED="$SUSTAINED" /opt/bin/seam_oci /run/out.seeds.json 20 0 0 /opt/bin/hex \
    /run/bundle-sleep /run/bundle-true > /run/seeds.log 2>&1 || echo "the seeds' run failed: $(tail -3 /run/seeds.log)"
  python3 -I -c "import json,sys; p=json.load(open(sys.argv[1]))['phases'].get('sustained', {})
print('seeds over the sustained phase: %d lifecycles at concurrency 32, %.0f/s;' % (p.get('lifecycles', 0), p.get('per_s', 0)),
      ', '.join('%s %s' % kv for kv in p.get('seeds', {}).items()))" /run/out.seeds.json
  for f in /run/out.*.json; do
    python3 -I -c "import json,sys; d=json.load(open(sys.argv[1])); print(sys.argv[1], 'errors', d.get('errors'))" "$f"
  done
} 2>&1 | tee "$OUT/summary.txt"
