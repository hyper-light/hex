#!/usr/bin/env bash
# seam_root.sh's lab (the node agent's seam as root) on a Linux 7.x kernel (goal 6, mount namespaces without
# copying): 7.1 brought open_tree(OPEN_TREE_NAMESPACE) and clone3(CLONE_EMPTY_MNTNS), which the lab hosts' 6.19
# lacks, so the tests run as root in a KVM guest booting Fedora's 7.2 kernel build (vm7_build.sh makes it once,
# under VM_DIR, ~/.cache/hex-t7/g6/vm). QEMU runs in a container from vm7_build.sh's image, held to the shared
# host's caps (CPUs 16-31, 8 GB, the OOM killer's first pick; VM_CPUS, BENCH_MEMORY, BENCH_OOM), with no
# network; the guest has VM_MEM (6G) and VM_SMP (8) vCPUs. Each run boots a fresh guest from the built root,
# kept as built (-snapshot), and powers it off at the end: a few seconds' boot buys a guest no earlier run
# touched. The test binaries' target dir is shared into the guest (9p) at the path it was built at, so the
# tests find the `hex` binary they name; the guest's PID 1 (vm7_inside.sh) mounts a tmpfs /run, as on a host,
# where a root node keeps its containers' state (decision 1), and cgroup v2 at its root, whose controllers
# seam_root_inside.sh hands down to a cgroup per test. HOME is /root on the guest's ext4 root, since a layered
# box's writable layer is refused on overlayfs and 9p alike. Prints the guest's kernel first, then whether
# open_tree takes OPEN_TREE_NAMESPACE (vm7_probe.py), then each test's tail. Exits non-zero on any failure.
# The serial console's log and the exit code stay in VM_DIR/runs/<UTC>-<pid>/.
# VM7_MODE=validate runs runtime-tools' validation suite instead (vm7_validate.sh), from the shared dir.
# VM7_MODE=breakdown runs the create breakdown instead (vm7_breakdown.sh), from the shared dir.
# Usage: vm7_lab.sh <target dir the tests were built in> <test binary>...
set -euo pipefail
TGT=$(realpath "${1:?target dir}"); shift
[ "$#" -gt 0 ] || { echo "usage: vm7_lab.sh <target dir> <test binary>..." >&2; exit 2; }
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
# Its container carries the run's labels and goes with the run, however it ends (containers.sh).
. "$(dirname "$(realpath "${BASH_SOURCE[0]}")")/../competitive/containers.sh"
VM_DIR=${VM_DIR:-$HOME/.cache/hex-t7/g6/vm}
IMG=hexlab-vm7:44
for f in vmlinuz initramfs.img rootfs.ext4 KERNEL; do
  [ -s "$VM_DIR/$f" ] || { echo "refusing: $VM_DIR/$f is missing; run vm7_build.sh first" >&2; exit 2; }
done
for b in "$@"; do
  case $(realpath "$b") in "$TGT"/*) ;; *) echo "refusing: $b is outside $TGT, which alone is shared" >&2; exit 2 ;; esac
done
RUN=$(date -u +%Y%m%dT%H%M%SZ)-$$
JOB=$VM_DIR/runs/$RUN
mkdir -p "$JOB"
printf '%s\n' "$TGT" > "$JOB/tgt"
for b in "$@"; do realpath "$b"; done > "$JOB/args"
printf '%s\n' "${VM7_MODE:-seam}" > "$JOB/mode"
echo "vm7 lab run $RUN"
cat "$VM_DIR/KERNEL"
NAME=hexlab-vm7-$RUN
bench_trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true'
set +e
docker run --rm --init --name "$NAME" "${BENCH_LABELS[@]}" --device /dev/kvm --security-opt label=disable --network=none \
  --cpuset-cpus "${VM_CPUS:-16-31}" --memory "${BENCH_MEMORY:-8g}" --oom-score-adj "${BENCH_OOM:-1000}" \
  -v "$VM_DIR:/vm:ro" -v "$HERE:/opt/lab:ro" -v "$JOB:/job" -v "$TGT:$TGT" "$IMG" \
  timeout "${VM_TIMEOUT:-3600}" qemu-system-x86_64 -machine q35,accel=kvm -cpu host \
    -smp "${VM_SMP:-8}" -m "${VM_MEM:-6G}" -nodefaults -display none -serial stdio -no-reboot -snapshot \
    -kernel /vm/vmlinuz -initrd /vm/initramfs.img \
    -append "console=ttyS0 root=/dev/vda rw init=/sbin/vm7-boot panic=-1 quiet loglevel=3 systemd.show_status=0 rd.udev.log_level=3" \
    -drive file=/vm/rootfs.ext4,format=raw,if=virtio \
    -virtfs local,path=/opt/lab,mount_tag=lab,security_model=none,readonly=on \
    -virtfs local,path=/job,mount_tag=job,security_model=passthrough \
    -virtfs local,path="$TGT",mount_tag=tgt,security_model=passthrough \
  2>&1 | tee "$JOB/console.log"
qrc=${PIPESTATUS[0]}
set -e
rc=$(cat "$JOB/exit" 2>/dev/null || echo "none")
echo "vm7 lab run $RUN: qemu exit $qrc, guest exit $rc (log $JOB/console.log)"
[ "$rc" = 0 ]
