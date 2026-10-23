#!/bin/bash
# run_refereed.sh <platform> <round> <first try> <idle-since epoch> <stack label> <bench command>
# One refereed round (T7 agent, 2026-10-23): each try passes platform_gate.sh, runs <bench command> (bash -c, with
# $OUTF the round file, $LOG its log and $STACK_CGROUPS from stack_cgroups.sh) as unit
# hexbench-<platform>-r<N>-t<try>, and is sampled by round_sampler.sh; the referee's figure goes into the round's
# JSON as "referee". A try counts if under 10% of its samples show foreign load; otherwise it is renamed to
# foreignload.try<t>.platform.<platform>.r<N>.json and the next try runs, up to try 3. If none is clean, the try
# with the lowest foreign share becomes the round, labelled with that share. A try whose stack cgroup was made
# anew mid-run (a restarted container) is void (restarted.try<t>...) and the next try runs; a try whose density
# found a sandbox's own processes outside STACK_CGROUPS stops the referee (stackmiss.try<t>...), so the list
# can be fixed. Tries already made can be passed as PRIOR="<try>:<foreign>:<samples> ...".
P=${1:?platform}; N=${2:?round}; FIRST=${3:?first try}; IDLE=${4:?idle since}; LABEL=${5:?stack label}; CMD=${6:?bench command}
T7=$HOME/.cache/hex-bench/t7; O=$HOME/.cache/hex-bench/out/20261023T025023Z; F=platform.$P.r$N.json
rec() { echo "$*" >> "$O/platforms.txt"; }
best_t=""; best_h=""; best_n=""
lower() { awk -v a="$1" -v b="$2" -v c="$3" -v d="$4" 'BEGIN{exit !(b > 0 && (d == 0 || a/b < c/d))}'; }  # a/b < c/d
for p in ${PRIOR:-}; do IFS=: read -r pt ph pn <<< "$p"
  if [ -z "$best_t" ] || lower "$ph" "$pn" "$best_h" "$best_n"; then best_t=$pt; best_h=$ph; best_n=$pn; fi; done
