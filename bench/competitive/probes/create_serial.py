#!/usr/bin/env python3
"""bench/competitive/probes: create_serial.sh's reader. bpftrace's off-CPU map (@off[kstack, comm]: ns) and on-CPU
profile (@cpu[kstack]: samples), grouped by blocking site, per lifecycle, concurrency 1 against 32.
  create_serial.py <bt.c1.txt> <lifecycles c1> <bt.c32.txt> <lifecycles c32>"""
import re, sys
from collections import defaultdict
SCHED = re.compile(r'^(finish_task_switch|__schedule|schedule$|schedule_timeout|schedule_hrtimeout|'
                   r'schedule_preempt_disabled|io_schedule|preempt_schedule|__cond_resched|__wait_for_common|'
                   r'wait_for_completion|schedule_idle|do_nanosleep$)')
def parse(path):
    off, cpu = defaultdict(int), defaultdict(int)
    txt = open(path, errors='replace').read()
    for m in re.finditer(r'@(off|cpu)\[\n(.*?)\n(?:, ([^\]]+))?\]: (\d+)', txt, re.S):
        kind, body, comm, val = m.group(1), m.group(2), m.group(3), int(m.group(4))
        frames = [re.sub(r'\+\d+$', '', f.strip()) for f in body.splitlines() if f.strip()]
        frames = [f.split('.')[0] if f.startswith(('__mutex_lock', 'finish_task_switch')) else f for f in frames]
        if kind == 'off':
            rest = [f for f in frames if not SCHED.match(f)]
            site = ' < '.join(rest[:4]) or '(scheduler only)'
            off[(comm.strip() if comm else '?', site)] += val
        else:
            cpu[' < '.join(frames[:3]) or '(user)'] += val
    return off, cpu
c1o, c1c = parse(sys.argv[1]); n1 = int(sys.argv[2])
c32o, c32c = parse(sys.argv[3]); n32 = int(sys.argv[4])
keys = set(c1o) | set(c32o)
rows = []
for k in keys:
    a = c1o.get(k, 0) / n1 / 1e6; b = c32o.get(k, 0) / n32 / 1e6
    rows.append((b, a, k))
rows.sort(reverse=True)
print(f"off-CPU ms per lifecycle by (comm, blocking site), c1 ({n1}) vs c32 ({n32}); top 30 by c32:")
print(f"{'c32':>8} {'c1':>8}  comm      site")
for b, a, (comm, site) in rows[:30]:
    print(f"{b:8.3f} {a:8.3f}  {comm:9.9} {site[:150]}")
tot1 = sum(c1c.values()) or 1; tot32 = sum(c32c.values()) or 1
print(f"\non-CPU samples (share), c32 top 15 (c1 share beside):")
for k, v in sorted(c32c.items(), key=lambda kv: -kv[1])[:15]:
    print(f"{100*v/tot32:6.2f}% {100*c1c.get(k,0)/tot1:6.2f}%  {k[:150]}")
