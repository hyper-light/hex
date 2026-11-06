#!/usr/bin/env bash
# Every gate of CLAUDE.md, in order; exits non-zero at the first failure. Run it before each commit;
# kiln CI runs it on every push and pull request (.kiln/ci.yaml).
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
gate() { printf '::: %s\n' "$*" >&2; "$@"; }
gate cargo fmt --all --check
# Linux on both arches, glibc and musl (the Shards guest's hex is musl aarch64): named, so the host's
# own target is named too.
gate cargo clippy --workspace --all-targets --locked --target x86_64-unknown-linux-gnu \
  --target aarch64-unknown-linux-gnu --target aarch64-unknown-linux-musl -- -D warnings
# The same lints for macOS and Windows (rust-toolchain.toml installs their targets; checking needs
# no linker or SDK while the code is pure Rust).
gate cargo clippy --workspace --all-targets --locked --target aarch64-apple-darwin -- -D warnings
gate cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings
gate bash scripts/check-production.sh
gate cargo test --workspace --locked
gate python3 scripts/vendor.py check
