#!/usr/bin/env bash
# Runs in the privileged container (run.sh): moves every process out of the cgroup namespace's root so
# the root can hand its controllers down (cgroup v2's no-internal-processes rule), then runs each test. Rootless, in a
# delegated systemd --user scope (rootless_validate.sh), CGROUP_ROOT is the scope's cgroup, split the same way.
# results.tsv: name, pass|fail|skip, the first failing TAP line, the checks the suite skipped.
set -uo pipefail
C=${CGROUP_ROOT:-/sys/fs/cgroup}
mkdir -p "$C/lab"
for p in $(cat "$C/cgroup.procs"); do echo "$p" > "$C/lab/cgroup.procs" 2>/dev/null; done
for c in $(cat "$C/cgroup.controllers"); do echo "+$c" > "$C/cgroup.subtree_control" 2>/dev/null; done
# Rows go to a local file and are copied to $OUT once: appends over the guest's 9p share (vm7_lab.sh,
# cache=loose) came back repeated, 1711 rows for 58 tests (aa_lab.sh, 2026-11-05).
RESULTS=$(mktemp)
cd "$RT" || exit 1
# TMPDIR: the container's /tmp, unless a rootless run names one off the host's shared /tmp (rootless_validate.sh).
export RUNTIME=${RUNTIME:-/opt/runtime} TMPDIR=${TMPDIR:-/tmp}
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
  printf '%s\t%s\t%s\t%s\n' "$name" "$result" "$first" "$skipped" >> "$RESULTS"
done
cat "$RESULTS" > "$OUT/results.tsv"
printf 'pass %s fail %s skip %s\n' "$(grep -c $'\tpass\t' "$RESULTS")" \
  "$(grep -c $'\tfail\t' "$RESULTS")" "$(grep -c $'\tskip\t' "$RESULTS")"
