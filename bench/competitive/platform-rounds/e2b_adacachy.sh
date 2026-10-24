#!/usr/bin/env bash
# bench/competitive/platform-rounds: E2B's self-hosted rounds on adacachy (2026-10-24). fedora couldn't reserve
# the 4 GiB of contiguous hugepages E2B's host setup needs (its memory too fragmented by other units), so Ada had
# E2B run here, with Hex's oci rounds beside it for a same-hardware ratio. adacachy runs production, at a 1-minute
# load of 40-110, so these rounds are unrefereed and not of record by CLAUDE.md §7: each round records the load.
#   e2b_adacachy.sh <out dir> <round> [n warmup lifecycles cap]
# The stack: e2b-dev/runtime at bca4b33, embed/compose, unchanged, brought up with HUGEPAGES=9216 (18 GiB: 32
# concurrent creates each map a 512 MiB sandbox's 256 pages, plus room for the base template). It runs inside one
# privileged container, hexbench-e2b-dind (../e2b-dind/), with its own Docker daemon and network namespace. That
# keeps adacachy's firewall as it is: its UFW drops the hop from each sandbox to the orchestrator's egress proxy
# on the host side of the sandbox's veth, a hop that here stays inside the container. The API and sandbox ports
# are published on 127.0.0.1. The API key comes from the stack's own sdk.env, read into a mode-600 file and never
# printed. STACK_CGROUPS is that container's scope: its Docker daemon, E2B's services, and the /e2b cgroups where
# the orchestrator puts each sandbox's Firecracker.
set -euo pipefail
HERE=$(dirname "$(realpath "${BASH_SOURCE[0]}")")
OUT=$(realpath "${1:?out dir}"); ROUND=${2:?round}
N=${3:-300}; WARMUP=${4:-10}; LIFECYCLES=${5:-100}; CAP=${6:-200}
E=$HOME/.cache/hex-bench/e2b; V=$HOME/.cache/hex-bench/venv-e2b; C=hexbench-e2b-dind
case "$(hostname)" in adacachy) ;; *) echo "refusing: this is adacachy's E2B round, not $(hostname)'s" >&2; exit 2 ;; esac
id=$(docker inspect --format '{{.Id}}' "$C")
SC="system.slice/docker-$id.scope"
[ -d "/sys/fs/cgroup/$SC/e2b" ] || { echo "refusing: no /sys/fs/cgroup/$SC/e2b; is E2B's orchestrator up?" >&2; exit 3; }
( umask 077; docker exec "$C" sh -c 'cd /e2b && docker compose exec -T ready cat /run/e2b/sdk.env' > "$E/sdk.env" )
echo "round $ROUND: start $(date -u +%FT%TZ), load $(cut -d' ' -f1-3 /proc/loadavg)" >> "$OUT/e2b-adacachy.txt"
# A delegated user unit of its own, so the client is alone in its cgroup, outside the stack's.
systemd-run --user --unit="hexbench-e2b-r$ROUND" --property=Delegate=yes --wait --collect --pipe \
  --setenv=STACK_CGROUPS="$SC" --setenv=E2B_NAME=e2b --setenv=HEXBENCH_PHASES="${HEXBENCH_PHASES:-}" \
  bash -c 'set -a; . "$1"; set +a; shift; exec "$@"' _ "$E/sdk.env" \
  taskset -c 4-31 "$V/bin/python" -I "$HERE/../platform_bench.py" e2b "$OUT/platform.e2b.r$ROUND.json" \
  "$N" "$WARMUP" "$LIFECYCLES" "$CAP" 2>&1 | tee -a "$OUT/e2b.log"
echo "round $ROUND: end $(date -u +%FT%TZ), load $(cut -d' ' -f1-3 /proc/loadavg)" >> "$OUT/e2b-adacachy.txt"
