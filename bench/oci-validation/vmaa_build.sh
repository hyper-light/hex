#!/usr/bin/env bash
# Builds an AppArmor guest for vm7_lab.sh once (goal 6, AC-T7.1.4: process.apparmorProfile applied where
# AppArmor runs). The lab hosts run SELinux (fedora) or neither, and no one kernel runs both
# (docs/research/runtime/goal6-lsm.md §4), so the AppArmor half runs in a KVM guest: Ubuntu 24.04 booting its
# own linux-image-virtual kernel, AppArmor its major LSM as Ubuntu builds it, with the apparmor tools, crun (the
# suite's reference on this kernel) and the
# profile runtime-tools' linux_process_apparmor_profile names (acme_secure_profile) loaded at boot. It is the
# vm7 guest's layout: vm7_lab.sh boots it with VM_DIR set to this one's dir, and its PID 1 (/sbin/vm7-boot)
# mounts the lab's shares and hands over to vm7_inside.sh, as the Fedora guest's does. Into VM_DIR
# (~/.cache/hex-t7/lsm/vm-aa): vmlinuz, initramfs.img, rootfs.ext4 and KERNEL. The build runs in an
# ubuntu:24.04 container held to the shared host's caps; nothing is installed on the host.
# Rebuilds only what's missing; FORCE=1 rebuilds it all.
# Usage: vmaa_build.sh
set -euo pipefail
# Its containers carry the run's labels and go with the run, however it ends (containers.sh).
. "$(dirname "$(realpath "${BASH_SOURCE[0]}")")/../competitive/containers.sh"
bench_trap :
VM_DIR=${VM_DIR:-$HOME/.cache/hex-t7/lsm/vm-aa}
say() { echo "$(date -u +%FT%TZ) $*"; }
mkdir -p "$VM_DIR"
[ "${FORCE:-0}" = 1 ] && rm -f "${VM_DIR:?}/vmlinuz" "${VM_DIR:?}/initramfs.img" "${VM_DIR:?}/rootfs.ext4"
if [ ! -s "$VM_DIR/rootfs.ext4" ] || [ ! -s "$VM_DIR/initramfs.img" ] || [ ! -s "$VM_DIR/vmlinuz" ]; then
  say "guest root, initramfs and kernel (Ubuntu 24.04)"
  docker run --rm "${BENCH_LABELS[@]}" --privileged --security-opt label=disable \
    --cpuset-cpus "${VM_CPUS:-16-31}" --memory "${BENCH_MEMORY:-8g}" --oom-score-adj "${BENCH_OOM:-1000}" \
    -v "$VM_DIR:/vm" ubuntu:24.04 bash -euc '
    export DEBIAN_FRONTEND=noninteractive
    apt-get -qq update && apt-get -qq install -y debootstrap e2fsprogs > /dev/null
    R=/build/root; mkdir -p $R
    debootstrap --variant=minbase --components=main,universe \
      --include=bash,coreutils,util-linux,procps,findutils,diffutils,iproute2,kmod,python3,tar,gzip,apparmor,crun,initramfs-tools \
      noble $R http://archive.ubuntu.com/ubuntu > /dev/null
    mount -t proc proc $R/proc; mount -t sysfs sys $R/sys; mount --bind /dev $R/dev
    printf "virtio_pci\nvirtio_blk\next4\n9p\n9pnet_virtio\n" >> $R/etc/initramfs-tools/modules
    chroot $R apt-get -qq install -y --no-install-recommends linux-image-virtual > /dev/null
    KVER=$(ls $R/lib/modules | head -1)
    chroot $R update-initramfs -c -k "$KVER" > /dev/null
    umount $R/dev $R/sys $R/proc
    cp $R/boot/vmlinuz-$KVER /vm/vmlinuz; cp $R/boot/initrd.img-$KVER /vm/initramfs.img
    mkdir -p $R/etc/apparmor.d/hex-lab
    # The profile runtime-tools sets, in complain mode: it allows what the suite does and still names the
    # process, which the tests read back from /proc/<pid>/attr/apparmor/current.
    cat > $R/etc/apparmor.d/hex-lab/acme_secure_profile <<"EOF"
abi <abi/4.0>,
profile acme_secure_profile flags=(complain) {
}
EOF
    # PID 1: the mounts every run needs, securityfs and the profile loaded, then vm7_inside.sh.
    cat > $R/sbin/vm7-boot <<"EOF"
#!/bin/bash
mount -t proc proc /proc; mount -t sysfs sys /sys; mount -t devtmpfs dev /dev 2>/dev/null
mount -t securityfs securityfs /sys/kernel/security
modprobe 9pnet_virtio 2>/dev/null; modprobe 9p 2>/dev/null
mkdir -p /opt/lab /job
mount -t 9p -o trans=virtio,version=9p2000.L,ro lab /opt/lab
# msize 512000: the virtio-9p transport maximum, PAGE_SIZE (4,096 on x86_64) x (VIRTQUEUE_NUM 128 - 3) (Linux v7.2
# net/9p/trans_virtio.c:38, :807); the client limits any larger msize to it (net/9p/client.c:900).
mount -t 9p -o trans=virtio,version=9p2000.L,msize=512000 job /job
echo "apparmor enabled: $(cat /sys/module/apparmor/parameters/enabled)"
apparmor_parser -r -W /etc/apparmor.d/hex-lab/acme_secure_profile && echo "loaded acme_secure_profile"
export HEX_LSM_APPARMOR_PROFILE=acme_secure_profile
exec bash /opt/lab/vm7_inside.sh
echo "VM7_EXIT=125 (vm7_inside.sh did not run)"; echo o > /proc/sysrq-trigger
EOF
    chmod 755 $R/sbin/vm7-boot
    rm -rf $R/var/cache/apt/archives/*.deb
    echo "$KVER" > /vm/KVER
    mkfs.ext4 -q -F -L hexvmaa -d $R /vm/rootfs.ext4 8G
    chown "$(stat -c %u:%g /vm)" /vm/rootfs.ext4 /vm/initramfs.img /vm/vmlinuz /vm/KVER
  '
fi
{
  echo "kernel: Ubuntu 24.04 linux-image-virtual $(cat "$VM_DIR/KVER") (AppArmor guest, vmaa_build.sh)"
  echo "vmlinuz sha256: $(sha256sum "$VM_DIR/vmlinuz" | cut -d' ' -f1)"
} > "$VM_DIR/KERNEL"
cat "$VM_DIR/KERNEL"
say "built in $VM_DIR"
