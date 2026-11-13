#!/usr/bin/env bash
# Rebuilds Hex's `shards`, the binary a node boots its Shards guests and image VMs with (SHARDS_BIN): its GPU VM
# process carrying the Venus renderer that renders a GPU guest on the host's driver, and its `shards api-run` hosting
# an image's boot at the image channel version the node's vendored API speaks, from Hex's main alone (AC-T7.4.1;
# joint plan steps 12 and 15):
#   scripts/shards-gpu/build.sh OUT [FROM] [--no-gpu]
# Upstream at vendor/MANIFEST.toml's [projects.shards-gpu] commit, the same as [projects.shards], which holds the
# physical-GPU filter, selector transport, native identity bridge and the managed launch and report the node's
# vendored API speaks to (D232, D233), and the run host the node's image boot speaks to (D231 to D249); then each
# vendor/patches/shards change (the API the run host and the node both speak, so the run host reads what the node
# writes; none at this commit), then each vendor/patches/shards-gpu change (Windows on arm64's GPU VM process), each
# in name order as scripts/vendor.py replays them; built as upstream's release builds it: upstream's scripts/gpu-libs
# builds the renderer from its pinned sources into OUT/gpu-libs, which SHARDS_GPU_LIBS names to
# `cargo build --release --locked -p shards`, with the Rust toolchain upstream's rust-toolchain.toml names, for this
# host's own target. With --no-gpu no renderer is built, and `shards` carries no GPU VM process and refuses a GPU, as
# upstream's build does without SHARDS_GPU_LIBS: what an image VM's run host needs alone.
# FROM is a clone that has the commit, for when the repository no longer serves it.
# Writes to OUT, which must be new or empty. gpu-libs builds in a scratch of mktemp's, which it removes; TMPDIR puts it
# under ~/.cache/hex/shards-gpu/tmp, since /tmp is a RAM disk every session shares on the Linux runners and a renderer
# build fills it:
# - src/: the patched source, a git repository whose HEAD is the pin and whose index holds the changes, each
#   changed file checked byte for byte against vendor/shards and vendor/shards-gpu (which `vendor.py check` proves);
# - gpu-libs/: the renderer's static libraries, unless --no-gpu; target/: cargo's build;
# - shards (shards.exe on Windows; on macOS signed ad hoc with the hypervisor entitlement, resources/hvf.entitlements,
#   as upstream signs what drives Hypervisor.framework; the VM process it carries upstream's build signs itself);
# - shards-init, the guest init it carries, which a run given a kernel of its own takes beside it (SHARDS_INIT with
#   SHARDS_KERNEL); and SHA256SUMS naming both;
# - toolchain.txt: rustc's, cargo's and zig's versions, cc's but on Windows, and with the renderer clang-cl's on
#   Windows and python3's.
# Natively on each host Shards runs on, whose own target it builds: Linux x86_64 or aarch64 (glibc), macOS arm64,
# Windows x86_64 or arm64 (under Git Bash, with MSVC's tools, which rustc finds). Needs git, a C compiler (cc; MSVC's
# on Windows), zig 0.16.0 on PATH (upstream's scripts/install-zig installs it; shards' build compiles its guest
# init's C with it), on Windows x86_64 NASM on PATH (AWS-LC's x64 assembly is built from source, never from the
# objects aws-lc-sys ships: upstream's release installs NASM 3.02, pinned), and rustup with that toolchain, this
# host's target and x86_64-unknown-linux-musl and aarch64-unknown-linux-musl already installed: it installs nothing
# (RUSTUP_AUTO_INSTALL=0). The renderer needs curl and python3 too (3.11 to 3.14, for its pinned PyYAML wheels), and
# on Windows LLVM's clang-cl on PATH or where gpu-libs looks. A tool missing from PATH stops it before it fetches
# anything. vendor/shards, vendor/shards-gpu and their changes check out byte for byte on every host (.gitattributes).
# The caller bounds it (CARGO_BUILD_JOBS, nice, a kiln run's --needs); it sets no limit of its own.
set -euo pipefail

usage="usage: build.sh OUT [FROM] [--no-gpu]"
out=${1:?$usage}
from=
gpu=yes
for arg in "${@:2}"; do
    case $arg in
    --no-gpu) gpu=no ;;
    --*)
        echo "$usage" >&2
        exit 2
        ;;
    *) from=$arg ;;
    esac
done
os=$(uname -s)
case $os in
Linux | Darwin) arch=$(uname -m) ;;
# Git Bash's uname names its own build's machine: the processor is Windows' word.
MINGW*_NT-* | MSYS_NT-* | CYGWIN_NT-*)
    os=Windows
    arch=${PROCESSOR_ARCHITEW6432:-${PROCESSOR_ARCHITECTURE:-}}
    ;;
*) arch= ;;
esac
case $os-$arch in
Linux-x86_64) triple=x86_64-unknown-linux-gnu ;;
Linux-aarch64) triple=aarch64-unknown-linux-gnu ;;
Darwin-arm64) triple=aarch64-apple-darwin ;;
Windows-AMD64) triple=x86_64-pc-windows-msvc ;;
Windows-ARM64) triple=aarch64-pc-windows-msvc ;;
*)
    echo "build.sh: shards is built on Linux and Windows on x86_64 and arm64 and on macOS on arm64, not $os $arch" >&2
    exit 2
    ;;
