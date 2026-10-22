#!/usr/bin/env bash
# Consumer #1 dispatched-leg smoke, step 1 (runs INSIDE the dispatched CI leg): show this leg is in a
# Hex box with its fair-share bounds, and that the machine is protected — the legs' parent box carries
# memory.high and the runner/agents carry memory.low. (CP-T7.1, AC-T7.1.1 machine-protection half.)
set -uo pipefail
ROOT=/sys/fs/cgroup
CG=$(awk -F: '$1=="0"{print $3}' /proc/self/cgroup)
echo "leg cgroup: $CG"

show() { # <dir> <label>
  local d="$1" label="$2"
  if [ ! -d "$d" ]; then printf '  [%-11s] %s: (absent)\n' "$label" "$d"; return; fi
  printf '  [%-11s] %-18s cpu.weight=%s io.weight=%q pids.max=%s memory.high=%s memory.low=%s\n' \
    "$label" "$(basename "$d")" \
    "$(cat "$d/cpu.weight" 2>/dev/null || echo -)" \
    "$(head -1 "$d/io.weight" 2>/dev/null || echo -)" \
    "$(cat "$d/pids.max" 2>/dev/null || echo -)" \
    "$(cat "$d/memory.high" 2>/dev/null || echo -)" \
    "$(cat "$d/memory.low" 2>/dev/null || echo -)"
}

echo "=== ancestor chain (leg box up to the cgroup root) ==="
d="$ROOT$CG"
while [ -n "$d" ] && [ "$d" != "$ROOT" ] && [ "$d" != "/" ]; do
  show "$d" "chain"
  d=$(dirname "$d")
done

# OWN = the delegated scope: the ancestor that holds the runner's `supervisor` leaf.
own="$ROOT$CG"
while [ -n "$own" ] && [ "$own" != "$ROOT" ] && [ "$own" != "/" ]; do
  [ -d "$own/supervisor" ] && break
  own=$(dirname "$own")
done
echo "=== machine protection (under OWN=${own#$ROOT}) ==="
show "$own/supervisor" "runner"     # memory.low expected
show "$own/hex" "agents"            # memory.low expected
show "$own/hex-legs" "legs-parent"  # memory.high expected

echo "=== host right now ==="
grep -E 'MemTotal|MemAvailable' /proc/meminfo
echo "loadavg: $(cat /proc/loadavg)"
