"""Times kube-scheduler's binds from the API server's own watch (AC-T7.11.3).

Reads `kubectl get pods -w -o custom-columns=NAME:.metadata.name,NODE:.spec.nodeName --no-headers`
on stdin. Once every expected pod has been listed it writes the ready file (the run script starts
kube-scheduler only then). It ends when every pod is bound or when no new bind has come for
`stall_s` seconds, and writes one JSON line: binds seen, and the monotonic times of the first and
last bind beside the scheduler's start time read from the start file.

    python3 -I watch_binds.py <expected> <ready-file> <start-file> <stall-s> <out-json>
"""

import json
import os
import select
import sys
import time


def main() -> int:
    expected = int(sys.argv[1])
    ready_file, start_file = sys.argv[2], sys.argv[3]
    stall_s = float(sys.argv[4])
    out = sys.argv[5]
    seen = set()
    bound = set()
    first = last = None
    ready = False
    fd = sys.stdin.fileno()
    buf = b""
    eof = False
    while not eof:
        wait = 1.0
        r, _, _ = select.select([fd], [], [], wait)
        now = time.monotonic()
        if r:
            chunk = os.read(fd, 1 << 20)
            if not chunk:
                eof = True
            buf += chunk
            *lines, buf = buf.split(b"\n")
            for line in lines:
                parts = line.split()
                if len(parts) != 2:
                    continue
                name, node = parts
                seen.add(name)
                if node != b"<none>" and name not in bound:
                    bound.add(name)
                    if first is None:
                        first = now
                    last = now
        if not ready and len(seen) >= expected:
            with open(ready_file, "w") as f:
                f.write("%f\n" % now)
            ready = True
        if len(bound) >= expected:
            break
        if ready and last is not None and now - last > stall_s:
            break
        if ready and last is None and os.path.exists(start_file):
            with open(start_file) as f:
                started = float(f.read())
            if now - started > 600:
                break
    started = None
    if os.path.exists(start_file):
        with open(start_file) as f:
            started = float(f.read())
    result = {
        "expected": expected,
        "listed": len(seen),
        "bound": len(bound),
        "scheduler_start": started,
        "first_bind": first,
        "last_bind": last,
    }
    if started is not None and last is not None and last > started:
        result["pods_per_s_from_start"] = len(bound) / (last - started)
    if first is not None and last is not None and last > first:
        result["pods_per_s_first_to_last"] = (len(bound) - 1) / (last - first)
    with open(out, "w") as f:
        f.write(json.dumps(result) + "\n")
    print(json.dumps(result))
    return 0


if __name__ == "__main__":
    sys.exit(main())
