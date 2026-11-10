#!/usr/bin/env bash
# bench/competitive/probes: tracebuf.sh's selftest (CLAUDE.md §6), no tracefs needed: a trace that lost events at a
# size is run again at twice it until one loses none, and a size the kernel refuses ends it nonzero.
#   bash tracebuf_test.sh
set -uo pipefail
mkdir -p "$HOME/.cache"; OUT=$(mktemp -d -p "$HOME/.cache" tracebuf-test.XXXXXX); trap 'rm -rf -- "${OUT:?}"' EXIT
. "$(dirname "$(realpath "${BASH_SOURCE[0]}")")/tracebuf.sh"
fails=0
expect() { [ "$2" = "$3" ] && echo "ok: $1" || { echo "FAIL: $1: got [$2], want [$3]"; fails=1; }; }

# Loses events below 4,000 KiB a CPU: run at 1,000, 2,000, then 4,000, which loses none.
lossy() {
  if [ "$TRACE_KB" -lt 4000 ]; then printf '== cpu0\noverrun: 5\ndropped events: 2\n' > "$OUT/l.stats"
  else printf '== cpu0\noverrun: 0\ndropped events: 0\n' > "$OUT/l.stats"; fi
}
grown 1000 "$OUT/l.stats" lossy; expect "a lossy trace grows until none is lost" "$?" 0
expect "each size tried is said, with what it lost" "$(cut -d: -f2 "$OUT/tracebuf.txt" | paste -sd'|')" \
  " 1000 KiB a CPU, 7 events lost| 2000 KiB a CPU, 7 events lost| 4000 KiB a CPU, 0 events lost"
expect "lost_in sums overruns and drops across CPUs" \
  "$(printf '== cpu0\noverrun: 3\ndropped events: 1\n== cpu1\noverrun: 2\ndropped events: 0\n' > "$OUT/two"; lost_in "$OUT/two")" 6

# The kernel refuses 2,000 KiB a CPU: it stops there, nonzero.
refused() { [ "$TRACE_KB" -lt 2000 ] || return 1; printf 'overrun: 1\n' > "$OUT/r.stats"; }
grown 1000 "$OUT/r.stats" refused; expect "a size the kernel refuses ends it nonzero" "$?" 1

# Loses nothing at the start: run once, at the starting size.
: > "$OUT/tracebuf.txt"
clean() { printf 'overrun: 0\n' > "$OUT/c.stats"; }
grown 4096 "$OUT/c.stats" clean; expect "a trace that lost nothing runs once" "$(wc -l < "$OUT/tracebuf.txt")" 1
exit $fails
