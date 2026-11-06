#!/usr/bin/env bash
# Hex's installer for macOS: the one admin step (native plan step 6; docs/design/runtime.md, decision
# "the installer, per OS"; sources in docs/research/runtime/installer-per-os.md §3). It sets up what the
# process class's box-user mode needs (native plan step 2), so a user's boxes run as users of their own
# with no further step:
#   - the box users' pool, as Nix's installer makes its build users (§3.1): a group `hexbox` and hidden
#     users `_hexbox1`..`_hexboxN` (shell /sbin/nologin, home /var/empty) in an id range checked free
#     first, never reusing a taken id;
#   - a group `hexnode` holding each --user: the users whose nodes may reach the helper;
#   - the privileged helper (`hex __box-helper`) as a launchd daemon (§3.2-3.4): its binary in
#     /Library/PrivilegedHelperTools (root's, so no user can replace what runs as root), a plain plist in
#     /Library/LaunchDaemons, bootstrapped into the system domain; its socket in /var/db/hex, a folder
#     of root and `hexnode` that only they may enter;
#   - the `hex` binary at /usr/local/bin/hex.
# Every thing it makes is a line of its plan ("<kind> <target>"), written to the manifest before it is
# made; uninstall undoes the manifest's lines and then lists what is left, which is nothing.
#
# usage: macos.sh install --binary <hex> [--user <name>]... [--count <n>] [--first-id <id>] [--dry-run]
#        macos.sh uninstall [--dry-run]
#        macos.sh check [<plan file>]
# `--dry-run` prints the plan and makes nothing. `check` lists what of a plan (default: the manifest's,
# else the fixed names) is still on the machine and exits 1 when anything is.
set -euo pipefail

LABEL=com.github.hyper-light.hex.box-helper
BIN=/usr/local/bin/hex
HELPER=/Library/PrivilegedHelperTools/$LABEL
PLIST=/Library/LaunchDaemons/$LABEL.plist
SOCKDIR=/var/db/hex
SOCK=$SOCKDIR/box-helper.sock
LOG=/var/log/hex-box-helper.log
STATE=/var/db/hex-install
MANIFEST=$STATE/manifest
NODE_GROUP=hexnode
POOL_GROUP=hexbox
POOL_PREFIX=_hexbox
# The pool's size, as Nix's 32 build users (install-multi-user.sh:25): one box-user box per pool user.
COUNT=30
# Apple's role-account range: sysadminctl's usage, macOS 26.4, "Role accounts require name starting with _
# and UID in 450-499 range" (§3.1). The pool's ids, hexnode's gid then hexbox's then the users' uids, are
# learned at install time: the first run of COUNT+2 ids there that no user, group or process holds
# (free_run), so no release's new system ids are reused. --first-id names a start instead.
ROLE_FIRST=450
ROLE_LAST=499
FIRST_ID=
# The most processes a box's user may hold (RLIMIT_NPROC, native plan step 2's helper default).
NPROC=1024
# How long launchd gets to start the helper: a bounded wait, counted in 0.1 s steps.
WAIT_STEPS=300
# dscl fails at times with eNotYetImplemented and succeeds when asked again (Nix's
# install-darwin-multi-user.sh:149): each write is tried at most this many times.
DSCL_TRIES=3

die() { echo "macos.sh: $*" >&2; exit 1; }

