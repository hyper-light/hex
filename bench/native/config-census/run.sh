#!/usr/bin/env bash
# The config.json census behind spec::MAX_CONFIG_BYTES (Ada's 2026-11-06 rule: an input bound is the largest
# legitimate input recorded, times 2). Sizes, in bytes, of:
#   - the config.json each engine hands its runtime for the six images of the tarball census
#     (docs/research/runtime/shards-guest.md §9, pinned by the digests below): Docker's (containerd's bundle for
#     runc, read from the host's /run/containerd through a privileged container with --pid=host) and Podman's
#     (its rootless bundle for crun, after `podman init`);
#   - every config.json runtime-tools' validation suite writes (the suite Hex runs, bench/oci-validation), each
#     recorded by a runtime that logs its bundle's config.json and hands the call on to runc.
#   run.sh <out dir> <runtime-tools checkout, its validation built>
# Writes <out>/sizes.tsv (source, image or test, bytes) and <out>/max.txt. Containers carry the label
# hex.bench=config-census and are removed on exit.
set -euo pipefail
OUT=$(realpath -m "${1:?out dir}")
RT=$(realpath "${2:?runtime-tools checkout}")
mkdir -p "$OUT"
LABEL=hex.bench=config-census
IMAGES=(
  docker.io/library/debian@sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc
  docker.io/library/ubuntu@sha256:fe81560990fae86dc4743202b2cb4d3df98d0bcf949173cecef15ce1127ef852
  docker.io/library/alpine@sha256:3f4d6b32e26703c6082839234f43769d5046b3ac7c910033911d1e71d8cd630a
  docker.io/library/fedora@sha256:b3e011d47e686a89781495c9a87bc80349af742965e5d51e79cd51711982e4a2
  docker.io/library/caddy@sha256:ce773ec7a742ce15ee6bf2bb5f65882b9e2a4487b10733e6f1484f86e6b5d8bc
  docker.io/grafana/grafana@sha256:488984b716c0862639f91a698adbe2862677a5a07bad42da5f03f3030883b205
)
ALPINE=${IMAGES[2]}
reap() {
  docker ps -aq --filter "label=$LABEL" | xargs -r docker rm -f > /dev/null
  podman ps -aq --filter "label=$LABEL" | xargs -r podman rm -f > /dev/null
}
trap reap EXIT
: > "$OUT/sizes.tsv"

for image in "${IMAGES[@]}"; do
  docker pull -q "$image" > /dev/null
  id=$(docker run -d -t --label "$LABEL" "$image")
  bytes=$(docker run --rm --privileged --pid=host --label "$LABEL" "$ALPINE" \
    sh -c "wc -c < /proc/1/root/run/containerd/io.containerd.runtime.v2.task/moby/$id/config.json")
  printf 'docker\t%s\t%s\n' "$image" "$bytes" >> "$OUT/sizes.tsv"
  docker rm -f "$id" > /dev/null

  podman pull -q "$image" > /dev/null
  id=$(podman create -t --label "$LABEL" "$image")
  podman init "$id" > /dev/null
  bytes=$(wc -c < "$(podman inspect -f '{{.StaticDir}}' "$id")/config.json")
  printf 'podman\t%s\t%s\n' "$image" "$bytes" >> "$OUT/sizes.tsv"
  podman rm -f "$id" > /dev/null
done

# runtime-tools: a runtime that logs the size of each bundle's config.json it is handed, then runs runc.
cat > "$OUT/record-runtime" <<'SH'
#!/bin/sh
prev=
for a in "$@"; do
  case "$prev" in --bundle|-b) printf '%s\t%s\n' "$a" "$(wc -c < "$a/config.json")" >> "$CENSUS_LOG" ;; esac
  case "$a" in --bundle=*) b=${a#--bundle=}; printf '%s\t%s\n' "$b" "$(wc -c < "$b/config.json")" >> "$CENSUS_LOG" ;; esac
  prev=$a
done
exec runc "$@"
SH
chmod +x "$OUT/record-runtime"
: > "$OUT/runtime-tools.log"
docker run --rm --privileged --security-opt label=disable --cgroupns=private --network=none --label "$LABEL" \
  -v /usr:/usr:ro -v "$RT:$RT:ro" -v "$OUT:$OUT" -e CENSUS_LOG="$OUT/runtime-tools.log" -e RT="$RT" \
  -e RUNTIME="$OUT/record-runtime" registry.fedoraproject.org/fedora:44 bash -c '
    cd "$RT" && for t in validation/*/*.t; do TMPDIR=/tmp timeout 60 "$t" > /dev/null 2>&1; done; true'
awk -F'\t' '!seen[$1]++ { printf "runtime-tools\t%s\t%s\n", $1, $2 }' "$OUT/runtime-tools.log" >> "$OUT/sizes.tsv"

awk -F'\t' '$3 > max { max = $3; at = $1 " " $2 } END { printf "%d %s\n", max, at }' "$OUT/sizes.tsv" > "$OUT/max.txt"
cat "$OUT/sizes.tsv" "$OUT/max.txt"
