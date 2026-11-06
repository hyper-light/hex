#!/usr/bin/env bash
# HEX-INST-3 (native plan step 6): the macOS installer's directory writes are tried once and a failure is
# refused naming dscl's own error, never retried by a picked count (Ada's rule, 2026-11-06; the lead: the
# eNotYetImplemented that Nix retries has never been seen here). dscl runs by absolute path, here a stub.
# Pure: runs anywhere bash does, `bash bench/install/macos_dscl_test.sh`, exit 0 when every case holds.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../../scripts/install/macos.sh
. "$HERE/../../scripts/install/macos.sh"
set +e

FAILS=0
STUB=$(mktemp -d "${TMPDIR:-/tmp}/hex-dscl-test.XXXXXX") || exit 1
trap 'rm -rf -- "${STUB:?}"' EXIT
cat >"$STUB/dscl" <<'EOF'
#!/usr/bin/env bash
echo call >>"$(dirname "$0")/calls"
[ -e "$(dirname "$0")/fail" ] || exit 0
echo "<dscl_cmd> DS Error: -14988 (eNotYetImplemented)" >&2
exit 1
EOF
chmod +x "$STUB/dscl"
DSCL=$STUB/dscl

: >"$STUB/calls"
out=$(dscl_write -create /Users/_hexbox1 2>&1)
st=$?
if [ "$st" = 0 ] && [ "$(wc -l <"$STUB/calls" | tr -d " ")" = 1 ]; then echo "PASS: a write that succeeds is made once"; else echo "FAIL: success: status $st, $(wc -l <"$STUB/calls" | tr -d " ") calls, $out"; FAILS=1; fi

: >"$STUB/calls"
touch "$STUB/fail"
out=$(dscl_write -create /Users/_hexbox1 2>&1)
st=$?
calls=$(wc -l <"$STUB/calls" | tr -d " ")
if [ "$st" != 0 ] && [ "$calls" = 1 ]; then echo "PASS: a failing write is tried once"; else echo "FAIL: failure: status $st, $calls calls"; FAILS=1; fi
if grep -q 'eNotYetImplemented' <<<"$out" && grep -q -- '-create /Users/_hexbox1' <<<"$out"; then echo "PASS: the refusal names the write and dscl's error"; else echo "FAIL: refusal text: $out"; FAILS=1; fi
exit "$FAILS"
