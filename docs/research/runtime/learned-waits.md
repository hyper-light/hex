# Learned waits: RFC 6298's timer for the node's waits, and the limits beside them

Source notes for decision 9 of `docs/design/runtime-architecture.md` (Ada's rule of 2026-11-06: no picked
constants; values learned at runtime, derived from a named record, or cited limits).

## RFC 6298, "Computing TCP's Retransmission Timer" (Paxson, Allman, Chu, Sargent; June 2011)

https://www.rfc-editor.org/rfc/rfc6298, §2-§5. The rules `hex_core::patience` follows, as the RFC numbers them:

- (2.1) Before any RTT measurement, RTO is set to 1 second, the back-off of (5.5) still applying.
- (2.2) On the first measurement R: SRTT <- R, RTTVAR <- R/2, RTO <- SRTT + max(G, K*RTTVAR) with K = 4.
- (2.3) On each later R': RTTVAR <- (1 - beta)*RTTVAR + beta*|SRTT - R'|, then SRTT <- (1 - alpha)*SRTT + alpha*R',
  with alpha = 1/8 and beta = 1/4 (after Jacobson and Karels); RTTVAR is updated with the SRTT before.
- (2.4) An RTO under 1 second SHOULD be rounded up to 1 second.
- (2.5) A maximum value MAY be placed on RTO provided it is at least 60 seconds.
- §3, Karn's algorithm: RTT samples MUST NOT be taken from retransmitted segments, whose answer is ambiguous. Hex's
  analogue: no sample from a create whose seed died or was taken for hung, nor from a wait that ran out.
- (5.5) When the timer expires, RTO <- RTO*2 ("back off the timer"), bounded by the (2.5) maximum; §5's note after
  (5.7): a new measurement recomputes RTO per §2, which may collapse it after back-off.

What carries over and what doesn't: TCP times a peer across a network; Hex times its own kernel and its own child
processes. The estimator's form (an EWMA of the mean and of the mean deviation, the wait four deviations past the
mean) is about any answer time with a shifting mean and spread; the 1 s floor is the RFC's guard against a wait
that ends before a slow but healthy answer, the same asymmetry Hex has (a refused create or killed seed costs more
than a late detection of a hung one).

## Allman and Paxson, "On Estimating End-to-End Network Path Properties", SIGCOMM '99

§3 evaluates retransmission-timer variants on traces and finds the minimum RTO the dominant factor in avoiding
spurious timeouts; the 1 s minimum RFC 6298 (2.4) adopts comes from it.

## Linux: pid_max

Documentation/admin-guide/sysctl/kernel.rst, "pid_max": the PID allocation wrap value; no task holds a pid at or
past it, so the threads a box's cgroups list are at most `pid_max` (read at runtime from
`/proc/sys/kernel/pid_max`). proc(5): `/proc/self/fd` holds one entry per open descriptor of the process.
getrlimit(2): `RLIMIT_NOFILE`, one more than the largest descriptor number a process may open.

## systemd: DefaultLimitNOFILE=

systemd-system.conf(5), "DefaultLimitNOFILE=": the default descriptor limit for services, 1024 soft and 524288 hard
since systemd v240 (the soft limit kept at 1024 for select(2)'s sake). The netns tests' "systemd's default" is
that soft half.

## Balls in bins: Dubhashi and Ranjan, "Balls and Bins: A Study in Negative Dependence"

Random Structures & Algorithms 13(2), 1998. The indicators that each bin is empty after independent throws are
negatively associated, so Chernoff-Hoeffding bounds apply to their sum. The store's shard test uses it: 256 ids
over 64 shards miss mu = 64·(63/64)^256 ≈ 1.14 shards on average, and P(misses ≥ 14) ≤ 2^-32 for a uniform hash.
