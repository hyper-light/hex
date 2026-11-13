#!/usr/bin/env bash
# The probe image's build context for crates/hex-cli/tests/image_grants_real.rs (joint plan step 15, AC-T7.7.2;
# docs/design/network-grants.md, Tests), which the test builds into its node's Shards home (HEX_GRANTS_CONTEXT), so
# no registry need serve it:
#   scripts/grants-probe/build.sh OUT
# OUT, which must be new or empty, gets the context: an Agentfile and its Dockerfile, both FROM the base, and the
# probe harness's directory, probe/, holding bin/probe (crates/hex-cli/tests/image_grants_probe, built by rustc
# alone, static, linked by the toolchain's rust-lld, for <arch>-unknown-linux-musl, <arch> this host's: a Shards
# guest is its host's arch, on KVM, HVF and WHP alike) and harness.json, which starts it with its options. Its
# Agentfile grants the probe, by D236's destinations and D122's ports:
# - out: the provider's name and the host itself (host.docker.internal) by TCP, each at its port, and port 80 by TCP
#   to any address, each of which Hex's narrowing drops unless a grant keeps it;
# - in: its two ports, the one the test publishes and the one it doesn't.
# Each input is an explicit variable, and a missing one stops it by name (no default stands in):
# - HEX_GRANTS_BASE: the base image, a registry's, pinned by its digest (NAME@sha256:<64 hex>), which `shards build`
#   pulls for this host's arch, with /bin/sh and tail (image_grants_real runs `/bin/sh -c "exec tail -f /dev/null"` as
#   a run's own command); alpine's index serves linux/amd64 and linux/arm64:
#   docker.io/library/alpine@sha256:3f4d6b32e26703c6082839234f43769d5046b3ac7c910033911d1e71d8cd630a;
# - HEX_GRANTS_MEMBER: the harness's name, an Agentfile member's (lowercase letters, digits, '.', '_', '-');
# - HEX_GRANTS_PORT, HEX_GRANTS_UNGRANTED_PORT: the two TCP ports it takes coming in;
# - HEX_GRANTS_PROVIDER: a provider's host name, granted at TCP 443; HEX_GRANTS_UNGRANTED: a host name it tries at
#   443 that no grant names;
# - HEX_GRANTS_HOST_PORT: the host's TCP port granted as host.docker.internal:<port>, where the test listens;
# - HEX_GRANTS_OPEN: an IPv4 address it tries at port 80, which only the port egress the image grants would reach.
# The probe waits for each try's own answer from its TCP stack, so no wait is an input.
# Runs on each host Shards boots guests on: Linux (KVM) and Windows (WHP, under Git Bash) on x86_64 and arm64, and
# macOS (HVF) on arm64 (Intel Mac is unsupported). Needs rustc of the pinned toolchain (rust-toolchain.toml) with the
# musl target installed: it installs nothing (RUSTUP_AUTO_INSTALL=0).
set -euo pipefail

usage="usage: build.sh OUT"
out=${1:?$usage}
os=$(uname -s)
case $os in
Linux | Darwin) arch=$(uname -m) ;;
# Git Bash's uname names its own build's machine: the processor is Windows' word, so a shell emulated on arm64 builds
# for the arm64 guest WHP runs there.
MINGW*_NT-* | MSYS_NT-* | CYGWIN_NT-*) arch=${PROCESSOR_ARCHITEW6432:-${PROCESSOR_ARCHITECTURE:-}} ;;
*) arch= ;;
esac
case $os-$arch in
Linux-x86_64 | MINGW*-AMD64 | MSYS*-AMD64 | CYGWIN*-AMD64) target=x86_64-unknown-linux-musl ;;
Linux-aarch64 | Darwin-arm64 | MINGW*-ARM64 | MSYS*-ARM64 | CYGWIN*-ARM64) target=aarch64-unknown-linux-musl ;;
*)
    echo "build.sh: Shards boots guests on Linux and Windows on x86_64 and arm64 and on macOS on arm64, not $os $arch" >&2
    exit 2
    ;;
esac
if command -v sha256sum >/dev/null; then
    sha256() { sha256sum "$@"; }
else
    sha256() { shasum -a 256 "$@"; }
fi
need() {
    local value=${!1:-}
    if [ -z "$value" ]; then
        echo "build.sh: $1 is unset: each input is explicit" >&2
        exit 2
    fi
    printf '%s' "$value"
}
base=$(need HEX_GRANTS_BASE)
member=$(need HEX_GRANTS_MEMBER)
port=$(need HEX_GRANTS_PORT)
ungranted_port=$(need HEX_GRANTS_UNGRANTED_PORT)
provider=$(need HEX_GRANTS_PROVIDER)
ungranted=$(need HEX_GRANTS_UNGRANTED)
host_port=$(need HEX_GRANTS_HOST_PORT)
open=$(need HEX_GRANTS_OPEN)
if ! [[ $base =~ ^[^@[:space:]]+@sha256:[0-9a-f]{64}$ ]]; then
    echo "build.sh: HEX_GRANTS_BASE $base isn't pinned by its digest (NAME@sha256:<64 hex>)" >&2
    exit 2
fi
if [ -e "$out" ] && [ -n "$(ls -A -- "$out")" ]; then
    echo "build.sh: $out is not empty" >&2
    exit 2
fi
repo=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd)
mkdir -p -- "$out/probe/bin"
export RUSTUP_AUTO_INSTALL=0
# The probe is std alone: the target's own musl and start files (self-contained), linked by rust-lld on every host, as
# Shards links its guest binaries.
(cd -- "$repo" && rustc --edition 2024 -C opt-level=2 -C target-feature=+crt-static -C linker=rust-lld \
    -C link-self-contained=yes --target "$target" -o "$out/probe/bin/probe" crates/hex-cli/tests/image_grants_probe/bin.rs)
printf '{"entrypoint":"probe","options":["--address","0.0.0.0","--listen","%s","--listen","%s","--egress","provider=%s:443","--egress","ungranted=%s:443","--egress","host=host.docker.internal:%s","--egress","open=%s:80"]}\n' \
    "$port" "$ungranted_port" "$provider" "$ungranted" "$host_port" "$open" >"$out/probe/harness.json"
printf 'FROM %s\n' "$base" >"$out/Dockerfile"
cat >"$out/Agentfile" <<AGENTFILE
FROM $base
HARNESS $member FROM ./probe
NETWORK --egress=$provider:443 --egress=host.docker.internal:$host_port provider FOR $member
NETWORK --egress=80 open FOR $member
NETWORK --ingress=$port --ingress=$ungranted_port front FOR $member
EXPOSE --harnesses=$member 443 $host_port AS egress FOR provider
EXPOSE --harnesses=$member 80 AS egress FOR open
EXPOSE --harnesses=$member $port $ungranted_port AS ingress FOR front
CONNECT --target-kind=harness $member WITH $member ON provider open front
AGENTFILE
(cd -- "$out" && sha256 probe/bin/probe probe/harness.json Agentfile Dockerfile)
