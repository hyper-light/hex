#!/bin/bash
# stack_cgroups.sh <platform>: prints STACK_CGROUPS (paths under /sys/fs/cgroup) for a platform's stack, as the
# T7 agent set them (2026-10-23): the stack's running containers' scopes plus docker.service and containerd.service.
# E2B adds `e2b`: its orchestrator (cgroup: host) puts each sandbox's Firecracker processes in /sys/fs/cgroup/e2b/<sandbox>
# (packages/orchestrator/pkg/sandbox/cgroup/manager.go, RootCgroupPath), outside every compose container.
extra=""
case "${1:?platform}" in
  daytona) ids=$(cd "$HOME/.cache/hex-bench/daytona" && docker compose -p hexbench-daytona -f docker/docker-compose.yaml -f docker/docker-compose.hexbench.yaml ps -q --no-trunc --status running) ;;
  docker)  echo "hexbench.slice system.slice/docker.service system.slice/containerd.service"; exit 0 ;;
  e2b)     ids=$(docker ps -q --no-trunc --filter "label=${STACK_LABEL:?STACK_LABEL for e2b}")
           [ -d /sys/fs/cgroup/e2b ] || { echo "missing /sys/fs/cgroup/e2b: is the orchestrator healthy?" >&2; exit 1; }
           extra="e2b " ;;
  *) ids=$(docker ps -q --no-trunc --filter "label=${STACK_LABEL:?STACK_LABEL for this platform}") ;;
esac
[ -n "$ids" ] || { echo "no running containers for $1" >&2; exit 1; }
out=""; for id in $ids; do d="system.slice/docker-$id.scope"; [ -d "/sys/fs/cgroup/$d" ] || { echo "missing /sys/fs/cgroup/$d" >&2; exit 1; }; out="$out$d "; done
echo "${out}${extra}system.slice/docker.service system.slice/containerd.service"
