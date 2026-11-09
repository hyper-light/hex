#!/usr/bin/env bash
# Pulls each image (by digest when given as name@sha256:..., else by tag, printing the digest it resolved),
# saves it, and runs census.py over its layers. Images this run pulled are removed after; any already on the
# host are left. Usage: run.sh <out dir> <image>...
set -uo pipefail
out=${1:?usage: run.sh <out dir> <image>...}; shift
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out"
for image in "$@"; do
    had=$(docker image inspect --format '{{.Id}}' "$image" 2>/dev/null || true)
    docker pull -q "$image" >/dev/null || { echo "pull failed: $image"; continue; }
    digest=$(docker image inspect --format '{{index .RepoDigests 0}}' "$image")
    slug=$(echo "$image" | tr '/:@' '___')
    echo "== $image -> $digest"
    docker save "$image" -o "$out/$slug.tar"
    python3 -I "$here/census.py" "$out/$slug.tar" > "$out/$slug.json"
    tail -n 1 "$out/$slug.json"
    rm -f "$out/$slug.tar"
    [ -n "$had" ] || docker rmi -f "$image" >/dev/null
done
