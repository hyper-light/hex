#!/usr/bin/env bash
# vm7_breakdown.sh's guest side, run by vm7_inside.sh in breakdown mode: <share> holds bin/, lab/, env and out/.
# The binaries and the lab go to the guest's own disk, so no create execs or traces through 9p. Two cgroups at the
# root, as a host has the bench container's beside other tenants': load/ holds IDLE_THREADS idle threads and LOOPS
# busy loops (nice 0, any vCPU, started and killed by their PIDs), bench/ the breakdown, in a cgroup namespace and
# a mount namespace of its own whose /sys/fs/cgroup is bench/, as the container's cgroupns=private gives it. The
# breakdown's results are copied back to <share>/out.
set -uo pipefail
S=${1:?share}
# shellcheck disable=SC1091
. "$S/env"
rm -rf /opt/bin /opt/comp /opt/out; mkdir -p /opt/bin /opt/comp /opt/out
cp "$S"/bin/* /opt/bin/; cp -r "$S"/lab/. /opt/comp/
# The mounts trace's call names: the host's header, where the guest has none (its root is a snapshot, so this is undone).
[ -s /usr/include/asm/unistd_64.h ] || { mkdir -p /usr/include/asm && cp /opt/comp/unistd_64.h /usr/include/asm/; }
C=/sys/fs/cgroup
for c in cpu memory pids io; do echo "+$c" > "$C/cgroup.subtree_control"; done
mkdir -p "$C/load" "$C/bench"
guest() { echo "$(date -u +%FT%TZ) guest: $(nproc) vCPUs, load $(cut -d' ' -f1-3 /proc/loadavg), threads $(cut -d' ' -f4 /proc/loadavg), uptime $(cut -d' ' -f1 /proc/uptime)s"; }
guest | tee /opt/out/guest.txt
PIDS=()
if [ "${IDLE_THREADS:-0}" -gt 0 ]; then
  (echo "$BASHPID" > "$C/load/cgroup.procs"; exec /opt/bin/idlethreads "$IDLE_THREADS") > /opt/out/idlethreads.out 2>&1 &
  PIDS+=("$!")
  for i in $(seq 1 150); do
    up=$(ls "/proc/${PIDS[0]}/task" 2>/dev/null | wc -l)
    [ "$up" -ge "$IDLE_THREADS" ] && break
    [ "$i" = 150 ] && { echo "refusing: only $up of $IDLE_THREADS idle threads came up"; kill "${PIDS[@]}"; exit 5; }
    sleep 0.2
  done
  echo "idle threads: $up up (asked $IDLE_THREADS)" | tee -a /opt/out/guest.txt
fi
for _ in $(seq 1 "${LOOPS:-0}"); do
  (echo "$BASHPID" > "$C/load/cgroup.procs"; exec bash -c 'while :; do :; done') &
  PIDS+=("$!")
done
echo "busy loops: ${LOOPS:-0}; pids ${PIDS[*]:-none}" | tee -a /opt/out/guest.txt
guest | tee -a /opt/out/guest.txt
bash -c 'echo $$ > /sys/fs/cgroup/bench/cgroup.procs
  exec unshare -m -C --propagation private bash -c "mount -t cgroup2 cgroup2 /sys/fs/cgroup && mount --bind /opt/comp /opt/lab && exec bash /opt/lab/probes/create_breakdown_inside.sh"' \
  2>&1 | tee /opt/out/probe.out
rc=${PIPESTATUS[0]}
guest | tee -a /opt/out/guest.txt
# No wait for the loops before the probe: its container, node and creates come after them. What shows they ran
# throughout is each one's CPU time at the end (utime + stime, in clock ticks, USER_HZ a second).
for p in "${PIDS[@]}"; do read -ra f < "/proc/$p/stat" && echo "$p $(( f[13] + f[14] ))"; done 2>/dev/null \
  | paste -sd' ' | sed "s/^/busy loops' CPU ticks by pid at the end: /" | tee -a /opt/out/guest.txt
kill "${PIDS[@]}" 2>/dev/null; wait "${PIDS[@]}" 2>/dev/null
cp -r /opt/out/. "$S/out/"
chown -R "$(stat -c %u:%g "$S")" "$S/out"
exit "$rc"
