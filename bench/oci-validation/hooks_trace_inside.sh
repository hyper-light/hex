#!/usr/bin/env bash
# Inside hooks_trace.sh's container. Checks, each a line "PASS|FAIL <what>" in $OUT/checks:
# 1. A hookless create through each binary writes no `H` on the init's sync pipe, and the after binary's
#    sync-pipe lines are the before binary's (each `P`, `R` line, in order).
# 2. A hooked create through the after binary writes `H` (the trace sees the stop it looks for: not vacuous).
# 3. The before binary reads and deletes a container the after binary made with a startContainer hook, its
#    state carrying the startContainer field (an old reader of the new state).
# strace's per-call counts of each hookless create are kept beside them ($OUT/<binary>.plain.count).
set -uo pipefail
CG=/sys/fs/cgroup
mkdir -p "$CG/lab"
for p in $(cat "$CG/cgroup.procs"); do echo "$p" > "$CG/lab/cgroup.procs" 2>/dev/null; done
for c in $(cat "$CG/cgroup.controllers"); do echo "+$c" > "$CG/cgroup.subtree_control" 2>/dev/null; done
export HEX_OCI_ROOT=/run/hex-oci
: > "$OUT/checks"
check() { if eval "$2"; then echo "PASS $1" | tee -a "$OUT/checks"; else echo "FAIL $1" | tee -a "$OUT/checks"; fi; }
W=$(mktemp -d); R="$W/rootfs"; mkdir -p "$R"/{proc,dev,tmp,usr}
for d in bin sbin lib lib64; do [ -L "/$d" ] && ln -s "$(readlink "/$d" | sed 's|^/||')" "$R/$d"; done
bundle() { # bundle <dir> <hooks json>
  mkdir -p "$1"; cat > "$1/config.json" <<JSON
{"ociVersion":"1.3.0","root":{"path":"$R"},
 "process":{"cwd":"/","args":["/usr/bin/sleep","600"],"env":["PATH=/usr/bin:/bin"],"user":{"uid":0,"gid":0},"noNewPrivileges":true},
 "mounts":[{"destination":"/proc","type":"proc","source":"proc"},
           {"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755"]}],
 "linux":{"namespaces":[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"}]},
 "hooks":$2}
JSON
}
bundle "$W/plain" '{}'
bundle "$W/hooked" '{"createContainer":[{"path":"/usr/bin/true"}],"startContainer":[{"path":"/usr/bin/true"}]}'
# The sync pipe's messages: the init's and the shim's writes of a single line, one of U, P<n>, H, R or E.
lines() { grep -oE 'write\([0-9]+, "(U|P[0-9]*|H|R|E[^"]*)\\n", [0-9]+\)' "$1" | sed -E 's/write\([0-9]+, "([A-Z])[^"]*\\n".*/\1/' | paste -sd' ' -; }
# traced <id> <strace output> <strace flags> -- <hex> <bundle>: the create under strace -f, in the background, since
# the init it makes stays at the exec FIFO and strace -f waits for every process it follows. Waits (at most 10 s, on
# the fact) for the create's state, so the create has returned; the caller's delete then ends the init and strace.
traced() {
  local id=$1 out=$2 flags=$3 hex=$5 bundle=$6
  /opt/strace -f -qq $flags -o "$out" "$hex" oci create --bundle "$bundle" "$id" & STRACE=$!
  for _ in $(seq 1 100); do grep -qs '"status":"created"' "$HEX_OCI_ROOT/$id/state.json" && return 0; sleep 0.1; done
  echo "the create of $id never reached created" | tee -a "$OUT/checks"; return 1
}
for b in before after; do
  traced "plain-$b" "$OUT/$b.plain.trace" "-e trace=write -s 32" -- "/opt/$b-hex" "$W/plain"
  "/opt/$b-hex" oci delete --force "plain-$b"; wait "$STRACE"
  echo "$b hookless create: sync lines $(lines "$OUT/$b.plain.trace")" | tee -a "$OUT/checks"
  traced "count-$b" "$OUT/$b.plain.count" "-c" -- "/opt/$b-hex" "$W/plain"
  "/opt/$b-hex" oci delete --force "count-$b"; wait "$STRACE"
done
# Each "no H" holds only over a trace that saw the create's ready line: a missing trace is a failure, not a pass.
check "a hookless create writes R and no H (before)" 'lines "$OUT/before.plain.trace" | grep -qw R && ! lines "$OUT/before.plain.trace" | grep -qw H'
check "a hookless create writes R and no H (after)" 'lines "$OUT/after.plain.trace" | grep -qw R && ! lines "$OUT/after.plain.trace" | grep -qw H'
check "a hookless create's sync lines are the same before and after" '[ -n "$(lines "$OUT/before.plain.trace")" ] && [ "$(lines "$OUT/before.plain.trace")" = "$(lines "$OUT/after.plain.trace")" ]'
traced hooked "$OUT/after.hooked.trace" "-e trace=write -s 32" -- /opt/after-hex "$W/hooked"
cp "$HEX_OCI_ROOT/hooked/state.json" "$OUT/hooked.state.json" 2>/dev/null
echo "after hooked create: sync lines $(lines "$OUT/after.hooked.trace")" | tee -a "$OUT/checks"
check "the after binary's state carries the startContainer field" 'grep -q "\"startContainer\":" "$OUT/hooked.state.json"'
/opt/before-hex oci state hooked > "$OUT/hooked.state-by-before.json" 2> "$OUT/hooked.state-by-before.err"
check "the before binary reads it (status created)" 'grep -q "\"status\":\"created\"" "$OUT/hooked.state-by-before.json"'
/opt/before-hex oci delete --force hooked 2>> "$OUT/hooked.state-by-before.err"; wait "$STRACE"
check "the before binary deletes it" '[ ! -e "$HEX_OCI_ROOT/hooked" ]'
check "a hooked create writes H (the trace sees the stop it looks for)" 'lines "$OUT/after.hooked.trace" | grep -qw H'
echo "strace -c, hookless create, before vs after (calls by syscall):"
diff <(awk 'NR>2 && $NF!="total" && NF>=5 {print $NF, $(NF-2)}' "$OUT/before.plain.count" | sort) \
     <(awk 'NR>2 && $NF!="total" && NF>=5 {print $NF, $(NF-2)}' "$OUT/after.plain.count" | sort) > "$OUT/plain.count.diff"
cat "$OUT/plain.count.diff"
! grep -q '^FAIL' "$OUT/checks"
