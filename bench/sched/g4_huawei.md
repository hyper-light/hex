# G4's bars on a recorded day of workloads (Huawei Cloud's serverless sandboxes)

G4 (CP-T7.5) is fair share, priority, gangs and preemption between the owners of Hex's workloads: OCI
containers, Firecracker microVMs and Shards microVMs (Ada, 2026-11-06: "You are NOT Kiln's load balancer
for jobs, CI, and general work. Your scheduler is for *microvms*." and "Your goals are still wrong and list
containers when we work with OCI containers, Firecracker, and Shards Microvms."). This records its two
day-scale bars, AC-T7.5.2 and T-T7.5.1, on the recorded workload G2 committed: Huawei Cloud's public
serverless trace 2025, region 5, day 30 (Joosen et al., "Serverless Cold Starts and Where to Find Them",
EuroSys 2025, CC BY 4.0), as `huawei-r5-day30.trace`. Each sandbox stands for a workload by its launch,
lifetime and shape, and each user is an owner; how the trace was made is in `huawei_workload.md`.

Measured 2026-11-06 with `crates/hex-sim/tests/huawei.rs`, release builds on fedora and
adas-mac-studio-3 (hex-sim is deterministic: the numbers are the same on any machine, and none is a
timing). Hex's policy is decision F5 of `docs/design/scheduler-fairness.md`: bands,
weighted DRF (F1) and preemption of over-share work in a band (F4).

## AC-T7.5.2: four equal-weight owners within 2% of 25% under contention

**How the day and cluster were chosen.** Day 30 is the only day of the trace with sandbox lifetimes.
No owner asks for a quarter of the trace's 10 m5d.metal hosts (480 cores): the busiest asks for 6.4
cores on average over the day. So the bar's four owners are the day's four busiest by dominant share
of those hosts (userID indexes 16, 8, 11 and 24: 2,323, 1,453, 675 and 567 sandboxes), and the
cluster is the largest at which each of the four asks, on average over the day, for more than a
quarter of it, counting only the workloads that cluster can hold (`tests/common/mod.rs`,
`quarter_cluster`). That is one node of 2,777 millicores, set by owner 24's 0.694 mean cores, with the
hosts' memory. On work it can run, the four ask for 2.51, 2.52, 1.59 and 0.69 cores. 756 of their 5,018
sandboxes are larger than it and are refused at admission.

| policy | contended | parts of the held share (16, 8, 11, 24) | worst from 25% | bar (5,000 ppm) |
|---|---|---|---:|---|
| Hex: bands, weighted DRF, preemption | 59.0 h, 6 stretches (longest 58.9 h) | 71.2%, 21.0%, 5.6%, 2.3% | 461,564 ppm | missed |
| plain DRF | the same, to the nanosecond | the same | 461,564 ppm | missed |
| submit order (FIFO) | 29.5 h, 2 stretches | 3.7%, 75.4%, 0.0%, 20.9% | 503,860 ppm | missed |

**Reading.** Hex's policy preempts nothing here (0 preemptions, against 1,373,026 passes over work that
holds nowhere). On a 2.777-core node, owner 11's workloads of 1.8 and 2.6 cores and owner 16's of 1.8
are 65% and 94% of the cluster. F4 takes a workload from another owner in the band only if that owner
stays above the preemptor's share once it's gone, Kueue's fair-sharing rule, which keeps two owners
from preempting each other back and forth. An owner holding one such workload would fall to nothing, so
it is never a victim, and the owners of 0.3-core workloads wait for up to 8.9 days. DRF's equal shares
assume tasks small beside the cluster (Ghodsi et al., NSDI 2011, §4). The bar's precondition, each
of four owners asking for more than a quarter, sizes this trace's cluster below its own grain.

## T-T7.5.1: a burst of 1,000 workloads leaves the other owners' start p95 as it was

**How.** Day 30 as recorded, all 339 owners, on the trace's 10 hosts; then the same day with 1,000
workloads from an owner that launched none that day, all at the start of the day's busiest hour (hour 2
from its first launch, 1,232 launches), each taking the shape and lifetime of one of the day's
sandboxes drawn uniformly (`generate::burst_from`, seed 7).

| policy | owners slower with the burst | workloads preempted | holds |
|---|---:|---:|---|
| Hex: bands, weighted DRF, preemption | 0 of 339 | 0 | yes |
| plain DRF | 0 of 339 | 0 | yes |
| FIFO | 0 of 339 | 0 | yes |

**Reading.** Hex's policy holds, but so does every policy: the ten 48-core hosts take a burst of mostly
0.3-core, short-lived sandboxes without any owner waiting, so this day can't tell the policies apart.
The generated burst of `tests/burst.rs` does (decision F5).

## Where the bars stand

Of record on generated days: AC-T7.5.2 holds under DRF at 479 ppm from 25% (FIFO 133,982), and T-T7.5.1
holds under Hex's policy with the others' p95 unchanged (plain DRF and FIFO miss). On the recorded
workload day, AC-T7.5.2's precondition can be met only on a cluster smaller than the trace's own
workloads allow DRF to work in, and T-T7.5.1's burst is too small for the trace's hosts to notice.
