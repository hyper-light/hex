#!/usr/bin/env bash
# The installer lab twice, red then green, for the box-user descriptor default (HEX-INST-6, native plan
# step 6): at b849699, whose box-user contract test checks a program's RLIMIT_NOFILE but whose helper
# doesn't set it (red: that check fails), and at this script's own commit, whose helper does (green).
#
#   bash bench/install/macos_lab_ab.sh prepare   as the lab user: makes each tree a git worktree at its
#                                                commit, checks it is clean there, and pre-builds it into a
#                                                target of its own, so the root run compiles nothing;
#   sudo bash bench/install/macos_lab_ab.sh      one sudo prompt: checks both trees again, then runs
#                                                macos_lab.sh on red and on green, and prints both records.
#
# Each lab run installs, tests and uninstalls (macos_lab.sh, its own watchdog and EXIT trap), so the Mac
# ends as it began. The verdict: red's record fails only the box-user contract, on the descriptor limit
# (and the oversubscribed box-user check, whose test red's tree predates); green's passes.
set -u

RED=b849699
[ "$(uname -s)" = Darwin ] || { echo "macos_lab_ab: this is the macOS lab" >&2; exit 2; }
HERE=$(cd "$(dirname "$0")" && pwd)
SRC=$(cd "$HERE/../.." && pwd)
GREEN=$(git -C "$SRC" rev-parse --short HEAD) || { echo "macos_lab_ab: $SRC is not a git tree" >&2; exit 2; }

if [ "$(id -u)" = 0 ]; then
  LABUSER=${SUDO_USER:-}
  [ -n "$LABUSER" ] && [ "$LABUSER" != root ] || { echo "macos_lab_ab: run it through sudo as the lab user" >&2; exit 2; }
  HOMEDIR=$(dscl . -read "/Users/$LABUSER" NFSHomeDirectory | awk '{print $2}')
else
  HOMEDIR=$HOME
fi
AB=$HOMEDIR/.cache/hex-t7/macos/lab-ab

# A tree is checked when it is a clone at its commit with nothing changed.
checked() { # <dir> <commit>
  [ "$(git -C "$1" rev-parse HEAD 2>/dev/null)" = "$(git -C "$SRC" rev-parse "$2^{commit}")" ] &&
    [ -z "$(git -C "$1" status --porcelain --untracked-files=no)" ]
}

if [ "${1:-}" = prepare ]; then
  [ "$(id -u)" != 0 ] || { echo "macos_lab_ab: prepare as the lab user, not root" >&2; exit 2; }
  [ -z "$(git -C "$SRC" status --porcelain --untracked-files=no)" ] || { echo "macos_lab_ab: $SRC has changes" >&2; exit 1; }
  CARGO=$HOME/.cargo/bin/cargo
  mkdir -p "$AB"
  for side in red green; do
    if [ "$side" = red ]; then c=$RED; else c=$GREEN; fi
    t=$AB/$side
    rm -rf -- "${t:?}"
    git clone -q --no-checkout "$SRC" "$t" && git -C "$t" checkout -q --detach "$c" || exit 1
    checked "$t" "$c" || { echo "macos_lab_ab: $side tree isn't clean at $c" >&2; exit 1; }
    echo "macos_lab_ab: $side at $(git -C "$t" rev-parse --short HEAD), tree $(git -C "$t" rev-parse HEAD^{tree})"
    CARGO_TARGET_DIR=$AB/target-$side nice -n 10 "$CARGO" build -q --manifest-path "$t/Cargo.toml" -p hex-cli --bin hex || exit 1
    CARGO_TARGET_DIR=$AB/target-$side nice -n 10 "$CARGO" test -q --manifest-path "$t/Cargo.toml" -p hex-cli --test macos_process_box --no-run || exit 1
  done
  echo "macos_lab_ab: prepared; now: sudo bash $0"
  exit 0
fi

[ "$(id -u)" = 0 ] || { echo "macos_lab_ab: run it as root: sudo bash $0 (after: bash $0 prepare)" >&2; exit 2; }
for side in red green; do
  if [ "$side" = red ]; then c=$RED; else c=$GREEN; fi
  checked "$AB/$side" "$c" || { echo "macos_lab_ab: the $side tree isn't prepared at $c: bash $0 prepare" >&2; exit 1; }
done

run_lab() { # <side>: macos_lab.sh, this tree's own, on that side's tree and target; prints its record's path
  HEX_LAB_REPO=$AB/$1 HEX_LAB_TARGET=$AB/target-$1 bash "$HERE/macos_lab.sh" | tee "$AB/$1.out" >/dev/null
  sed -n 's/^=== RECORD \(.*\) ===$/\1/p' "$AB/$1.out" | tail -1
}

echo "macos_lab_ab: red at $RED (expect the box-user contract to fail on the descriptor limit)"
RED_RECORD=$(run_lab red)
echo "macos_lab_ab: green at $GREEN (expect PASS)"
GREEN_RECORD=$(run_lab green)

RED_OK=0
if [ -n "$RED_RECORD" ] && grep -q '^RESULT: FAIL' "$RED_RECORD" &&
  [ "$(grep '^FAIL: ' "$RED_RECORD" | grep -vc "the helper's ledger holds a box of 2x and 4x")" = 1 ] &&
  grep -q '^FAIL: the box-user HEX-RT contract' "$RED_RECORD" &&
  grep -q "descriptor limits" "$RED_RECORD"; then
  RED_OK=1
fi
GREEN_OK=0
if [ -n "$GREEN_RECORD" ] && grep -q '^RESULT: PASS' "$GREEN_RECORD"; then GREEN_OK=1; fi

echo
echo "red   ($RED):   $([ "$RED_OK" = 1 ] && echo 'failed as expected (box-user descriptor limit only)' || echo 'NOT as expected')  $RED_RECORD"
echo "green ($GREEN): $([ "$GREEN_OK" = 1 ] && echo PASS || echo FAIL)  $GREEN_RECORD"
[ "$RED_OK" = 1 ] && [ "$GREEN_OK" = 1 ]
