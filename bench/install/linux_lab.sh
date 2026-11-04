#!/usr/bin/env bash
# The Linux installer's lab (native plan step 6; HEX-INST-1, HEX-INST-3, HEX-INST-4): a Fedora machine
# fresh of Hex, as a privileged container that boots systemd, with the host's /usr (read-only, so its
# systemd) and a cgroup namespace of its own. Inside it (linux_lab_inside.sh):
#   1. a user whose manager already runs (lingering, as a CI runner's does) is refused both Linux
#      classes by name by a node started before install, saying the io controller is missing;
#   2. the installer's dry run lists its plan, and install runs once as root;
#   3. the installed node serves both classes to that user with no further step (the conformance suite
#      over its socket, node_socket.rs's an_installed_node_serves_every_class);
#   4. uninstall leaves nothing of the plan on the machine, listed line by line.
# The image's /etc masks Fedora desktop's uresourced drop-in (it delegates io itself), so the machine
# is a server edition's: user@.service delegates systemd's own pids, memory and cpu.
# Usage: linux_lab.sh <hex binary> <node_socket test binary> <record file>
set -euo pipefail
HEX=$(realpath "${1:?hex binary}")
TESTBIN=$(realpath "${2:?node_socket test binary}")
RECORD=${3:?record file}
IMAGE=${IMAGE:-registry.fedoraproject.org/fedora:44}
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
REPO=$(realpath "$HERE/../..")
NAME=hex-native-s6-lab-$(date -u +%Y%m%dT%H%M%SZ)
# fedora's limits for every container (memory fedora-unit-limits): 8 GiB, first for the OOM killer.
MEM=${BENCH_MEMORY:-8g}
OOM=${BENCH_OOM:-1000}
# The lab user's home is a host directory of this run's (the container's own root is overlayfs, on
# which a box's writable layer is refused by name), owned by the host user, whose uid the lab user takes.
LAB_HOME=$HOME/.cache/hex-t7/native-s6/home-${NAME#hex-native-s6-lab-}
mkdir -p "$LAB_HOME"
# Every path the binaries name lives at the same path inside.
mounts=(-v /usr:/usr:ro --tmpfs /usr/local:rw,exec -v "$HERE:/opt/lab:ro" -v "$REPO/scripts/install:/opt/install:ro"
  -v "$LAB_HOME:/home/hexlab" -e LAB_UID="$(id -u)")
# The test names the hex binary it was built beside (CARGO_BIN_EXE_hex), so its profile's directory.
BUILT=$(dirname "$(dirname "$TESTBIN")")
mounts+=(-v "$BUILT:$BUILT:ro")
case $HEX in "$BUILT"/*) ;; *) mounts+=(-v "$HEX:$HEX:ro") ;; esac
# A console (-t), so systemd's own words are in the container's log when it can't start.
cid=$(docker run -d -t --name "$NAME" --privileged --security-opt label=disable --cgroupns=private \
  --network=none --memory "$MEM" --oom-score-adj "$OOM" \
  --tmpfs /run:rw,exec --tmpfs /run/lock --tmpfs /tmp:rw,exec -e container=docker \
  "${mounts[@]}" "$IMAGE" /usr/lib/systemd/systemd)
echo "lab container $NAME ($cid)"
set +e
docker exec "$NAME" bash /opt/lab/linux_lab_inside.sh "$HEX" "$TESTBIN" 2>&1 | tee "$RECORD"
status=${PIPESTATUS[0]}
set -e
if [ "$status" != 0 ] && [ "$(docker inspect -f '{{.State.Status}}' "$NAME")" != running ]; then
  echo "the lab container ended ($(docker inspect -f 'exit {{.State.ExitCode}}' "$NAME")); its systemd said:" | tee -a "$RECORD"
  docker logs --tail 8 "$NAME" 2>&1 | tr -d '\r' | sed 's/\x1b\[[0-9;:?]*[A-Za-z]//g' | tee -a "$RECORD"
fi
# The container is this lab's, by the name it was given.
docker rm -f "$NAME" >/dev/null
exit "$status"