usage() {
  sed -n '/^# usage:/,/^# `check`/p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

dscl_write() {
  local i
  for ((i = 1; i <= DSCL_TRIES; i++)); do
    dscl . "$@" && return 0
  done
  die "dscl . $* failed $DSCL_TRIES times"
}

# The first id f in [first, last] with f..f+need-1 all free of the taken ids (one per line on stdin), or a
# failure when no such run fits. A counted walk over the range, no larger than it.
free_run() { # <need> <first> <last>
  local need=$1 first=$2 last=$3 taken f run=0 id
  taken=$(sort -un)
  f=$first
  for ((id = first; id <= last; id++)); do
    if grep -qx "$id" <<<"$taken"; then
      run=0
      f=$((id + 1))
    else
      run=$((run + 1))
      if [ "$run" = "$need" ]; then
        echo "$f"
        return 0
      fi
    fi
  done
  return 1
}

# The ids the machine holds: every user's uid, group's gid and running process's uid.
taken_ids() {
  dscl . -list /Users UniqueID | awk '{print $2}'
  dscl . -list /Groups PrimaryGroupID | awk '{print $2}'
  ps -axo uid= | tr -d ' '
}

node_gid() { echo "$FIRST_ID"; }
pool_gid() { echo $((FIRST_ID + 1)); }
pool_uid() { echo $((FIRST_ID + 1 + $1)); }
pool_uids() {
  local i out=
  for ((i = 1; i <= COUNT; i++)); do out=${out:+$out,}$(pool_uid "$i"); done
  echo "$out"
}

# The plan: every thing install makes, one "<kind> <target>" line each.
plan() {
  local d i
  for d in "$(dirname "$BIN")" "$(dirname "$HELPER")"; do
    [ -d "$d" ] || echo "folder $d"
  done
  echo "group $NODE_GROUP"
  echo "group $POOL_GROUP"
  for ((i = 1; i <= COUNT; i++)); do echo "user $POOL_PREFIX$i"; done
  echo "file $BIN"
  echo "file $HELPER"
  echo "dir $SOCKDIR"
  echo "file $LOG"
  echo "file $PLIST"
  echo "daemon $LABEL"
  [ -d "$STATE" ] || echo "folder $STATE"
  echo "file $MANIFEST"
}

# Whether one plan line's thing is on the machine. Each is read unprivileged (§3.5).
present() {
  local kind=$1 target=$2
  case $kind in
    file) [ -f "$target" ] ;;
    dir | folder) [ -d "$target" ] ;;
    group) dscl . -read "/Groups/$target" PrimaryGroupID >/dev/null 2>&1 ;;
    user) dscl . -read "/Users/$target" UniqueID >/dev/null 2>&1 ;;
    # launchctl print's exit is 0 for a loaded service and 113 for none; its output is not API (§3.3).
    daemon) launchctl print "system/$target" >/dev/null 2>&1 ;;
    *) die "unknown plan line kind $kind" ;;
  esac
}

check() {
  local list=$1 left=0 kind target
  while read -r kind target; do
    [ -n "$kind" ] || continue
    if present "$kind" "$target"; then
      echo "left: $kind $target"
      left=1
    fi
  done <"$list"
  [ "$left" = 0 ] && echo "nothing of Hex's is left"
  return "$left"
}

# Refuses, by name, every id or name of the plan that the machine already has, so a taken id is never
# reused (§3.1: the range Apple takes grows from release to release).
preflight() {
  [ "$(uname -s)" = Darwin ] || die "this is macOS' installer; this machine is $(uname -s)"
  if [ -z "$FIRST_ID" ]; then
    FIRST_ID=$(taken_ids | free_run $((COUNT + 2)) "$ROLE_FIRST" "$ROLE_LAST") ||
      die "no run of $((COUNT + 2)) free ids in Apple's role range $ROLE_FIRST-$ROLE_LAST; name one with --first-id"
  fi
  echo "macos.sh: ids $FIRST_ID-$((FIRST_ID + COUNT + 1)): gid $(node_gid) $NODE_GROUP, gid $(pool_gid) $POOL_GROUP, uids $(pool_uid 1)-$(pool_uid "$COUNT")" >&2
  local uids gids taken=0 id i name
  uids=$(dscl . -list /Users UniqueID | awk '{print $2}')
  gids=$(dscl . -list /Groups PrimaryGroupID | awk '{print $2}')
  for id in "$(node_gid)" "$(pool_gid)"; do
    if grep -qx "$id" <<<"$gids"; then echo "macos.sh: gid $id is taken: $(dscl . -search /Groups PrimaryGroupID "$id" | awk 'NR==1 {print $1}')" >&2; taken=1; fi
  done
  for ((i = 1; i <= COUNT; i++)); do
    id=$(pool_uid "$i")
    if grep -qx "$id" <<<"$uids"; then echo "macos.sh: uid $id is taken: $(dscl . -search /Users UniqueID "$id" | awk 'NR==1 {print $1}')" >&2; taken=1; fi
    # A live process at a pool uid would be ended by a box's delete: the uid must be no one's.
    if ps -axo uid= | tr -d ' ' | grep -qx "$id"; then echo "macos.sh: a process runs as uid $id" >&2; taken=1; fi
  done
  for name in "$NODE_GROUP" "$POOL_GROUP"; do
    if present group "$name"; then echo "macos.sh: group $name exists" >&2; taken=1; fi
  done
  for ((i = 1; i <= COUNT; i++)); do
    if present user "$POOL_PREFIX$i"; then echo "macos.sh: user $POOL_PREFIX$i exists" >&2; taken=1; fi
  done
  [ "$taken" = 0 ] || die "the ids above are taken; pick a free range with --first-id"
}

