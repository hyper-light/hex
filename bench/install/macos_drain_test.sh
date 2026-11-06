#!/usr/bin/env bash
# CLAUDE.md §4 and Ada's rule (2026-11-06): the macOS installer's uninstall waits for a pool user's processes
# to end after SIGKILL by the fact (pgrep finds none), bounded by the measured kill-to-empty envelope doubled
# for the user's size, the bound hex_core::drain gives the helper and the process class
# (bench/native/macos/records/killscale-a84e094-20261106T160339Z.txt). Pure: pgrep and pkill are stubs.
# `bash bench/install/macos_drain_test.sh`, exit 0 when every case holds.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../../scripts/install/macos.sh
. "$HERE/../../scripts/install/macos.sh"
set +e

FAILS=0
check() { if [ "$2" = "$3" ]; then echo "PASS: $1"; else echo "FAIL: $1: want $3, got $2"; FAILS=1; fi; }

# The same envelope as hex_core::drain::bound: 2 x (0.883 ms + 0.298 ms per member past the first).
check "one member's bound" "$(drain_bound_s 1)" 0.001766
check "32 members' bound" "$(drain_bound_s 32)" 0.020242
check "6,000 members' bound" "$(drain_bound_s 6000)" 3.577170
check "an empty listing gets one member's bound" "$(drain_bound_s 0)" 0.001766

STUB=$(mktemp -d "${TMPDIR:-/tmp}/hex-drain-test.XXXXXX") || exit 1
trap 'rm -rf -- "${STUB:?}"' EXIT
cat >"$STUB/pgrep" <<'EOF'
#!/usr/bin/env bash
n=$(cat "$(dirname "$0")/left")
for ((i = 0; i < n; i++)); do echo $((1000 + i)); done
[ "$n" -gt 0 ]
EOF
cat >"$STUB/pkill" <<'EOF'
#!/usr/bin/env bash
[ -e "$(dirname "$0")/stuck" ] || echo 0 >"$(dirname "$0")/left"
EOF
chmod +x "$STUB/pgrep" "$STUB/pkill"
PGREP=$STUB/pgrep
PKILL=$STUB/pkill

echo 3 >"$STUB/left"
end_pool_processes 7000 2>/dev/null
check "a user whose processes end is emptied" "$?" 0

echo 3 >"$STUB/left"
touch "$STUB/stuck"
out=$(end_pool_processes 7000 2>&1)
st=$?
check "a user whose processes don't end is refused" "$st" 1
if grep -q 'still holds' <<<"$out" && grep -q '0.002958' <<<"$out"; then echo "PASS: the refusal names the user and its bound"; else echo "FAIL: refusal: $out"; FAILS=1; fi
exit "$FAILS"
