#!/usr/bin/env bash
# bench/competitive: oci_bench.py's throughput, sustained and lifecycle_nonet phases against a stand-in runtime,
# and box_bench.py's throughput against stand-in `hex` and `systemd-run`, off the bench host (the README's
# HEXBENCH_* overrides): no containers, cgroups or load, so its numbers mean nothing. It checks the control flow:
#   1. every lifecycle is counted once at each concurrency level (the stand-in logs each create);
#   2. a failing call ends its level, with its error recorded as the phase's;
#   3. no worker process outlives the phase (each carries a marker in its environment);
#   4. the sustained phase's windows add up to the lifecycles it ran, and a failure keeps its count;
#   5. lifecycle_nonet times every operation, under ids of its own;
#   6. box_bench.py's throughput runs each method's lifecycles once per level, a failing run ends its level
#      with its error, and no worker outlives the phase.
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

# 4: three sustained windows, then a failure in the second.
: > "$T/creates.log"
CHECK_LOG=$T/creates.log HEXBENCH_PHASES=sustained HEXBENCH_SUSTAINED=3000 HEXBENCH_CHECK_MARK=$marker \
  python3 -I "$HERE/oci_bench.py" crun "$T/sus.json" 1 0 "$L" 0 64 > "$T/sus.out"
check "sustained: 3 windows over 3,000 lifecycles, each created once" \
  "python3 -I -c 'import json,sys; s=json.load(open(sys.argv[1]))[\"phases\"][\"sustained\"]; sys.exit(0 if len(s[\"window_per_s\"])==3 and s[\"lifecycles\"]==3000 and s[\"last_over_first\"] else 1)' $T/sus.json && [ \$(sort -u $T/creates.log | wc -l) -eq 3000 ]"
CHECK_LOG=$T/creates.log FAIL_ID=crun-c32-001500 HEXBENCH_PHASES=sustained HEXBENCH_SUSTAINED=3000 HEXBENCH_CHECK_MARK=$marker \
  python3 -I "$HERE/oci_bench.py" crun "$T/susfail.json" 1 0 "$L" 0 64 > "$T/susfail.out"
check "sustained: the failure is the phase's error, with the lifecycles done before it" \
  "python3 -I -c 'import json,sys,re; e=json.load(open(sys.argv[1]))[\"errors\"].get(\"sustained\",\"\"); m=re.search(r\"after (\\d+) of 3000 lifecycles\", e); sys.exit(0 if m and 1000 <= int(m.group(1)) < 3000 and \"crun-c32-001500\" in e else 1)' $T/susfail.json"
check "no worker outlived the sustained runs" "[ \$(leftover) -eq 0 ]"

# 5: lifecycle_nonet times each operation.
: > "$T/creates.log"
CHECK_LOG=$T/creates.log HEXBENCH_PHASES=lifecycle_nonet python3 -I "$HERE/oci_bench.py" crun "$T/nonet.json" 20 2 "$L" 0 64 > "$T/nonet.out"
check "lifecycle_nonet: 20 timed samples of create, start, exec and delete, under ids of its own" \
  "python3 -I -c 'import json,sys; m=json.load(open(sys.argv[1]))[\"phases\"][\"lifecycle_nonet\"][\"ms\"]; sys.exit(0 if all(len(m[o])==20 for o in (\"create\",\"start\",\"exec\",\"delete\")) else 1)' $T/nonet.json && [ \$(grep -c '^crun-n' $T/creates.log) -eq 22 ]"
# 6: box_bench.py's throughput. The stand-ins log each run, under a lock so the count is exact, and fail run
# $FAIL_AT. Lifecycles per level is a multiple of 32, so each level runs exactly $L.
mkdir -p "$T/box/bin" "$T/box/user@0.service/s"
for b in hex systemd-run; do
  cat > "$T/box/bin/$b" <<'SH'
#!/bin/sh
exec 9>>"$CHECK_LOG.lock"; flock 9
n=$(($(wc -l < "$CHECK_LOG") + 1))
kind=$(basename "$0"); case " $* " in *" --scope "*) kind=scope ;; *" --wait "*) kind=service ;; esac
echo "$kind $n" >> "$CHECK_LOG"
[ "$n" = "${FAIL_AT:-0}" ] && exit 3
exit 0
SH
  chmod +x "$T/box/bin/$b"
done
boxrun() {  # boxrun <out.json> [FAIL_AT]
  : > "$T/box/runs.log"
  PATH=$T/box/bin:$PATH CHECK_LOG=$T/box/runs.log FAIL_AT=${2:-0} HEXBENCH_CHECK_MARK=$marker HEXBENCH_PHASES=throughput \
    HEXBENCH_SELF_CGROUP=$T/box/user@0.service/s HEXBENCH_USER_CGROUP=$T/box/user@0.service/s \
    python3 -I "$HERE/box_bench.py" "$1" 1 0 "$L" 0 64 "$T/box/bin/hex" > "$1.out"
}
boxrun "$T/box.json"
for m in hex scope service; do
  n=$(grep -c "^$m " "$T/box/runs.log" || true)
  check "box_bench $m: $L runs at each of 3 levels (ran $n)" "[ $n -eq $((3 * L)) ]"
done
check "box_bench: every level's JSON counts $L" \
  "python3 -I -c 'import json,sys; m=json.load(open(sys.argv[1]))[\"methods\"]; sys.exit(0 if all(m[k][\"phases\"][\"throughput\"][c][\"lifecycles\"]==$L for k in m for c in (\"1\",\"8\",\"32\")) and not any(m[k][\"errors\"] for k in m) else 1)' $T/box.json"
boxrun "$T/boxfail.json" $((L + L / 2))
check "box_bench: a failing run at concurrency 8 is hex's phase error, and concurrency 32 didn't run" \
  "python3 -I -c 'import json,sys; e=json.load(open(sys.argv[1]))[\"methods\"][\"hex\"][\"errors\"].get(\"throughput\",\"\"); sys.exit(0 if e.startswith(\"RuntimeError: concurrency 8:\") and \"exit 3\" in e else 1)' $T/boxfail.json && [ \$(grep -c '^hex ' $T/box/runs.log) -le $((2 * L)) ]"
check "no box_bench worker outlived its runs" "[ \$(leftover) -eq 0 ]"
[ "$fails" -eq 0 ] && echo "all checks passed" || { echo "$fails checks failed"; exit 1; }
