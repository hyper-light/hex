#!/usr/bin/env bash
set -uo pipefail
export HEX_OCI_ROOT="$WORK/state"
B="$WORK/bundle"; rm -rf "$B"; mkdir -p "$B/rootfs"
# A minimal rootfs: the host's /usr mounted in gives us busybox-less coreutils via /bin -> /usr/bin.
mkdir -p "$B/rootfs"/{proc,dev,bin,usr,lib,lib64,etc,tmp}
for d in bin usr lib lib64; do rm -rf "$B/rootfs/$d"; cp -a "/$d" "$B/rootfs/$d" 2>/dev/null || ln -s "/usr/$d" "$B/rootfs/$d"; done
cat > "$B/config.json" <<JSON
{
  "ociVersion": "1.3.0",
  "root": {"path": "rootfs", "readonly": false},
  "hostname": "hexbox",
  "process": {
    "cwd": "/",
    "args": ["/bin/sh", "-c", "echo hello-from-hex; hostname; id -u"],
    "env": ["PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"],
    "user": {"uid": 0, "gid": 0},
    "capabilities": {"bounding":["CAP_AUDIT_WRITE","CAP_KILL","CAP_NET_BIND_SERVICE"],"effective":["CAP_AUDIT_WRITE","CAP_KILL"],"permitted":["CAP_AUDIT_WRITE","CAP_KILL"]},
    "rlimits": [{"type":"RLIMIT_NOFILE","hard":1024,"soft":1024}],
    "noNewPrivileges": true
  },
  "mounts": [
    {"destination": "/proc", "type": "proc", "source": "proc"},
    {"destination": "/dev", "type": "tmpfs", "source": "tmpfs", "options": ["nosuid","strictatime","mode=755","size=65536k"]},
    {"destination": "/tmp", "type": "tmpfs", "source": "tmpfs", "options": ["nosuid","nodev"]}
  ],
  "linux": {
    "namespaces": [{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}],
    "maskedPaths": ["/proc/kcore"],
    "readonlyPaths": ["/proc/sys"]
  }
}
JSON
# move procs out of the cgroup ns root (no-internal-processes), as the validation lab does
mkdir -p /sys/fs/cgroup/lab 2>/dev/null
for p in $(cat /sys/fs/cgroup/cgroup.procs 2>/dev/null); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
set -x
/opt/hex oci create --bundle "$B" --pid-file "$WORK/pid" testbox; echo "create=$?"
/opt/hex oci state testbox; echo "state=$?"
/opt/hex oci start testbox; echo "start=$?"
sleep 0.3
/opt/hex oci state testbox; echo "state2=$?"
/opt/hex oci delete testbox; echo "delete=$?"
