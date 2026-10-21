#!/usr/bin/env bash
# Runs in the privileged container (run.sh): moves every process out of the cgroup namespace's root so
# the root can hand its controllers down (cgroup v2's no-internal-processes rule), then runs each test.
# results.tsv: name, pass|fail|skip, the first failing TAP line, the checks the suite skipped.
set -uo pipefail
mkdir -p /sys/fs/cgroup/lab
for p in $(cat /sys/fs/cgroup/cgroup.procs); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null; done
for c in $(cat /sys/fs/cgroup/cgroup.controllers); do echo "+$c" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null; done
: > "$OUT/results.tsv"
cd "$RT" || exit 1
export RUNTIME=${RUNTIME:-/opt/runtime} TMPDIR=/tmp
for dir in validation/*/; do
  name=$(basename "$dir")
  [ -x "validation/$name/$name.t" ] || continue
  log="$OUT/$name.log"
  timeout 60 "validation/$name/$name.t" > "$log" 2>&1
  code=$?
  first=$(grep -m1 '^not ok' "$log" | tr '\t' ' ')
  # TAP: a test passes when it ran checks, none failed and it exited 0; "ok N # SKIP" lines are
  # checks the suite itself declares not applicable, counted in the fourth column. A plan of
  # "1..0 # SKIP" is a test that skipped entirely.
  skipped=$(grep -cE '^ok [0-9]+ # (SKIP|skip)' "$log")
  if grep -qE '^1\.\.0 # (SKIP|skip)' "$log" && ! grep -q '^not ok' "$log"; then
    result=skip
  elif [ "$code" -eq 0 ] && [ -z "$first" ] && grep -q '^ok' "$log"; then
    result=pass
  else
    result=fail
    [ -n "$first" ] || first="exit $code: $(tail -n1 "$log" | tr '\t' ' ')"
  fi
  printf '%s\t%s\t%s\t%s\n' "$name" "$result" "$first" "$skipped" >> "$OUT/results.tsv"
done
printf 'pass %s fail %s skip %s\n' "$(grep -c $'\tpass\t' "$OUT/results.tsv")" \
  "$(grep -c $'\tfail\t' "$OUT/results.tsv")" "$(grep -c $'\tskip\t' "$OUT/results.tsv")"
