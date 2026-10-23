#!/bin/bash
# platform_gate.sh <platform> <round> <idle-since epoch> <stack label>: the platform-pass gate
# (T7 agent, 2026-10-23, sixth revision). <stack label> is the docker label naming the stack's containers,
# e.g. com.docker.compose.project=hexbench-daytona or io.x-k8s.kind.cluster=hexbench. STACK_EXTRA_CG (optional,
# space-separated cgroup paths such as /e2b, E2B's per-sandbox Firecracker cgroups) adds stack cgroups outside
# every container; the round's own unit, hexbench-<platform>-r<N>-t<try>.service, is the bench, never foreign.
#  (a) no other benchmark unit is active (a .service or .scope unit whose name holds bench, routing-gate or
#      big-goals-check; never a .slice, which stays active empty until stopped), and no process outside the
#      platform's stack runs at a nice below 10 using over 50% CPU, from a 5-second sample (the second frame of
#      `top -b -n 2 -d 5`). The stack is the processes of the labelled containers (anything under their
#      docker-<id>.scope cgroups, nested container engines included) and the host's dockerd and containerd
#      (shims too), which serve them. A busy process that exits before its cgroup can be read counts as
#      outside the stack. Processes at nice 10 or above never block (a); each one using CPU is listed in the
#      round's record.
#  (b) the 1-minute load is under 2 plus the stack's idle CPU. A load average remembers every process's last
#      minutes, so it can't measure the stack alone: the idle figure is the stack's own CPU use while idle,
#      from its cgroups (STACK_CGROUPS, from stack_cgroups.sh, as run_refereed.sh computes them). Once the
#      stack has sat up and idle for 5 minutes with no rounds running, the sum of their cpu.stat usage_usec
#      is read, again after a minute with (a) holding throughout (checked by back-to-back 5-second samples;
#      the minute starts over when (a) breaks, or when a stack cgroup goes, appears or is made anew within
#      it), and IDLE = the sum's increase / the minute's measured length / 1e6, in CPUs: the stack's
#      contribution to the load average, its runnable tasks, near enough. The minute's length is measured
#      between the two reads, 60 s and at most one sample's 5 s more. A stack cgroup absent at both reads
#      (docker's hexbench.slice before its first container) counts 0 and is named in the record.
# With (a) holding and (b) not, it waits up to 45 minutes, then lets the round run anyway, saying so.
# DENSITY_ONLY=1 (a density-only round: held memory does not depend on other work's CPU load): (a) checks only
# benchmark units, not foreign load, and the 5-minute idle wait and (b) are skipped.
# Appends the round's idle CPU and how it was read, the start load, top 5 processes and niced CPU users to
# $OUT/platforms.txt.
PLATFORM=${1:?platform}; ROUND=${2:?round}; IDLE_SINCE=${3:?idle since}; STACK_LABEL=${4:?stack label}
O=$HOME/.cache/hex-bench/out/20261023T025023Z; T7=$HOME/.cache/hex-bench/t7
SAMPLE=$T7/gate.sample  # overwritten by each reading
load1() { cut -d' ' -f1 /proc/loadavg; }
sample() {  # the second frame of a 5-second top sample, as "pid ni %cpu command" lines, busiest first
  top -b -n 2 -d 5 -w 512 | awk '/^ *PID /{f++; next} f==2 && NF>=12 {print $1, $4, $9, $12}' | sort -k3,3 -gr > "$SAMPLE"
}
KILN_HOST_PID=185421  # the kiln agent host, this session's own parent: tallied separately, never foreign
whose() {  # "stack", "stack-infra", "bench", "kiln-host" or "foreign:<cgroup tail>" for a pid
  local cg ids
  [ "$1" = "$KILN_HOST_PID" ] && { echo kiln-host; return; }
  cg=$(cut -d: -f3- "/proc/$1/cgroup" 2>/dev/null) || { echo "foreign:exited"; return; }
  [ -z "$cg" ] && { echo "foreign:exited"; return; }
  # The round's own bench client: (a) admits no other benchmark unit, so the only hexbench unit a sample can
  # see is the round's own, and its CPU is part of what the round measures.
  case "${cg##*/}" in hexbench-*.service) echo bench; return ;; esac
  for x in ${STACK_EXTRA_CG:-}; do case "$cg" in "$x"|"$x"/*) echo stack; return ;; esac; done
  ids=$(docker ps -q --no-trunc --filter "label=$STACK_LABEL" 2>/dev/null)
  for id in $ids; do case "$cg" in *"docker-$id.scope"*|*"/docker/$id"*) echo stack; return ;; esac; done
  case "$cg" in /system.slice/docker.service*|/system.slice/containerd.service*) echo stack-infra; return ;; esac
  echo "foreign:${cg##*/}"
}
bench_units() {  # active benchmark units: .service and .scope only, never a .slice
  systemctl --user list-units --no-legend --plain --state=active,activating,deactivating 2>/dev/null \
    | awk '{print $1}' | grep -E '\.(service|scope)$' | grep -E 'bench|routing-gate|big-goals-check' | tr '\n' ' '
}
why_not_a() {  # prints why (a) fails, nothing when it holds; takes a fresh sample
  local units busy pid ni cpu comm w
  sample
  units=$(bench_units)
  busy=""
  if [ "${DENSITY_ONLY:-0}" != 1 ]; then
    while read -r pid ni cpu comm; do
      awk -v c="$cpu" -v n="$ni" 'BEGIN{exit !(c > 50 && n < 10)}' || continue
      w=$(whose "$pid")
      case "$w" in foreign:*) busy="$busy$comm pid $pid nice $ni $cpu% (${w#foreign:}) " ;; esac
    done < "$SAMPLE"
  fi
  [ -n "$units" ] && printf 'benchmark units active: %s' "$units"
  [ -n "$busy" ] && printf 'busy processes outside the stack: %s' "$busy"
}
log() { echo "$(date '+%T') $*"; }
cpu_usec() {  # "<cgroup> <usage_usec>" per stack cgroup in $SC, "<cgroup> -" for one that isn't there
  local c f
  for c in $SC; do
    f=/sys/fs/cgroup/${c#/}/cpu.stat
    if [ -r "$f" ]; then echo "$c $(awk '$1 == "usage_usec" {print $2}' "$f")"; else echo "$c -"; fi
  done
}

if [ "${DENSITY_ONLY:-0}" = 1 ]; then
  # Density only: no idle wait and no (b); (a) waits only for other benchmark units (3 hours at most).
  t0=$(date +%s); last=""
  while w=$(why_not_a); [ -n "$w" ]; do
    [ "$w" != "$last" ] && { log "(a) does not hold: $w"; last=$w; }
    [ $(( $(date +%s) - t0 )) -ge 10800 ] && { log "GIVING UP: (a) has not held for 3 hours"; exit 4; }
    sleep 15
  done
  { echo "-- gate for $PLATFORM round $ROUND, $(date -u +%FT%TZ), DENSITY ONLY (stack: containers labelled $STACK_LABEL${STACK_EXTRA_CG:+, cgroups $STACK_EXTRA_CG}, plus the host's dockerd and containerd)"
    echo "   density-only rule: (a) checks only benchmark units; the idle wait and (b) are skipped"
    echo "   load at start: $(cut -d' ' -f1-3 /proc/loadavg)"
    echo "   top 5 by current CPU at start (5-second top sample; pid nice %cpu command, whose):"
    head -5 "$SAMPLE" | while read -r pid ni cpu comm; do echo "     $pid $ni $cpu $comm, $(whose "$pid")"; done
    echo "   processes at nice 10 or above using CPU in that sample (they do not block (a)):"
    awk '$2>=10 && $3>0 {print "     " $0}' "$SAMPLE" | grep . || echo "     none"
  } >> "$O/platforms.txt"
  log "GO (density only): load at start $(load1)"
  exit 0
fi

# 5 minutes up and idle with no rounds running.
now=$(date +%s); [ "$now" -lt $((IDLE_SINCE + 300)) ] && { log "stack idle since $(date -d @"$IDLE_SINCE" '+%T'); waiting until 5 minutes have passed"; sleep $((IDLE_SINCE + 300 - now)); }

# The stack's idle CPU: its cgroups' usage over a minute with (a) holding throughout (3 hours at most
# waiting for (a) and a clean minute).
t0=$(date +%s); last=""; restarts=0
while :; do
  while w=$(why_not_a); [ -n "$w" ]; do
    [ "$w" != "$last" ] && { log "(a) does not hold: $w"; last=$w; }
    [ $(( $(date +%s) - t0 )) -ge 10800 ] && { log "GIVING UP: (a) has not held through an idle minute for 3 hours"; exit 4; }
    sleep 15
  done
  last=""
  SC=$(STACK_LABEL="$STACK_LABEL" "$T7/stack_cgroups.sh" "$PLATFORM") || { log "no STACK_CGROUPS for $PLATFORM"; exit 5; }
  s0=$(cpu_usec); n0=$(date +%s%N); broke=""
  while [ $(( $(date +%s%N) - n0 )) -lt 60000000000 ]; do
    w=$(why_not_a)
    [ -n "$w" ] && { broke=$w; break; }
  done
  s1=$(cpu_usec); n1=$(date +%s%N)
  if [ -n "$broke" ]; then
    restarts=$((restarts + 1)); log "(a) broke within the idle minute ($broke); the minute starts over"; continue
  fi
  # "ok <usec> <cgroups read> <absent at both reads>" or "redo <why>"
  verdict=$(awk -v a="$s0" -v b="$s1" 'BEGIN {
      n = split(a, x, "\n"); for (i = 1; i <= n; i++) { split(x[i], f, " "); v0[f[1]] = f[2] }
      m = split(b, y, "\n"); sum = 0; read = 0; absent = ""
      for (i = 1; i <= m; i++) {
        split(y[i], f, " "); c = f[1]; v1 = f[2]
        if (v0[c] == "-" && v1 == "-") { absent = absent c ","; continue }
        if (v0[c] == "-" || v1 == "-") { print "redo " c " went or appeared within the minute"; exit }
        if (v1 + 0 < v0[c] + 0) { print "redo " c " was made anew within the minute"; exit }
        sum += v1 - v0[c]; read++
      }
      printf "ok %d %d %s\n", sum, read, (absent == "" ? "none" : absent) }')
  case "$verdict" in
    ok*) break ;;
    *) restarts=$((restarts + 1)); log "idle minute void: ${verdict#redo }; the minute starts over" ;;
  esac
