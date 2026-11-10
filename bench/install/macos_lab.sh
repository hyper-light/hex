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
#      hexnode can't connect), and the pool is listed; and step 2's root checks run against the installed
#      pool (its C1 membership and its caps, bench/native/macos/boxuser_probe.c), so this one admin run covers
#      the installer, the pool's growth and step 2's helper together (the lead, 2026-11-06);
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
# The tree under test: this script's own, unless HEX_LAB_REPO names another (macos_lab_ab.sh runs this
# script on a red and a green tree, each pre-built into its own HEX_LAB_TARGET).
REPO=${HEX_LAB_REPO:-$(cd "$HERE/../.." && pwd)}
INSTALLER=$REPO/scripts/install/macos.sh
HOMEDIR=$(dscl . -read "/Users/$LABUSER" NFSHomeDirectory | awk '{print $2}')
DIR=$HOMEDIR/.cache/hex-t7/macos
TARGET=${HEX_LAB_TARGET:-$DIR/lab-target}
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
# Twice the lab's recorded phases (18.950 s) plus each unrecorded phase's own guard: step 2's probe waits
# (10.700 s) and the oversubscribed box-user test's six passes, each ended by its first hog's wait, bounded by
# the test's HANG_GUARD (6 x 39.507 = 237.042 s): bench/native/macos/records/labphases-40454fe-20261106T165836Z.txt
# (the lead's call 5, 2026-11-06). Ada's run prints every phase's time (TIME: lines); her record sets it after.
WATCHDOG_SECS=266.692
(sleep "$WATCHDOG_SECS"; echo "macos_lab: watchdog fired after ${WATCHDOG_SECS} s" >&2; kill -TERM "$MAINPID") &
WATCHDOG_PID=$!

as_user mkdir -p "$DIR" || exit 1
# sudo may reset PATH, so the user's own toolchain is named outright where rustup put it.
CARGO=$HOMEDIR/.cargo/bin/cargo
[ -x "$CARGO" ] || CARGO=$(as_user bash -lc 'command -v cargo') || { echo "macos_lab: no cargo for $LABUSER" >&2; exit 1; }
echo "macos_lab: building as $LABUSER (never root) into $TARGET"
as_user env CARGO_TARGET_DIR="$TARGET" nice -n 10 "$CARGO" build -q --manifest-path "$REPO/Cargo.toml" -p hex-cli --bin hex ||
  { echo "macos_lab: hex build failed" >&2; exit 1; }
as_user env CARGO_TARGET_DIR="$TARGET" nice -n 10 "$CARGO" test -q --manifest-path "$REPO/Cargo.toml" -p hex-cli --test macos_process_box --no-run ||
  { echo "macos_lab: test build failed" >&2; exit 1; }
HEXBIN=$TARGET/debug/hex
# The newest of the test's executables: the target keeps every earlier build's beside the one just made.
TESTBIN=$(find "$TARGET/debug/deps" -maxdepth 1 -name 'macos_process_box-*' -type f -perm +111 -print0 |
  xargs -0 ls -t | head -1)
[ -n "$TESTBIN" ] || { echo "macos_lab: no test binary" >&2; exit 1; }
# Step 2's root probe (bench/native/macos/boxuser_probe.c), built as the lab user: its membership and cap
# checks run here against the installed pool, so Ada's one installer run covers step 2's helper too.
PROBE=$TARGET/boxuser_probe
as_user cc -O2 -Wall -Wextra -o "$PROBE" "$REPO/bench/native/macos/boxuser_probe.c" ||
  { echo "macos_lab: probe build failed" >&2; exit 1; }

