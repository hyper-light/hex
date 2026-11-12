#!/usr/bin/env bash
# Rebuilds the `shards` binary a node's Shards GPU guests boot with (SHARDS_BIN), its GPU VM process carrying
# the Venus renderer that renders them on the host's driver, from Hex's main alone (AC-T7.4.1):
#   scripts/shards-gpu/build.sh OUT [FROM]
# Upstream at vendor/MANIFEST.toml's [projects.shards-gpu] commit, which holds the physical-GPU filter,
# selector transport, native identity bridge and the managed launch and report the node's vendored API speaks to
# (D232, D233), each vendor/patches/shards-gpu change applied in name order as scripts/vendor.py replays them
# (Windows on arm64's GPU VM process, which upstream holds), built as upstream's release builds Linux with glibc
# (.github/workflows/release.yml): upstream's scripts/gpu-libs builds the renderer from its pinned sources, with
# its renderer filter and transport, into OUT/gpu-libs, which SHARDS_GPU_LIBS names to
# `cargo build --release --locked -p shards`, with the Rust toolchain upstream's rust-toolchain.toml names, for
# this host's <arch>-unknown-linux-gnu.
# FROM is a clone that has the commit, for when the repository no longer serves it.
# Writes to OUT, which must be new or empty. gpu-libs builds in a scratch of mktemp's, which it removes; TMPDIR puts it
# under ~/.cache/hex/shards-gpu/tmp, since /tmp is a RAM disk every session shares on the Linux runners and a renderer
# build fills it:
# - src/: the patched source, a git repository whose HEAD is the pin and whose index holds the changes, each
#   changed file checked byte for byte against vendor/shards-gpu (which `vendor.py check` proves);
# - gpu-libs/: the renderer's static libraries; target/: cargo's build;
# - shards, and SHA256SUMS naming it;
# - toolchain.txt: rustc's, cargo's, zig's, cc's and python3's versions.
# Linux (x86_64 or aarch64, D201) only, natively: the renderer is built for the host. Needs git, curl, python3
# (3.11 to 3.14, for the renderer's pinned PyYAML wheels; and scripts/vendor.py's), cc, zig 0.16.0 on PATH
# (upstream's scripts/install-zig installs it; shards' build compiles its guest init's C with it), and rustup
# with that toolchain and its <arch>-unknown-linux-gnu, x86_64-unknown-linux-musl and
# aarch64-unknown-linux-musl targets already installed: it installs nothing (RUSTUP_AUTO_INSTALL=0). The
# caller bounds it (CARGO_BUILD_JOBS, nice, a kiln run's --needs); it sets no limit of its own.
set -euo pipefail

usage="usage: build.sh OUT [FROM]"
out=${1:?$usage}
from=${2:-}
case "$(uname -s)-$(uname -m)" in
Linux-x86_64) triple=x86_64-unknown-linux-gnu ;;
Linux-aarch64) triple=aarch64-unknown-linux-gnu ;;
*)
    echo "build.sh: Shards' renderer is built on Linux x86_64 or aarch64 here, not $(uname -s)-$(uname -m)" >&2
    exit 2
    ;;
esac

here=$(cd "$(dirname "$0")" && pwd)
hex=$(cd "$here/../.." && pwd)
mkdir -p "$out"
out=$(cd "$out" && pwd)
if [ -n "$(ls -A "$out")" ]; then
    echo "build.sh: $out is not empty" >&2
    exit 1
fi

# The pin and repository, read by vendor.py's own manifest reader: name, kind, commit, path, changes, repository.
pin=$(python3 "$hex/scripts/vendor.py" list | awk -F'\t' '$1 == "shards-gpu" { print $3, $6 }')
read -r commit repository <<<"$pin" || true
if [ -z "${commit:-}" ] || [ -z "${repository:-}" ]; then
    echo "build.sh: vendor/MANIFEST.toml has no [projects.shards-gpu]" >&2
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
for patch in "$hex"/vendor/patches/shards-gpu/*.patch; do
    sed -n '/^diff --git /,$p' "$patch" | git -C "$src" apply --index --whitespace=nowarn -
done
(cd "$hex/vendor/shards-gpu" && find . -type f ! -name UPSTREAM.md -print0) |
    while IFS= read -r -d '' rel; do
        cmp -s "$hex/vendor/shards-gpu/$rel" "$src/$rel" || {
            echo "build.sh: $rel differs from vendor/shards-gpu after the recorded changes" >&2
            exit 1
        }
    done

# gpu-libs' scratch (mktemp -d), off /tmp: a RAM disk every session shares on the Linux runners (Hex's hazard rule:
# big files under ~/.cache, never /tmp).
export TMPDIR="${XDG_CACHE_HOME:-$HOME/.cache}/hex/shards-gpu/tmp"
mkdir -p "$TMPDIR"

channel=$(sed -n 's/^channel = "\(.*\)"$/\1/p' "$src/rust-toolchain.toml")
if [ -z "$channel" ]; then
    echo "build.sh: upstream's rust-toolchain.toml names no channel" >&2
    exit 1
fi
export RUSTUP_AUTO_INSTALL=0 CARGO_TARGET_DIR="$out/target" SHARDS_GPU_LIBS="$out/gpu-libs"
# The version shards reports, as upstream's release names it: the pin's short commit.
export SHARDS_GIT_COMMIT=${commit:0:7}
{
    rustc "+$channel" -vV
    cargo "+$channel" -V
    zig version
    cc --version | sed -n 1p
    python3 --version
} >"$out/toolchain.txt"
"$src/scripts/gpu-libs" "$SHARDS_GPU_LIBS"
(cd "$src" && cargo "+$channel" build --release --locked --target "$triple" -p shards)
cp "$out/target/$triple/release/shards" "$out/shards"
(cd "$out" && sha256sum shards >SHA256SUMS)
cat "$out/SHA256SUMS"
