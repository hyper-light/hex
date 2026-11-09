#!/usr/bin/env bash
# Every gate of CLAUDE.md, in order; exits non-zero at the first failure. Run it before each commit;
# kiln CI runs it on every push and pull request (.kiln/ci.yaml).
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
gate() { printf '::: %s\n' "$*" >&2; "$@"; }
# The targets a gate lints, installed into the pinned toolchain right before it: only a machine that runs
# the gates installs them, and no one else's toolchain is touched (rust-toolchain.toml names none). A
# target already there is a no-op.
targets() { rustup target add "$@" >&2; }
gate cargo fmt --all --check
# Linux on both arches, glibc and musl (the Shards guest's hex is musl aarch64): named, so the host's
# own target is named too.
targets x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-unknown-linux-musl aarch64-unknown-linux-musl
gate cargo clippy --workspace --all-targets --locked --target x86_64-unknown-linux-gnu \
  --target aarch64-unknown-linux-gnu --target x86_64-unknown-linux-musl --target aarch64-unknown-linux-musl -- -D warnings
# The same lints for macOS and Windows (checking needs no linker or SDK while the code is pure Rust).
# macOS on aarch64 alone; Windows on both arches (Ada, 2026-11-09: "AMD 64 and ARM64 excepting MacOS x86_64").
targets aarch64-apple-darwin
gate cargo clippy --workspace --all-targets --locked --target aarch64-apple-darwin -- -D warnings
targets x86_64-pc-windows-msvc aarch64-pc-windows-msvc
gate cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc \
  --target aarch64-pc-windows-msvc -- -D warnings
gate bash scripts/check-production.sh
# A skipped test says so in the skip record (crates/hex-testkit), which libtest's capture can't hide:
# emptied before the tests, printed after them, pass or fail (HEX-GATE-1).
target=$(cargo metadata --format-version 1 --no-deps --locked |
  python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
export HEX_SKIPS="$target/tmp/hex-skips.tsv"
rm -f -- "${HEX_SKIPS:?}"
tests=0
gate cargo test --workspace --locked || tests=$?
if [ -s "$HEX_SKIPS" ]; then
  printf '::: %s skipped (binary, test, missing, why):\n' "$(wc -l <"$HEX_SKIPS")" >&2
  cat -- "$HEX_SKIPS" >&2
else
  printf '::: no test skipped\n' >&2
fi
[ "$tests" -eq 0 ] || exit "$tests"
gate python3 scripts/vendor.py check
