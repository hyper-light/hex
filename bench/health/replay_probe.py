"""Replay a recorded healthy-disk probe trace (`<ms> <latency ns>` lines) through candidate disk-probe
timeout rules, as hex-health's DiskProbe judges: RFC 6298's SRTT/RTTVAR (alpha 1/8, beta 1/4, K 4,
G given), Karn (a probe past its timeout is no sample), Suspect past one timeout, Drain past
DRAIN_TIMEOUTS timeouts or after SLOW_STREAK completed-but-slow probes in a row. Counts false
suspects and false drains on the healthy trace, and the time a stall at the trace's end takes to be
suspected and drained."""
import sys

def rto(srtt, rttvar, g):
    return srtt + max(g, 4 * rttvar)

def run(trace, rule, g=1_000_000, drain_timeouts=3, slow_streak=3):
    srtt = None; rttvar = 0; learned = None; streak = 0
    suspects = drains = 0
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
    for _, lat in trace:
        t = timeout()
        drained = lat > drain_timeouts * t
        if lat > t:
            suspects += 1
            streak += 1
            if streak >= slow_streak:
                drained = True
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
    return suspects, drains, t, drain_timeouts * t

trace = [tuple(map(int, l.split())) for l in open(sys.argv[1]) if l.strip()]
print(f"trace {sys.argv[1]}: {len(trace)} probes, max {max(l for _, l in trace)/1e6:.1f} ms")
for rule in ["rfc-clamped", "rfc-bare", "learned-max"]:
    s, d, t, dt = run(trace, rule)
    print(f"{rule:12s} false suspects {s:6d}  false drains {d:4d}  a stall at the end: suspected at {t/1e6:8.1f} ms, drained at {dt/1e6:8.1f} ms")
