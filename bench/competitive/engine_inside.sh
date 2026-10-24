#!/usr/bin/env bash
# bench/competitive: one platform round of containerd with nerdctl, inside the privileged lab container
# (started by engine.sh): platform_bench.py's docker adapter with ENGINE=nerdctl.
#   engine_inside.sh <out.json> <n> <warmup> <lifecycles> <density cap>
# containerd runs at its defaults alone in the cgroup /stack, its sandboxes under /sandboxes (and Kata's
# overhead under /kata_overhead): the stack's cgroups. The client runs in /client, so its own CPU is its
# rusage, as on the host. With /opt/kata mounted, Kata's shim answers for io.containerd.kata-<h>.v2, for
# each hypervisor <h> in KATA_HYPERVISORS, through a wrapper naming that hypervisor's configuration.
# Env: ENGINE_RUNTIME, ENGINE_NAME (platform_bench.py), HEXBENCH_PHASES, KATA_HYPERVISORS (qemu clh).
set -euo pipefail
CG=/sys/fs/cgroup

# cgroup v2's no-internal-processes rule: everything here moves to a leaf before the root hands controllers down.
mkdir -p "$CG/lab"
for p in $(cat "$CG/cgroup.procs"); do echo "$p" > "$CG/lab/cgroup.procs" 2>/dev/null || true; done
echo "+cpu +io +memory +pids" > "$CG/cgroup.subtree_control"
mkdir "$CG/stack" "$CG/sandboxes" "$CG/kata_overhead" "$CG/client"

# containerd finds its shims, and the runc shim finds runc (the OCI table's), on its PATH.
mkdir -p /run/engine/bin
export PATH=/run/engine/bin:/opt/bin:$PATH
if [ -d /opt/kata/bin ]; then
  KC=/opt/kata/share/defaults/kata-containers
  for h in ${KATA_HYPERVISORS:-qemu clh}; do
    [ -f "$KC/configuration-$h.toml" ] || { echo "refusing: no $KC/configuration-$h.toml; there are: $(cd "$KC" && ls -- *.toml | tr '\n' ' ')" >&2; exit 4; }
    printf '#!/bin/sh\nKATA_CONF_FILE=/opt/kata/share/defaults/kata-containers/configuration-%s.toml exec /opt/kata/bin/containerd-shim-kata-v2 "$@"\n' \
      "$h" > "/run/engine/bin/containerd-shim-kata-$h-v2"
    chmod 755 "/run/engine/bin/containerd-shim-kata-$h-v2"
  done
fi

( echo "$BASHPID" > "$CG/stack/cgroup.procs" && exec containerd ) > /opt/out/containerd.log 2>&1 &
# Up when it answers; bounded.
for i in $(seq 1 600); do
  nerdctl version > /dev/null 2>&1 && break
  [ "$i" = 600 ] && { echo "refusing: containerd didn't answer within 60 s (containerd.log)" >&2; exit 3; }
  sleep 0.1
done
nerdctl load -i /opt/image/rootfs.tar > /dev/null
{ echo "containerd $(containerd --version)"; echo "nerdctl $(nerdctl --version)"; echo "runc $(runc --version | head -1)"
  [ -x /opt/kata/bin/kata-runtime ] && echo "kata $(/opt/kata/bin/kata-runtime --version | head -1)"
} >> /opt/out/engine_versions.txt

( echo "$BASHPID" > "$CG/client/cgroup.procs" \
  && exec env ENGINE=nerdctl DOCKER_CGROUP_PARENT=/sandboxes STACK_CGROUPS="stack sandboxes kata_overhead" \
       python3 -I /opt/lab/platform_bench.py docker "$@" )
