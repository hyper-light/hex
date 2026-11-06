#!/usr/bin/env bash
# HEX-INST-3 (native plan step 6): the macOS installer waits on the helper's start by facts at both ends,
# the lead's #4 under Ada's rule (2026-11-06). Success is the socket accepting a connect; failure is
# launchd reporting the job exited, named with its status; only "running but never accepting" is bounded,
# at twice the measured start (bench/native/macos/records/agentstart-40454fe-20261106T150233Z.txt). Pure:
# launchctl and the connect probe are stubs. `bash bench/install/macos_wait_test.sh`, exit 0 when all hold.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../../scripts/install/macos.sh
. "$HERE/../../scripts/install/macos.sh"
set +e

FAILS=0
STUB=$(mktemp -d "${TMPDIR:-/tmp}/hex-wait-test.XXXXXX") || exit 1
trap 'rm -rf -- "${STUB:?}"' EXIT
cat >"$STUB/launchctl" <<'EOF'
#!/usr/bin/env bash
cat "$(dirname "$0")/print"
EOF
chmod +x "$STUB/launchctl"
LAUNCHCTL=$STUB/launchctl
running() { printf '\tstate = running\n\tlast exit code = (never exited)\n' >"$STUB/print"; }
exited() { printf '\tstate = not running\n\tlast exit code = %s\n' "$1" >"$STUB/print"; }

running
helper_accepts() { return 0; }
out=$(wait_serving 2>&1)
if [ $? = 0 ] && grep -q 'accepted after' <<<"$out"; then echo "PASS: a socket that accepts is the start"; else echo "FAIL: accepting: $out"; FAILS=1; fi

exited 78
helper_accepts() { return 1; }
out=$(wait_serving 2>&1)
if [ $? != 0 ] && grep -q 'exited' <<<"$out" && grep -q '78' <<<"$out"; then echo "PASS: a job launchd reports exited is refused with its status"; else echo "FAIL: exited: $out"; FAILS=1; fi

running
t0=$(perl -MTime::HiRes=time -e 'printf "%.6f", time')
out=$(wait_serving 2>&1)
st=$?
t1=$(perl -MTime::HiRes=time -e 'printf "%.6f", time')
waited=$(perl -e "printf '%.3f', $t1 - $t0")
if [ "$st" != 0 ] && grep -q 'never accepted' <<<"$out" && perl -e "exit !($waited >= $START_BOUND_S)"; then echo "PASS: running but never accepting is refused after the bound (${waited} s)"; else echo "FAIL: never accepting: status $st after $waited s: $out"; FAILS=1; fi

if [ "$START_BOUND_S" = 0.075360 ]; then echo "PASS: the bound is twice the measured start's max, 0.037680 s"; else echo "FAIL: bound $START_BOUND_S"; FAILS=1; fi
exit "$FAILS"
