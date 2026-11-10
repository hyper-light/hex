#!/usr/bin/env bash
# bench/competitive/probes: the waits census's suites arm (waits_census.md). Runs the suites, gate 6's
# `cargo test --workspace`, in <tree> again and again beside the census's other arms, with the tests' own census on
# (HEX_WAITS_CENSUS: hex_testkit::waits records each test wait's answer, and each named wait's in a test process, in the
# census's form), so the record holds the tests' own waits, a row for each measuring one to seed it, and the debug arm
# meets the suites' load. Every loop runs every suite through (--no-fail-fast), so a failing test leaves the load and
# the record whole; its exit status is read before anything else runs and recorded, one line a loop, in suites.txt.
#   waits_census_suites.sh <tree> <census run directory> <stop file>
# It loops until <stop file> exists, which waits_census.sh makes when its probe ends, then says how many loops it ran.
# CARGO_TARGET_DIR is the caller's, the tree's own (one target directory a checkout); N (10000) bounds each test
# process's census at N x 128 lines, as the probe's arms are.
set -uo pipefail
TREE=$(realpath "${1:?the source tree}"); OUT=$(realpath "${2:?the census run directory}"); STOP=${3:?the stop file}
export HEX_WAITS_CENSUS="$OUT/census-suites.txt" HEX_WAITS_CENSUS_MAX=$((${N:-10000} * 128))
cd "$TREE" || exit 2
loops=0
while [ ! -e "$STOP" ]; do
  nice -n 10 cargo test --workspace --locked --no-fail-fast > "$OUT/suites-$loops.log" 2>&1
  status=$?
  echo "$(date -u +%FT%TZ) loop $loops exit $status" >> "$OUT/suites.txt"
  loops=$((loops + 1))
done
echo "$(date -u +%FT%TZ) stopped after $loops loops" >> "$OUT/suites.txt"
