#!/usr/bin/env bash
# bench/competitive: one platform round of Zeroboot, inside the privileged lab container (zeroboot.sh):
# its template made the way its docs make one, its server alone in the cgroup /stack (the stack), and
# platform_bench.py's zeroboot adapter in /client.
#   zeroboot_inside.sh <out.json> <n> <warmup> <lifecycles> <density cap>
set -euo pipefail
CG=/sys/fs/cgroup
mkdir -p "$CG/lab"
for p in $(cat "$CG/cgroup.procs"); do echo "$p" > "$CG/lab/cgroup.procs" 2>/dev/null || true; done
echo "+cpu +io +memory +pids" > "$CG/cgroup.subtree_control"
mkdir "$CG/stack" "$CG/client"

# The guest: upstream's init alone on an ext4 root (mount points for the proc, sysfs and devtmpfs it
# mounts). Firecracker writes to the rootfs as it boots, so the template gets a fresh copy (DEPLOYMENT.md).
Z=/opt/zeroboot; W=/run/zb
mkdir -p "$W/root"/{dev,proc,sys}
cp "$Z/init" "$W/root/init"
mkfs.ext4 -q -d "$W/root" "$W/rootfs.ext4" 16M
# `zeroboot template` runs `firecracker` from PATH: v1.12.0, whose snapshot layout Zeroboot reads. Its
# defaults: a 3 s wait before the snapshot, 512 MiB of guest memory.
PATH="$Z:$PATH" zeroboot template "$Z/vmlinux" "$W/rootfs.ext4" "$W/workdir" 3 /init > /opt/out/zeroboot-template.log 2>&1

( echo "$BASHPID" > "$CG/stack/cgroup.procs" && exec "$Z/zeroboot" serve "$W/workdir" 8080 ) > /opt/out/zeroboot-serve.log 2>&1 &
# Up when it answers; bounded.
for i in $(seq 1 600); do
  python3 -I -c 'import urllib.request; urllib.request.urlopen("http://127.0.0.1:8080/v1/health", timeout=1)' 2> /dev/null && break
  [ "$i" = 600 ] && { echo "refusing: zeroboot didn't answer within 60 s (zeroboot-serve.log)" >&2; exit 3; }
  sleep 0.1
done
( echo "$BASHPID" > "$CG/client/cgroup.procs" \
  && exec env STACK_CGROUPS=stack ZEROBOOT_URL=http://127.0.0.1:8080 python3 -I /opt/lab/platform_bench.py zeroboot "$@" )