done
read -r _ DELTA NREAD ABSENT <<< "$verdict"
WINDOW=$(awk -v a="$n0" -v b="$n1" 'BEGIN{printf "%.3f", (b - a) / 1e9}')
IDLE=$(awk -v d="$DELTA" -v w="$WINDOW" 'BEGIN{printf "%.3f", d / w / 1e6}')
THRESH=$(awk -v i="$IDLE" 'BEGIN{printf "%.2f", 2 + i}')
IDLE_LOAD1=$(load1)
log "stack idle CPU $IDLE CPUs ($DELTA usec over $NREAD cgroups in $WINDOW s, (a) holding throughout, $restarts restarts); 1-minute load then $IDLE_LOAD1; threshold $THRESH"

# (b), with (a) re-checked at every reading; the 45 minutes count only time with (a) holding.
held_since=""; forced=""; last=""
while :; do
  w=$(why_not_a)
  if [ -n "$w" ]; then
    [ "$w" != "$last" ] && { log "(a) does not hold: $w"; last=$w; }
  else
    last=""; l=$(load1); [ -z "$held_since" ] && held_since=$(date +%s)
    awk -v l="$l" -v t="$THRESH" 'BEGIN{exit !(l < t)}' && break
    [ $(( $(date +%s) - held_since )) -ge 2700 ] && { forced="yes"; break; }
  fi
  sleep 15
