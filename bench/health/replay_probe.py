"""Replay a recorded healthy-disk probe trace (`<ms> <latency ns>` lines) through candidate disk-probe
timeout rules, as hex-health's DiskProbe judges: RFC 6298's SRTT/RTTVAR (alpha 1/8, beta 1/4, K 4,
G given), Karn (a probe past its timeout is no sample), Suspect past one timeout, Drain past
DRAIN_TIMEOUTS timeouts or after SLOW_STREAK completed-but-slow probes in a row. Counts false
suspects and false drains on the healthy trace, and the time a stall at the trace's end takes to be
suspected and drained."""
import sys

def rto(srtt, rttvar, g):
    return srtt + max(g, 4 * rttvar)

def run(trace, rule, g=1_000_000, drain_timeouts=3, slow_streak=3, calibrate=False):
    srtt = None; rttvar = 0; learned = None; streak = 0
    suspects = drains = 0
    causes = []
    if calibrate:
        # The calibration write: its latency is the first sample and the floor's start, unjudged.
        _, first = trace[0]
        trace = trace[1:]
        srtt, rttvar, learned = first, first // 2, first
    def timeout():
        base = 1_000_000_000 if srtt is None else rto(srtt, rttvar, g)
        if rule == "rfc-clamped":
            return min(max(base, 1_000_000_000), 60_000_000_000)
        if rule == "rfc-bare":
            return base
        if rule == "learned-max":   # longest completed probe without a drain, prior 1 s before any
            if learned is None:
                return 1_000_000_000
            return max(base, learned)
        raise SystemExit(rule)
    for i, (_, lat) in enumerate(trace):
        t = timeout()
        drained = lat > drain_timeouts * t
        if drained:
            causes.append(("age", i, lat, t))
        if lat > t:
            suspects += 1
            streak += 1
            if streak >= slow_streak and not drained:
                drained = True
                causes.append(("streak", i, lat, t))
        else:
            streak = 0
            if srtt is None:
                srtt, rttvar = lat, lat // 2
            else:
                rttvar = (3 * rttvar + abs(srtt - lat)) // 4
                srtt = (7 * srtt + lat) // 8
        if drained:
            drains += 1
            streak = 0
        elif rule == "learned-max":
            learned = lat if learned is None else max(learned, lat)
    t = timeout()
    return suspects, drains, t, drain_timeouts * t, causes

def recorded_2x(trace, seed_ns):
    """Hex's rule for learned waits: slowest = max(record's, node's), only rising, Karn (an answer past
    the bound is none); Suspect past the slowest, Drain past twice it."""
    slowest = seed_ns
    suspects = drains = 0
    for _, lat in trace:
        bound = 2 * slowest
        if lat > bound:
            drains += 1
            continue
        if lat > slowest:
            suspects += 1
            slowest = lat
    return suspects, drains, slowest, 2 * slowest

trace = [tuple(map(int, l.split())) for l in open(sys.argv[1]) if l.strip()]
print(f"trace {sys.argv[1]}: {len(trace)} probes, max {max(l for _, l in trace)/1e6:.1f} ms")
if len(sys.argv) > 2:
    seed = int(sys.argv[2])
    s, d, sl, b = recorded_2x(trace, seed)
    print(f"recorded-2x (seed {seed/1e6:.1f} ms) false suspects {s:6d}  false drains {d:4d}  a stall at the end: suspected at {sl/1e6:8.1f} ms, drained at {b/1e6:8.1f} ms")
for rule, cal in [("rfc-clamped", False), ("rfc-bare", False), ("learned-max", False), ("learned-max", True)]:
    s, d, t, dt, causes = run(trace, rule, calibrate=cal)
    name = rule + (" +cal" if cal else "")
    print(f"{name:17s} false suspects {s:6d}  false drains {d:4d}  a stall at the end: suspected at {t/1e6:8.1f} ms, drained at {dt/1e6:8.1f} ms")
    for c in causes[:5]:
        print(f"    drain by {c[0]} at probe {c[1]}: latency {c[2]/1e6:.1f} ms, timeout {c[3]/1e6:.1f} ms")


def cost_rule(trace, record, replace_s):
    """Ada's cost-based drain (2026-11-06), every input learned or measured. The drain age T minimizes
    the expected loss per probe, (1 - pi) * S(T) * C + pi * L * T: a healthy write outliving T costs a
    false drain C (the work moved times its measured re-placement time); a stall waited on for T costs
    the work blocked, L per second. S is the share of healthy answers slower than T, over the record's
    answers and then this node's (a Jeffreys half-count past the slowest, so never zero); pi is the
    Jeffreys posterior of a stall per probe, (k + 1/2)/(n + 1). With C/L the re-placement time per
    workload, `replace_s`, the loss is (1 - pi) * S(T) * replace_s + pi * T (seconds of work)."""
    import bisect
    healthy = sorted(record)
    probes = stalls = 0
    false_drains = 0
    def drain_age():
        n = len(healthy)
        pi = (stalls + 0.5) / (probes + 1)
        best_t, best = None, None
        # S(T) at each candidate T = a healthy answer: answers strictly slower, plus the half-count.
        for i in range(n):
            t = healthy[i]
            s = (n - 1 - i + 0.5) / (n + 1)
            loss = (1 - pi) * s * replace_s + pi * (t / 1e9)
            if best is None or loss < best:
                best, best_t = loss, t
        return best_t
    t_drain = drain_age()
    next_full = 1
    for k, (_, lat) in enumerate(trace):
        # The drain age is re-decided at every probe that reaches it (exact where it matters), and
        # otherwise whenever the answers seen have grown by half again (it moves only with them).
        if lat > t_drain:
            t_drain = drain_age()
        if lat > t_drain:
            false_drains += 1
        else:
            bisect.insort(healthy, lat)
        probes += 1
        if probes >= next_full:
            t_drain = drain_age()
            next_full = probes + probes // 2 + 1
    return false_drains, drain_age()


if len(sys.argv) > 3:
    record = [int(l.split()[1]) for l in open(sys.argv[3]) if l.strip()]
    for replace_s in (0.002, 0.194):
        fd, t = cost_rule(trace, record, replace_s)
        print(f"cost rule (re-placement {replace_s*1e3:.0f} ms/workload, record {len(record)} answers): false drains {fd:4d}  a stall at the end drained at {t/1e6:8.1f} ms")
