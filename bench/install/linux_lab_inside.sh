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

# docker exec puts this shell in the cgroup namespace's root, where cgroup v2's no-internal-processes
# rule keeps systemd from handing controllers down: it moves into systemd's own leaf first (once systemd
# has made it), and systemd enables them when the root is empty.
for ((i = 0; i < 300; i++)); do [ -d /sys/fs/cgroup/init.scope ] && break; sleep 0.1; done
echo $$ >/sys/fs/cgroup/init.scope/cgroup.procs

step "the machine: $(date -u +%Y-%m-%dT%H:%M:%SZ), $(systemctl --version | head -1), kernel $(uname -r)"
wait_running() {
  local i s
  for ((i = 0; i < 300; i++)); do
    s=$(systemctl is-system-running 2>/dev/null || true)
    case $s in running | degraded) echo "systemd: $s"; return 0 ;; esac
    sleep 0.1
  done
  echo "systemd didn't finish starting: $s"; return 1
}
wait_running
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
for ((i = 0; i < 300; i++)); do [ -S /run/user/$UID_/bus ] && break; sleep 0.1; done
systemctl show user@$UID_.service -p DelegateControllers
# setpriv, not runuser: the container has no PAM session to give.
as_user() {
  setpriv --reuid=$U --regid=$U --init-groups -- env XDG_RUNTIME_DIR=/run/user/$UID_ DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$UID_/bus \
    HOME=/home/$U "$@"
}
# The fact to wait on: the user's manager has finished starting (bounded).
echo "user manager: $(as_user timeout 30 systemctl --user is-system-running --wait || true)"

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
bash $INSTALL install --binary "$HEX" --user $U
systemctl show user@$UID_.service -p DelegateControllers
bash $INSTALL check /tmp/plan || true

# The node's descriptors at its bounds (330,112) and the four /proc/net files each of up to 66,048 kept
# network namespaces may hold (cp/t7-ext-s6 crates/hex-node/src/netns.rs, FilesBound: 256 x 2 + 65,536
# kept, four files each): 330,112 + 264,192.
NOFILE_NEED=594304
step "HEX-INST-5: the installed node's soft descriptor limit covers its own and every kept namespace's files"
NODE_PID=$(as_user systemctl --user show -p MainPID --value hex-node.service)
grep "Max open files" /proc/$NODE_PID/limits
soft=$(awk '/Max open files/ {print $4}' /proc/$NODE_PID/limits)
if [ "$soft" = unlimited ] || [ "$soft" -ge "$NOFILE_NEED" ]; then
  echo "PASS: soft $soft >= $NOFILE_NEED"
else
  echo "FAIL: soft $soft < $NOFILE_NEED"; exit 1
fi

step "HEX-INST-3: the installed node serves every class to $U with no further step"
set +e
as_user env HEX_INSTALLED_SOCKET=/run/user/$UID_/hex/node.sock \
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
