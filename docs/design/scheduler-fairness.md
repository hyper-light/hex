# Priority, fair share, gangs and preemption (G4)

Scheduler implementation goal 4 (`scheduler-architecture.md` S5 and its "Implementation goals" 4; CP-T7.5)
as built in hex-sim's own control plane and serial specification, so that a laptop, a fleet and the
simulator run one order (CLAUDE.md §8). Sources are `docs/research/scheduler/placement-algorithms-and-fairness.md`
(cited `PA[n]`) and `cluster-managers-at-scale.md` (`CM[n]`), and the primary documents below, each
read for this record (kiln research `01a1a9977c9b777c9f84cbc522d6db5c`):

- [DRF] Ghodsi et al., "Dominant Resource Fairness: Fair Allocation of Multiple Resource Types", NSDI 2011,
  https://www.usenix.org/legacy/event/nsdi11/tech/full_papers/Ghodsi.pdf (PA[18]).
- [Borg] Verma et al., "Large-scale cluster management at Google with Borg", EuroSys 2015,
  https://storage.googleapis.com/gweb-research2023-media/pubtools/pdf/43438.pdf (CM[1]).
- [KueuePre] Kueue, Preemption, https://raw.githubusercontent.com/kubernetes-sigs/kueue/main/site/content/en/docs/concepts/preemption/_index.md (PA[20d]).
- [KueueAoN] Kueue, All-or-nothing scheduling, https://raw.githubusercontent.com/kubernetes-sigs/kueue/main/site/content/en/docs/concepts/all_or_nothing.md.
- [K8sPrio] Kubernetes, Pod Priority and Preemption, https://raw.githubusercontent.com/kubernetes/website/main/content/en/docs/concepts/scheduling-eviction/pod-priority-preemption.md.
- [K8sPDB] Kubernetes, Disruptions, https://raw.githubusercontent.com/kubernetes/website/main/content/en/docs/concepts/workloads/pods/disruptions.md.
- [VolcanoDRF] Volcano, DRF design record, https://raw.githubusercontent.com/volcano-sh/volcano/master/docs/design/drf.md (PA[21f]).

## The model

A workload carries a `Tenancy` (`crates/hex-sim/src/model.rs`): its owner (a repository or a person),
its priority band (0-255, higher first), an optional gang and an optional disruption group. A cluster
names its owners' weights and its groups' budgets. The recorded-workload format takes them as
append-only fields (`replay.rs`): a trace written before G4 reads as it did, one owner of weight 1.

## Decisions

### F1. Queues by band and owner, taken in weighted-DRF order

**The choice.** Pending work waits in one queue per (band, owner), each in submit order. The committer
(`sim.rs`) and the specification (`spec.rs`) both rank queues by band, highest first, then by the
owner's dominant share of the cluster (the largest of its placed CPU, memory and GPU fractions)
divided by its weight, lowest first, then by owner id (`fair.rs`, `Shares::rank`). Shares are whole
numbers in units of 2^-40, so the control plane and its oracle can't disagree by rounding. `Policy::Fifo`
(one queue, submit order) stays as the baseline G0-G2 were measured with.

**Sources.** DRF "pick[s] user i with lowest dominant share" and launches its next task ([DRF] §4,
Algorithm 1); weighted DRF divides each share by the user's weight ([DRF] §4.3); Volcano's DRF plugin
orders jobs the same way ("The job having the lowest share will have higher priority", [VolcanoDRF] line
17), and Kueue's fair sharing is "a variation of DRF" (PA[20g]). Borg's bands are non-overlapping and
taken highest first ([Borg] §2.5).

