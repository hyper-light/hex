#!/usr/bin/env bash
# Hex's installer for Linux: the one admin step (native plan step 6; docs/design/runtime.md, decision
# "the installer, per OS"; sources in docs/research/runtime/installer-per-os.md). It sets up what the
# process and oci classes' cgroup v2 delegation needs, so a user's node runs with no further step:
#   - the `hex` binary at /usr/local/bin/hex;
#   - a drop-in delegating cpu, io, memory and pids to every user's manager (user@.service);
#   - the `hex-node` user service (Delegate=yes, DelegateSubgroup=agent: the node runs in a leaf and its
#     boxes in a sibling), enabled for every user;
#   - for each --user, lingering (their manager, and so their node, runs without a session), the
#     service started now.
# Every thing it makes is a line of its plan ("<kind> <target>"), written to the manifest before it is
# made; uninstall undoes the manifest's lines and then lists what is left, which is nothing.
#
# usage: linux.sh install --binary <hex> [--user <name>]... [--dry-run]
#        linux.sh uninstall [--dry-run]
#        linux.sh check [<plan file>]
# `--dry-run` prints the plan and makes nothing. `check` lists what of a plan (default: the manifest's,
# else the fixed paths) is still on the machine and exits 1 when anything is.
set -euo pipefail

BIN=/usr/local/bin/hex
DROPIN=/etc/systemd/system/user@.service.d/50-hex-delegate.conf
UNIT=/etc/systemd/user/hex-node.service
WANTS=/etc/systemd/user/default.target.wants/hex-node.service
STATE=/var/lib/hex-install
MANIFEST=$STATE/manifest
# The node's descriptor limit, soft and hard (the unit below says why).
NOFILE=1048576
# DelegateSubgroup= is systemd 254's (systemd.resource-control(5)).
SYSTEMD_MIN=254
# How long a user's manager and node get to come up: a bounded wait, counted in 0.1 s steps.
WAIT_STEPS=300

die() { echo "linux.sh: $*" >&2; exit 1; }

usage() {
  sed -n '/^# usage:/,/^# `check`/p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

user_uid() { id -u "$1"; }

# systemctl --user as user $1 against their running manager, through its private socket under
# XDG_RUNTIME_DIR (setpriv: no PAM session or machined needed, unlike runuser or -M user@), each call
# bounded by the installer's wait.
user_ctl() {
  local u=$1 uid
  shift
  uid=$(id -u "$u")
  setpriv --reuid="$uid" --regid="$(id -g "$u")" --init-groups -- \
    env XDG_RUNTIME_DIR="/run/user/$uid" timeout $((WAIT_STEPS / 10)) systemctl --user "$@"
}
user_home() { getent passwd "$1" | cut -d: -f6; }

# The plan: every thing install makes, one "<kind> <target>" line each.
plan() {
  local d
  for d in "$(dirname "$BIN")" "$(dirname "$DROPIN")" "$(dirname "$WANTS")"; do
    [ -d "$d" ] || echo "folder $d"
  done
  echo "file $BIN"
  echo "file $DROPIN"
  echo "file $UNIT"
  echo "link $WANTS"
  local u uid home
  for u in "${USERS[@]}"; do
    uid=$(user_uid "$u")
    home=$(user_home "$u")
    [ -e "/var/lib/systemd/linger/$u" ] || echo "linger $u"
    echo "service $u"
    echo "dir $home/.local/state/hex"
    echo "dir /run/user/$uid/hex"
    echo "dir /run/user/$uid/hex-node"
    echo "cgroup /sys/fs/cgroup/user.slice/user-$uid.slice/user@$uid.service/app.slice/hex-node.service"
  done
  [ -d "$STATE" ] || echo "folder $STATE"
  echo "file $MANIFEST"
}

# Whether one plan line's thing is on the machine.
present() {
  local kind=$1 target=$2
  case $kind in
    file) [ -f "$target" ] ;;
    link) [ -L "$target" ] ;;
    dir | folder | cgroup) [ -d "$target" ] ;;
    linger) [ -e "/var/lib/systemd/linger/$target" ] ;;
    service)
      local uid
      uid=$(id -u "$target" 2>/dev/null) || return 1
      [ -d "/run/user/$uid/systemd" ] || return 1
      user_ctl "$target" is-active --quiet hex-node.service 2>/dev/null
      ;;
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

