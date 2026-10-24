# The lab image for Zeroboot's rounds (zeroboot.sh): Fedora 44 with musl's compiler, which builds the
# guest init statically as Zeroboot's Makefile does, mkfs.ext4 for its rootfs, and python3 for the client.
FROM registry.fedoraproject.org/fedora:44
RUN dnf -y install --setopt=install_weak_deps=False musl-gcc musl-libc-static e2fsprogs python3 \
    && dnf clean all
