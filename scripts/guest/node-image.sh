#!/usr/bin/env bash
# Builds Hex's node image for shards-init (docs/design/node-under-shards-init.md §4) into a Shards home:
#   scripts/guest/node-image.sh HEX SHARDS_HOME TAG
# HEX is the guest's static `hex`, built for the guest's architecture, which is the host's (image.sh or build.sh
# builds it); SHARDS_HOME is the node's Shards home, whose store takes the image; TAG names it there. `shards` is
# SHARDS_BIN, else the one on PATH. The build context, node.Agentfile and a copy of HEX, is made under
# ${XDG_CACHE_HOME:-$HOME/.cache}/hex-node-image and removed at the end. Prints the image pinned by the manifest digest
# its build says, TAG@sha256:..., as HEX_GUEST_IMAGE takes it: Shards' store finds a pinned image by content, whatever
# it is named (Store::holding).
set -euo pipefail

usage="usage: node-image.sh HEX SHARDS_HOME TAG"
hex=${1:?$usage}
home=${2:?$usage}
tag=${3:?$usage}
here=$(cd "$(dirname "$0")" && pwd)
shards=${SHARDS_BIN:-shards}
cache=${XDG_CACHE_HOME:-$HOME/.cache}/hex-node-image
mkdir -p "$cache"
work=$(mktemp -d "$cache/build.XXXXXX")
trap 'rm -rf "${work:?}"' EXIT
mkdir "$work/context"
cp "$hex" "$work/context/hex"
cp "$here/node.Agentfile" "$work/context/Agentfile"
# The build runs in the home's daemon, which it starts; it is stopped once the image is in the store.
SHARDS_HOME=$home "$shards" build image -q -t "$tag" --metadata-file "$work/build.json" "$work/context" >&2
SHARDS_HOME=$home "$shards" daemon stop >&2
digest=$(python3 -I -c 'import json, sys; print(json.load(open(sys.argv[1]))["containerimage.digest"])' "$work/build.json")
case $digest in
sha256:*) echo "$tag@$digest" ;;
*)
    echo "the build named no sha256 manifest digest: $digest" >&2
    exit 1
    ;;
esac
