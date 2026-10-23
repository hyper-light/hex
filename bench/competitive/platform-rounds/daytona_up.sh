#!/bin/bash
# daytona_up.sh: no other benchmark unit, then compose up (unit hexbench-daytona-up), then /api/health within 20 minutes,
# then the stack's images and digests into $OUT/platforms.txt. Prints nothing secret.
T7=$HOME/.cache/hex-bench/t7; O=$HOME/.cache/hex-bench/out/20261023T025023Z; D=$HOME/.cache/hex-bench/daytona
systemctl --user list-units --no-legend --plain --state=active,activating 2>/dev/null | awk "{print \$1}" | grep -E "bench|routing-gate|big-goals-check" && { echo "another benchmark unit is active; not bringing the stack up"; exit 3; }
mv -f "$T7/daytona-up.log" "$T7/daytona-up.1.log" 2>/dev/null
systemd-run --user --unit=hexbench-daytona-up --collect --working-directory="$D" --setenv=PATH="$PATH" --setenv=LOG="$T7/daytona-up.log" \
  bash -c 'docker compose -p hexbench-daytona -f docker/docker-compose.yaml -f docker/docker-compose.hexbench.yaml up -d > "$LOG" 2>&1; rc=$?; echo "compose up exit $rc" >> "$LOG"; exit $rc' || exit 4
start=$(date +%s)
while [ "$(systemctl --user is-active hexbench-daytona-up 2>/dev/null)" = active ]; do
  [ $(( $(date +%s) - start )) -ge 1200 ] && { echo "TIMEOUT: compose up still running after 20 min"; exit 2; }; sleep 10; done
tail -1 "$T7/daytona-up.log"
grep -q 'compose up exit 0' "$T7/daytona-up.log" || { echo "compose up failed"; grep -iE 'error|denied' "$T7/daytona-up.log" | head -5; exit 5; }
until curl -sf -o /dev/null http://localhost:3000/api/health; do
  [ $(( $(date +%s) - start )) -ge 1200 ] && { echo "TIMEOUT: /api/health not answering after 20 min"; exit 6; }; sleep 10; done
echo "HEALTHY after $(( $(date +%s) - start ))s"
{ echo "== Daytona stack up $(date -u +%FT%TZ): /api/health answered after $(( $(date +%s) - start ))s"
  echo '$ docker compose -p hexbench-daytona images'
  (cd "$D" && docker compose -p hexbench-daytona -f docker/docker-compose.yaml -f docker/docker-compose.hexbench.yaml images)
  echo "RepoDigests:"
  for img in $(cd "$D" && docker compose -p hexbench-daytona -f docker/docker-compose.yaml -f docker/docker-compose.hexbench.yaml images -q | sort -u); do
    echo "  $(docker image inspect "$img" --format '{{.Id}} {{json .RepoTags}} {{json .RepoDigests}}')"; done
  echo; } >> "$O/platforms.txt"
echo recorded
