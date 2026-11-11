#!/usr/bin/env bash
# Rebuilds the Firecracker VMM Hex's `firecracker` class launches, from Hex's main alone (AC-T7.4.1):
#   scripts/firecracker/build.sh x86_64|aarch64 OUT [FROM]
# Upstream at vendor/MANIFEST.toml's [projects.firecracker] commit (the class's PREBOOT_REVISION), each
# vendor/patches/firecracker change applied in name order as scripts/vendor.py replays them, built as
# upstream's tools/release.sh builds a release: static musl, `--release`, `--locked`, with the Rust
# toolchain upstream's rust-toolchain.toml names and upstream's .cargo/config.toml (AWS-LC's flags).
# FROM is a clone that has the commit, for when the repository no longer serves it.
# Writes to OUT, which must be new or empty (nothing outside it):
# - src/: the patched source, a git repository whose HEAD is the pin and whose index holds the changes,
#   each changed file checked byte for byte against vendor/firecracker (which `vendor.py check` proves);
# - target/: cargo's build;
# - firecracker, and SHA256SUMS naming it: the hash a Firecracker Config holds as binary_sha256;
# - toolchain.txt: rustc's and cargo's versions and the C compiler and archiver used.
# Needs git, python3 (3.11, scripts/vendor.py's), and rustup with that toolchain and its
# <arch>-unknown-linux-musl target already installed: it installs nothing (RUSTUP_AUTO_INSTALL=0). AWS-LC
# compiles C and assembly, so CC_<arch>_unknown_linux_musl and AR_<arch>_unknown_linux_musl must name a
# compiler and archiver for that musl target (musl-gcc, or `zig cc -target <arch>-linux-musl` behind a
# wrapper that respells cc-rs's --target=<arch>-unknown-linux-musl as <arch>-linux-musl, zig's name).
# The caller bounds it (CARGO_BUILD_JOBS, nice, a kiln run's --needs); it sets no limit of its own.
set -euo pipefail

usage="usage: build.sh x86_64|aarch64 OUT [FROM]"
arch=${1:?$usage}
out=${2:?$usage}
from=${3:-}
case $arch in
x86_64 | aarch64) ;;
*)
    echo "$usage" >&2
    exit 2
    ;;
esac
triple="$arch-unknown-linux-musl"
cc_var="CC_${triple//-/_}"
ar_var="AR_${triple//-/_}"
if [ -z "${!cc_var:-}" ] || [ -z "${!ar_var:-}" ]; then
    echo "build.sh: set $cc_var and $ar_var to a C compiler and archiver for $triple" >&2
    exit 2
fi

here=$(cd "$(dirname "$0")" && pwd)
hex=$(cd "$here/../.." && pwd)
mkdir -p "$out"
out=$(cd "$out" && pwd)
if [ -n "$(ls -A "$out")" ]; then
    echo "build.sh: $out is not empty" >&2
    exit 1
fi

# The pin and repository, read by vendor.py's own manifest reader: name, kind, commit, path, changes, repository.
pin=$(python3 "$hex/scripts/vendor.py" list | awk -F'\t' '$1 == "firecracker" { print $3, $6 }')
read -r commit repository <<<"$pin" || true
if [ -z "${commit:-}" ] || [ -z "${repository:-}" ]; then
    echo "build.sh: vendor/MANIFEST.toml has no [projects.firecracker]" >&2
    exit 1
fi

src="$out/src"
git init -q "$src"
sources=("$repository")
if [ -n "$from" ]; then
    sources=("$from" "$repository")
fi
fetched=""
for url in "${sources[@]}"; do
    if git -C "$src" fetch -q --depth 1 --no-tags "$url" "$commit"; then
        fetched=$url
        break
    fi
done
if [ -z "$fetched" ]; then
    echo "build.sh: no commit $commit in ${from:+$from or }$repository" >&2
    exit 1
fi
git -C "$src" -c advice.detachedHead=false checkout -q --detach FETCH_HEAD
if [ "$(git -C "$src" rev-parse HEAD)" != "$commit" ]; then
    echo "build.sh: $fetched served another commit for $commit" >&2
    exit 1
fi

# vendor.py applies a change's diff alone (its Subject:/Why: header stripped), in name order.
for patch in "$hex"/vendor/patches/firecracker/*.patch; do
    sed -n '/^diff --git /,$p' "$patch" | git -C "$src" apply --index --whitespace=nowarn -
done
(cd "$hex/vendor/firecracker" && find . -type f ! -name UPSTREAM.md -print0) |
    while IFS= read -r -d '' rel; do
        cmp -s "$hex/vendor/firecracker/$rel" "$src/$rel" || {
            echo "build.sh: $rel differs from vendor/firecracker after the recorded changes" >&2
            exit 1
        }
    done

channel=$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$src/rust-toolchain.toml")
if [ -z "$channel" ]; then
    echo "build.sh: upstream's rust-toolchain.toml names no channel" >&2
    exit 1
fi
export RUSTUP_AUTO_INSTALL=0 CARGO_TARGET_DIR="$out/target"
{
    rustc "+$channel" -vV
    cargo "+$channel" -V
    echo "$cc_var=${!cc_var}"
    echo "$ar_var=${!ar_var}"
} >"$out/toolchain.txt"
(cd "$src" && cargo "+$channel" build --release --locked --target "$triple" -p firecracker --bin firecracker)
cp "$out/target/$triple/release/firecracker" "$out/firecracker"
(cd "$out" && sha256sum firecracker >SHA256SUMS)
cat "$out/SHA256SUMS"
