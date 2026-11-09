#!/usr/bin/env bash
# Fetches the lab's inputs (inputs.txt) into <dir>, each checked against its pinned SHA-256; a file already
# there with the right hash is kept. Any mismatch fails the run (docs/design/shards-guest.md §6a).
#   fetch.sh <dir> [base URL, default https://dl-cdn.alpinelinux.org/alpine/v3.24]
set -euo pipefail
out=${1:?usage: fetch.sh <dir> [base URL]}
base=${2:-https://dl-cdn.alpinelinux.org/alpine/v3.24}
here=$(cd "$(dirname "$0")" && pwd)
sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
while read -r hash path file; do
    case $hash in '#'* | '') continue ;; esac
    mkdir -p "$out/$(dirname "$file")"
    if [ ! -f "$out/$file" ] || [ "$(sha256 "$out/$file")" != "$hash" ]; then
        curl -fsSL -o "$out/$file.part" "$base/$path"
        mv "$out/$file.part" "$out/$file"
    fi
    got=$(sha256 "$out/$file")
    [ "$got" = "$hash" ] || { echo "$file: sha256 $got, pinned $hash" >&2; exit 1; }
done <"$here/inputs.txt"
echo "fetched and checked: $(grep -cv '^#' "$here/inputs.txt") files"
