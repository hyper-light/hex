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
targets x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu aarch64-unknown-linux-musl
gate cargo clippy --workspace --all-targets --locked --target x86_64-unknown-linux-gnu \
  --target aarch64-unknown-linux-gnu --target aarch64-unknown-linux-musl -- -D warnings
# The same lints for macOS and Windows (checking needs no linker or SDK while the code is pure Rust).
targets aarch64-apple-darwin x86_64-apple-darwin
gate cargo clippy --workspace --all-targets --locked --target aarch64-apple-darwin \
  --target x86_64-apple-darwin -- -D warnings
targets x86_64-pc-windows-msvc
gate cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings
gate bash scripts/check-production.sh
gate cargo test --workspace --locked
gate python3 scripts/vendor.py check
