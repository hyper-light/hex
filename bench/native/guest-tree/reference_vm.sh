#!/bin/bash
# HEX-GUEST-21's reference where no host containerd is to hand (a Mac Studio's Docker VM; the lead's call,
# 2026-11-06): run inside a privileged container, it fetches containerd's own static release, starts
# containerd with its state on a Docker volume (an ext4 the overlayfs snapshotter can use), and runs
# reference.py --here against it, exactly the fedora method otherwise: pull by digest for linux/amd64 (no amd64
# code runs: an unpack writes files), save the blobs byte-exact, mount each snapshot's view read-only and walk
# it as root. Writes OUT/RECORD.txt with the VM's kernel.
# usage (in the container): reference_vm.sh OUT IMAGE...   (the walker at /hex; this tree at /src)
set -euo pipefail
OUT=$1; shift
mkdir -p "$OUT"
V=2.3.6
ARCH=$(uname -m | sed 's/aarch64/arm64/; s/x86_64/amd64/')
T=containerd-static-$V-linux-$ARCH.tar.gz
U=https://github.com/containerd/containerd/releases/download/v$V
python3 -I - "$U/$T" "$U/$T.sha256sum" /tmp/$T <<'PY'
import hashlib, sys, urllib.request
url, sums, out = sys.argv[1:4]
want = urllib.request.urlopen(sums, timeout=120).read().decode().split()[0]
data = urllib.request.urlopen(url, timeout=300).read()
got = hashlib.sha256(data).hexdigest()
if got != want:
    sys.exit(f"containerd release {url}: sha256 {got}, its sum says {want}")
open(out, "wb").write(data)
print("containerd release", url, "sha256", got)
PY
tar -xzf /tmp/$T -C /usr/local
mkdir -p /var/lib/containerd /run/containerd
/usr/local/bin/containerd --root /var/lib/containerd --state /run/containerd >"$OUT/containerd.log" 2>&1 &
PID=$!
echo "containerd pid $PID"
for _ in $(seq 1 600); do [ -S /run/containerd/containerd.sock ] && break; sleep 0.1; done
export PATH=/usr/local/bin:$PATH
set +e
python3 -I /src/bench/native/guest-tree/reference.py --here /hex "$OUT" "$@"
S=$?
kill "$PID"; wait "$PID" 2>/dev/null
echo "vm kernel: $(uname -r) $(uname -m); platform forced: linux/amd64; containerd from $U/$T" >> "$OUT/RECORD.txt"
exit $S
