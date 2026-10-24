# The lab image for the process-sandbox and microVM passes (sandbox_bench.py, vm_bench.py): Fedora 44 with
# each tool at a pinned version, built once on the bench host (the only step that needs the network), then
# run with none.
# host.sh records the base image's id and the built image's id beside the results.
FROM registry.fedoraproject.org/fedora:44

# bubblewrap, firejail and systemd-nspawn from Fedora 44; nsjail's and minijail's build dependencies;
# node for sandbox-runtime, which needs bubblewrap, socat and ripgrep on Linux; static glibc and cpio for
# the VM pass's guest init and initramfs.
RUN dnf -y install --setopt=install_weak_deps=False \
        bubblewrap firejail systemd-container python3 \
        git gcc gcc-c++ make pkgconf-pkg-config flex bison protobuf-devel protobuf-compiler libnl3-devel \
        libcap-devel nodejs npm socat ripgrep glibc-static cpio \
    && dnf clean all

# nsjail 3.6 (with its kafel submodule) and minijail linux-v2026.05.18, from source at their latest tags (minijail
# tags regularly, but its last GitHub release is linux-v18, of 2023). minijail takes its preload library's path
# from LIBDIR: its Makefile quotes it into the C string PRELOADPATH, so PRELOADPATH itself can't be given bare. Its
# common.mk writes its output to OUT, by default the shell's $PWD, which `make -C` doesn't change, so OUT is given.
RUN git clone --depth 1 --recurse-submodules --branch 3.6 https://github.com/google/nsjail /src/nsjail \
    && make -C /src/nsjail -j8 \
    && install -m 755 /src/nsjail/nsjail /usr/local/bin/nsjail
RUN git clone --depth 1 --branch linux-v2026.05.18 https://github.com/google/minijail /src/minijail \
    && make -C /src/minijail -j8 LIBDIR=/usr/local/lib OUT=/src/minijail \
    && install -m 755 /src/minijail/minijail0 /usr/local/bin/minijail0 \
    && install -m 644 /src/minijail/libminijailpreload.so /usr/local/lib/libminijailpreload.so

# Anthropic's sandbox-runtime, pinned.
RUN npm install -g @anthropic-ai/sandbox-runtime@0.0.81

# codex's static musl build, which host.sh fetches and checks against its sha256 before the build.
COPY codex /usr/local/bin/codex
