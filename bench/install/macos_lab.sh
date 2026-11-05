#!/usr/bin/env bash
# The macOS installer's lab (native plan step 6; HEX-INST-6, HEX-INST-7): the Mac, fresh of Hex, and its
# one admin step. Run once by an administrator, as `sudo bash bench/install/macos_lab.sh`; it never asks
# for or prints a password. It builds as the invoking user (never cargo as root), then:
#   1. before install: nothing of the installer's plan is on the machine, and the process class runs
#      without the helper as a process group, so the box-user contract is not met (the class says
#      ProcessGroup in its inspect);
#   2. the installer's dry run lists its plan, and install runs once as root;
#   3. with no further step the invoking user's boxes run as pool users: the box-user HEX-RT contract
#      passes over the installed helper's socket as that ordinary user (HEX-INST-6), the helper is
#      loaded in launchd's system domain, its socket is root's and hexnode's alone (a user outside
#      hexnode can't connect), and the pool is listed;
#   4. uninstall leaves nothing of the plan on the machine, listed line by line and by dscl and launchctl
#      on their own (HEX-INST-7).
# On any exit before uninstall ran, its EXIT trap uninstalls by the manifest; a watchdog bounds the run.
set -u

[ "$(uname -s)" = Darwin ] || { echo "macos_lab: this is the macOS lab" >&2; exit 2; }
[ "$(id -u)" = 0 ] || { echo "macos_lab: run it as root: sudo bash $0" >&2; exit 2; }
LABUSER=${SUDO_USER:-}
if [ -z "$LABUSER" ] || [ "$LABUSER" = root ]; then
  echo "macos_lab: run it through sudo as an ordinary user, whose boxes it checks" >&2
  exit 2
fi
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
INSTALLER=$REPO/scripts/install/macos.sh
HOMEDIR=$(dscl . -read "/Users/$LABUSER" NFSHomeDirectory | awk '{print $2}')
DIR=$HOMEDIR/.cache/hex-t7/macos
TARGET=$DIR/lab-target
TS=$(date -u +%Y%m%dT%H%M%SZ)
COMMIT=$(git -C "$REPO" rev-parse --short HEAD 2>/dev/null || echo unknown)
RECORD=$DIR/install-macos-$COMMIT-$TS.txt
SOCK=/var/db/hex/box-helper.sock
LABEL=com.github.hyper-light.hex.box-helper

as_user() { sudo -u "$LABUSER" -H "$@"; }

INSTALLED=0
WATCHDOG_PID=
cleanup() {
  if [ "$INSTALLED" = 1 ]; then
    echo "macos_lab: ending early; uninstalling by the manifest" >&2
    bash "$INSTALLER" uninstall >&2 || true
  fi
  [ -n "$WATCHDOG_PID" ] && kill "$WATCHDOG_PID" 2>/dev/null
  chown -R "$LABUSER" "$DIR" 2>/dev/null || true
}
trap cleanup EXIT
trap 'exit 130' TERM INT
MAINPID=$$
WATCHDOG_SECS=1800
(sleep "$WATCHDOG_SECS"; echo "macos_lab: watchdog fired after ${WATCHDOG_SECS} s" >&2; kill -TERM "$MAINPID") &
WATCHDOG_PID=$!

as_user mkdir -p "$DIR" || exit 1
echo "macos_lab: building as $LABUSER (never root) into $TARGET"
as_user env CARGO_TARGET_DIR="$TARGET" nice -n 10 cargo build -q --manifest-path "$REPO/Cargo.toml" -p hex-cli --bin hex ||
  { echo "macos_lab: hex build failed" >&2; exit 1; }
as_user env CARGO_TARGET_DIR="$TARGET" nice -n 10 cargo test -q --manifest-path "$REPO/Cargo.toml" -p hex-cli --test macos_process_box --no-run ||
  { echo "macos_lab: test build failed" >&2; exit 1; }
HEXBIN=$TARGET/debug/hex
# The newest of the test's executables: the target keeps every earlier build's beside the one just made.
TESTBIN=$(find "$TARGET/debug/deps" -maxdepth 1 -name 'macos_process_box-*' -type f -perm +111 -print0 |
  xargs -0 ls -t | head -1)
[ -n "$TESTBIN" ] || { echo "macos_lab: no test binary" >&2; exit 1; }

