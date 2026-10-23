#!/bin/bash
# round_sampler.sh <unit> <stack label> <tag>: while the round's unit runs, takes a 5-second top sample about
# every 10 s and counts the samples with a process outside the stack at nice below 10 over 50% CPU (the
# referee's foreign samples). The kiln agent host (this session's parent) is tallied separately, not foreign.
# Writes "<foreign> <samples> <kiln-host>" to t7/sampler.<tag>.result and each foreign sample to its .log.
UNIT=$1; STACK_LABEL=$2; TAG=$3
T7=$HOME/.cache/hex-bench/t7; SAMPLE=$T7/sampler.sample; LOG=$T7/sampler.$TAG.log; RES=$T7/sampler.$TAG.result
source <(sed -n '/^KILN_HOST_PID=/p;/^whose()/,/^}/p' "$T7/platform_gate.sh")
: > "$LOG"; n=0; hit=0; kh=0
while [ "$(systemctl --user is-active "$UNIT" 2>/dev/null)" = active ]; do
  top -b -n 2 -d 5 -w 512 | awk '/^ *PID /{f++; next} f==2 && NF>=12 && $9>50 && $4<10 {print $1, $4, $9, $12}' > "$SAMPLE"
  [ "$(systemctl --user is-active "$UNIT" 2>/dev/null)" = active ] || break  # a sample must lie inside the round
  n=$((n + 1)); line=""; khs=""
  while read -r pid ni cpu comm; do
    w=$(whose "$pid")
    case "$w" in foreign:*) line="$line$comm($cpu%, ${w#foreign:}) " ;; kiln-host) khs="yes" ;; esac
  done < "$SAMPLE"
  [ -n "$khs" ] && kh=$((kh + 1))
  [ -n "$line" ] && { hit=$((hit + 1)); echo "$(date '+%T') $line" >> "$LOG"; }
  sleep 5
done
echo "$hit $n $kh" > "$RES"