make_group() { # <name> <gid> <real name>
  dseditgroup -o create -r "$3" -i "$2" "$1" || die "making group $1"
}

make_pool_user() { # <index>
  local name=$POOL_PREFIX$1 uid
  uid=$(pool_uid "$1")
  dscl_write -create "/Users/$name"
  dscl_write -create "/Users/$name" UniqueID "$uid"
  dscl_write -create "/Users/$name" PrimaryGroupID "$(pool_gid)"
  dscl_write -create "/Users/$name" IsHidden 1
  dscl_write -create "/Users/$name" NFSHomeDirectory /var/empty
  dscl_write -create "/Users/$name" UserShell /sbin/nologin
  dscl_write -create "/Users/$name" RealName "Hex box user $1"
  dseditgroup -o edit -t user -a "$name" "$POOL_GROUP" || die "adding $name to $POOL_GROUP"
}

# Waits (bounded) for a condition; says which and fails when it doesn't come.
wait_for() {
  local what=$1 i
  shift
  for ((i = 0; i < WAIT_STEPS; i++)); do
    "$@" && return 0
    sleep 0.1
  done
  echo "macos.sh: $what didn't happen within $((WAIT_STEPS / 10)) s" >&2
  return 1
}

install_hex() {
  [ -n "$BINARY" ] || die "install needs --binary <hex>"
  [ -x "$BINARY" ] || die "$BINARY isn't an executable"
  local u
  for u in "${USERS[@]}"; do id -u "$u" >/dev/null 2>&1 || die "no user $u"; done
  preflight
  if [ "$DRY_RUN" = 1 ]; then plan; return; fi
  [ "$(id -u)" = 0 ] || die "install is the admin step: run it as root (sudo)"
  [ ! -e "$MANIFEST" ] || die "Hex is installed already ($MANIFEST); uninstall first"
  local made i
  made=$(plan)
  mkdir -p "$STATE"
  # The plan is the manifest from the start, so an install cut short is undone by uninstall too.
  printf '%s\n' "$made" >"$MANIFEST"
  make_group "$NODE_GROUP" "$(node_gid)" "Hex nodes"
  for u in "${USERS[@]}"; do
    dseditgroup -o edit -t user -a "$u" "$NODE_GROUP" || die "adding $u to $NODE_GROUP"
  done
  make_group "$POOL_GROUP" "$(pool_gid)" "Hex box users"
  for ((i = 1; i <= COUNT; i++)); do make_pool_user "$i"; done
  install -d -m 0755 "$(dirname "$BIN")" "$(dirname "$HELPER")"
  install -m 0755 -o root -g wheel "$BINARY" "$BIN"
  install -m 0755 -o root -g wheel "$BINARY" "$HELPER"
  # The socket's folder: root's and hexnode's only. A socket bound in it takes the folder's group (BSD's
  # rule for a new file's group, open(2)), and launchd's Umask makes it 0770, so a node of hexnode connects
  # and no one else does.
  install -d -m 0750 -o root -g "$NODE_GROUP" "$SOCKDIR"
  : >"$LOG"
  chown root:wheel "$LOG"
  chmod 0644 "$LOG"
  cat >"$PLIST" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<!-- Installed by Hex's installer (scripts/install/macos.sh): the box helper, which starts each box's
     programs as a pool user and arms their memory limits (native plan step 2). -->
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>$LABEL</string>
  <key>ProgramArguments</key>
  <array>
    <string>$HELPER</string>
    <string>__box-helper</string>
    <string>--socket</string>
    <string>$SOCK</string>
    <string>--users</string>
    <string>$(pool_uids)</string>
    <string>--nproc</string>
    <string>$NPROC</string>
  </array>
  <key>Umask</key>
  <integer>7</integer>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardErrorPath</key>
  <string>$LOG</string>
</dict>
</plist>
EOF
  chown root:wheel "$PLIST"
  chmod 0644 "$PLIST"
  plutil -lint "$PLIST" >/dev/null || die "$PLIST isn't a valid property list"
  # bootstrap's exit says whether it loaded, where load's is always 0 (launchctl.1:527-528, §3.3).
  launchctl bootstrap system "$PLIST" || die "launchctl bootstrap system $PLIST failed"
  if ! wait_for "the helper serving $SOCK" test -S "$SOCK"; then
    tail -n 5 "$LOG" >&2 || true
    die "the helper didn't come up (its log's last lines are above)"
  fi
  echo "the box helper serves $SOCK for group $NODE_GROUP (${USERS[*]:-no users yet})"
  echo "installed; uninstall with: sudo bash $0 uninstall"
}