preflight() {
  [ "$(stat -fc %T /sys/fs/cgroup)" = cgroup2fs ] ||
    die "cgroup v2 isn't mounted at /sys/fs/cgroup; Hex's boxes need the unified hierarchy"
  [ -d /run/systemd/system ] || die "systemd isn't this machine's init; the installer sets Hex up through it"
  local v
  v=$(systemctl --version | awk 'NR==1 {print $2}')
  [ "${v%%.*}" -ge "$SYSTEMD_MIN" ] 2>/dev/null ||
    die "systemd $v is older than $SYSTEMD_MIN, which DelegateSubgroup= needs"
}

write_file() { # <path> <mode>, content on stdin
  mkdir -p "$(dirname "$1")"
  cat >"$1"
  chmod "$2" "$1"
}

# Waits (bounded) for a condition; says which and fails when it doesn't come.
wait_for() {
  local what=$1 i
  shift
  for ((i = 0; i < WAIT_STEPS; i++)); do
    "$@" && return 0
    sleep 0.1
  done
  echo "linux.sh: $what didn't happen within $((WAIT_STEPS / 10)) s" >&2
  return 1
}

# Re-executes every running user manager (user@.service's reload), keeping its processes: a manager
# reads the controllers it may hand on only when it starts, so daemon-reload alone leaves a running one
# without the new delegation (docs/research/runtime/installer-per-os.md §1.4; measured in the lab).
reload_user_managers() {
  local unit
  for unit in $(systemctl list-units 'user@*.service' --state=running --plain --no-legend | awk '{print $1}'); do
    timeout $((WAIT_STEPS / 10)) systemctl reload "$unit"
  done
}

install_hex() {
  [ -n "$BINARY" ] || die "install needs --binary <hex>"
  [ -x "$BINARY" ] || die "$BINARY isn't an executable"
  preflight
  local u
  for u in "${USERS[@]}"; do id -u "$u" >/dev/null 2>&1 || die "no user $u"; done
  if [ "$DRY_RUN" = 1 ]; then plan; return; fi
  [ "$(id -u)" = 0 ] || die "install is the admin step: run it as root (sudo)"
  [ ! -e "$MANIFEST" ] || die "Hex is installed already ($MANIFEST); uninstall first"
  local made
  made=$(plan)
  mkdir -p "$STATE"
  # The plan is the manifest from the start, so an install cut short is undone by uninstall too.
  printf '%s\n' "$made" >"$MANIFEST"
  install -D -m 0755 "$BINARY" "$BIN"
  write_file "$DROPIN" 0644 <<EOF
# Installed by Hex's installer (scripts/install/linux.sh): every user's manager is delegated the
# controllers Hex's boxes use, so a user's hex-node service hands them on to its boxes, and a hard
# descriptor limit its hex-node service may take up to (a user unit can't exceed its manager's; the soft
# limit everything else starts with stays systemd's 1024). docs/research/runtime/nofile-limit.md.
[Service]
Delegate=cpu io memory pids
LimitNOFILE=1024:$NOFILE
EOF
  write_file "$UNIT" 0644 <<EOF
# Installed by Hex's installer (scripts/install/linux.sh): the node agent, serving its socket at
# \$XDG_RUNTIME_DIR/hex/node.sock. It runs in the leaf "agent" and its boxes in "hex" beside it.
[Unit]
Description=Hex node agent
Documentation=https://github.com/hyper-light/hex

[Service]
Type=exec
ExecStart=$BIN node --socket %t/hex/node.sock --state %S/hex
RuntimeDirectory=hex
StateDirectory=hex
Delegate=yes
DelegateSubgroup=agent
# Room for the node's own descriptors at its bounds (330,112) and every kept network namespace's held
# /proc/net files (4 x 66,048): 594,304. 1,048,576 is the kernel's default fs.nr_open, the most a hard
# limit may be without that sysctl changed, and dockerd's before moby dropped infinity
# (docs/research/runtime/nofile-limit.md §2-§3, §5).
LimitNOFILE=$NOFILE
Restart=on-failure

[Install]
WantedBy=default.target
EOF
  systemctl daemon-reload
  systemctl --global enable hex-node.service
  local u uid
  for u in "${USERS[@]}"; do loginctl enable-linger "$u"; done
  reload_user_managers
  for u in "${USERS[@]}"; do
    uid=$(user_uid "$u")
    # The user's manager is up once its private socket is.
    wait_for "$u's manager starting" test -S "/run/user/$uid/systemd/private" || die "$u's manager didn't start"
    # The manager finishes starting (bounded) before it is asked for the node.
    user_ctl "$u" is-system-running --wait >/dev/null || true
    user_ctl "$u" daemon-reload
    user_ctl "$u" start hex-node.service
    if ! wait_for "$u's node serving /run/user/$uid/hex/node.sock" test -S "/run/user/$uid/hex/node.sock"; then
      user_ctl "$u" status hex-node.service --no-pager >&2 || true
      journalctl --no-pager -n 5 _UID="$uid" _COMM=hex >&2 || true
      die "$u's node didn't come up (its status is above)"
    fi
    echo "$u's node serves /run/user/$uid/hex/node.sock"
  done
  echo "installed; uninstall with: sudo bash $0 uninstall"
}

