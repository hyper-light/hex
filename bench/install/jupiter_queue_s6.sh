#!/usr/bin/env bash
# HEX-INST-9: current-source, focused native-s6 checks. The lead runs benchmarks and wider gates.
set -euo pipefail
: "${CARGO_TARGET_DIR:?set an isolated target cache}"
: "${CARGO_BUILD_JOBS:?set the admitted build jobs}"
# Git Bash nice cannot lower Windows priority; descendants inherit this native priority class.
powershell -NoProfile -Command '
  $ErrorActionPreference = "Stop"
  [Diagnostics.Process]::GetCurrentProcess().PriorityClass = "BelowNormal"
  & cargo test --locked -p hex-runtime --lib windows::features::tests
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  & cargo test --locked -p hex-runtime --lib windows::wmi::tests
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  & cargo clippy --locked -p hex-core -p hex-runtime -p hex-cli --all-targets -- -D warnings
  exit $LASTEXITCODE
'
