#!/usr/bin/env bash
# Hex's installer for Linux: the one admin step (native plan step 6; docs/design/runtime.md, decision
# "the installer, per OS"; sources in docs/research/runtime/installer-per-os.md and nofile-limit.md). It
# sets up what the process and oci classes' cgroup v2 delegation needs, so each named user's node runs
# with no further step:
#   - the `hex` binary at /usr/local/bin/hex;
#   - the `hex-node@.service` system template: a node per user, run as that user (User=), its cgroup
#     delegated to it (Delegate=yes, every controller the system has; DelegateSubgroup=agent, so the node
#     runs in a leaf and its boxes in a sibling), its descriptor limit set by root (LimitNOFILE=);
#   - for each --user, `hex-node@<user>.service` enabled and started, serving /run/hex/<user>/node.sock.
# No user's own manager (user@.service) is touched: one that already runs can't take a new hard limit
# without a restart, which would end what runs under it (measured in the lab; the lead, 2026-11-05).
# Every thing it makes is a line of its plan ("<kind> <target>"), written to the manifest before it is
# made; uninstall undoes the manifest's lines and then lists what is left, which is nothing.
#
# usage: linux.sh install --binary <hex> --user <name> [--user <name>]... [--dry-run]
#        linux.sh uninstall [--dry-run]
#        linux.sh check [<plan file>]
# `--dry-run` prints the plan and makes nothing. `check` lists what of a plan (default: the manifest's,
# else the fixed paths) is still on the machine and exits 1 when anything is.
set -euo pipefail

BIN=/usr/local/bin/hex
UNIT=/etc/systemd/system/hex-node@.service
WANTS=/etc/systemd/system/multi-user.target.wants
RUN=/run/hex
LIB=/var/lib/hex
STATE=/var/lib/hex-install
MANIFEST=$STATE/manifest
# The node's descriptor limit, soft and hard (the unit below says why).
NOFILE=1048576
# DelegateSubgroup= is systemd 254's (systemd.resource-control(5)).
SYSTEMD_MIN=254
# How long a node gets to come up: a bounded wait, counted in 0.1 s steps.
WAIT_STEPS=300

die() { echo "linux.sh: $*" >&2; exit 1; }

usage() {
  sed -n '/^# usage:/,/^# `check`/p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

# The plan: every thing install makes, one "<kind> <target>" line each.
plan() {
  local d u
  for d in "$(dirname "$BIN")" "$WANTS" "$RUN" "$LIB"; do
    [ -d "$d" ] || echo "folder $d"
  done
  echo "file $BIN"
  echo "file $UNIT"
  for u in "${USERS[@]}"; do
    echo "link $WANTS/hex-node@$u.service"
    echo "service $u"
    echo "dir $RUN/$u"
    echo "dir $LIB/$u"
    echo 'cgroup /sys/fs/cgroup/system.slice/system-hex\x2dnode.slice/hex-node@'"$u"'.service'
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
    service) systemctl is-active --quiet "hex-node@$target.service" ;;
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

install_hex() {
  [ -n "$BINARY" ] || die "install needs --binary <hex>"
  [ -x "$BINARY" ] || die "$BINARY isn't an executable"
  [ "${#USERS[@]}" -gt 0 ] || die "install needs --user <name>: whose node it starts"
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
  write_file "$UNIT" 0644 <<EOF
# Installed by Hex's installer (scripts/install/linux.sh): user %i's node agent, serving its socket at
# $RUN/%i/node.sock. It runs as %i in the leaf "agent" of a cgroup delegated to %i, its boxes in "hex"
# beside it; its containers' state is on the tmpfs under $RUN/%i.
[Unit]
Description=Hex node agent for %i
Documentation=https://github.com/hyper-light/hex

[Service]
Type=exec
User=%i
ExecStart=$BIN node --socket $RUN/%i/node.sock --state $LIB/%i
Environment=XDG_RUNTIME_DIR=$RUN/%i
RuntimeDirectory=hex/%i
RuntimeDirectoryMode=0700
StateDirectory=hex/%i
StateDirectoryMode=0700
Delegate=yes
DelegateSubgroup=agent
# Room for the node's own descriptors at its bounds (330,112) and every kept network namespace's held
# /proc/net files (4 x 66,048): 594,304. 1,048,576 is the kernel's default fs.nr_open, the most a hard
# limit may be without that sysctl changed, and dockerd's before moby dropped infinity
# (docs/research/runtime/nofile-limit.md §2-§3, §5). A system unit's limit is root's to set, where a user
# unit's is capped at its manager's (§1.3).
LimitNOFILE=$NOFILE
Restart=on-failure

[Install]
WantedBy=multi-user.target
EOF
  systemctl daemon-reload
  for u in "${USERS[@]}"; do
    systemctl enable --now "hex-node@$u.service"
    if ! wait_for "$u's node serving $RUN/$u/node.sock" test -S "$RUN/$u/node.sock"; then
      systemctl status "hex-node@$u.service" --no-pager >&2 || true
      die "$u's node didn't come up (its status is above)"
    fi
    echo "$u's node serves $RUN/$u/node.sock"
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
    systemctl disable --now "hex-node@$target.service" 2>/dev/null || true
  done <<<"$list"
  while read -r kind target; do
    case $kind in
      file | link) [ "$target" = "$MANIFEST" ] || rm -f -- "${target:?}" ;;
      dir)
        # Only a user's own directory under Hex's two, by place: a manifest line removes nothing else.
        case $(dirname "$target") in
          "$RUN" | "$LIB") rm -rf -- "${target:?}" ;;
          *) die "refusing to remove $target: not one of Hex's directories" ;;
        esac
        ;;
      folder | service | cgroup) ;;
      *) die "unknown manifest line kind $kind" ;;
    esac
  done <<<"$list"
  # Folders install made, once emptied of what it put there.
  while read -r kind target; do
    [ "$kind" = folder ] && [ -d "$target" ] && rmdir --ignore-fail-on-non-empty -- "$target"
  done <<<"$list"
  systemctl daemon-reload
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
