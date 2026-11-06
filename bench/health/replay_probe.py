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
