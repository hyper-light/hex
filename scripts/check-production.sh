#!/usr/bin/env bash
# The no-panic rule (CLAUDE.md §1) on production code alone: libraries and binaries (cargo's default
# targets) built without cfg(test), so the test-only allowances at each crate root don't apply. The
# lints are forbidden, not just denied: an #[allow] or #[expect] for one in production code fails.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
lints=(
  -D warnings
  -F clippy::panic
  -F clippy::unwrap_used
  -F clippy::expect_used
  -F clippy::unreachable
  -F clippy::todo
  -F clippy::unimplemented
  -F clippy::indexing_slicing
  -F clippy::arithmetic_side_effects
  -F clippy::string_slice
  -F clippy::cast_possible_truncation
  -F clippy::cast_sign_loss
  -F clippy::cast_possible_wrap
  -F clippy::disallowed_macros
  -F clippy::disallowed_methods
  -F clippy::disallowed_types
  -F clippy::dbg_macro
  -F clippy::exit
)
# hex-node's `test-controls` feature holds the builders only hex-cli's tests call (each fixes what a node otherwise
# learns, or turns a pool off, so a test reaches a state it couldn't otherwise). Production never builds them: no
# normal or build dependency edge turns the feature on, and each such builder is under the feature's cfg.
controls=(with_prepared_procs with_init_budget with_netns_bound with_ipc_uts_bound with_inits_ahead
  with_parked_procs_bound with_device_program_bound with_classic_roots hold_run_removals)
if cargo tree --workspace --locked -e normal,build -f '{p} {f}' | grep -q 'test-controls'; then
  echo "check-production: a production build turns on hex-node's test-controls" >&2; exit 1
fi
for c in "${controls[@]}"; do
  grep -B1 -E "pub fn $c\(" crates/hex-node/src/node.rs | grep -q 'cfg(feature = "test-controls")' \
    || { echo "check-production: hex-node's $c is built into production, not under test-controls" >&2; exit 1; }
done
cargo clippy --workspace --locked "$@" -- "${lints[@]}"