FAIL=0
step() { echo; echo "## $*"; }
now() { perl -MTime::HiRes=time -e 'printf "%.6f", time'; }
expect() { # <what> <command...>: records PASS when the command succeeds, else FAIL, and how long it took
  local what=$1 t0 st
  shift
  t0=$(now)
  if "$@"; then st=PASS; else st=FAIL; FAIL=1; fi
  echo "$st: $what"
  # Each phase's time, so a recorded run sets the watchdog for the next (the lead's call 5, 2026-11-06).
  echo "TIME: $(perl -e "printf '%.3f', $(now) - $t0") s: $what"
}
box_user_contract() { as_user env HEX_BOX_HELPER_SOCK="$SOCK" "$TESTBIN" --exact --ignored box_user_boxes_hold_the_contract --test-threads 1 2>&1 | tail -n 15; return "${PIPESTATUS[0]}"; }
# Ada's two-layer bound (2026-11-01): the helper's ledger holds a box of 2x and 4x the cores' hogs within 64
# MiB of its bound (the lead, 2026-11-06), as the lab user over the installed helper.
# It must run, not match nothing: libtest passes a filter that names no test.
oversubscribed_box_user() {
  local out st
  out=$(as_user env HEX_BOX_HELPER_SOCK="$SOCK" "$TESTBIN" --exact --ignored a_box_user_box_holds_the_bar_oversubscribed --nocapture --test-threads 1 2>&1)
  st=$?
  grep -E 'OVERSHOOT|test result|panicked|passes past' <<<"$out"
  [ "$st" = 0 ] && grep -q 'test result: ok. 1 passed' <<<"$out"
}
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
# A connect(2) to the helper's socket, as its caller: prints "ok", or the refusal's errno text. macOS's
# `nc -U -z` exits 1 even against a listening socket (measured on .local-2, 2026-11-06), so it can't probe.
CONNECT='use IO::Socket::UNIX; IO::Socket::UNIX->new(Type => SOCK_STREAM(), Peer => $ARGV[0]) ? print "ok\n" : print "$!\n"'
connect_as() { sudo -u "$1" perl -e "$CONNECT" "$SOCK"; }
outsider_refused() { # a user outside hexnode (nobody) is refused the socket by its folder's mode (EACCES)
  local got
  got=$(connect_as nobody)
  echo "nobody's connect: $got"
  [ "$got" = "Permission denied" ]
}
pool_uid_of() { "$INSTALLED_DSCL" . -read "/Users/$1" UniqueID 2>/dev/null | awk '{print $2}'; }
INSTALLED_DSCL=/usr/bin/dscl
# Step 2's C1 (box membership): six members run as the installed pool's first user, one daemonized; the
# user listing finds all six, RLIMIT_NPROC refuses a seventh in the kernel, and a kill-all leaves none.
membership_c1() {
  local uid out
  uid=$(pool_uid_of _hexbox1)
  [ -n "$uid" ] || { echo "no _hexbox1"; return 1; }
  out=$("$PROBE" uid "$uid" 6)
  echo "$out"
  grep -q "started 6 members, proc_listpids(PROC_UID_ONLY) found 6" <<<"$out" &&
    grep -q "bit -> a fork past the cap was refused" <<<"$out" &&
    grep -q "after delete, 0 processes left" <<<"$out"
}
# Step 2's caps: the fatal footprint cap kills at 64 and 256 MiB (SIGKILL), and the soft one notifies
# (EXC_RESOURCE), as root.
caps() {
  local mb out ok=0
  for mb in 64 256; do
    out=$("$PROBE" cap fatal "$mb")
    echo "$out"
    grep -q "killed by signal 9" <<<"$out" || ok=1
  done
  out=$("$PROBE" cap soft 64)
  echo "$out"
  grep -q "EXC_RESOURCE after" <<<"$out" || ok=1
  return "$ok"
}
member_connects() { # the lab user, in hexnode, connects
  local got
  got=$(connect_as "$LABUSER")
  echo "$LABUSER's connect: $got"
  [ "$got" = ok ]
}

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
expect "the helper's ledger holds a box of 2x and 4x the cores' hogs within 64 MiB of its bound, every pass" oversubscribed_box_user
expect "step 2's C1: the installed pool's user holds its members, its cap bites, a kill-all empties it" membership_c1
expect "step 2's caps: the fatal footprint cap kills at 64 and 256 MiB, the soft one notifies" caps
expect "the process class still passes HEX-RT-1..9" process_contract

step "4. uninstall"
expect "uninstall, which lists nothing of the plan left" bash "$INSTALLER" uninstall
INSTALLED=0
expect "dscl and launchctl list nothing of Hex's, and its paths are gone (HEX-INST-7)" nothing_left_listed

echo
if [ "$FAIL" = 0 ]; then echo "RESULT: PASS"; else echo "RESULT: FAIL"; fi
echo "=== RECORD $RECORD ==="
exit "$FAIL"
