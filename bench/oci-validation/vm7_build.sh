#!/usr/bin/env bash
# Builds vm7_lab.sh's virtual machine once: a Fedora 44 guest booting Fedora's own kernel 7.2 build, a kernel
# with open_tree(OPEN_TREE_NAMESPACE) and clone3(CLONE_EMPTY_MNTNS) (goal 6), which the 6.19 lab hosts lack.
# Everything runs in containers from one Fedora 44 image holding QEMU and the guest's build tools, so the host
# needs Docker and a /dev/kvm it may open, nothing installed on it. Into VM_DIR (~/.cache/hex-t7/g6/vm):
#   rpms/       the kernel's RPMs from Fedora's build system (koji), their sha256 in rpms/SHA256SUMS
#   vmlinuz, initramfs.img
#               the kernel, and a dracut initramfs that loads the virtio and 9p drivers the guest's root and
#               shares need (Fedora builds them as modules)
#   rootfs.ext4 the guest's root: Fedora 44 with the kernel's modules, its PID 1 /sbin/vm7-boot, which mounts
#               the lab's shares and hands over to vm7_inside.sh; booted with -snapshot, so it stays as built
#   KERNEL      the kernel's name, version, source and the RPMs' sha256, which vm7_lab.sh prints
# Rebuilds only what's missing; FORCE=1 rebuilds it all. Its containers are held to vm7_lab.sh's caps (VM_CPUS,
# BENCH_MEMORY, BENCH_OOM; the OOM score 1000 is the kernel's OOM_SCORE_ADJ_MAX, include/uapi/linux/oom.h).
# Usage: vm7_build.sh
set -euo pipefail
KVER=${KVER:-7.2.15-200.fc44.x86_64}
VM_DIR=${VM_DIR:-$HOME/.cache/hex-t7/g6/vm}
IMG=hexlab-vm7:44
KOJI=https://kojipkgs.fedoraproject.org/packages/kernel
say() { echo "$(date -u +%FT%TZ) $*"; }
mkdir -p "$VM_DIR/rpms"
[ "${FORCE:-0}" = 1 ] && rm -f "${VM_DIR:?}/vmlinuz" "${VM_DIR:?}/initramfs.img" "${VM_DIR:?}/rootfs.ext4"

say "image $IMG"
if ! docker image inspect "$IMG" > /dev/null 2>&1; then
  NAME=hexlab-vm7-image-$$; trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT
  docker run --name "$NAME" --cpuset-cpus "${VM_CPUS:-16-31}" --memory "${BENCH_MEMORY:-8g}" \
    --oom-score-adj "${BENCH_OOM:-1000}" registry.fedoraproject.org/fedora:44 sh -c \
    "dnf -y -q install --setopt=install_weak_deps=False qemu-system-x86-core qemu-img e2fsprogs rpm cpio && dnf clean all" > /dev/null
  docker commit --change "CMD [\"bash\"]" "$NAME" "$IMG" > /dev/null
  docker rm "$NAME" > /dev/null
fi

# The kernel's version and release name its directory at koji: 7.2.15-200.fc44.x86_64 is 7.2.15/200.fc44/x86_64.
V=${KVER%%-*}; REST=${KVER#*-}; R=${REST%.*}; ARCH=${REST##*.}
for p in kernel-core kernel-modules-core kernel-modules; do
  f=$p-$V-$R.$ARCH.rpm
  [ -s "$VM_DIR/rpms/$f" ] || { say "fetch $f"; curl -fsSL -o "$VM_DIR/rpms/$f.part" "$KOJI/$V/$R/$ARCH/$f"; mv "$VM_DIR/rpms/$f.part" "$VM_DIR/rpms/$f"; }
done
(cd "$VM_DIR/rpms" && sha256sum kernel-*-"$V-$R.$ARCH".rpm > SHA256SUMS)

if [ ! -s "$VM_DIR/rootfs.ext4" ] || [ ! -s "$VM_DIR/initramfs.img" ] || [ ! -s "$VM_DIR/vmlinuz" ]; then
  say "guest root, initramfs and kernel for $KVER"
  # Privileged for the chroot's /proc, which dracut reads; the build's root is a directory in the container.
  docker run --rm --privileged --security-opt label=disable \
    --cpuset-cpus "${VM_CPUS:-16-31}" --memory "${BENCH_MEMORY:-8g}" --oom-score-adj "${BENCH_OOM:-1000}" -v "$VM_DIR:/vm" -e KVER="$KVER" "$IMG" bash -euc '
    rpm -K /vm/rpms/kernel-*.rpm  # koji builds are unsigned: digests checked, KERNEL keeps the sha256
    R=/build/root; mkdir -p $R
    dnf -y -q --installroot=$R --releasever=44 --use-host-config --setopt=install_weak_deps=False install \
      bash coreutils util-linux procps-ng findutils diffutils iproute kmod systemd dracut python3 \
      /vm/rpms/kernel-core-*.rpm /vm/rpms/kernel-modules-core-*.rpm /vm/rpms/kernel-modules-[0-9]*.rpm > /dev/null
    mount -t proc proc $R/proc
    chroot $R dracut -q --no-hostonly --force --kver "$KVER" \
      --add-drivers "virtio_pci virtio_blk virtio_console 9p 9pnet_virtio netfs ext4" /boot/vm7-initramfs.img
    umount $R/proc
    cp $R/boot/vm7-initramfs.img /vm/initramfs.img
    cp $R/usr/lib/modules/$KVER/vmlinuz /vm/vmlinuz
    # PID 1: the mounts every run needs and the two shares that say what to run, then vm7_inside.sh.
    cat > $R/sbin/vm7-boot <<"EOF"
#!/bin/bash
mount -t proc proc /proc; mount -t sysfs sys /sys; mount -t devtmpfs dev /dev 2>/dev/null
mkdir -p /opt/lab /job
mount -t 9p -o trans=virtio,version=9p2000.L,ro lab /opt/lab
mount -t 9p -o trans=virtio,version=9p2000.L,msize=1048576 job /job
exec bash /opt/lab/vm7_inside.sh
echo "VM7_EXIT=125 (vm7_inside.sh did not run)"; echo o > /proc/sysrq-trigger
EOF
    chmod 755 $R/sbin/vm7-boot
    rm -rf $R/var/cache/dnf
    mkfs.ext4 -q -F -L hexvm7 -d $R /vm/rootfs.ext4 8G
    chown "$(stat -c %u:%g /vm)" /vm/rootfs.ext4 /vm/initramfs.img /vm/vmlinuz
  '
fi
{
  echo "kernel: Fedora kernel $KVER (koji build kernel-$V-$R)"
  echo "source: $KOJI/$V/$R/$ARCH/"
  sed 's/^/sha256: /' "$VM_DIR/rpms/SHA256SUMS"
  echo "vmlinuz sha256: $(sha256sum "$VM_DIR/vmlinuz" | cut -d' ' -f1)"
} > "$VM_DIR/KERNEL"
cat "$VM_DIR/KERNEL"
say "built in $VM_DIR"
