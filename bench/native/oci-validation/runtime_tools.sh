#!/usr/bin/env bash
# Builds opencontainers/runtime-tools at bd3b0bd (the suite of record, bench/oci-validation/SUMMARY-hex.txt)
# for a Linux guest's architecture: every validation/<name>/<name>.t and runtimetest, static (CGO off), with
# the suite's own vendored modules (no network). Its bundles' rootfs for arm64, which upstream doesn't ship, is
# Alpine's aarch64 minirootfs (inputs.txt), as the Mac's Linux run of record used an Alpine arm64 rootfs.
#   runtime_tools.sh <src checkout at bd3b0bd> <amd64|arm64> <inputs dir (fetch.sh)> <out tar.gz>
set -euo pipefail
src=$(cd "${1:?usage}" && pwd); arch=${2:?usage}; inputs=$(cd "${3:?usage}" && pwd); out=${4:?usage}
[ "$(git -C "$src" rev-parse HEAD)" = bd3b0bd00f44e15f0f1ff4130c61dbea1b1c56cc ] \
    || { echo "runtime-tools isn't at bd3b0bd" >&2; exit 1; }
work=$(mktemp -d "${TMPDIR:-$HOME/.cache}/rt-$arch.XXXXXX")
trap 'rm -rf "${work:?}"' EXIT
git -C "$src" archive HEAD | tar -x -C "$work"
cd "$work"
export CGO_ENABLED=0 GOOS=linux GOARCH=$arch GOFLAGS=-mod=vendor
go build -tags "netgo osusergo" -o runtimetest ./cmd/runtimetest
for f in $(find ./validation -name '*.go' | grep -v util); do go build -o "${f%.go}.t" "$f"; done
case $arch in
arm64) cp "$inputs/alpine-minirootfs-3.24.4-aarch64.tar.gz" rootfs-arm64.tar.gz ;;
amd64) ;;
*) echo "arch $arch is neither amd64 nor arm64" >&2; exit 2 ;;
esac
tar -czf "$out" --exclude=.git .
echo "built $(find validation -name '*.t' | wc -l) tests for $arch: $out"
