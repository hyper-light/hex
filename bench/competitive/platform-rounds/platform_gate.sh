#!/bin/bash
# platform_gate.sh <platform> <round> <idle-since epoch> <stack label>: the platform-pass gate
# (T7 agent, 2026-10-23, third revision). <stack label> is the docker label naming the stack's containers,
# e.g. com.docker.compose.project=hexbench-daytona or io.x-k8s.kind.cluster=hexbench.
#  (a) no other benchmark unit is active, and no process outside the platform's stack runs at a nice below
#      10 using over 50% CPU, from a 5-second sample (the second frame of `top -b -n 2 -d 5`). The stack is
#      the processes of the labelled containers (anything under their docker-<id>.scope cgroups, nested
#      container engines included) and the host's dockerd and containerd (shims too), which serve them.
#      A busy process that exits before its cgroup can be read counts as outside the stack. Processes at
#      nice 10 or above never block (a); each one using CPU is listed in the round's record.
#  (b) the 1-minute load is under 2 plus the stack's idle load: the 1-minute load once the stack has sat up
#      and idle for 5 minutes with no rounds running, read at a moment when (a) holds.
# With (a) holding and (b) not, it waits up to 45 minutes, then lets the round run anyway, saying so.
# Appends the round's idle load, start load, top 5 processes and niced CPU users to $OUT/platforms.txt.
PLATFORM=${1:?platform}; ROUND=${2:?round}; IDLE_SINCE=${3:?idle since}; STACK_LABEL=${4:?stack label}
O=$HOME/.cache/hex-bench/out/20261023T025023Z; SAMPLE=$HOME/.cache/hex-bench/t7/gate.sample  # overwritten by each reading
load1() { cut -d' ' -f1 /proc/loadavg; }
sample() {  # the second frame of a 5-second top sample, as "pid ni %cpu command" lines, busiest first
  top -b -n 2 -d 5 -w 512 | awk '/^ *PID /{f++; next} f==2 && NF>=12 {print $1, $4, $9, $12}' | sort -k3,3 -gr > "$SAMPLE"
}
KILN_HOST_PID=185421  # the kiln agent host, this session's own parent: tallied separately, never foreign
whose() {  # "stack", "stack-infra", "kiln-host" or "foreign:<cgroup tail>" for a pid
  local cg ids
  [ "$1" = "$KILN_HOST_PID" ] && { echo kiln-host; return; }
  cg=$(cut -d: -f3- "/proc/$1/cgroup" 2>/dev/null) || { echo "foreign:exited"; return; }
  [ -z "$cg" ] && { echo "foreign:exited"; return; }
  ids=$(docker ps -q --no-trunc --filter "label=$STACK_LABEL" 2>/dev/null)
  for id in $ids; do case "$cg" in *"docker-$id.scope"*|*"/docker/$id"*) echo stack; return ;; esac; done
  case "$cg" in /system.slice/docker.service*|/system.slice/containerd.service*) echo stack-infra; return ;; esac
  echo "foreign:${cg##*/}"
}
why_not_a() {  # prints why (a) fails, nothing when it holds; takes a fresh sample
  local units busy pid ni cpu comm w
  sample
  units=$(systemctl --user list-units --no-legend --plain --state=active,activating,deactivating 2>/dev/null \
    | awk '{print $1}' | grep -E 'bench|routing-gate|big-goals-check' | tr '\n' ' ')
  busy=""
  while read -r pid ni cpu comm; do
    awk -v c="$cpu" -v n="$ni" 'BEGIN{exit !(c > 50 && n < 10)}' || continue
    w=$(whose "$pid")
    case "$w" in foreign:*) busy="$busy$comm pid $pid nice $ni $cpu% (${w#foreign:}) " ;; esac
  done < "$SAMPLE"
  [ -n "$units" ] && printf 'benchmark units active: %s' "$units"
  [ -n "$busy" ] && printf 'busy processes outside the stack: %s' "$busy"
}
log() { echo "$(date '+%T') $*"; }

# 5 minutes up and idle with no rounds running.
now=$(date +%s); [ "$now" -lt $((IDLE_SINCE + 300)) ] && { log "stack idle since $(date -d @"$IDLE_SINCE" '+%T'); waiting until 5 minutes have passed"; sleep $((IDLE_SINCE + 300 - now)); }

# The idle load, read when (a) holds (3 hours at most waiting for (a)).
t0=$(date +%s); last=""
while w=$(why_not_a); [ -n "$w" ]; do
  [ "$w" != "$last" ] && { log "(a) does not hold: $w"; last=$w; }
  [ $(( $(date +%s) - t0 )) -ge 10800 ] && { log "GIVING UP: (a) has not held for 3 hours"; exit 4; }
  sleep 15
done
IDLE=$(load1); THRESH=$(awk -v i="$IDLE" 'BEGIN{printf "%.2f", 2 + i}')
log "idle load $IDLE with (a) holding; threshold $THRESH"

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
{ echo "-- gate for $PLATFORM round $ROUND, $(date -u +%FT%TZ) (stack: containers labelled $STACK_LABEL, plus the host's dockerd and containerd)"
  echo "   stack idle load (1-min, after 5 min up and idle, (a) holding): $IDLE; threshold 2 + idle = $THRESH"
  echo "   load at start: $(cut -d' ' -f1-3 /proc/loadavg)"
  [ -n "$forced" ] && echo "   RAN ANYWAY: (a) held but the 1-minute load stayed at or over $THRESH for 45 minutes"
  echo "   top 5 by current CPU at start (5-second top sample; pid nice %cpu command, whose):"
  head -5 "$SAMPLE" | while read -r pid ni cpu comm; do echo "     $pid $ni $cpu $comm, $(whose "$pid")"; done
  echo "   processes at nice 10 or above using CPU in that sample (they do not block (a)):"
  awk '$2>=10 && $3>0 {print "     " $0}' "$SAMPLE" | grep . || echo "     none"
} >> "$O/platforms.txt"
log "GO: load at start $(load1) (threshold $THRESH)${forced:+, run anyway after 45 minutes}"