**Evidence.** `AC-T7.5.2`: `tests/fair.rs` `four_equal_weight_repositories_each_hold_a_quarter_under_contention`,
on a generated four-repository day (64 nodes of 16 cores and 64 GiB; 70,318 legs; the busy hours ask
60%, 50%, 45% and 45% of the cluster). Measured 2026-11-05 on jupiter (Windows x86_64, release, hex-sim
deterministic, so the numbers are the same on any machine; the Mac Studio gave the same for the earlier
day): under bands-then-DRF, 16.48 contended hours, parts of the held share 24.98%, 25.05%, 25.01%, 24.96%,
worst 479 parts per million from a quarter against a bar of 5,000 (2% of 25%); under submit order on the
same day, 38.4%, 22.3%, 27.0%, 12.3%, worst 133,982 ppm, so the test can fail. An earlier day where two
repositories asked for 30% read worst 16,563 ppm under DRF: DRF gives each owner here about 28% of the
cluster (three CPU-bound owners and a memory-bound one share the CPU), and arrivals at 30% often fell
below it, so those owners were not backlogged while all four had some leg waiting; an owner short of
work holds less than its share under any policy. At 45% and above every owner stays backlogged, and the
same day with all four at 60% read 668 ppm. The recorded kiln day replaces the generated one when the
CI-history recording lands (`replay.rs` reads owners and bands already).
`T-T7.4.1` under the new order: `placements_equal_the_serial_specification_with_owners_and_bands` (500
generated clusters, 1-5 owners of weights 1-4, 1-3 bands; the log of every run is one the specification
makes) and the crash sweep over a three-owner, two-band cluster (`AC-T7.3.3`).

**What would falsify it.** A run whose shares under contention miss 25% by more than 2% of it on a
recorded day; or a model-based run the specification refuses.

### F2. Allocation stops at the first queue whose work doesn't fit

**The choice.** Within a queue, work that holds on no node now is passed over (backfill, as before G4).
Across queues it is not: when none of the first-ranked queue's work holds on any node now, the
committer places nothing from any queue after it until capacity comes back or the ranks change.

**Sources.** DRF's own loop ends there: if the lowest share's next task doesn't fit, "the cluster is
full" and allocation returns ([DRF] Algorithm 1). Kueue's `StrictFIFO` queueing is the same choice within
one queue ("an older workload that does not fit blocks newer ones", PA[20b]).

**Evidence.** Measured, hex-sim fedora unit `hex-sched-g4-quick-20261105T030339Z` on the day of
`tests/fair.rs`: DRF order *with* backfill across queues gave the repository of one-core legs 53.1% of the
held share and the four-core and up-to-eight-core repositories 10.4% and 10.1% (worst 28.1 points from a
quarter), worse than submit order (42.2%, 24.0%, 19.7%, 14.1%; worst 17.2 points): small legs of the
largest share took every core that freed while the larger legs of the smallest shares fitted nowhere.

**What would falsify it.** Strict order costing more goodput than the fairness bar is worth on a recorded
day (idle capacity while the first queue waits); then the falsifier is a reservation for the first
queue's head (Borg-style) with backfill elsewhere, measured against the same bar. GPU work is the case to
watch: a GPU leg at the lowest share holds CPU-only work back too.

### F3. A gang is placed in one store write, all members or none

