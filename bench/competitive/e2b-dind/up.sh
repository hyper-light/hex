#!/usr/bin/env bash
# bench/competitive/e2b-dind: brings up E2B's stack (e2b-dev/runtime at bca4b33, embed/compose, unchanged) inside
# one privileged container with its own Docker daemon and network namespace, on adacachy (2026-10-24; see
# ../platform-rounds/e2b_adacachy.sh for why). The host's /dev is bound in, since E2B's host setup loads nbd and
# a container's own /dev never sees devices made after it started, along with the host's kernel modules. E2B's
# working directories are volumes on the host's disk: its template build writes overlay whiteouts (0:0 character
# devices) as it unpacks images, which the container's own overlayfs root refuses (EPERM). Its
# hugepages are the host's own (2 MiB pages are global), compacted first so 9216 of them reserve. Down:
# `docker exec hexbench-e2b-dind sh -c 'cd /e2b && docker compose down -v && docker compose --profile purge run
# --rm host-teardown'`, then `docker rm -f hexbench-e2b-dind` and `docker volume rm hexbench-e2b-{dind,orch,lib,fcvm,fcenvd}`.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
E=$HOME/.cache/hex-bench/e2b
docker build -q -t hexbench/e2b-dind:1 "$HERE"
for v in dind orch lib fcvm fcenvd; do docker volume create --label hex.bench=competitive "hexbench-e2b-$v" > /dev/null; done
docker run -d --name hexbench-e2b-dind --label hex.bench=competitive --privileged --cgroupns=private --cpuset-cpus=4-31 \
  -v /dev:/dev -v hexbench-e2b-dind:/var/lib/docker -v "$E:/e2b" -v /usr/lib/modules:/usr/lib/modules:ro \
  -v hexbench-e2b-orch:/orchestrator -v hexbench-e2b-lib:/var/lib/e2b -v hexbench-e2b-fcvm:/fc-vm -v hexbench-e2b-fcenvd:/fc-envd \
  -p 127.0.0.1:3000:3000 -p 127.0.0.1:3001:3001 -p 127.0.0.1:3002:3002 hexbench/e2b-dind:1
for i in $(seq 1 100); do docker exec hexbench-e2b-dind docker version > /dev/null 2>&1 && break; sleep 0.1; done
sudo -n sh -c 'sync; echo 1 > /proc/sys/vm/compact_memory'
docker exec -e HUGEPAGES=9216 hexbench-e2b-dind sh -c 'cd /e2b && docker compose up -d --wait'
