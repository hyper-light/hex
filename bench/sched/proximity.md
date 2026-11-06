# Proximity-scored placement against proximity-blind on a modelled WAN (S4, A2; G3)

**Question:** does scoring placement by hyper-swim's Vivaldi coordinates improve goodput or the
completion-time tail over placement that ignores proximity (scheduler-architecture.md S4's falsifier),
for the work Hex places: OCI containers, Firecracker microVMs and Shards microVMs?
**Result: yes, on every one of 120 studies** (2 fleets × 5 seeds × 2 home layouts × 2 loads × 3
compute steps), over all workloads and for each kind alone. Over all workloads, goodput is 0.9% to 18.5%
higher and the p99 completion time 7.0% to 93.9% lower. For each kind alone, the least gain is goodput
+1.0% and p99 −4.4% for OCI containers, +0.7% and −7.4% for Firecracker microVMs, +0.8% and −4.4% for
Shards microVMs. The gain shrinks as the compute between round trips grows against the RTT. It is
recorded, and proximity stays a soft score (S4); it is not dropped to a tie-breaker.

**Shards' overhead is a stand-in.** Each kind's runtime overhead is the recorded workload's, and in it
Shards' is 5 MiB, the same as Firecracker's, because no source states Shards' own yet. When Shards' is
measured, the trace changes, and this run is repeated at that commit.

## What was measured

`crates/hex-bench/benches/sched_proximity.rs` over `hex_sim::proximity` (`docs/design/hex-sim.md` H20, H21):
- 1,000 nodes of 16 cores and 64 GiB over Azure's eight regions (`hex_sim::wan`: Azure's published P50
  inter-region round trips, zones under 2 ms, access heights 25-250 µs, probes stretched up to 10%).
  Every node learns hyper-swim's coordinates: 8 probes a round, 64 rounds.
- The two fleets of H10: **AllKinds**, every node Linux x86_64 with KVM running all three kinds (the
  main one); **Mixed**, Hex's own fleet's mix drawn per node, six in ten Linux (all three kinds), two
  x86_64 Windows (OCI containers and Shards microVMs), two arm64 macOS (Shards microVMs alone). A
  workload is placed only on a node that runs its kind.
- 20,000 workloads each, of Hex's three kinds by G2's rule. Each workload's owner is drawn by that
  owner's share of the launches in G2's recorded workload (`bench/sched/huawei-r5-day30.trace`: Huawei
  Cloud, Region 5, day 30; 339 owners, 25,295 launches). The study's jobs carry no owners of their own,
  so they take the recorded day's. Each owner's workloads are one kind, and owners go to kinds so each
  kind has a third of the study's launches (`proximity::classes_by_owner`, `huawei_trace.py:31-35`).
  The day's largest owner made 30% of its launches, so a study's kinds split near thirds, not exactly.
- Each workload asks 1-4 cores and 4 GiB a core, plus its kind's overhead from the trace: OCI 440,402
  B (0.42 MiB), Firecracker 5,242,880 B (5 MiB), Shards 5,242,880 B (**a stand-in**).
- A workload talks to a home node and makes `round_trips` sequential round trips to it, each after
  `step` of compute. Round trips are log-normal through TraceLab's p50 25 and p99 1,438 tool calls a
  session. Its run time is `round_trips × (step + true RTT)`.
- Homes: `Uniform` (region drawn uniformly) or `Skewed` (half in East US, the rest uniform).
- Load: arrivals offer 50% or 90% of the cluster's CPU against blind placement's expected occupancy,
  each workload's mean RTT taken over the nodes that run its kind.
- Policies, over the fast path's own sample (`FitIndex::sampled_fit`, which admits only nodes that run
  the workload's kind): **blind** (sampled best fit), **proximity** (least predicted RTT read as an
  upper bound, `rtt × (1 + 2·mean error)`; best fit breaks ties), **tie-break** (best fit, proximity
  only among nodes whose free capacity ties), **oracle** (least true RTT).
- Goodput: workloads finished by the horizon (the last arrival), per second. Completion time: arrival to
  end, every workload run to its end. Both overall and for each kind.

Virtual time: the numbers are a function of the seed and the code, not of the host's load.

## The run of record

- Host: fedora, `kiln-a094a12d.attlocal.net`, Intel Core i9-14900HX, Linux 6.19.10-300.fc44; release
  (`cargo bench` profile).