**The choice.** A gang (a matrix job's legs) is submitted member by member but placed whole: the
committer decides it when its first member's turn comes in its queue and every member is pending,
choosing each member's node in order on the view, each taking its room before the next is sought
(`sim::fit_whole`), and commits every member in one store write (`Store::apply_gang`), which applies
all of them or none and is one write to the nemesis. A gang the empty cluster can't hold whole is
refused at admission, with a typed refusal, not queued forever. A gang's last member arriving sends
back a commit in flight in its own queue, since the gang may now hold ahead of it. The specification
accepts a gang's placements only as one write, whole, at its first member's place in its queue.

**Sources.** Kueue admits a workload's pods all or nothing ([KueueAoN]): a job that needs all its
members and holds only some wastes what it holds, and two half-placed jobs can wait on each other
for good.

**Evidence.** `AC-T7.5.1`: `check_gangs` counts every write after which a gang has members placed
and members not. Zero over: the crash sweep with gangs (`tests/sim.rs`
`a_crash_at_every_control_plane_write_loses_and_doubles_nothing`, a crash before and after every
write, gang writes included); 500 generated clusters with gangs of 2 to 8 among singles
(`gangs_are_placed_whole_on_generated_clusters`, every log checked by the specification); and the
contended day of F1 with a tenth of its legs in gangs of 2 to 12 (`tests/fair.rs`
`matrix_gangs_on_a_contended_day_are_placed_whole`: 4,393 gangs placed, 1,214 of them needing more
than one node and waiting over a minute for capacity, partial 0). The mutant committing members one by
one is refused by the specification and counted (`the_oracle_refuses_a_gang_placed_member_by_member`).
Measured 2026-11-05 on jupiter (Windows x86_64, release).

**What would falsify it.** A write after which a gang is part placed, in any run; or gangs waiting
behind singles so long that a recorded day's matrix jobs start later than their legs would one by
one, which would call for a reservation for the gang at the head of its queue.

### F4. Preemption within disruption budgets

**The choice.** With preemption on (`Config::preemptions`, a budget of workloads preempted per run;
0 is off, past it nothing more is preempted and the refusals are counted), when the committer reaches
work in the first queue that holds on no node, it looks for victims (`fair::choose_victims`, one pure
function the committer and the specification both call): placed workloads in a strictly lower band,
or in the same band under another owner whose share, less what it would lose, stays above the
preemptor's owner's share with the preemptor placed. Never a gang's member. Never one whose
disruption group would then have more members preempted and not placed again than its budget.
Candidates are taken borrowing first, then lowest band, then newest, node by node until the node
admits the preemptor; the node needing fewest victims wins, then the lowest id. The victims go back
to pending (at their old place in their queues) and the preemptor is placed, in one store write
(`Store::apply_preempt`), which itself refuses a write past any group's budget (`Conflict::Budget`),
so a budget holds even against a wrong choice. Each placement starts a workload once: a preempted
workload placed again starts again, and `check_end` counts a double start as more starts than
placements. The node agent stops a preempted run and drops any report of it still to come.

**Sources.** Kueue's preemption ([KueuePre]): candidates "from ClusterQueues borrowing first", then
"lowest priority", then "most recently admitted"; under fair sharing a workload preempts another only
if its ClusterQueue's share after admission stays below the target's share after preemption, which
is the same-band rule here. Kubernetes ([K8sPrio]) picks the node whose victims are fewest and of
lowest priority, and ([K8sPDB]) counts a budget as pods disrupted at once. Borg's production tier
preempts only lower priorities ([Borg] §2.5).

**Evidence.** `AC-T7.5.3`: `tests/preempt.rs` `preemption_never_breaks_a_budget`, a proptest of 256
generated clusters (1 to 8 nodes, 8 to 90 workloads; the first half in band 0 never ending, the
rest in bands 1 and 2; up to 3 owners; most workloads in up to 4 groups with budgets 1 to 3) with
the crash nemesis at random writes: every run's log is replayed by the specification, which refuses a
preemption past a budget (`Violation::BudgetBroken`), one it wouldn't make, and a higher band left
waiting while it could preempt within the budgets (`Violation::Inversion`); none occurs, and nothing is
lost or started twice. Non-vacuity, over 300 such clusters without crashes: 825 workloads preempted
in 589 preemptions, candidates passed over for a budget 22,383 times, none refused for the run's
budget (`preemptions_happen_and_budgets_bind`). A crash before and after every write of a contested
cluster with preemptions (`a_crash_at_every_write_with_preemption_loses_and_doubles_nothing`). The
mutant ignoring budgets is refused (`the_oracle_refuses_a_preemption_past_a_budget`), as is an inversion
(`the_oracle_refuses_a_higher_band_left_waiting`). The proptest found one bug: a placement that ended
a disruption left its queue's cursor past work that could then preempt
(`a_disruption_ended_by_a_placement_reopens_its_queue`). Measured 2026-11-05 on jupiter (Windows x86_64,
release).

**What would falsify it.** Any run whose log the specification refuses for a budget or an inversion; or,
on a recorded day, preemption churn (work preempted and placed again) costing more goodput than the
bands gain, which would call for a minimum run time before a workload can be preempted.
