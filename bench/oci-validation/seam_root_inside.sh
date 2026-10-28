#!/usr/bin/env bash
# Inside the privileged container (seam_root.sh): every process moved out of the cgroup namespace's root,
# its controllers handed down, then each test binary run as root in a fresh cgroup of its own.
set -uo pipefail
CG=/sys/fs/cgroup
[ "$(id -u)" = 0 ] || { echo "refusing: not root" >&2; exit 2; }
mkdir -p "$CG/lab"
for p in $(cat "$CG/cgroup.procs"); do echo "$p" > "$CG/lab/cgroup.procs" 2>/dev/null || true; done
echo "+cpu +io +memory +pids" > "$CG/cgroup.subtree_control" || { echo "refusing: can't hand controllers down" >&2; exit 2; }
fails=0; n=0
for bin in "$@"; do
  n=$((n + 1)); name=$(basename "$bin"); mkdir -p "$CG/t$n"
  echo "=== $name (root, cgroup /t$n)"
  # The subshell joins the test's own cgroup, then becomes the test: its own cgroup holds it alone.
  ( echo "$BASHPID" > "$CG/t$n/cgroup.procs" && exec "$bin" --test-threads=1 --nocapture ) 2>&1 | tail -40
  rc=${PIPESTATUS[0]}
  if [ "$rc" = 0 ]; then echo "PASS $name"; else echo "FAIL $name (exit $rc)"; fails=$((fails + 1)); fi
done
echo "seam as root: $((n - fails)) of $n test binaries passed"
[ "$fails" = 0 ]