uninstall_hex() {
  local list
  list=$(if [ -f "$MANIFEST" ]; then cat "$MANIFEST"; else plan; fi)
  if [ "$DRY_RUN" = 1 ]; then printf '%s\n' "$list"; return; fi
  [ "$(id -u)" = 0 ] || die "uninstall is the admin step: run it as root (sudo)"
  local kind target
  # The services stop first, so their boxes and runtime directories go with them.
  while read -r kind target; do
    [ "$kind" = service ] || continue
    if present service "$target"; then
      user_ctl "$target" stop hex-node.service
    fi
  done <<<"$list"
  [ -L "$WANTS" ] && systemctl --global disable hex-node.service
  while read -r kind target; do
    case $kind in
      file | link) [ "$target" = "$MANIFEST" ] || rm -f -- "${target:?}" ;;
      dir)
        # Only Hex's own directories, by name: a manifest line never removes anything else.
        case ${target##*/} in
          hex | hex-node) rm -rf -- "${target:?}" ;;
          *) die "refusing to remove $target: not one of Hex's directories" ;;
        esac
        ;;
      folder) ;;
      linger) loginctl disable-linger "$target" ;;
      service | cgroup) ;;
      *) die "unknown manifest line kind $kind" ;;
    esac
  done <<<"$list"
  # Folders install made, once emptied of what it put there.
  while read -r kind target; do
    [ "$kind" = folder ] && [ -d "$target" ] && rmdir --ignore-fail-on-non-empty -- "$target"
  done <<<"$list"
  systemctl daemon-reload
  reload_user_managers
  rm -f -- "${MANIFEST:?}"
  rmdir --ignore-fail-on-non-empty "$STATE"
  local saved
  saved=$(mktemp)
  printf '%s\n' "$list" >"$saved"
  check "$saved" || { rm -f -- "${saved:?}"; die "uninstall left the lines above"; }
  rm -f -- "${saved:?}"
}

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
    --dry-run) DRY_RUN=1; shift ;;
    *) [ "$VERB" = check ] && [ "$#" = 1 ] && break; die "unknown argument $1" ;;
  esac
done

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