- Commit: `9af44c0` (cp/t7-sched-g3, the merge of cp/t7-sched `f9472ac`). Date: 2026-11-06, 14:09Z.
  Unit: `hex-sched-g3-bench-20261106T134428Z` (CPUs 16-31, exit 0; hex-sim's proximity tests first,
  9 of 9 passed). Load average at the start 24.2 (noisy; it doesn't change virtual-time results).
  Co-tenants: `hexg5rec-kids-hex-g5rec-g16sb-20261106T132322Z`, `goofy_keldysh`,
  `issue556oss-control-plane`, three kiln buildkit containers, `kind-registry`, `kiln-t10-minio`.
- Coordinates: median relative RTT error 9.0-9.7% over 20,000 pairs (Vivaldi's own evaluation: 11%,
  PF[1]).
- The full log is on fedora at `~/.cache/hex-t7/sched-g3/out/hex-sched-g3-bench-20261106T134428Z.log`.

## Proximity against blind, per seed (goodput gain %, p99 completion cut %), all workloads then each kind

```
AllKinds step   10 ms load 500‰ Uniform all        : (+10.8, +93.2) (+11.1, +93.6) (+11.5, +92.9) (+11.7, +93.6) (+11.8, +93.4)
AllKinds step   10 ms load 500‰ Uniform Oci        : (+11.3, +93.3) (+11.1, +93.2) (+11.6, +93.2) (+11.9, +93.6) (+11.5, +93.7)
AllKinds step   10 ms load 500‰ Uniform Firecracker: (+10.8, +93.4) (+11.1, +93.6) (+11.8, +93.1) (+11.4, +93.0) (+11.8, +93.5)
AllKinds step   10 ms load 500‰ Uniform Shards     : (+10.3, +93.1) (+11.0, +93.8) (+11.0, +93.2) (+11.9, +93.3) (+12.1, +92.9)
AllKinds step   10 ms load 500‰ Skewed all        : (+10.4, +92.4) (+10.8, +92.7) (+10.8, +91.7) (+10.9, +92.5) (+10.2, +92.4)
AllKinds step   10 ms load 500‰ Skewed Oci        : (+10.5, +92.6) (+10.5, +91.8) (+10.7, +92.5) (+11.0, +92.1) (+9.9, +92.3)
AllKinds step   10 ms load 500‰ Skewed Firecracker: (+10.5, +92.5) (+11.0, +93.0) (+10.8, +90.8) (+10.5, +92.8) (+10.2, +92.5)
AllKinds step   10 ms load 500‰ Skewed Shards     : (+10.3, +92.4) (+10.8, +92.7) (+10.9, +91.6) (+11.3, +92.7) (+10.6, +92.4)
AllKinds step   10 ms load 900‰ Uniform all        : (+17.8, +93.5) (+18.3, +93.9) (+17.8, +93.0) (+18.5, +93.7) (+18.5, +93.7)
AllKinds step   10 ms load 900‰ Uniform Oci        : (+18.0, +94.1) (+18.0, +93.7) (+17.5, +93.4) (+18.8, +93.3) (+17.7, +94.1)
AllKinds step   10 ms load 900‰ Uniform Firecracker: (+18.8, +93.0) (+18.6, +94.2) (+17.7, +93.6) (+18.2, +93.9) (+18.8, +93.8)
AllKinds step   10 ms load 900‰ Uniform Shards     : (+16.7, +93.9) (+18.5, +93.7) (+18.1, +92.9) (+18.4, +93.4) (+18.9, +93.5)
AllKinds step   10 ms load 900‰ Skewed all        : (+16.5, +92.2) (+17.1, +92.8) (+16.5, +92.1) (+17.0, +92.3) (+16.1, +92.4)
AllKinds step   10 ms load 900‰ Skewed Oci        : (+17.7, +91.7) (+17.3, +91.7) (+16.6, +92.4) (+16.8, +92.6) (+16.0, +91.9)
AllKinds step   10 ms load 900‰ Skewed Firecracker: (+16.7, +92.2) (+17.2, +92.5) (+16.9, +91.8) (+16.1, +92.3) (+15.9, +92.0)
AllKinds step   10 ms load 900‰ Skewed Shards     : (+15.2, +92.0) (+16.7, +93.6) (+15.9, +91.7) (+18.2, +91.9) (+16.3, +93.0)
AllKinds step  100 ms load 500‰ Uniform all        : (+6.4, +61.9) (+6.7, +59.5) (+6.4, +59.4) (+6.9, +59.8) (+6.8, +61.0)
AllKinds step  100 ms load 500‰ Uniform Oci        : (+6.7, +62.9) (+6.5, +58.7) (+6.5, +61.2) (+7.2, +60.4) (+6.5, +60.8)
AllKinds step  100 ms load 500‰ Uniform Firecracker: (+6.2, +62.3) (+6.7, +61.5) (+6.5, +60.3) (+6.5, +59.3) (+7.1, +59.8)
AllKinds step  100 ms load 500‰ Uniform Shards     : (+6.4, +60.3) (+6.9, +59.5) (+6.3, +56.1) (+7.1, +59.8) (+6.7, +61.7)
AllKinds step  100 ms load 500‰ Skewed all        : (+5.9, +54.2) (+5.9, +53.5) (+6.0, +51.9) (+5.8, +53.7) (+6.0, +55.7)
AllKinds step  100 ms load 500‰ Skewed Oci        : (+6.2, +54.8) (+6.0, +51.0) (+6.0, +53.8) (+5.6, +53.8) (+6.3, +52.4)
AllKinds step  100 ms load 500‰ Skewed Firecracker: (+6.1, +52.1) (+5.6, +54.4) (+6.1, +48.4) (+5.1, +48.8) (+5.4, +55.6)
AllKinds step  100 ms load 500‰ Skewed Shards     : (+5.4, +54.5) (+6.0, +57.9) (+5.7, +51.2) (+6.8, +56.3) (+6.2, +60.0)
AllKinds step  100 ms load 900‰ Uniform all        : (+10.1, +59.6) (+10.7, +61.1) (+10.5, +59.3) (+10.4, +58.2) (+10.3, +59.6)
AllKinds step  100 ms load 900‰ Uniform Oci        : (+9.8, +63.7) (+10.4, +60.8) (+10.1, +59.3) (+10.9, +57.1) (+10.2, +61.6)
AllKinds step  100 ms load 900‰ Uniform Firecracker: (+10.7, +57.1) (+10.7, +61.7) (+10.5, +58.1) (+10.5, +59.4) (+10.5, +57.0)
AllKinds step  100 ms load 900‰ Uniform Shards     : (+9.7, +61.2) (+11.0, +61.0) (+10.9, +59.6) (+9.8, +57.6) (+10.1, +60.9)
AllKinds step  100 ms load 900‰ Skewed all        : (+8.5, +49.9) (+8.2, +49.8) (+8.3, +51.4) (+8.6, +49.7) (+8.4, +52.9)
AllKinds step  100 ms load 900‰ Skewed Oci        : (+8.9, +48.7) (+8.2, +50.8) (+7.9, +54.6) (+8.5, +49.6) (+8.0, +55.8)
AllKinds step  100 ms load 900‰ Skewed Firecracker: (+8.7, +50.3) (+8.0, +49.1) (+8.3, +48.5) (+7.9, +45.2) (+8.0, +52.9)
AllKinds step  100 ms load 900‰ Skewed Shards     : (+7.9, +51.6) (+8.4, +53.1) (+8.7, +52.1) (+9.4, +51.3) (+9.3, +52.6)
AllKinds step 1000 ms load 500‰ Uniform all        : (+1.2, +12.3) (+1.4, +12.4) (+1.4, +13.3) (+1.2, +13.6) (+1.4, +11.5)
AllKinds step 1000 ms load 500‰ Uniform Oci        : (+1.3, +15.5) (+1.6, +9.8) (+1.3, +11.0) (+1.5, +13.2) (+1.4, +10.2)
AllKinds step 1000 ms load 500‰ Uniform Firecracker: (+1.3, +12.4) (+1.2, +9.4) (+1.4, +12.1) (+1.1, +14.2) (+1.4, +9.6)
AllKinds step 1000 ms load 500‰ Uniform Shards     : (+1.1, +12.9) (+1.5, +13.3) (+1.5, +12.7) (+1.0, +14.1) (+1.3, +12.2)
AllKinds step 1000 ms load 500‰ Skewed all        : (+1.0, +8.9) (+1.1, +13.4) (+1.1, +8.1) (+1.2, +8.7) (+1.1, +11.0)
AllKinds step 1000 ms load 500‰ Skewed Oci        : (+1.2, +11.3) (+1.1, +9.5) (+1.1, +10.4) (+1.2, +11.4) (+1.2, +9.8)
AllKinds step 1000 ms load 500‰ Skewed Firecracker: (+1.1, +10.2) (+0.9, +8.7) (+1.3, +8.4) (+1.2, +8.4) (+0.9, +7.4)
AllKinds step 1000 ms load 500‰ Skewed Shards     : (+0.8, +4.4) (+1.2, +10.8) (+0.8, +9.2) (+1.3, +9.7) (+1.1, +15.8)
AllKinds step 1000 ms load 900‰ Uniform all        : (+2.1, +13.8) (+2.0, +13.1) (+2.1, +11.7) (+1.9, +12.8) (+1.9, +11.5)
AllKinds step 1000 ms load 900‰ Uniform Oci        : (+1.9, +10.0) (+1.7, +14.5) (+2.1, +11.8) (+1.6, +14.6) (+1.8, +12.4)
AllKinds step 1000 ms load 900‰ Uniform Firecracker: (+2.2, +12.8) (+2.0, +13.0) (+2.3, +8.8) (+2.2, +15.4) (+2.0, +11.5)
AllKinds step 1000 ms load 900‰ Uniform Shards     : (+2.1, +13.2) (+2.2, +10.4) (+2.0, +12.6) (+1.9, +14.4) (+2.0, +9.6)
AllKinds step 1000 ms load 900‰ Skewed all        : (+1.5, +10.8) (+1.6, +9.7) (+1.5, +9.4) (+1.6, +7.0) (+1.6, +11.8)
AllKinds step 1000 ms load 900‰ Skewed Oci        : (+1.2, +11.1) (+1.7, +9.4) (+1.4, +7.6) (+1.5, +4.7) (+1.5, +9.8)
AllKinds step 1000 ms load 900‰ Skewed Firecracker: (+1.6, +11.9) (+1.8, +10.0) (+1.6, +9.1) (+1.4, +10.2) (+1.5, +8.5)
AllKinds step 1000 ms load 900‰ Skewed Shards     : (+1.7, +11.1) (+1.4, +10.7) (+1.6, +10.3) (+1.8, +5.5) (+1.7, +13.3)
Mixed step   10 ms load 500‰ Uniform all        : (+10.9, +93.4) (+11.0, +93.3) (+11.3, +93.2) (+11.3, +93.7) (+11.3, +93.6)
Mixed step   10 ms load 500‰ Uniform Oci        : (+10.8, +93.7) (+11.0, +93.2) (+11.1, +93.1) (+11.2, +93.6) (+11.5, +93.2)
Mixed step   10 ms load 500‰ Uniform Firecracker: (+10.7, +92.6) (+10.9, +93.6) (+11.6, +93.3) (+11.4, +93.8) (+11.5, +93.9)
Mixed step   10 ms load 500‰ Uniform Shards     : (+11.2, +93.6) (+11.1, +93.5) (+11.2, +92.6) (+11.1, +93.8) (+11.0, +93.6)
Mixed step   10 ms load 500‰ Skewed all        : (+10.4, +92.2) (+10.1, +93.1) (+10.7, +92.0) (+10.8, +92.7) (+10.7, +92.2)
Mixed step   10 ms load 500‰ Skewed Oci        : (+10.6, +92.9) (+10.7, +92.8) (+10.5, +92.6) (+10.9, +92.4) (+10.4, +92.5)
Mixed step   10 ms load 500‰ Skewed Firecracker: (+10.6, +92.2) (+10.3, +93.0) (+11.1, +91.0) (+10.5, +92.9) (+10.4, +91.8)
Mixed step   10 ms load 500‰ Skewed Shards     : (+10.1, +92.0) (+9.3, +93.1) (+10.5, +91.7) (+10.9, +93.1) (+11.3, +92.5)
Mixed step   10 ms load 900‰ Uniform all        : (+17.3, +93.3) (+18.3, +93.4) (+17.5, +93.6) (+17.2, +93.3) (+18.2, +93.1)
Mixed step   10 ms load 900‰ Uniform Oci        : (+17.5, +92.7) (+17.8, +93.1) (+17.2, +93.9) (+17.6, +93.7) (+17.7, +92.7)
Mixed step   10 ms load 900‰ Uniform Firecracker: (+17.1, +93.8) (+18.3, +93.8) (+18.4, +93.2) (+16.8, +93.0) (+18.0, +93.3)
Mixed step   10 ms load 900‰ Uniform Shards     : (+17.3, +93.6) (+18.7, +93.5) (+17.0, +93.7) (+17.1, +93.8) (+19.0, +93.1)
Mixed step   10 ms load 900‰ Skewed all        : (+16.4, +92.7) (+16.7, +92.5) (+16.7, +92.0) (+17.4, +92.4) (+16.3, +92.2)
Mixed step   10 ms load 900‰ Skewed Oci        : (+17.2, +92.6) (+16.3, +91.6) (+16.4, +92.2) (+17.5, +93.0) (+16.1, +92.3)
Mixed step   10 ms load 900‰ Skewed Firecracker: (+16.0, +92.3) (+17.0, +92.1) (+17.6, +91.7) (+16.2, +92.0) (+15.4, +91.8)
Mixed step   10 ms load 900‰ Skewed Shards     : (+16.1, +93.0) (+16.8, +93.3) (+16.0, +91.6) (+18.4, +92.3) (+17.5, +91.9)
Mixed step  100 ms load 500‰ Uniform all        : (+6.2, +59.3) (+6.6, +60.9) (+7.0, +58.6) (+6.5, +58.7) (+6.7, +61.6)
Mixed step  100 ms load 500‰ Uniform Oci        : (+6.2, +58.8) (+6.9, +61.8) (+6.9, +61.4) (+6.3, +60.3) (+6.7, +61.6)
Mixed step  100 ms load 500‰ Uniform Firecracker: (+6.2, +60.3) (+6.5, +62.8) (+7.1, +60.1) (+6.6, +58.7) (+6.8, +59.5)
Mixed step  100 ms load 500‰ Uniform Shards     : (+6.3, +56.6) (+6.5, +61.9) (+6.9, +56.3) (+6.6, +57.8) (+6.7, +61.7)
Mixed step  100 ms load 500‰ Skewed all        : (+5.9, +55.5) (+5.9, +56.2) (+5.6, +52.7) (+6.0, +51.5) (+5.4, +51.9)
Mixed step  100 ms load 500‰ Skewed Oci        : (+6.0, +59.2) (+6.2, +54.0) (+6.1, +56.2) (+6.2, +52.3) (+5.7, +53.6)
Mixed step  100 ms load 500‰ Skewed Firecracker: (+6.0, +54.3) (+5.6, +56.2) (+5.2, +51.7) (+5.7, +49.5) (+4.8, +51.0)
Mixed step  100 ms load 500‰ Skewed Shards     : (+5.7, +52.1) (+5.8, +56.4) (+5.5, +50.7) (+6.2, +51.2) (+5.8, +54.2)
Mixed step  100 ms load 900‰ Uniform all        : (+10.3, +59.2) (+10.4, +60.1) (+10.2, +59.8) (+9.8, +58.7) (+10.6, +60.6)
Mixed step  100 ms load 900‰ Uniform Oci        : (+9.9, +58.8) (+9.7, +58.3) (+10.9, +63.4) (+9.7, +60.6) (+10.4, +61.3)
Mixed step  100 ms load 900‰ Uniform Firecracker: (+10.5, +61.4) (+10.1, +62.5) (+10.3, +59.6) (+9.9, +58.6) (+10.8, +59.7)
Mixed step  100 ms load 900‰ Uniform Shards     : (+10.4, +59.5) (+11.3, +58.7) (+9.4, +55.6) (+9.8, +60.0) (+10.7, +60.4)
Mixed step  100 ms load 900‰ Skewed all        : (+8.3, +50.8) (+8.4, +50.8) (+8.1, +47.2) (+8.2, +49.6) (+7.9, +51.1)
Mixed step  100 ms load 900‰ Skewed Oci        : (+8.6, +48.8) (+8.1, +49.4) (+8.3, +49.3) (+8.3, +48.2) (+8.2, +52.6)
Mixed step  100 ms load 900‰ Skewed Firecracker: (+7.8, +51.1) (+7.6, +47.6) (+7.8, +44.5) (+7.4, +49.2) (+7.0, +47.8)
Mixed step  100 ms load 900‰ Skewed Shards     : (+8.6, +50.7) (+9.4, +55.2) (+8.3, +46.2) (+8.8, +49.4) (+8.5, +51.6)
Mixed step 1000 ms load 500‰ Uniform all        : (+1.3, +12.7) (+1.3, +12.9) (+1.5, +13.9) (+1.2, +10.8) (+1.4, +12.5)
Mixed step 1000 ms load 500‰ Uniform Oci        : (+1.4, +12.4) (+1.3, +12.2) (+1.4, +12.9) (+1.2, +16.1) (+1.5, +13.8)
Mixed step 1000 ms load 500‰ Uniform Firecracker: (+1.2, +12.3) (+1.1, +10.5) (+1.5, +11.5) (+1.1, +12.7) (+1.5, +10.4)
Mixed step 1000 ms load 500‰ Uniform Shards     : (+1.2, +13.2) (+1.4, +14.1) (+1.4, +11.0) (+1.3, +16.7) (+1.3, +14.3)
Mixed step 1000 ms load 500‰ Skewed all        : (+1.0, +9.7) (+1.0, +13.7) (+1.0, +8.8) (+1.1, +9.0) (+0.9, +10.9)
Mixed step 1000 ms load 500‰ Skewed Oci        : (+1.1, +7.9) (+1.1, +9.1) (+1.0, +11.7) (+1.1, +8.1) (+1.1, +7.5)
Mixed step 1000 ms load 500‰ Skewed Firecracker: (+1.1, +13.2) (+0.8, +15.4) (+1.1, +10.2) (+1.0, +10.7) (+0.7, +10.6)
Mixed step 1000 ms load 500‰ Skewed Shards     : (+0.8, +10.9) (+1.2, +12.9) (+1.0, +10.7) (+1.2, +10.0) (+1.1, +14.2)
Mixed step 1000 ms load 900‰ Uniform all        : (+1.9, +14.2) (+2.3, +12.4) (+1.9, +14.0) (+1.9, +12.3) (+2.1, +13.2)
Mixed step 1000 ms load 900‰ Uniform Oci        : (+1.9, +14.6) (+2.3, +12.9) (+1.9, +12.9) (+1.7, +18.5) (+1.9, +12.7)
Mixed step 1000 ms load 900‰ Uniform Firecracker: (+1.8, +14.2) (+1.9, +12.0) (+1.9, +14.7) (+2.0, +11.2) (+2.1, +10.7)
Mixed step 1000 ms load 900‰ Uniform Shards     : (+1.9, +15.0) (+2.7, +14.8) (+1.9, +13.5) (+2.0, +12.6) (+2.2, +15.0)
Mixed step 1000 ms load 900‰ Skewed all        : (+1.5, +7.8) (+1.5, +14.3) (+1.5, +10.3) (+1.4, +8.0) (+1.5, +13.5)
Mixed step 1000 ms load 900‰ Skewed Oci        : (+1.3, +9.5) (+1.5, +8.3) (+1.3, +11.0) (+1.4, +4.4) (+1.4, +11.1)
Mixed step 1000 ms load 900‰ Skewed Firecracker: (+1.5, +7.5) (+1.6, +14.3) (+1.6, +9.3) (+1.2, +9.7) (+1.3, +9.2)
Mixed step 1000 ms load 900‰ Skewed Shards     : (+1.8, +6.7) (+1.3, +7.6) (+1.7, +10.7) (+1.6, +11.0) (+2.0, +14.1)
LEAST all        : goodput +0.9%, p99 cut +7.0%
LEAST Oci        : goodput +1.0%, p99 cut +4.4%
LEAST Firecracker: goodput +0.7%, p99 cut +7.4%
LEAST Shards     : goodput +0.8%, p99 cut +4.4%
```

## Every policy, per seed, all workloads (goodput /s, p99 completion s; mean true RTT to home, ms)

| fleet | step ms | load ‰ | homes | seed | coord err | blind | proximity | tie-break | oracle | RTT b/p/t/o |
|---|---|---|---|---|---|---|---|---|---|---|
| AllKinds | 10 | 500 | Uniform | 1 | 9.1% | 162.3, 216.4 | 179.8, 14.7 | 163.5, 206.0 | 180.0, 13.7 | 143.5/0.8/130.0/0.2 |
| AllKinds | 10 | 500 | Uniform | 2 | 9.7% | 163.6, 230.4 | 181.7, 14.9 | 165.5, 220.2 | 181.8, 14.3 | 141.6/0.7/128.6/0.2 |
| AllKinds | 10 | 500 | Uniform | 3 | 9.5% | 163.0, 229.0 | 181.7, 16.2 | 165.5, 213.3 | 181.8, 14.8 | 143.4/0.9/125.3/0.2 |
| AllKinds | 10 | 500 | Uniform | 4 | 9.3% | 170.3, 233.9 | 190.2, 14.9 | 172.0, 207.4 | 190.4, 14.3 | 143.5/0.7/130.1/0.2 |
| AllKinds | 10 | 500 | Uniform | 5 | 9.0% | 168.5, 237.4 | 188.4, 15.6 | 170.1, 236.6 | 188.6, 14.4 | 144.0/0.8/128.9/0.2 |
| AllKinds | 10 | 500 | Skewed | 1 | 9.1% | 181.5, 194.9 | 200.4, 14.8 | 187.4, 143.7 | 200.5, 14.1 | 119.8/0.7/78.0/0.2 |
| AllKinds | 10 | 500 | Skewed | 2 | 9.7% | 187.7, 183.0 | 207.9, 13.4 | 193.9, 146.7 | 208.0, 13.1 | 121.1/0.7/80.8/0.2 |
| AllKinds | 10 | 500 | Skewed | 3 | 9.5% | 184.5, 192.1 | 204.4, 15.9 | 190.4, 148.3 | 204.5, 14.8 | 119.4/0.9/76.9/0.2 |
| AllKinds | 10 | 500 | Skewed | 4 | 9.3% | 191.8, 195.1 | 212.8, 14.7 | 198.5, 138.0 | 212.9, 14.2 | 119.8/0.8/80.3/0.2 |
| AllKinds | 10 | 500 | Skewed | 5 | 9.0% | 178.9, 202.5 | 197.2, 15.3 | 184.1, 155.3 | 197.3, 14.3 | 118.9/0.8/81.7/0.2 |
| AllKinds | 10 | 900 | Uniform | 1 | 9.1% | 272.5, 224.7 | 321.0, 14.7 | 276.4, 225.1 | 321.3, 13.7 | 144.9/0.8/131.0/0.2 |
| AllKinds | 10 | 900 | Uniform | 2 | 9.7% | 274.1, 243.7 | 324.4, 14.9 | 278.3, 218.2 | 324.7, 14.3 | 144.1/0.7/130.4/0.2 |
| AllKinds | 10 | 900 | Uniform | 3 | 9.5% | 275.7, 231.4 | 324.7, 16.2 | 278.4, 215.5 | 325.1, 14.8 | 143.1/0.9/129.1/0.2 |
| AllKinds | 10 | 900 | Uniform | 4 | 9.3% | 286.2, 236.1 | 339.0, 14.9 | 290.5, 206.5 | 339.6, 14.3 | 144.0/0.7/128.4/0.2 |
| AllKinds | 10 | 900 | Uniform | 5 | 9.0% | 284.0, 245.1 | 336.4, 15.6 | 289.3, 214.9 | 336.7, 14.4 | 143.2/0.8/128.8/0.2 |
| AllKinds | 10 | 900 | Skewed | 1 | 9.1% | 306.6, 188.8 | 357.2, 14.8 | 319.5, 142.7 | 357.6, 14.1 | 120.0/0.8/84.5/0.2 |
| AllKinds | 10 | 900 | Skewed | 2 | 9.7% | 317.0, 186.5 | 371.2, 13.5 | 330.1, 146.3 | 371.5, 13.0 | 120.5/0.7/84.8/0.2 |
| AllKinds | 10 | 900 | Skewed | 3 | 9.5% | 312.8, 201.1 | 364.3, 15.9 | 326.1, 156.4 | 364.9, 14.8 | 119.2/0.9/82.1/0.2 |
| AllKinds | 10 | 900 | Skewed | 4 | 9.3% | 324.1, 190.2 | 379.3, 14.7 | 337.9, 157.2 | 379.7, 14.2 | 118.8/0.8/84.4/0.2 |
| AllKinds | 10 | 900 | Skewed | 5 | 9.0% | 302.8, 200.6 | 351.5, 15.3 | 314.4, 153.2 | 351.9, 14.3 | 119.2/0.8/82.8/0.2 |
| AllKinds | 100 | 500 | Uniform | 1 | 9.1% | 102.2, 357.2 | 108.8, 135.9 | 102.9, 331.1 | 108.8, 134.5 | 144.7/0.8/130.1/0.2 |
| AllKinds | 100 | 500 | Uniform | 2 | 9.7% | 103.3, 346.3 | 110.2, 140.1 | 103.9, 349.9 | 110.2, 139.4 | 143.7/0.7/129.3/0.2 |
| AllKinds | 100 | 500 | Uniform | 3 | 9.5% | 103.5, 359.8 | 110.2, 146.1 | 104.4, 339.9 | 110.2, 145.5 | 142.8/0.9/126.6/0.2 |
| AllKinds | 100 | 500 | Uniform | 4 | 9.3% | 107.3, 349.7 | 114.7, 140.7 | 108.3, 318.1 | 114.8, 140.4 | 143.3/0.7/127.8/0.2 |
| AllKinds | 100 | 500 | Uniform | 5 | 9.0% | 106.7, 366.2 | 113.9, 142.8 | 107.7, 323.7 | 114.0, 142.1 | 145.1/0.8/125.8/0.2 |
| AllKinds | 100 | 500 | Skewed | 1 | 9.1% | 109.6, 321.7 | 116.1, 147.2 | 111.5, 265.9 | 116.1, 146.7 | 124.5/6.3/86.3/5.3 |
| AllKinds | 100 | 500 | Skewed | 2 | 9.7% | 113.6, 300.9 | 120.3, 139.8 | 115.5, 262.7 | 120.4, 134.8 | 126.3/5.6/88.0/5.4 |
| AllKinds | 100 | 500 | Skewed | 3 | 9.5% | 111.2, 321.2 | 117.8, 154.7 | 113.3, 277.4 | 117.8, 153.0 | 126.0/6.7/88.7/6.1 |
| AllKinds | 100 | 500 | Skewed | 4 | 9.3% | 115.8, 324.4 | 122.5, 150.1 | 117.5, 278.3 | 122.6, 149.1 | 125.6/7.3/91.5/6.6 |
| AllKinds | 100 | 500 | Skewed | 5 | 9.0% | 108.0, 337.3 | 114.4, 149.3 | 110.1, 268.8 | 114.4, 148.3 | 125.1/6.4/86.2/5.3 |
| AllKinds | 100 | 900 | Uniform | 1 | 9.1% | 170.9, 336.0 | 188.1, 135.9 | 172.9, 319.9 | 188.2, 134.5 | 144.9/0.8/129.4/0.2 |
| AllKinds | 100 | 900 | Uniform | 2 | 9.7% | 172.5, 360.8 | 190.9, 140.3 | 174.2, 330.0 | 191.0, 139.4 | 144.7/0.7/131.6/0.2 |
| AllKinds | 100 | 900 | Uniform | 3 | 9.5% | 172.2, 359.0 | 190.3, 146.1 | 174.0, 334.5 | 190.4, 145.5 | 144.7/0.9/129.9/0.2 |
| AllKinds | 100 | 900 | Uniform | 4 | 9.3% | 179.3, 336.8 | 197.9, 140.7 | 181.8, 313.5 | 198.0, 140.4 | 145.1/0.7/129.6/0.2 |
| AllKinds | 100 | 900 | Uniform | 5 | 9.0% | 178.7, 354.6 | 197.1, 143.1 | 180.4, 344.0 | 197.2, 142.1 | 145.1/0.8/131.4/0.2 |
| AllKinds | 100 | 900 | Skewed | 1 | 9.1% | 183.1, 312.1 | 198.7, 156.3 | 186.6, 281.4 | 198.8, 156.9 | 125.6/12.2/93.5/12.1 |
| AllKinds | 100 | 900 | Skewed | 2 | 9.7% | 191.1, 303.7 | 206.8, 152.6 | 194.3, 277.0 | 206.8, 148.7 | 126.2/11.4/94.6/11.3 |
| AllKinds | 100 | 900 | Skewed | 3 | 9.5% | 186.2, 341.7 | 201.7, 165.9 | 189.9, 287.2 | 201.6, 165.6 | 125.3/13.2/92.3/13.0 |
| AllKinds | 100 | 900 | Skewed | 4 | 9.3% | 192.9, 317.8 | 209.4, 160.0 | 197.2, 282.2 | 209.6, 160.0 | 125.5/12.9/93.9/12.5 |
| AllKinds | 100 | 900 | Skewed | 5 | 9.0% | 180.4, 334.6 | 195.6, 157.5 | 184.9, 284.6 | 196.0, 157.9 | 124.9/13.1/92.3/12.5 |
| AllKinds | 1000 | 500 | Uniform | 1 | 9.1% | 21.9, 1531.6 | 22.1, 1343.6 | 21.9, 1503.3 | 22.1, 1342.3 | 146.3/0.8/131.0/0.2 |
| AllKinds | 1000 | 500 | Uniform | 2 | 9.7% | 22.2, 1588.0 | 22.5, 1391.6 | 22.2, 1563.4 | 22.5, 1390.4 | 145.0/0.7/130.7/0.2 |
| AllKinds | 1000 | 500 | Uniform | 3 | 9.5% | 22.1, 1677.4 | 22.4, 1454.8 | 22.2, 1634.6 | 22.4, 1453.2 | 144.6/0.9/130.0/0.2 |
| AllKinds | 1000 | 500 | Uniform | 4 | 9.3% | 23.0, 1623.4 | 23.3, 1402.4 | 23.0, 1589.8 | 23.3, 1402.1 | 146.9/0.7/131.1/0.2 |
| AllKinds | 1000 | 500 | Uniform | 5 | 9.0% | 22.9, 1604.1 | 23.2, 1420.2 | 22.9, 1575.1 | 23.2, 1418.3 | 146.8/0.8/128.8/0.2 |
| AllKinds | 1000 | 500 | Skewed | 1 | 9.1% | 22.3, 1529.2 | 22.5, 1393.3 | 22.3, 1494.3 | 22.5, 1388.3 | 130.8/13.7/93.3/13.2 |
| AllKinds | 1000 | 500 | Skewed | 2 | 9.7% | 23.1, 1486.6 | 23.4, 1287.4 | 23.2, 1443.4 | 23.4, 1290.4 | 130.4/13.4/93.5/13.2 |
| AllKinds | 1000 | 500 | Skewed | 3 | 9.5% | 22.7, 1598.4 | 22.9, 1468.2 | 22.7, 1580.9 | 22.9, 1474.8 | 131.3/14.9/92.7/14.7 |
| AllKinds | 1000 | 500 | Skewed | 4 | 9.3% | 23.4, 1552.7 | 23.7, 1417.0 | 23.5, 1499.4 | 23.7, 1417.0 | 129.7/14.8/94.8/14.4 |
| AllKinds | 1000 | 500 | Skewed | 5 | 9.0% | 22.0, 1606.2 | 22.3, 1429.5 | 22.1, 1535.7 | 22.3, 1431.0 | 128.3/14.3/91.9/14.4 |
| AllKinds | 1000 | 900 | Uniform | 1 | 9.1% | 36.6, 1558.5 | 37.3, 1343.6 | 36.6, 1536.0 | 37.3, 1342.4 | 146.5/0.8/134.1/0.2 |
| AllKinds | 1000 | 900 | Uniform | 2 | 9.7% | 37.0, 1600.6 | 37.7, 1391.6 | 37.1, 1574.0 | 37.7, 1390.4 | 145.6/0.7/130.5/0.2 |
| AllKinds | 1000 | 900 | Uniform | 3 | 9.5% | 36.7, 1648.3 | 37.4, 1454.8 | 36.8, 1638.7 | 37.4, 1453.2 | 144.7/0.9/130.5/0.2 |
| AllKinds | 1000 | 900 | Uniform | 4 | 9.3% | 38.3, 1607.9 | 39.0, 1402.6 | 38.3, 1539.1 | 39.0, 1402.1 | 146.4/0.7/132.0/0.2 |
| AllKinds | 1000 | 900 | Uniform | 5 | 9.0% | 38.3, 1601.8 | 39.0, 1418.3 | 38.4, 1558.2 | 39.0, 1418.2 | 145.3/0.8/131.3/0.2 |
| AllKinds | 1000 | 900 | Skewed | 1 | 9.1% | 37.1, 1565.1 | 37.7, 1396.6 | 37.3, 1503.8 | 37.7, 1396.5 | 130.8/21.8/98.9/21.3 |
| AllKinds | 1000 | 900 | Skewed | 2 | 9.7% | 38.5, 1446.7 | 39.1, 1306.0 | 38.7, 1466.6 | 39.1, 1307.4 | 131.0/21.2/100.0/20.8 |
| AllKinds | 1000 | 900 | Skewed | 3 | 9.5% | 37.8, 1634.9 | 38.4, 1481.5 | 37.9, 1590.1 | 38.4, 1482.0 | 130.4/22.8/101.4/22.1 |
| AllKinds | 1000 | 900 | Skewed | 4 | 9.3% | 38.9, 1548.4 | 39.5, 1440.4 | 39.0, 1533.8 | 39.5, 1437.0 | 130.3/22.1/102.0/21.9 |
| AllKinds | 1000 | 900 | Skewed | 5 | 9.0% | 36.7, 1620.3 | 37.2, 1429.5 | 36.8, 1561.1 | 37.2, 1431.0 | 131.0/22.3/101.2/21.4 |
| Mixed | 10 | 500 | Uniform | 1 | 9.1% | 164.9, 239.0 | 182.9, 15.8 | 166.0, 217.0 | 183.0, 14.8 | 142.5/0.9/130.1/0.2 |
| Mixed | 10 | 500 | Uniform | 2 | 9.7% | 161.3, 219.4 | 179.0, 14.7 | 162.8, 209.9 | 179.1, 14.0 | 144.4/0.7/128.9/0.2 |
| Mixed | 10 | 500 | Uniform | 3 | 9.5% | 160.5, 225.5 | 178.6, 15.4 | 162.1, 212.8 | 178.7, 14.6 | 145.0/0.9/130.6/0.2 |
| Mixed | 10 | 500 | Uniform | 4 | 9.3% | 161.5, 247.8 | 179.7, 15.6 | 163.7, 209.6 | 179.7, 14.9 | 144.3/0.7/128.9/0.2 |
| Mixed | 10 | 500 | Uniform | 5 | 9.0% | 166.0, 230.0 | 184.7, 14.8 | 167.6, 212.0 | 184.8, 13.7 | 142.5/0.8/130.6/0.2 |
| Mixed | 10 | 500 | Skewed | 1 | 9.1% | 181.5, 188.7 | 200.4, 14.8 | 186.3, 147.9 | 200.5, 14.1 | 119.6/0.8/81.7/0.2 |
| Mixed | 10 | 500 | Skewed | 2 | 9.7% | 188.8, 192.7 | 207.9, 13.3 | 192.7, 148.8 | 208.0, 13.1 | 120.9/0.7/86.4/0.2 |
| Mixed | 10 | 500 | Skewed | 3 | 9.5% | 183.4, 196.7 | 203.0, 15.8 | 188.4, 154.1 | 203.2, 14.8 | 120.7/0.9/83.7/0.2 |
| Mixed | 10 | 500 | Skewed | 4 | 9.3% | 190.0, 202.9 | 210.4, 14.9 | 195.3, 150.6 | 210.5, 14.2 | 118.8/0.8/83.8/0.2 |
| Mixed | 10 | 500 | Skewed | 5 | 9.0% | 177.2, 201.4 | 196.2, 15.6 | 182.2, 149.5 | 196.2, 14.3 | 120.8/0.8/85.7/0.2 |
| Mixed | 10 | 900 | Uniform | 1 | 9.1% | 278.2, 236.9 | 326.4, 15.8 | 282.4, 222.9 | 326.8, 14.8 | 143.4/0.9/129.2/0.2 |
| Mixed | 10 | 900 | Uniform | 2 | 9.7% | 270.0, 221.8 | 319.4, 14.7 | 274.1, 218.0 | 319.7, 14.0 | 143.2/0.7/130.4/0.2 |
| Mixed | 10 | 900 | Uniform | 3 | 9.5% | 271.0, 238.7 | 318.5, 15.4 | 274.6, 233.5 | 318.9, 14.6 | 143.4/0.9/133.4/0.2 |
| Mixed | 10 | 900 | Uniform | 4 | 9.3% | 274.1, 235.5 | 321.2, 15.7 | 278.0, 207.5 | 321.5, 14.9 | 144.0/0.7/128.8/0.2 |
| Mixed | 10 | 900 | Uniform | 5 | 9.0% | 278.9, 214.8 | 329.7, 14.8 | 281.9, 218.4 | 330.1, 13.7 | 142.7/0.8/129.6/0.2 |
| Mixed | 10 | 900 | Skewed | 1 | 9.1% | 306.7, 201.2 | 357.0, 14.8 | 317.0, 155.4 | 357.3, 14.1 | 119.2/0.8/87.8/0.2 |
| Mixed | 10 | 900 | Skewed | 2 | 9.7% | 318.1, 178.7 | 371.2, 13.3 | 329.2, 142.2 | 371.6, 13.1 | 118.7/0.7/91.2/0.2 |
| Mixed | 10 | 900 | Skewed | 3 | 9.5% | 309.9, 196.0 | 361.7, 15.7 | 320.2, 153.2 | 362.4, 14.9 | 120.6/0.9/89.7/0.2 |
| Mixed | 10 | 900 | Skewed | 4 | 9.3% | 319.3, 197.3 | 374.8, 14.9 | 330.0, 156.2 | 375.1, 14.3 | 118.9/0.8/90.6/0.2 |
| Mixed | 10 | 900 | Skewed | 5 | 9.0% | 300.4, 200.2 | 349.3, 15.7 | 309.5, 167.7 | 349.8, 14.6 | 119.6/0.9/91.8/0.3 |
| Mixed | 100 | 500 | Uniform | 1 | 9.1% | 104.2, 360.2 | 110.7, 146.8 | 104.6, 351.5 | 110.7, 145.5 | 143.4/0.9/130.8/0.2 |
| Mixed | 100 | 500 | Uniform | 2 | 9.7% | 101.1, 353.8 | 107.7, 138.2 | 101.7, 332.1 | 107.8, 138.0 | 145.3/0.7/131.5/0.2 |
| Mixed | 100 | 500 | Uniform | 3 | 9.5% | 101.4, 349.9 | 108.5, 144.9 | 102.2, 335.4 | 108.5, 143.7 | 145.0/0.9/131.8/0.2 |
| Mixed | 100 | 500 | Uniform | 4 | 9.3% | 102.2, 356.1 | 108.9, 147.1 | 102.8, 351.7 | 108.9, 145.8 | 145.4/0.8/132.2/0.2 |
| Mixed | 100 | 500 | Uniform | 5 | 9.0% | 104.4, 354.3 | 111.4, 136.1 | 104.9, 332.8 | 111.5, 134.4 | 142.4/0.8/133.5/0.2 |
| Mixed | 100 | 500 | Skewed | 1 | 9.1% | 109.3, 333.6 | 115.7, 148.4 | 111.0, 270.8 | 115.7, 147.2 | 124.7/6.9/91.0/6.5 |
| Mixed | 100 | 500 | Skewed | 2 | 9.7% | 113.6, 308.7 | 120.2, 135.2 | 115.2, 272.1 | 120.3, 140.6 | 126.1/6.9/93.5/6.6 |
| Mixed | 100 | 500 | Skewed | 3 | 9.5% | 111.1, 327.4 | 117.3, 154.9 | 112.8, 265.9 | 117.3, 155.1 | 125.5/7.9/92.8/7.5 |
| Mixed | 100 | 500 | Skewed | 4 | 9.3% | 114.0, 312.3 | 120.9, 151.4 | 115.7, 264.8 | 120.9, 150.1 | 125.2/7.8/92.8/7.6 |
| Mixed | 100 | 500 | Skewed | 5 | 9.0% | 107.8, 323.4 | 113.7, 155.6 | 109.2, 278.6 | 113.6, 153.4 | 124.9/9.4/96.0/8.8 |
| Mixed | 100 | 900 | Uniform | 1 | 9.1% | 173.9, 360.0 | 191.8, 146.8 | 175.9, 350.7 | 191.9, 145.8 | 144.9/0.9/132.2/0.2 |
| Mixed | 100 | 900 | Uniform | 2 | 9.7% | 168.6, 346.6 | 186.1, 138.2 | 170.0, 316.3 | 186.1, 138.1 | 144.1/0.7/132.9/0.2 |
| Mixed | 100 | 900 | Uniform | 3 | 9.5% | 170.1, 359.9 | 187.5, 144.8 | 171.4, 337.0 | 187.5, 143.7 | 144.4/0.9/133.0/0.2 |
| Mixed | 100 | 900 | Uniform | 4 | 9.3% | 171.2, 355.9 | 187.9, 147.1 | 172.4, 341.6 | 188.0, 145.8 | 145.8/0.7/131.7/0.2 |
| Mixed | 100 | 900 | Uniform | 5 | 9.0% | 174.2, 345.1 | 192.8, 136.0 | 176.5, 329.9 | 192.9, 134.5 | 145.7/0.9/130.8/0.2 |
| Mixed | 100 | 900 | Skewed | 1 | 9.1% | 182.2, 324.2 | 197.3, 159.6 | 185.7, 283.6 | 197.5, 157.6 | 126.4/14.7/97.8/14.2 |
| Mixed | 100 | 900 | Skewed | 2 | 9.7% | 190.1, 310.0 | 205.9, 152.5 | 193.4, 271.5 | 206.0, 149.0 | 126.4/13.6/97.8/14.1 |
| Mixed | 100 | 900 | Skewed | 3 | 9.5% | 185.5, 321.6 | 200.5, 169.8 | 188.6, 287.3 | 200.4, 170.8 | 126.7/15.6/98.5/15.1 |
| Mixed | 100 | 900 | Skewed | 4 | 9.3% | 191.4, 332.2 | 207.0, 167.3 | 194.8, 284.9 | 206.9, 162.4 | 125.4/14.3/98.4/14.8 |
| Mixed | 100 | 900 | Skewed | 5 | 9.0% | 180.5, 341.0 | 194.8, 166.8 | 183.0, 295.1 | 195.0, 161.6 | 125.1/17.1/101.3/16.2 |
| Mixed | 1000 | 500 | Uniform | 1 | 9.1% | 22.2, 1667.0 | 22.5, 1456.1 | 22.3, 1650.8 | 22.5, 1453.4 | 146.2/1.0/133.8/0.2 |
| Mixed | 1000 | 500 | Uniform | 2 | 9.7% | 21.5, 1582.1 | 21.8, 1377.5 | 21.5, 1549.5 | 21.8, 1377.3 | 146.8/0.7/131.5/0.2 |
| Mixed | 1000 | 500 | Uniform | 3 | 9.5% | 21.8, 1667.5 | 22.2, 1435.2 | 21.9, 1622.1 | 22.2, 1435.2 | 147.5/0.9/134.5/0.2 |
| Mixed | 1000 | 500 | Uniform | 4 | 9.3% | 21.8, 1631.9 | 22.1, 1456.3 | 21.9, 1634.5 | 22.1, 1454.5 | 145.1/0.7/130.8/0.2 |
| Mixed | 1000 | 500 | Uniform | 5 | 9.0% | 22.3, 1535.9 | 22.6, 1343.2 | 22.3, 1551.5 | 22.6, 1341.3 | 144.3/0.9/132.6/0.2 |
| Mixed | 1000 | 500 | Skewed | 1 | 9.1% | 22.2, 1547.3 | 22.4, 1396.9 | 22.3, 1539.8 | 22.4, 1395.3 | 129.5/15.9/96.0/15.3 |
| Mixed | 1000 | 500 | Skewed | 2 | 9.7% | 23.1, 1494.5 | 23.3, 1290.3 | 23.2, 1437.6 | 23.3, 1290.2 | 131.3/15.6/99.8/15.2 |
| Mixed | 1000 | 500 | Skewed | 3 | 9.5% | 22.6, 1624.7 | 22.8, 1481.3 | 22.6, 1575.2 | 22.8, 1474.6 | 132.0/16.7/97.6/16.3 |
| Mixed | 1000 | 500 | Skewed | 4 | 9.3% | 23.1, 1572.2 | 23.4, 1431.4 | 23.2, 1520.1 | 23.4, 1432.3 | 129.9/15.9/96.1/16.3 |
| Mixed | 1000 | 500 | Skewed | 5 | 9.0% | 22.0, 1606.2 | 22.2, 1431.6 | 22.1, 1562.7 | 22.2, 1429.2 | 129.4/18.0/100.3/17.7 |
| Mixed | 1000 | 900 | Uniform | 1 | 9.1% | 37.1, 1694.9 | 37.8, 1454.5 | 37.2, 1651.0 | 37.8, 1453.4 | 145.2/1.1/134.0/0.3 |
| Mixed | 1000 | 900 | Uniform | 2 | 9.7% | 35.8, 1572.7 | 36.6, 1377.5 | 35.9, 1545.9 | 36.6, 1377.4 | 147.1/0.7/132.6/0.2 |
| Mixed | 1000 | 900 | Uniform | 3 | 9.5% | 36.4, 1669.2 | 37.1, 1435.2 | 36.5, 1639.7 | 37.1, 1435.2 | 146.3/0.9/133.2/0.2 |
| Mixed | 1000 | 900 | Uniform | 4 | 9.3% | 36.5, 1660.5 | 37.2, 1456.3 | 36.5, 1621.1 | 37.2, 1454.5 | 146.3/0.7/133.8/0.2 |
| Mixed | 1000 | 900 | Uniform | 5 | 9.0% | 37.3, 1545.0 | 38.0, 1341.4 | 37.3, 1555.5 | 38.0, 1341.3 | 145.7/0.8/133.8/0.2 |
| Mixed | 1000 | 900 | Skewed | 1 | 9.1% | 36.9, 1537.9 | 37.4, 1417.4 | 37.0, 1521.6 | 37.4, 1396.4 | 131.3/24.2/103.9/23.5 |
| Mixed | 1000 | 900 | Skewed | 2 | 9.7% | 38.6, 1503.1 | 39.2, 1288.6 | 38.8, 1423.5 | 39.2, 1292.3 | 129.3/23.5/106.3/22.8 |
| Mixed | 1000 | 900 | Skewed | 3 | 9.5% | 37.7, 1652.1 | 38.3, 1482.2 | 37.9, 1597.7 | 38.2, 1496.1 | 131.2/24.6/104.1/24.6 |
| Mixed | 1000 | 900 | Skewed | 4 | 9.3% | 38.6, 1562.1 | 39.1, 1437.4 | 38.7, 1515.1 | 39.1, 1440.4 | 129.9/24.1/105.0/23.9 |
| Mixed | 1000 | 900 | Skewed | 5 | 9.0% | 36.7, 1653.0 | 37.2, 1429.5 | 36.7, 1565.3 | 37.2, 1429.4 | 130.5/25.5/105.4/24.4 |

## Reading it

- Each kind gains as the whole does. Proximity never chose a node that can't run the workload's kind:
  the fast path admits only nodes that run it, and `a_kind_no_node_runs_is_refused_not_misplaced`
  shows a kind with no node is refused, not placed.
- The mixed fleet gains about as much as the all-kinds one. Shards microVMs run on every node of it,
  Firecracker microVMs on six in ten, and in each region proximity still finds a near node that runs
  the kind.
- Proximity is within 0.21% of the oracle's goodput in every study: at 9-10% median error the
  coordinates rank as well as the truth does here. Where homes are skewed the oracle's p99 is sometimes
  a little worse than proximity's (it packs the near region harder).
- The tie-break keeps much less of the gain than it did over generic jobs (goodput −0.02% to +4.3%
  over blind, mean RTT 77-134 ms against proximity's 0.7-25 ms). It acts only where best fit's free
  capacity ties exactly, and the kinds' overheads (0.42 MiB against 5 MiB) leave nodes' free memory
  apart by odd bytes, so ties are rare. A soft score, which every admitting node can win, doesn't
  depend on ties.
- What the gain rests on (the model, not measured on Hex's fleet): each workload talks to one home, its
  round trips sequential; bandwidth is not modelled (A2's bandwidth gap stands); the WAN's jitter and
  heights may be kinder than a real WAN (H20's falsifier); Shards' overhead is a 5 MiB stand-in.
