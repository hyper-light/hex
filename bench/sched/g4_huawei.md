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

**How.** Day 30 as recorded, all 339 owners; then the same day with 1,000 workloads from an owner that
launched none that day, all at the start of the day's busiest hour (hour 2 from its first launch,
1,232 launches), each taking the shape and lifetime of one of the day's sandboxes drawn uniformly
(`generate::burst_from`, seed 7). On the trace's 10 hosts no policy lets the burst slow anyone, so the
host count is set by a rule fixed before Hex's numbers were read: from 10 hosts down to 1, the largest
count at which submit order (FIFO) lets the burst slow another owner's start p95.

| hosts | FIFO: owners slower with the burst | most slower by |
|---:|---:|---:|
| 10 | 0 of 339 | 0 |
| 9 | 0 of 339 | 0 |
| 8 | 1 of 339 | 93.7 ms |

The rule picks 8 hosts (384 cores). There:

| policy, 8 hosts | owners slower with the burst | most slower by | workloads preempted | holds |
|---|---:|---:|---:|---|
| Hex: bands, weighted DRF, preemption | 0 of 339 | 0 | 3 | yes |
| plain DRF | 1 of 339 | 3.0 ms | 0 | no |
| FIFO | 1 of 339 | 93.7 ms | 0 | no |

**Reading.** At the largest host count where the burst is felt at all, Hex's policy keeps every other
owner's start p95 where it was, by preempting 3 of the burst's workloads; plain DRF, which orders but
doesn't preempt, lets one owner start 3 ms later at p95, and FIFO 94 ms. Measured 2026-11-06 on fedora
(unit `hex-sched-g4-test-20261106T124405Z`).

## Where the bars stand

AC-T7.5.2 holds on a recorded day: day 1 of Azure's Packing Trace 2020 (below), at 166 ppm from 25%.
T-T7.5.1 holds on the recorded Huawei day at the host count its rule picks (above). On generated days,
AC-T7.5.2 holds under DRF at 479 ppm from 25% (FIFO 133,982), and T-T7.5.1 holds under Hex's policy
with the others' p95 unchanged (plain DRF and FIFO miss). On Huawei's R5 day 30, AC-T7.5.2's
precondition can be met only on a cluster smaller than the trace's own workloads allow DRF to work in.

## Searching for a recorded day where the bar's precondition holds

AC-T7.5.2's precondition is four owners each asking for more than a quarter of the cluster; for DRF's
equal shares to mean anything, the cluster should be whole m5d.metal hosts (48 cores, 384 GiB), each
sandbox small beside one. So a qualifying day needs four owners each asking, on average over the day,
for more than a quarter of at least one host: 12 cores or 96 GiB. `bench/sched/huawei_owners.py` reads a
region's cold starts and request logs (lifetimes as `huawei_trace.py` takes them) and prints the
busiest owners and the largest such cluster.