FAIL=0
step() { echo; echo "## $*"; }
expect() { # <what> <command...>: records PASS when the command succeeds, else FAIL
  local what=$1
  shift
  if "$@"; then echo "PASS: $what"; else echo "FAIL: $what"; FAIL=1; fi
}
box_user_contract() { as_user env HEX_BOX_HELPER_SOCK="$SOCK" "$TESTBIN" --exact --ignored box_user_boxes_hold_the_contract --test-threads 1 2>&1 | tail -n 15; return "${PIPESTATUS[0]}"; }
process_contract() { as_user "$TESTBIN" --exact macos_process_boxes_hold_the_contract 2>&1 | tail -n 3; return "${PIPESTATUS[0]}"; }
nothing_left_listed() {
  local n
  n=$(dscl . -list /Users | grep -c '^_hexbox' || true)
  echo "dscl users _hexbox*: $n"
  [ "$n" = 0 ] || return 1
  if dscl . -read /Groups/hexbox >/dev/null 2>&1 || dscl . -read /Groups/hexnode >/dev/null 2>&1; then
    echo "a group hexbox or hexnode is listed"
    return 1
  fi
  launchctl print "system/$LABEL" >/dev/null 2>&1
  local st=$?
  echo "launchctl print system/$LABEL: exit $st (113: no such service)"
  [ "$st" = 113 ] || return 1
  local p
  for p in /usr/local/bin/hex "/Library/PrivilegedHelperTools/$LABEL" "/Library/LaunchDaemons/$LABEL.plist" \
    /var/db/hex /var/log/hex-box-helper.log /var/db/hex-install; do
    if [ -e "$p" ]; then echo "left: $p"; return 1; fi
  done
}
outsider_refused() { # a user outside hexnode (nobody) can't connect to the helper's socket
  ! sudo -u nobody nc -U -z "$SOCK" 2>/dev/null
}
member_connects() { as_user nc -U -z "$SOCK"; }

# The record takes everything from here, in this shell (not a pipeline's subshell), so the EXIT trap sees
# whether install ran.
exec > >(tee "$RECORD") 2>&1
echo "# macOS installer lab (native plan step 6), $(scutil --get LocalHostName 2>/dev/null).local, macOS $(sw_vers -productVersion) $(sw_vers -buildVersion), $(uname -m), commit $COMMIT, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "# load1 $(sysctl -n vm.loadavg | awk '{print $2}'), cores $(sysctl -n hw.ncpu), lab user $LABUSER"

step "1. before install"
expect "nothing of the installer's plan is on the machine" bash "$INSTALLER" check
expect "the process class runs without the helper (HEX-RT-1..9, process group)" process_contract
expect "without the helper the box-user contract is not met (the class says ProcessGroup)" eval '! box_user_contract'

step "2. install, the one admin step"
bash "$INSTALLER" install --binary "$HEXBIN" --user "$LABUSER" --dry-run
INSTALLED=1
expect "install" bash "$INSTALLER" install --binary "$HEXBIN" --user "$LABUSER"

step "3. the installed helper, with no further step"
launchctl print "system/$LABEL" >/dev/null 2>&1
echo "launchctl print system/$LABEL: exit $?"
stat -f "socket %N: %Sp %Su:%Sg" "$SOCK"
stat -f "folder %N: %Sp %Su:%Sg" "$(dirname "$SOCK")"
echo "pool: $(dscl . -list /Users UniqueID | awk '/^_hexbox/ {printf "%s=%s ", $1, $2}')"
echo "$LABUSER's groups (a fresh login): $(as_user id -Gn)"
expect "$LABUSER (hexnode) connects to the helper's socket" member_connects
expect "nobody (outside hexnode) is refused the helper's socket" outsider_refused
expect "the box-user HEX-RT contract passes for $LABUSER over the installed helper (HEX-INST-6)" box_user_contract
expect "the process class still passes HEX-RT-1..9" process_contract

step "4. uninstall"
expect "uninstall, which lists nothing of the plan left" bash "$INSTALLER" uninstall
INSTALLED=0
expect "dscl and launchctl list nothing of Hex's, and its paths are gone (HEX-INST-7)" nothing_left_listed

echo
if [ "$FAIL" = 0 ]; then echo "RESULT: PASS"; else echo "RESULT: FAIL"; fi
echo "=== RECORD $RECORD ==="
exit "$FAIL"