# Ends every process of a pool user (a box's members outlive the helper that started them), by uid, in
# rounds counted against WAIT_STEPS. The uids are Hex's own, refused at install if anyone held them.
end_pool_processes() { # <uid>
  local uid=$1
  wait_for "uid $uid's processes ending" sh -c "pkill -KILL -U $uid; ! pgrep -U $uid >/dev/null"
}

uninstall_hex() {
  local list
  list=$(if [ -f "$MANIFEST" ]; then cat "$MANIFEST"; else plan; fi)
  if [ "$DRY_RUN" = 1 ]; then printf '%s\n' "$list"; return; fi
  [ "$(id -u)" = 0 ] || die "uninstall is the admin step: run it as root (sudo)"
  local kind target uid
  # The helper goes first, so no box starts while the pool empties.
  if present daemon "$LABEL"; then
    launchctl bootout "system/$LABEL" || die "launchctl bootout system/$LABEL failed"
  fi
  while read -r kind target; do
    [ "$kind" = user ] || continue
    case $target in "$POOL_PREFIX"*) ;; *) die "refusing to remove user $target: not one of Hex's" ;; esac
    if present user "$target"; then
      uid=$(dscl . -read "/Users/$target" UniqueID | awk '{print $2}')
      end_pool_processes "$uid" || die "$target's processes didn't end"
      dscl_write -delete "/Users/$target"
    fi
  done <<<"$list"
  while read -r kind target; do
    case $kind in
      group)
        case $target in "$NODE_GROUP" | "$POOL_GROUP") ;; *) die "refusing to remove group $target: not one of Hex's" ;; esac
        if present group "$target"; then dseditgroup -o delete "$target" || die "removing group $target"; fi
        ;;
      file) [ "$target" = "$MANIFEST" ] || rm -f -- "${target:?}" ;;
      dir)
        # Only Hex's own folder, by name: a manifest line never removes anything else.
        [ "$target" = "$SOCKDIR" ] || die "refusing to remove $target: not one of Hex's directories"
        rm -rf -- "${target:?}"
        ;;
      folder | user | daemon) ;;
      *) die "unknown manifest line kind $kind" ;;
    esac
  done <<<"$list"
  # Folders install made, once emptied of what it put there.
  while read -r kind target; do
    [ "$kind" = folder ] && [ -d "$target" ] && { rmdir -- "$target" 2>/dev/null || true; }
  done <<<"$list"
  rm -f -- "${MANIFEST:?}"
  rmdir "$STATE" 2>/dev/null || true
  local saved
  saved=$(mktemp)
  printf '%s\n' "$list" >"$saved"
  check "$saved" || { rm -f -- "${saved:?}"; die "uninstall left the lines above"; }
  rm -f -- "${saved:?}"
}

main() {
[ "$#" -ge 1 ] || usage
VERB=$1
shift
BINARY=
DRY_RUN=0
USERS=()
while [ "$#" -gt 0 ]; do
  case $1 in
    --binary) BINARY=${2:?--binary needs a path}; shift 2 ;;
    --user) USERS+=("${2:?--user needs a name}"); shift 2 ;;
    --count) COUNT=${2:?--count needs a number}; shift 2 ;;
    --first-id) FIRST_ID=${2:?--first-id needs an id}; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    *) [ "$VERB" = check ] && [ "$#" = 1 ] && break; die "unknown argument $1" ;;
  esac
done
[[ $COUNT =~ ^[1-9][0-9]*$ ]] || die "--count must be a positive number"
[ -z "$FIRST_ID" ] || [[ $FIRST_ID =~ ^[1-9][0-9]*$ ]] || die "--first-id must be a positive number"

case $VERB in
  install) install_hex ;;
  uninstall) uninstall_hex ;;
  check)
    if [ "$#" = 1 ]; then
      check "$1"
    elif [ -f "$MANIFEST" ]; then
      check "$MANIFEST"
    else
      list=$(mktemp)
      plan >"$list"
      status=0
      check "$list" || status=$?
      rm -f -- "${list:?}"
      exit "$status"
    fi
    ;;
  *) usage ;;
esac
}

# Run as a command; sourced (bench/install/macos_ids_test.sh), it only defines its functions.
if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  main "$@"
fi