What the release has (its README, https://github.com/sir-lab/data-release, `README_data_release_2025.md`
and `README_data_release_2023.md`, read 2026-11-06):
- request logs, which give lifetimes, exist only for day 30, in all five regions ("event-level logs of
  individual requests for day 30 of our dataset"); cold starts cover all 31 days;
- region 1's request logs keep "the top 100 pods per function", and region 3's are "a subsample", which
  bias lifetimes;
- the 2023 traces are per-function time series ("Percentage of allocated CPU used per function
  averaged over all pods", "Number of pods allocated to that function") with no row per sandbox and no
  owner, so no day of them can carry the bar.

| region, day 30 | owners | busiest four: mean ask as a share of one host | largest qualifying cluster | qualifies |
|---|---:|---|---:|---|
| R5 | 339 | 13.3%, 5.3%, 3.4%, 1.4% | 0 hosts | no |
| R4 | 1,663 | 128.6%, 14.7%, 14.5%, 4.8% | 0 hosts | no |
| R2 | | not read: Google Drive refused the downloads ("Quota exceeded") after 7 of 10 request files | | pending |
| R3 | | not read: Drive quota (request logs subsampled) | | pending |
| R1 | | not read: Drive quota (request logs keep the top 100 pods per function) | | pending |

R5's numbers match `huawei-r5-day30.trace`'s, so the scan reads the release as the trace does. Largest
sandbox: 6.6 cores in R5 (13.8% of a host), 3.4 in R4 (7.1%). Measured 2026-11-06 on fedora (unit
`hex-sched-g4-huawei-20261106T113124Z`), with these files' sha256s: R5 requests
`5825f744…6e60d`; R4 requests `bab840de…ff2bd` (part 2; part 1's is in the run's log).

## Azure's public VM traces

Azure Public Dataset (https://github.com/Azure/AzurePublicDataset), licensed CC BY 4.0
(https://creativecommons.org/licenses/by/4.0/; the repository's `LICENSE`, read 2026-11-06), cited as
it asks: the Packing Trace 2020 as Hadary et al., "Protean: VM Allocation Service at Scale", OSDI 2020;
the VM trace 2019 (V2) as Cortez et al., "Resource Central", SOSP 2017. Each has a row per VM with its
owner, start, end and size. `bench/sched/azure_owners.py` scans every day of each; an owner's ask is the
larger of its summed CPU and its summed memory over the day, as a share of hosts (its dominant share, as
DRF counts it). Downloaded and read on fedora under `~/.cache` (packing zip sha256 `af967499…5fe16`).

| trace | days | owners per day | busiest four (hosts asked, typical day) | largest qualifying cluster | qualifies |
|---|---:|---:|---|---:|---|
| Packing 2020 (one zone; host = machine type 16, which runs the most VM types) | 14 | 251,890-306,794 | 144.5, 138.2, 116.3, 116.3 | 465 hosts, every day | yes |
| VM trace 2019 (one region; m5d.metal hosts; sizes at their buckets' lower bounds, so asks are understated) | 30 | 5,710-5,843 | 188.0, 146.5, 94-118, 93-110 | 373-441 hosts, every day | yes, by ask; not converted |

In both, the busiest owners hold VMs alive the whole trace, so a qualifying day is mostly a fill of
long-lived VMs: Packing 2020's four owners below start or end 0 to 22 VMs a day. VM trace 2019's largest
request is 24 cores (half a host); whether its busiest owners' requests are each at most a quarter of a
host, as the converter requires, is not yet checked.

**The day of record.** `bench/sched/azure_trace.py` converts Packing 2020 by rules fixed before any
replay (its docstring): the four tenants asking most over the 14 days whose every request is at most a
quarter of a host (tenants 459906, 1073289, 88764, 1581670); the day with most of their starts and ends
(day 1, 44); the largest whole number of hosts at which each asks for more than a quarter over that day
(431: they ask 144.5, 116.3, 116.3 and 107.8 hosts); each owner's requests one kind by G2's rule (owner
0 OCI containers, owners 1 and 2 Shards microVMs, owner 3 Firecracker microVMs), with each kind's
overhead. The result, `azure-packing2020-day1.trace` (sha256 `d7c8c809…aba9b`, 4,280 requests), is
measured over the day by `crates/hex-sim/tests/azure.rs`:

| policy | contended | parts of the held share (owners 0-3) | worst from 25% | bar (5,000 ppm) |
|---|---|---|---:|---|
| Hex: bands, weighted DRF, preemption | 24.0 h, one stretch | 24.99%, 25.01%, 25.01%, 24.98% | 166 ppm | met |
| plain DRF | the same | the same | 166 ppm | met |
| submit order (FIFO) | 0.000 h | 0%, 0%, 100%, 0% | 750,000 ppm | missed |

**Reading.** Hex's policy meets the bar on a recorded day, but by DRF's order alone: nothing is preempted,
and plain DRF's numbers are the same. Under submit order the four owners are all waiting together for
only an instant, as the VMs alive at the day's start arrive together and fill the cluster in VM order
before some owners have anything left waiting; so FIFO misses the bar with almost nothing measured.
Measured 2026-11-06 on fedora (unit `hex-sched-g4-test-20261106T124405Z`).
