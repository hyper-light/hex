#!/usr/bin/env bash
# HEX-INST-3 (native plan step 6): the macOS installer picks the box users' ids at install time, the first
# run of free ids in Apple's role-account range (sysadminctl: "Role accounts require name starting with _
# and UID in 450-499 range", macOS 26.4), never a fixed range that a release may take. Pure: runs anywhere
# bash does, `bash bench/install/macos_ids_test.sh`, exit 0 when every case holds.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=../../scripts/install/macos.sh
. "$HERE/../../scripts/install/macos.sh"
set +e

FAILS=0
case_is() { # <what> <want> <need> <first> <last> <taken ids...>: free_run's answer, or "none" when it refuses
  local what=$1 want=$2 need=$3 first=$4 last=$5 got
  shift 5
  got=$(printf '%s\n' "$@" | free_run "$need" "$first" "$last") || got=none
  if [ "$got" = "$want" ]; then echo "PASS: $what"; else echo "FAIL: $what: want $want, got $got"; FAILS=1; fi
}

case_is "an empty range starts at its first id" 450 32 450 499
case_is "ids outside the range don't matter" 450 32 450 499 0 1 300 304 350 382 500 501
case_is "a taken id moves the run past it" 451 32 450 499 450
case_is "a taken id mid-run moves it past that id" 461 32 450 499 460
case_is "the run may end on the range's last id" 468 32 450 499 467
case_is "a run that can't fit is refused" none 32 450 499 470
case_is "a full range is refused" none 1 450 451 450 451
case_is "duplicates and order in the taken list don't matter" 456 3 450 460 455 450 452 451 455 453 454
case_is "a run of one finds the first free id" 452 1 450 499 450 451
case_is "a need larger than the range is refused" none 51 450 499

ROLE=$((ROLE_LAST - ROLE_FIRST + 1))
if [ "$ROLE_FIRST" = 450 ] && [ "$ROLE_LAST" = 499 ]; then echo "PASS: the role range is Apple's 450-499"; else echo "FAIL: role range $ROLE_FIRST-$ROLE_LAST"; FAILS=1; fi
if [ $((COUNT + 2)) -le "$ROLE" ]; then echo "PASS: the pool and its two groups fit the role range"; else echo "FAIL: $((COUNT + 2)) ids don't fit $ROLE"; FAILS=1; fi
exit "$FAILS"
