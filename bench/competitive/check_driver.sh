#!/usr/bin/env bash
# bench/competitive: oci_bench.py's throughput phase against a stand-in runtime, off the bench host (the README's
# HEXBENCH_* overrides): no containers, cgroups or load, so its numbers mean nothing. It checks the control flow:
#   1. every lifecycle is counted once at each concurrency level (the stand-in logs each create);
#   2. a failing call ends its level, with its error recorded as the phase's;
#   3. no worker process outlives the phase (each carries a marker in its environment).
#   check_driver.sh [lifecycles per level (64)]
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
L=${1:-64}
T=$(mktemp -d "$HOME/.cache/hexbench-driver-check.XXXXXX")
trap 'rm -rf -- "${T:?}"' EXIT
mkdir -p "$T/bin" "$T/cg" "$T/err"
echo "usage_usec 0" > "$T/cg/cpu.stat"
# The stand-in: `crun --root R <op> ...`. It logs each create's id, and fails the start of $FAIL_ID if set.
cat > "$T/bin/crun" <<'SH'
#!/bin/sh
op=$3
case $op in
  create) echo "$6" >> "$CHECK_LOG" ;;
  start) [ "$4" = "${FAIL_ID:-}" ] && { echo "the stand-in fails $4" >&2; exit 3; } ;;
esac
exit 0
SH
chmod +x "$T/bin/crun"
export HEXBENCH_BIN=$T/bin HEXBENCH_CGROUP=$T/cg HEXBENCH_ERR=$T/err HEXBENCH_PHASES=throughput
fails=0
check() { if eval "$2"; then echo "ok: $1"; else echo "FAIL: $1"; fails=$((fails + 1)); fi; }
marker=hexbench-check-$$-$RANDOM
leftover() {  # processes still carrying this run's marker
  grep -lsz "HEXBENCH_CHECK_MARK=$marker" /proc/[0-9]*/environ 2>/dev/null | wc -l
}

# 1 and 3: a clean run.
CHECK_LOG=$T/creates.log HEXBENCH_CHECK_MARK=$marker python3 -I "$HERE/oci_bench.py" crun "$T/ok.json" 1 0 "$L" 0 64 > "$T/ok.out"
check "the phase ran without an error" "python3 -I -c 'import json,sys; sys.exit(1 if json.load(open(sys.argv[1]))[\"errors\"] else 0)' $T/ok.json"
for c in 1 8 32; do
  n=$(grep -c "^crun-c$(printf %02d $c)-" "$T/creates.log" || true)
  u=$(grep "^crun-c$(printf %02d $c)-" "$T/creates.log" | sort -u | wc -l)
  check "concurrency $c: $L lifecycles, each once (created $n, distinct $u)" "[ $n -eq $L ] && [ $u -eq $L ]"
done
check "no worker outlived the clean run" "[ \$(leftover) -eq 0 ]"

# 2 and 3: the start of lifecycle 10 at concurrency 8 fails.
: > "$T/creates.log"
CHECK_LOG=$T/creates.log FAIL_ID=crun-c08-000010 HEXBENCH_CHECK_MARK=$marker \
  python3 -I "$HERE/oci_bench.py" crun "$T/fail.json" 1 0 "$L" 0 64 > "$T/fail.out"
check "the failure is the phase's error, at concurrency 8" \
  "python3 -I -c 'import json,sys; e=json.load(open(sys.argv[1]))[\"errors\"].get(\"throughput\",\"\"); sys.exit(0 if e.startswith(\"concurrency 8:\") and \"the stand-in fails crun-c08-000010\" in e else 1)' $T/fail.json"
check "concurrency 32 didn't run after the failing level" "! grep -q '^crun-c32-' $T/creates.log"
check "no worker outlived the failing run" "[ \$(leftover) -eq 0 ]"
[ "$fails" -eq 0 ] && echo "all checks passed" || { echo "$fails checks failed"; exit 1; }
