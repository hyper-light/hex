#!/usr/bin/env bash
# runtime-tools' validation suite rootless (goal 17's rootless variant, rootless_record.sh): <runtime> run as this
# user, with no container and nothing as root, in a delegated systemd --user scope of its own (Delegate=yes, as
# hex-cgroup-delegation.md describes), whose cgroup inside.sh splits as it splits a container's root (CGROUP_ROOT).
# The suite's tests run as the user, so a test that needs root fails here for every runtime alike; the rootless record
# names those as crun's passes as root that it misses rootless. Its temporary files go under <out dir>, off the host's
# shared /tmp. With `hex` last, <runtime> is the hex binary, run as `hex oci`.
# Usage: rootless_validate.sh <runtime-tools checkout> <runtime binary> <out dir> [hex]
set -euo pipefail
RT=$(realpath "${1:?runtime-tools}"); BIN=$(realpath "${2:?runtime binary}"); OUT=$(realpath -m "${3:?out dir}")
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
[ "$(id -u)" != 0 ] || { echo "refusing: the rootless suite runs as a user, not root" >&2; exit 2; }
[ -S "${XDG_RUNTIME_DIR:-/nonexistent}/systemd/private" ] \
  || { echo "refusing: no systemd --user manager answers for a delegated scope" >&2; exit 2; }
[ ! -e "$OUT" ] || { echo "refusing: $OUT exists; each run gets a new dir" >&2; exit 2; }
mkdir -p "$OUT/tmp"
if [ "${4:-}" = hex ]; then
  printf '#!/bin/sh\nexec %s oci "$@"\n' "$BIN" > "$OUT/runtime"
else
  printf '#!/bin/sh\nexec %s "$@"\n' "$BIN" > "$OUT/runtime"
fi
chmod +x "$OUT/runtime"
{ echo "host: $(hostname), kernel $(uname -r), uid $(id -u), load $(cut -d' ' -f1-3 /proc/loadavg), $(date -u +%FT%TZ)"
  echo "runtime: $BIN ($("$OUT/runtime" --version 2>&1 | head -1)), runtime-tools $(git -C "$RT" rev-parse --short HEAD 2>/dev/null)"
} > "$OUT/host.txt"
exec systemd-run --user --scope --quiet -p Delegate=yes -- env RT="$RT" OUT="$OUT" RUNTIME="$OUT/runtime" \
  TMPDIR="$OUT/tmp" bash -c 'export CGROUP_ROOT=/sys/fs/cgroup$(cut -d: -f3 /proc/self/cgroup); exec bash "$1"' _ "$HERE/inside.sh"
