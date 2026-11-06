#!/usr/bin/env bash
# Inside the installer's lab container (linux_lab.sh), as root, systemd its init. Prints a record; exits
# non-zero at the first step that fails.
# Usage: linux_lab_inside.sh <hex binary> <node_socket test binary>
set -euo pipefail
HEX=${1:?hex binary}
TESTBIN=${2:?node_socket test binary}
INSTALL=/opt/install/linux.sh
U=hexlab
step() { echo; echo "## $*"; }
# Waits until a command succeeds, at most LAB_BOUND_S (the host systemd's unit start bound, linux_lab.sh).
# The 0.1 s between tries paces the checks; it bounds nothing.
wait_until() {
  local what=$1 until=$((SECONDS + LAB_BOUND_S))
  shift
  until "$@"; do
    [ "$SECONDS" -lt "$until" ] || { echo "FAIL: $what didn't happen within $LAB_BOUND_S s"; return 1; }
    sleep 0.1
  done
}

# docker exec puts this shell in the cgroup namespace's root, where cgroup v2's no-internal-processes
# rule keeps systemd from handing controllers down: it moves into systemd's own leaf first (once systemd
# has made it), and systemd enables them when the root is empty.
wait_until "systemd making init.scope" test -d /sys/fs/cgroup/init.scope
echo $$ >/sys/fs/cgroup/init.scope/cgroup.procs

step "the machine: $(date -u +%Y-%m-%dT%H:%M:%SZ), $(systemctl --version | head -1), kernel $(uname -r)"
running() { case $(systemctl is-system-running 2>/dev/null) in running | degraded) ;; *) return 1 ;; esac; }
wait_until "systemd finishing its start" running
echo "systemd: $(systemctl is-system-running)"
# The image's /etc lacks what Fedora's install makes at enable time from /usr: its system users and
# the dbus.service aliases (system and user), without which logind, lingering and a user's own
# systemd-run can't start.
systemd-sysusers >/dev/null 2>&1
systemctl enable dbus-broker.service
systemctl --global enable dbus-broker.service dbus.socket
systemctl reset-failed
systemctl start dbus.socket systemd-logind

# A server edition's user@.service: mask Fedora desktop's uresourced drop-in, which delegates io itself.
mkdir -p /etc/systemd/system/user@.service.d
ln -sf /dev/null /etc/systemd/system/user@.service.d/00-uresourced.conf
systemctl daemon-reload
useradd -M -d /home/$U -u "$LAB_UID" $U
chown $U: /home/$U
UID_=$(id -u $U)
# The user's manager runs before Hex is installed, as a CI runner's does.
loginctl enable-linger $U
wait_until "$U's manager opening its bus" test -S /run/user/$UID_/bus
systemctl show user@$UID_.service -p DelegateControllers
# setpriv, not runuser: the container has no PAM session to give.
as_user() {
  setpriv --reuid=$U --regid=$U --init-groups -- env XDG_RUNTIME_DIR=/run/user/$UID_ DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$UID_/bus \
    HOME=/home/$U "$@"
}
# The fact to wait on: the user's manager has finished starting (bounded).
echo "user manager: $(as_user timeout "$LAB_BOUND_S" systemctl --user is-system-running --wait || true)"

step "fresh of Hex: the installer's check lists nothing"
bash $INSTALL check

step "HEX-INST-1: before install, a node in a delegated unit of the user's is refused both classes by name"
for d in "" user.slice user.slice/user-$UID_.slice user.slice/user-$UID_.slice/user@$UID_.service \
  user.slice/user-$UID_.slice/user@$UID_.service/app.slice; do
  echo "/$d: controllers [$(cat /sys/fs/cgroup/$d/cgroup.controllers)], enabled [$(cat /sys/fs/cgroup/$d/cgroup.subtree_control)]"
done
set +e
said=$(as_user systemd-run --user --quiet --wait --pipe -p Delegate=yes -p DelegateSubgroup=agent \
  "$HEX" node --socket /run/user/$UID_/pre.sock --state /home/$U/pre-state 2>&1)
rc=$?
set -e
echo "exit $rc: $said"
[ "$rc" != 0 ] || { echo "FAIL: the node served without the delegation"; exit 1; }
case $said in
  *"the box root of the process and oci classes isn't available on linux: the io controller isn't delegated to"*) echo "PASS: refused by name" ;;
  *) echo "FAIL: not the by-name refusal"; exit 1 ;;
esac
rm -rf -- "/home/${U:?}/pre-state"

step "the plan (install --dry-run)"
bash $INSTALL install --binary "$HEX" --user $U --dry-run | tee /tmp/plan
grep -q "^linger " /tmp/plan && { echo "FAIL: the lab's linger isn't install's"; exit 1; }

step "install: the one admin step"
MANAGER_BEFORE=$(systemctl show -p MainPID --value user@$UID_.service)
bash $INSTALL install --binary "$HEX" --user $U
bash $INSTALL check /tmp/plan || true

step "HEX-INST-8: the installed node is up exactly when it serves (Type=notify), no wait of the installer's"
TYPE=$(systemctl show -p Type --value hex-node@$U.service)
echo "hex-node@$U.service: Type=$TYPE, start bound $(systemctl show -p TimeoutStartUSec --value hex-node@$U.service)"
[ "$TYPE" = notify ] || { echo "FAIL: the unit isn't Type=notify"; exit 1; }
[ -S /run/hex/$U/node.sock ] || { echo "FAIL: install returned before the node served"; exit 1; }
echo "PASS: up when it serves"

step "HEX-INST-7: install leaves $U's running manager as it was (a CI runner's work runs under it)"
MANAGER_AFTER=$(systemctl show -p MainPID --value user@$UID_.service)
echo "user@$UID_.service main pid: $MANAGER_BEFORE before, $MANAGER_AFTER after"
[ "$MANAGER_BEFORE" = "$MANAGER_AFTER" ] || { echo "FAIL: install restarted $U's manager"; exit 1; }
echo "PASS: the manager wasn't restarted"

# The node's descriptors at its bounds (330,112) and the four /proc/net files each of up to 66,048 kept
# network namespaces may hold (cp/t7-ext-s6 crates/hex-node/src/netns.rs, FilesBound: 256 x 2 + 65,536
# kept, four files each): 330,112 + 264,192.
NOFILE_NEED=594304
step "HEX-INST-5: the installed node's soft descriptor limit covers its own and every kept namespace's files"
NODE_PID=$(systemctl show -p MainPID --value hex-node@$U.service)
grep "Max open files" /proc/$NODE_PID/limits
soft=$(awk '/Max open files/ {print $4}' /proc/$NODE_PID/limits)
if [ "$soft" = unlimited ] || [ "$soft" -ge "$NOFILE_NEED" ]; then
  echo "PASS: soft $soft >= $NOFILE_NEED"
else
  echo "FAIL: soft $soft < $NOFILE_NEED"; exit 1
fi

step "HEX-INST-3: the installed node serves every class to $U with no further step"
set +e
as_user env HEX_INSTALLED_SOCKET=/run/hex/$U/node.sock \
  "$TESTBIN" an_installed_node_serves_every_class --exact --nocapture --test-threads=1 2>&1 | tail -20
rc=${PIPESTATUS[0]}
set -e
[ "$rc" = 0 ] || { echo "FAIL: the installed node's conformance (exit $rc)"; exit 1; }
echo "PASS: conformance over the installed node"

step "HEX-INST-4: uninstall, then the plan's every line is checked gone"
bash $INSTALL uninstall
bash $INSTALL check /tmp/plan
echo "PASS: nothing of Hex's is left"
loginctl disable-linger $U
