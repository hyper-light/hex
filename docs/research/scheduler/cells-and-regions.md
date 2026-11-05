# Cells and regions: failure domains, re-placing a lost cluster's work, and differential testing

The sources for implementation goal 7 (CP-T7.12) that the other notes don't hold. G7 has three bars:
- `AC-T7.12.2`: a cell's loss disturbs no workload in another cell.
- `T-T7.12.1`: a lost region's work is re-placed within the others' capacity.
- `AC-T7.12.3`: the differential suite passes on one node, one cell and three regions.

This note covers how Borg and Twine bound a control plane's blast radius, and how Karmada and
Kueue's MultiKueue detect a lost member cluster, re-place its work and check capacity. It also covers
what differential testing across implementations, or configurations, of one specification asks for.
Method and kinds follow [the research method](../README.md#method). Every source was opened and read on
2026-11-04. Source files are pinned to a commit and cited by line; papers by page and section.
`cluster-managers-at-scale.md` already holds Borg's cells, Twine's sharding and Karmada's 100-cluster
report (its [1], [4], [41]); they are cited there by line, not repeated.

## Findings

1. **A cell is a failure domain first and a scale unit second.** Borg: "Each cell is independent of the
   others to minimize the chance of correlated operator errors and failure propagation. These goals, not
   scalability limitations, are the primary argument against larger cells" CR[1] §4 p. 6 (DE). Inside a
   cell, Borg "reduces correlated failures by spreading tasks of a job across failure domains such as
   machines, racks, and power domains" CR[1] §4 p. 6 (DE). Running tasks keep running when every
   Borgmaster replica fails (`cluster-managers-at-scale.md:265-266`).
2. **A regional control plane widens the blast radius, so Twine bounds it with shards, replicas and
   running tasks.** Twine's regional plane risks that "a control plane failure may impact all jobs in a
   region as opposed to just a cluster" CR[2] §4 p. 9 (DE). Its mitigations:
   - "Each shard manages a small fraction of machines and jobs in a region, limiting the impact of a
     shard failure", at 20 shards of 7.5 MW, "no bigger than a traditional cluster".
   - Replicas of a shard sit in different DCs.
   - "Even if all Twine components fail, existing tasks continue to run" CR[2] §4 p. 9 (DE).
3. **Across regions, Twine moved from one global allocator to a federation over regional planes,
   with explicit per-region capacity.** "Machines in a region are largely fungible ... but this is not
   true for machines distributed across regions. Hence, it is better to explicitly decompose a service's
   global capacity needs into capacity needs for specific regions, as opposed to global allocators making
   ad hoc decisions"; global jobs were 8% of jobs; global Twine is being replaced "with a new Federation
   system built atop regional Twine deployments to provide stronger capacity guarantees" CR[2] §7.3
   p. 14 (DE). Twine's earlier critique of federation as a scale-out tool stands beside it: "it adds
   complexity as a scale-out solution" CR[2] §3.3 p. 9 (DE).
4. **Karmada detects a lost member cluster in tens of seconds and evicts only on an explicit taint.**
   - Detection: the cluster controller checks pull-mode clusters every 5 s and sets `Ready` to
     `Unknown` after 40 s unheard (`cluster-monitor-grace-period`) CR[3] `:32,34`. A push-mode cluster
     is unhealthy after 30 s unreachable (`cluster-failure-threshold`) CR[3] `:19-22` (DE).
   - What a failure triggers: an unhealthy cluster gets a `NoSchedule` taint CR[4] `:9-18`. Karmada
     "does not actively add `NoExecute` taints"; users add them CR[4] `:25`. Only an untolerated
     `NoExecute` removes the cluster from scheduling results and reschedules its resources CR[4] `:29`.
   - Off by default: eviction by `NoExecute` and the `Failover` gate (Beta) both are CR[5] `:15-16`,
     CR[6] `:28` (DE).
5. **Karmada re-places within its spread rules and paces evictions; capacity is an estimate.**
   - Rescheduled work still meets `ClusterAffinity` and `SpreadConstraints`, and replicas on healthy
     clusters stay put CR[4] `:31-33`.
   - `Duplicated` work moves only if the new candidates number at least the failed clusters CR[4]
     `:37`. `Divided` work "will try to migrate replicas to the other healthy clusters" CR[4] `:99`
     (DE).
   - Evictions run at `--resource-eviction-rate`, 0.5/s by default (12 workloads take about 24 s)
     CR[6] `:144-148`. Graceful eviction waits for health on the new cluster for up to 10 minutes
     CR[4] `:163,171` (DE).
   - Capacity: `dynamicWeight: AvailableReplicas` divides replicas "by what each cluster can host"
     CR[7] `:650-656`, using `karmada-scheduler-estimator` CR[8] `:5`. Karmada's own docs say the
     estimator reads a stale snapshot and "may over-commit the same cluster's resources across multiple
     consecutive scheduling decisions", which the newer "assume and deduct" overcommit protection closes
     CR[9] `:7-14` (DE).
   - Spread: `spreadConstraints` group clusters by cluster, region, zone or provider, with
     `minGroups` and `maxGroups` CR[7] `:446-461` (DE).
6. **MultiKueue treats a lost worker as a total loss and re-admits against the manager's global
   quota.** The manager reserves quota, then copies the workload to worker clusters (AllAtOnce by
   default, or Incremental in rounds of up to 3, 5 minutes each). The first worker to admit wins, and
   the manager deletes the other copies CR[10] `:56-66,90-116` (DE). If the connection to a worker is
   lost, "the management cluster assumes the total loss of all running/admitted workloads and moves
   them back to non-admitted/queued state". On reconnect, "If there is enough of global quota, the
   unknown admitted workloads would be re-admitted ... If not, some workloads will be preempted ... In
   case of duplicates, all but one of them will be removed" CR[11] `:351-356` (DE). The wait before
   the loss is `WorkerLostTimeout`, 15 minutes by default CR[11] `:423-424`, CR[12] `:51` (DE). The
   capacity check is the manager's quota, which "Ideally ... should equal the total quota available in
   all worker clusters"; if higher, it "may dispatch and monitor workloads that are unlikely to be
   admitted" CR[10] `:33-38` (DE).
7. **Differential testing needs comparable, deterministic implementations, and compares only what the
   specification fixes.** McKeeman: "Differential testing requires that two or more comparable systems
   be available"; "If (we might say when) the results differ or one of the systems loops indefinitely
   or crashes, the tester has a candidate for a bug-exposing test" CR[13] p. 1 (DE). Csmith: "if one has
   multiple, deterministic implementations of the same specification, all implementations must produce
   the same result from the same valid input" CR[14] §2.1 p. 2 (DE). It found "more than 325 previously
   unknown bugs" CR[14] p. 1 (MV). Where the specification leaves a choice open (int widths), Csmith
   compares "within an equivalence class but not across classes" CR[14] §2.6 p. 5 (DE).

## What this means for G7's bars (inference, for the design to decide)

- **Takeover times.** The systems that publish them re-place a lost cluster's work in tens of seconds
  to minutes: Karmada's 40 s detection plus paced eviction, MultiKueue's 15 minutes. Both rest on
  capacity they estimate or assume. Karmada's stale estimate overcommits; MultiKueue's quota may exceed
  the workers'. Neither checks that the lost work fits before moving it. `T-T7.12.1`'s "within the
  others' capacity" asks for that check.
- **A cell can't reach into another.** Borg and Twine bound the blast radius by making a cell or
  shard's control plane unable to act on another's nodes, and by keeping tasks running when the plane
  is gone. `AC-T7.12.2`'s zero is that property, measured.
- **What the differential suite compares.** By Csmith's rule, one node, one cell and three regions
  compare on what the specification fixes: lost or doubled, the per-cell serial specification's log,
  the filters. Not on which node a workload lands, where capacity differs by configuration.

## Gaps

- No source publishes a cross-region re-placement time measured under a stated load. Karmada's
  timings are flag defaults; its 100-cluster report measured cluster selection, not failover
  (`cluster-managers-at-scale.md:238-245`).
- Twine's Federation system over regional planes is described, not measured (CR[2] §7.3).
- No primary source found applies differential testing across deployment scales of one system.
  McKeeman and Csmith compare independent implementations; Hex's suite compares configurations of one
  implementation against one specification, which is the same rule applied to another axis.

## References

[CR1] A. Verma, L. Pedrosa, M. Korupolu, D. Oppenheimer, E. Tune, J. Wilkes (Google), "Large-scale
cluster management at Google with Borg", EuroSys 2015.
https://storage.googleapis.com/gweb-research2023-media/pubtools/pdf/43438.pdf Kind: DE.
- p. 6 §4: "Each cell is independent of the others to minimize the chance of correlated operator errors and failure propagation."
- p. 6 §4: "These goals, not scalability limitations, are the primary argument against larger cells."
- p. 6 §4: "reduces correlated failures by spreading tasks of a job across failure domains such as machines, racks, and power domains"

[CR2] C. Tang et al. (Facebook), "Twine: A Unified Cluster Management System for Shared Infrastructure",
OSDI 2020. https://www.usenix.org/system/files/osdi20-tang.pdf Kind: DE.
- p. 9 §4: "a control plane failure may impact all jobs in a region as opposed to just a cluster"
- p. 9 §4: "Each shard manages a small fraction of machines and jobs in a region, limiting the impact of a shard failure."
- p. 9 §4: "Even if all Twine components fail, existing tasks continue to run."
- p. 9 §3.3: "it adds complexity as a scale-out solution"
- p. 14 §7.3: "it is better to explicitly decompose a service’s global capacity needs into capacity needs for specific regions"
- p. 14 §7.3: "We are replacing global Twine deployments with a new Federation system built atop regional Twine deployments"

Karmada documents [CR3]-[CR9] are files of karmada-io/website at commit
09a91484291f42b53ca695abbce9bbe6b95bb7f1, under
`https://raw.githubusercontent.com/karmada-io/website/09a91484291f42b53ca695abbce9bbe6b95bb7f1/`. Kind: DE.

[CR3] `docs/userguide/failover/cluster-status-maintenance.md`.
- `:19-22`: "cluster is unreachable for a period of configured interval." ... "`--cluster-failure-threshold` flag(default is 30 seconds)"
- `:32`: "every `--cluster-monitor-period` period(default is 5 seconds)"
- `:34`: "has not heard from the cluster in the last `--cluster-monitor-grace-period`(default is 40 seconds)"

[CR4] `docs/userguide/failover/failover-analysis.md`.
- `:9`: "a `taint{effect: NoSchedule}` will be added to the cluster"
- `:25`: "Karmada controller does not actively add `NoExecute` taints to cluster objects."
- `:29`: "the Karmada controller will remove the cluster from the scheduling results ... Afterward, the Karmada scheduler will reschedule all affected resources."
- `:31-33`: "it still needs to meet the restrictions of `PropagationPolicy/ClusterPropagationPolicy`, such as `ClusterAffinity` or `SpreadConstraints`" ... "will remain during failover rescheduling."
- `:37`: "only proceed if the number of candidate clusters ... is greater than or equal to the number of failed clusters"
- `:99`: "Karmada scheduler will try to migrate replicas to the other healthy clusters."
- `:163`: "the removal of evicted workloads will be delayed until the workloads are available on new clusters."
- `:171`: "`graceful-eviction-timeout` flag(default is 10 minutes)"

[CR5] `docs/userguide/failover/cluster-taint-management.md`.
- `:15-16`: "the eviction feature of NoExecute is disabled by default" ... "explicitly enable the `Failover` feature gate."

[CR6] `docs/userguide/failover/cluster-failover.md`.
- `:28`: "`Failover` feature gate is currently in the `Beta` stage and is turned off by default"
- `:144-148`: "`--resource-eviction-rate`: Number of evictions per second. Default: **0.5**." ... "completes in about 24 seconds."

[CR7] `docs/userguide/scheduling/propagation-policy.md`.
- `:446-461`: `spreadConstraints` "based on topology domains (cluster, region, zone, or provider)"; `SpreadByField`, `MaxGroups`, `MinGroups`.
- `:650-656`: "weights are calculated based on how many replicas each cluster can accommodate"

[CR8] `docs/userguide/scheduling/scheduler-estimator.md`.
- `:5`: "When some clusters lack resources, the scheduler will avoid assigning excessive replicas to them by using the Karmada-scheduler-estimator."

[CR9] `docs/userguide/scheduling/scheduling-overcommit-protection.md`.
- `:7-12`: "the estimator operates on a stale snapshot and may over-commit the same cluster's resources across multiple consecutive scheduling decisions."
- `:14`: "an "assume and deduct" mechanism between the scheduler and the estimator."

Kueue documents [CR10]-[CR12] are files of kubernetes-sigs/kueue at commit
a6a134fe1c86f59cf350f1083cd15be4552fe8c3, under
`https://raw.githubusercontent.com/kubernetes-sigs/kueue/a6a134fe1c86f59cf350f1083cd15be4552fe8c3/`. Kind: DE.

[CR10] `site/content/en/docs/concepts/multikueue.md`.
- `:33-38`: "Ideally, the quota in the manager cluster should equal the total quota available in all worker clusters" ... "may dispatch and monitor workloads that are unlikely to be admitted in the worker clusters."
- `:58-66`: "When a worker cluster admits one of these remote Workloads: The manager deletes the Workloads from the other clusters."
- `:90-91`: "the Workload is copied to all available worker clusters as soon as it obtains a QuotaReservation"
- `:100-102`: "If none of the nominated clusters admit the Workload within a fixed duration (5 minutes), the next batch of clusters ... is incrementally added"
- `:116`: "The default maximum batch size is 3."

[CR11] `keps/693-multikueue/README.md`.
- `:351-356`: "If the connection between management cluster and worker cluster is lost, the management cluster assumes the total loss of all running/admitted workloads and moves them back to non-admitted/queued state." ... "In case of duplicates, all but one of them will be removed."
- `:423-424`: "`WorkerLostTimeout` - defines the time a local workload's multikueue admission check state is kept Ready if the connection with its reserving worker cluster is lost."

[CR12] `apis/config/v1beta2/defaults.go`.
- `:51`: `DefaultMultiKueueWorkerLostTimeout = 15 * time.Minute`

[CR13] W. M. McKeeman (DIGITAL), "Differential Testing for Software", Digital Technical Journal 10(1),
1998, pp. 100-107. Copy read: https://web.archive.org/web/2003id_/http://www.hpl.hp.com/hpjournal/dtj/vol10num1/vol10num1art9.pdf
(the original hpl.hp.com URL timed out). Kind: DE.
- p. 1: "Differential testing requires that two or more comparable systems be available to the tester."
- p. 1: "If (we might say when) the results differ or one of the systems loops indefinitely or crashes, the tester has a candidate for a bug-exposing test."

[CR14] X. Yang, Y. Chen, E. Eide, J. Regehr (University of Utah), "Finding and Understanding Bugs in C
Compilers", PLDI 2011. Copy read: https://users.cs.utah.edu/~regehr/papers/pldi11-preprint.pdf
Kinds: DE; MV for the bug count.
- p. 1: "more than 325 previously unknown bugs" (MV)
- p. 2 §2.1: "if one has multiple, deterministic implementations of the same specification, all implementations must produce the same result from the same valid input."
- p. 5 §2.6: "we can perform differential testing within an equivalence class but not across classes"
