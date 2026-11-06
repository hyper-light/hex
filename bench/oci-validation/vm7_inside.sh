#!/usr/bin/env bash
# The guest's PID 1 after /sbin/vm7-boot (vm7_lab.sh): the mounts a host has, the target dir at the path it was
# built at, the kernel's version, the open_tree probe, then seam_root_inside.sh's run of the tests as root. Its
# exit code goes to the run's directory (/job/exit), and the guest powers off: PID 1 never returns. In validate
# mode (vm7_validate.sh) the shared dir holds runtime-tools' checkout (rt/), the hex binary, a tar (the guest has
# none; the suite unpacks its rootfs with it) and out/, where inside.sh runs the suite against `hex oci` as
# root, as run_hex.sh does in a container, and leaves results.tsv, handed back to the shared dir's owner.
# In breakdown mode (vm7_breakdown.sh) vm7_breakdown_inside.sh runs the create breakdown from the shared dir.
set -uo pipefail
mkdir -p /dev/pts /dev/shm /sys/fs/cgroup /run /tmp
mount -t devpts -o newinstance,ptmxmode=0666 devpts /dev/pts
mount -t tmpfs -o nosuid,nodev shm /dev/shm
mount -t cgroup2 cgroup2 /sys/fs/cgroup
mount -t tmpfs -o rw,exec,nosuid,size=1g run /run
mount -t tmpfs -o rw,nosuid,nodev,mode=1777 tmp /tmp
ip link set lo up
TGT=$(cat /job/tgt)
mkdir -p "$TGT"
mount -t 9p -o trans=virtio,version=9p2000.L,msize=1048576,cache=loose tgt "$TGT"
export HOME=/root PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin
echo "guest kernel: $(uname -r)"
python3 -I /opt/lab/vm7_probe.py
if [ "$(cat /job/mode 2>/dev/null)" = validate ]; then
  PATH=$TGT/bin:$PATH RT=$TGT/rt OUT=$TGT/out RUNTIME=$TGT/out/hex-oci HEX_OCI_ROOT=/run/hex-oci \
    bash /opt/lab/inside.sh
  rc=$?
  chown -R "$(stat -c %u:%g "$TGT")" "$TGT/out"
elif [ "$(cat /job/mode 2>/dev/null)" = breakdown ]; then
  bash /opt/lab/vm7_breakdown_inside.sh "$TGT"
  rc=$?
else
  mapfile -t TESTS < /job/args
  bash /opt/lab/seam_root_inside.sh "${TESTS[@]}"
  rc=$?
fi
echo "$rc" > /job/exit
echo "VM7_EXIT=$rc"
sync
umount /job "$TGT"
echo o > /proc/sysrq-trigger
# PID 1 must outlive the power-off it asked for; vm7_lab.sh's VM_TIMEOUT bounds the guest.
exec sleep infinity