esac
exe=shards
if [ "$os" = Windows ]; then
    exe=shards.exe
fi
tools=(git zig cargo rustc)
if [ "$os" != Windows ]; then
    tools+=(cc)
fi
if [ "$triple" = x86_64-pc-windows-msvc ]; then
    tools+=(nasm)
fi
if [ "$gpu" = yes ]; then
    tools+=(curl python3)
fi
for tool in "${tools[@]}"; do
    command -v "$tool" >/dev/null || {
        echo "build.sh: no $tool on PATH (this script's header says what each is for)" >&2
        exit 2
    }
done
if command -v sha256sum >/dev/null; then
    sha256() { sha256sum "$@"; }
else
    sha256() { shasum -a 256 "$@"; }
fi

here=$(cd "$(dirname "$0")" && pwd)
hex=$(cd "$here/../.." && pwd)
mkdir -p "$out"
out=$(cd "$out" && pwd)
if [ -n "$(ls -A "$out")" ]; then
    echo "build.sh: $out is not empty" >&2
    exit 1
fi

# The pin and repository: [projects.shards-gpu]'s commit and repository in vendor/MANIFEST.toml, its lines ended as
# TOML ends them, by LF or CRLF (a Windows checkout's), so that a build without the renderer needs no Python.
table=$(tr -d '\r' <"$hex/vendor/MANIFEST.toml" | sed -n '/^\[projects\.shards-gpu\]$/,/^\[/p')
commit=$(sed -n 's/^commit = "\([0-9a-f]\{40\}\)"$/\1/p' <<<"$table")
repository=$(sed -n 's/^repository = "\(.*\)"$/\1/p' <<<"$table")
if [ -z "$commit" ] || [ -z "$repository" ]; then
    echo "build.sh: vendor/MANIFEST.toml's [projects.shards-gpu] names no commit and repository" >&2
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

# vendor.py applies a change's diff alone (its Subject:/Why: header stripped), in name order: the API's first
# (vendor/shards, the same commit), then the build's own.
for patch in "$hex"/vendor/patches/shards/*.patch "$hex"/vendor/patches/shards-gpu/*.patch; do
    [ -e "$patch" ] || continue
    sed -n '/^diff --git /,$p' "$patch" | git -C "$src" apply --index --whitespace=nowarn -
done
# Each vendored file, the API's but Hex's own workspace scaffold, as upstream plus the recorded changes leave it.
for vendored in shards shards-gpu; do
    (cd "$hex/vendor/$vendored" && find . -type f ! -name UPSTREAM.md ! -path ./Cargo.toml -print0) |
        while IFS= read -r -d '' rel; do
            cmp -s "$hex/vendor/$vendored/$rel" "$src/$rel" || {
                echo "build.sh: $rel differs from vendor/$vendored after the recorded changes" >&2
                exit 1
            }
        done
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
export RUSTUP_AUTO_INSTALL=0 CARGO_TARGET_DIR="$out/target"
# The version shards reports, as upstream's release names it: the pin's short commit.
export SHARDS_GIT_COMMIT=${commit:0:7}
{
    rustc "+$channel" -vV
    cargo "+$channel" -V
    zig version
    if [ "$os" != Windows ]; then cc --version | sed -n 1p; fi
    if [ "$gpu" = yes ]; then
        if [ "$os" = Windows ]; then clang-cl --version | sed -n 1p; fi
        python3 --version
    fi
    echo "gpu: $gpu"
} >"$out/toolchain.txt"
if [ "$gpu" = yes ]; then
    export SHARDS_GPU_LIBS="$out/gpu-libs"
    "$src/scripts/gpu-libs" "$SHARDS_GPU_LIBS"
fi
(cd "$src" && cargo "+$channel" build --release --locked --target "$triple" -p shards)
cp "$out/target/$triple/release/$exe" "$out/$exe"
# The guest init it carries, which shards' build script leaves in its OUT_DIR, one in this new target: a run given a
# kernel of its own takes it beside that kernel (SHARDS_KERNEL and SHARDS_INIT go together).
inits=("$out/target/$triple/release/build"/shards-*/out/shards-init)
if [ "${#inits[@]}" -ne 1 ] || [ ! -f "${inits[0]}" ]; then
    echo "build.sh: the build left no one shards-init: ${inits[*]}" >&2
    exit 1
fi
cp "${inits[0]}" "$out/shards-init"
if [ "$os" = Darwin ]; then
    codesign --force --sign - --entitlements "$src/resources/hvf.entitlements" "$out/$exe"
    codesign -d --entitlements - "$out/$exe" 2>/dev/null | grep -q com.apple.security.hypervisor || {
        echo "build.sh: $out/$exe carries no hypervisor entitlement" >&2
        exit 1
    }
fi
(cd "$out" && sha256 "$exe" shards-init >SHA256SUMS)
cat "$out/SHA256SUMS"