done
{ echo "-- gate for $PLATFORM round $ROUND, $(date -u +%FT%TZ) (stack: containers labelled $STACK_LABEL${STACK_EXTRA_CG:+, cgroups $STACK_EXTRA_CG}, plus the host's dockerd and containerd)"
  echo "   stack idle CPU (after 5 min up and idle): $IDLE CPUs = the rise in cpu.stat usage_usec summed over the stack's $NREAD cgroups, $DELTA usec, / the $WINDOW s between the reads / 1e6; (a) held throughout that minute (minutes started over: $restarts); stack cgroups absent at both reads, counted 0: $ABSENT"
  echo "   stack cgroups: $SC"
  echo "   threshold 2 + idle CPU = $THRESH (1-minute load when the idle CPU was read: $IDLE_LOAD1, not used)"
  echo "   load at start: $(cut -d' ' -f1-3 /proc/loadavg)"
  [ -n "$forced" ] && echo "   RAN ANYWAY: (a) held but the 1-minute load stayed at or over $THRESH for 45 minutes"
  echo "   top 5 by current CPU at start (5-second top sample; pid nice %cpu command, whose):"
  head -5 "$SAMPLE" | while read -r pid ni cpu comm; do echo "     $pid $ni $cpu $comm, $(whose "$pid")"; done
  echo "   processes at nice 10 or above using CPU in that sample (they do not block (a)):"
  awk '$2>=10 && $3>0 {print "     " $0}' "$SAMPLE" | grep . || echo "     none"
} >> "$O/platforms.txt"
log "GO: load at start $(load1) (threshold $THRESH)${forced:+, run anyway after 45 minutes}"
