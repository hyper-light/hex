#!/usr/bin/env bash
set -uo pipefail
export HEX_OCI_ROOT=/run/hex-oci
B=/run/bundle; rm -rf "$B"; mkdir -p "$B/rootfs"/{usr,proc,dev,tmp}
# A minimal rootfs: relative symlinks into a read-only bind of the host's /usr (fast; no copy).
( cd "$B/rootfs" && ln -sfn usr/bin bin && ln -sfn usr/lib lib && ln -sfn usr/lib64 lib64 && ln -sfn usr/sbin sbin )
cat > "$B/config.json" <<JSON
{ "ociVersion":"1.3.0","root":{"path":"rootfs","readonly":false},"hostname":"b",
  "process":{"cwd":"/","args":["/usr/bin/true"],"env":["PATH=/usr/bin:/usr/sbin"],"user":{"uid":0,"gid":0}},
  "mounts":[{"destination":"/usr","type":"bind","source":"/usr","options":["rbind","ro"]},
            {"destination":"/proc","type":"proc","source":"proc"},
            {"destination":"/dev","type":"tmpfs","source":"tmpfs","options":["nosuid","mode=755","size=65536k"]}],
  "linux":{"namespaces":[{"type":"pid"},{"type":"mount"},{"type":"ipc"},{"type":"uts"},{"type":"network"}]} }
JSON
mkdir -p /sys/fs/cgroup/lab 2>/dev/null
for p in $(cat /sys/fs/cgroup/cgroup.procs 2>/dev/null); do echo "$p" > /sys/fs/cgroup/lab/cgroup.procs 2>/dev/null || true; done
# prove one run works before timing (surfaces errors instead of hanging the loop)
/opt/hex oci create --bundle "$B" probe && /opt/hex oci start probe && /opt/hex oci delete --force probe || { echo "PROBE FAILED"; /opt/hex oci create --bundle "$B" probe2; /opt/hex oci delete --force probe2 2>/dev/null; exit 1; }
echo "probe ok; timing $N"
python3 -u - "$N" <<'PY'
import subprocess,sys,time,os
N=int(sys.argv[1]); B="/run/bundle"; env=dict(os.environ); ms=[]
# The oci runtime is detached: `create` leaves the container's init alive (its shim parent in waitpid,
# its container child blocked opening exec.fifo until `start`), and that init inherits this process's
# stdout/stderr. So we must NOT pipe-capture the hex calls: subprocess with PIPE (capture_output)
# drains stdout/stderr to EOF, which cannot come until the container execs and exits, which needs
# `start`, which we cannot send while still blocked reading `create` -- a deadlock on the first
# `create` (and a leaked --rm container). This deadlocks against any compliant detached runtime. Send
# stdio to /dev/null (success) and a reused file (so an error stays legible), and wait only on each
# call's exit, never on fd EOF.
dn=open(os.devnull,"wb"); err=open("/opt/out/last_err.txt","w+b")
def hx(*a):
    err.seek(0); err.truncate()
    r=subprocess.run(["/opt/hex","oci",*a],stdout=dn,stderr=err,env=env)
    if r.returncode:
        err.seek(0); r.msg=err.read().decode(errors="replace")[:300]
    return r
for i in range(10+N):
    cid=f"t{i}"
    t=time.perf_counter_ns()
    c=hx("create","--bundle",B,cid)
    s=hx("start",cid)
    dt=(time.perf_counter_ns()-t)/1e6
    hx("delete","--force",cid)
    if c.returncode or s.returncode:
        sys.stdout.write("FAIL "+(("create "+c.msg) if c.returncode else ("start "+s.msg))+"\n"); sys.exit(1)
    if i>=10: ms.append(dt)   # drop 10 warm-up
    if i%20==0: sys.stdout.write(f"  iter {i}\n")
ms.sort(); p=lambda q: ms[min(len(ms)-1,int(q*len(ms)))]
line=f"n={len(ms)} create+start ms: p50={p(0.5):.1f} p90={p(0.9):.1f} p99={p(0.99):.1f} min={ms[0]:.1f} max={ms[-1]:.1f}\nAC-T7.1.3 (oci start p50<=50ms): "+("PASS" if p(0.5)<=50 else "FAIL")+"\n"
sys.stdout.write(line); open("/opt/out/result.txt","w").write(line)
PY
