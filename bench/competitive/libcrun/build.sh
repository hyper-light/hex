#!/usr/bin/env bash
# bench/competitive/libcrun: builds libcrun-driver (driver.c) against libcrun from crun's release tarball.
#   build.sh <crun-<version>.tar.gz> <output binary>
# libcrun static, as crun's configure makes it by default (LT_INIT([disable-shared])), without systemd:
# the bench runs crun with cgroupfs, not its systemd cgroup manager. blake3 is crun's embedded copy, so the
# binary needs only json-c, libcap and libseccomp, the distribution's, as shared libraries the OCI pass's
# host /usr also has. It links what configure found for crun itself (FOUND_LIBS, JSON_C_LIBS). BUILD_DIR
# (~/.cache) holds the unpacked tree until the build ends; JOBS (8) bounds make.
set -euo pipefail
TARBALL=$(realpath "${1:?crun release tarball}"); OUT=$(realpath -m "${2:?output binary}")
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
W=$(mktemp -d "${BUILD_DIR:-$HOME/.cache}/libcrun-build.XXXXXX")
trap 'rm -rf -- "${W:?}"' EXIT
tar -xzf "$TARBALL" -C "$W"
S=$(find "$W" -mindepth 1 -maxdepth 1 -type d -name 'crun-*')
env -C "$S" ./configure --quiet --disable-systemd --enable-embedded-blake3 --enable-static --disable-shared
# git-version.h is one of BUILT_SOURCES, which only `make all` makes; the tarball carries it. libocispec
# generates the headers libcrun compiles against, so it is built whole first: in one parallel make, libcrun's
# objects can start before those headers exist.
make -C "$S" --quiet .version git-version.h
make -C "$S/libocispec" --quiet -j"${JOBS:-8}"
make -C "$S" --quiet -j"${JOBS:-8}" libcrun.la
libs() { sed -n "s/^$1 = //p" "$S/Makefile"; }
# shellcheck disable=SC2046  # the configured flags are words
gcc -O2 -Wall -Wextra -Werror -I "$S" -I "$S/src" -I "$S/libocispec/src" -o "$OUT" "$HERE/driver.c" \
  "$S/.libs/libcrun.a" "$S/libocispec/.libs/libocispec.a" $(libs FOUND_LIBS) $(libs JSON_C_LIBS)
echo "built $OUT from $(basename "$TARBALL") ($(sha256sum "$TARBALL" | cut -c1-16))"