for t in $(seq "$FIRST" 3); do
  [ -e "$O/$F" ] && { echo "refusing: $F already exists"; exit 5; }
  "$T7/platform_gate.sh" "$P" "$N (try $t)" "$IDLE" "$LABEL" || exit 3
  SC=$(STACK_LABEL="$LABEL" "$T7/stack_cgroups.sh" "$P") || { echo "no STACK_CGROUPS for $P"; exit 8; }
  unit="hexbench-$P-r$N-t$t"; tag="$P.r$N.t$t"
  systemd-run --user --unit="$unit" --collect --setenv=PATH="$PATH" --setenv=LOG="$T7/$P-r$N-t$t.log" \
    --setenv=OUTF="$O/$F" --setenv=STACK_CGROUPS="$SC" \
    bash -c "$CMD"'; rc=$?; echo "platform_bench exit $rc" >> "$LOG"; exit $rc' || exit 4
  echo "$(date '+%T') started $unit at load $(cut -d' ' -f1 /proc/loadavg); STACK_CGROUPS has $(echo "$SC" | wc -w) cgroups"
  "$T7/round_sampler.sh" "$unit" "$LABEL" "$tag"
  while [ "$(systemctl --user is-active "$unit" 2>/dev/null)" = active ]; do sleep 5; done
  IDLE=$(date +%s)
  read -r h n kh < "$T7/sampler.$tag.result"
  share=$(awk -v h="$h" -v n="$n" 'BEGIN{printf "%.1f", n ? 100*h/n : 100}')
  if [ ! -e "$O/$F" ]; then
    rec "== referee: $P round $N try $t: no round file (see t7/$P-r$N-t$t.log); foreign $h of $n samples, kiln host $kh"
    echo "$(date '+%T') try $t: no round file"; continue
  fi
  # The referee's figure into the round's JSON; then the two cgroup checks.
  verdict=$(python3 -I - "$O/$F" "$n" "$h" "$kh" "$t" <<'PY'
import json, re, sys
path, n, h, kh, t = sys.argv[1], *map(int, sys.argv[2:])
d = json.load(open(path))
d["referee"] = {"samples": n, "foreign": h, "kiln_host": kh, "try": t}
with open(path, "w") as f:
    json.dump(d, f)
if any("was made anew mid-run" in str(v) for v in d.get("errors", {}).values()):
    print("restarted"); sys.exit()
dens = d.get("phases", {}).get("density") or {}
new = dens.get("new_processes_outside_stack") or {}
count = dens.get("count") or 0
sandboxish = {c: k for c, k in new.items() if re.match(r"^(sleep|firecracker|containerd-shim.*|pause|daytona.*|runc|conmon)$", c)}
print("stackmiss " + json.dumps(sandboxish) if sum(sandboxish.values()) >= max(5, 0.1 * count) else "ok")
PY
)
  newproc=$(python3 -I -c 'import json,sys; d=json.load(open(sys.argv[1])); print(json.dumps((d.get("phases",{}).get("density") or {}).get("new_processes_outside_stack")))' "$O/$F")
  case "$verdict" in
    restarted)
      mv -n "$O/$F" "$O/restarted.try$t.$F"
      rec "== referee: $P round $N try $t VOID: a stack cgroup was made anew mid-run (a container restarted), so its CPU and memory are unknown; kept as restarted.try$t.$F; foreign $h of $n samples ($share%)"; rec ""
      echo "$(date '+%T') VOID try $t: a stack cgroup was made anew mid-run"; continue ;;
    stackmiss*)
      mv -n "$O/$F" "$O/stackmiss.try$t.$F"
      rec "== referee: $P round $N try $t STOPPED: density found a sandbox's own processes outside STACK_CGROUPS (${verdict#stackmiss }), so the list misses part of the stack; kept as stackmiss.try$t.$F; new processes outside the stack: $newproc"; rec ""
      echo "$(date '+%T') STACKMISS try $t: ${verdict#stackmiss }; new outside: $newproc"; exit 7 ;;
  esac
  if awk -v h="$h" -v n="$n" 'BEGIN{exit !(n > 0 && h/n < 0.10)}'; then
    rec "== referee: $P round $N try $t ACCEPTED: foreign load in $h of $n samples ($share%), under 10%; kiln host over 50% in $kh samples (not counted); new processes outside the stack at density: $newproc"
    [ "$h" -gt 0 ] && sed 's/^/     /' "$T7/sampler.$tag.log" >> "$O/platforms.txt"; rec ""
    echo "$(date '+%T') ACCEPTED $F try $t: foreign $h/$n ($share%), kiln host $kh; new outside: $newproc"; exit 0
  fi
  mv -n "$O/$F" "$O/foreignload.try$t.$F"
  rec "== referee: $P round $N try $t REJECTED: foreign load in $h of $n samples ($share%); kiln host over 50% in $kh samples (not counted); kept as foreignload.try$t.$F; new processes outside the stack at density: $newproc"
  sed 's/^/     /' "$T7/sampler.$tag.log" | head -15 >> "$O/platforms.txt"; rec ""
  echo "$(date '+%T') REJECTED try $t: foreign $h/$n ($share%); new outside: $newproc"
  if [ -z "$best_t" ] || lower "$h" "$n" "$best_h" "$best_n"; then best_t=$t; best_h=$h; best_n=$n; fi
done
[ -z "$best_t" ] && { rec "== referee: $P round $N: no try produced a usable round file"; echo "NO USABLE ROUND FILE in any try"; exit 6; }
[ -e "$O/foreignload.try$best_t.$F" ] || { rec "== referee: $P round $N: none of 3 tries was clean, and the lowest-share try ($best_t) predates the stack-cgroup patch, so no round is kept"; echo "NONE CLEAN; lowest-share try $best_t is not usable"; exit 6; }
mv -n "$O/foreignload.try$best_t.$F" "$O/$F"
share=$(awk -v h="$best_h" -v n="$best_n" 'BEGIN{printf "%.1f", 100*h/n}')
rec "== referee: $P round $N: none of 3 tries was clean; try $best_t, the lowest foreign share ($best_h of $best_n samples, $share%), is kept as $F, LABELLED: measured with foreign load in $share% of its samples"; rec ""
echo "$(date '+%T') KEPT LOWEST $F = try $best_t: foreign $best_h/$best_n ($share%)"
