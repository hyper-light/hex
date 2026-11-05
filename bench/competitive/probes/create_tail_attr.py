# Attribute create_tail runs' slowest creates' charges by kind of teardown work. Usage: create_tail_attr.py <probes dir> <run dir>...
import sys, collections, importlib.util
spec = importlib.util.spec_from_file_location("ct", sys.argv[1] + "/create_tail.py"); ct = importlib.util.module_from_spec(spec); spec.loader.exec_module(ct)
def cat(k):
    if "css_" in k: return "css_*_work_fn (preempting)"
    if "fqdir" in k: return "fqdir_work_fn (preempting)"
    if "cleanup_net" in k: return "cleanup_net (preempting)"
    if "RCU" in k: return "RCU softirq (in-run or preempting)"
    if k.startswith("blocked:") and ("cgroup_css_set_fork" in k or "cgroup_mkdir" in k or "cgroup_kn_lock_live" in k): return "cgroup_mutex wait"
    if k.startswith("runq: preempted") and ("work " in k or "softirq" in k): return "other kworker/softirq preempting"
    if k.startswith("runq:"): return "runq other"
    if k.startswith("run:"): return "run (runtime's own)"
    if k.startswith("softirq") or k.startswith("irq"): return "other irq in run"
    return "other"
for run in sys.argv[2:]:
    with open(run + "/trace.txt", errors="replace") as f:
        tasks, calls, irqs = ct.parse(f)
    cs = sorted([c for c in calls if c[0] == "create"], key=lambda c: c[3] - c[2])
    n = len(cs); spans = [(c[3]-c[2])*1e3 for c in cs]
    bands = {"mid": range(int(.45*n), int(.55*n)), "top1": range(n - n//100, n), "top01": range(n - max(1, n//1000), n)}
    acc = {b: collections.Counter() for b in bands}
    for b, idx in bands.items():
        for i in idx:
            for k, d in ct.walk(tasks, irqs, cs[i][2], cs[i][3], cs[i][4]):
                acc[b][cat(k)] += d * 1e3 / len(idx)
    q = lambda f: spans[min(n-1, int(f*n))]
    print(f"== {run.split('/')[-1]}: n={n} p50 {q(.5):.3f} p99 {q(.99):.3f} p99.9 {q(.999):.3f} max {spans[-1]:.3f}")
    print("| kind | mid | top 1% | top 0.1% | top1-mid | top0.1-mid |")
    for k in sorted(set().union(*acc.values()), key=lambda k: -(acc["top1"][k]-acc["mid"][k])):
        print(f"| {k} | {acc['mid'][k]:.3f} | {acc['top1'][k]:.3f} | {acc['top01'][k]:.3f} | {acc['top1'][k]-acc['mid'][k]:+.3f} | {acc['top01'][k]-acc['mid'][k]:+.3f} |")
