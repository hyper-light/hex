# Proximity-scored placement against proximity-blind on a modelled WAN (S4, A2; G3)

**Question:** does scoring placement by hyper-swim's Vivaldi coordinates improve goodput or the
completion-time tail over placement that ignores proximity (scheduler-architecture.md S4's falsifier)?
**Result: yes, on every one of 60 studies** (5 seeds × 2 home layouts × 2 loads × 3 compute steps):
goodput +0.9% to +18.6%, p99 completion time 5.7% to 93.7% lower. The gain shrinks as the compute
between round trips grows against the RTT. The gain is recorded and proximity stays a soft score (S4);
it is not dropped to a tie-breaker.

## What was measured

`crates/hex-bench/benches/sched_proximity.rs` over `hex_sim::proximity` (`docs/design/hex-sim.md` H12, H13):
- 1,000 nodes of 16 cores and 64 GiB over Azure's eight regions (`hex_sim::wan`: Azure's published P50
  inter-region round trips, zones under 2 ms, access heights 25-250 µs, probes stretched up to 10%).
  Every node learns hyper-swim's coordinates: 8 probes a round, 64 rounds.
- 20,000 workloads each. A workload talks to a home node and makes `round_trips` sequential round trips
  to it, each after `step` of compute. Round trips are log-normal through TraceLab's p50 25 and p99
  1,438 tool calls a session. Its run time is `round_trips × (step + true RTT)`.
- Homes: `Uniform` (region drawn uniformly) or `Skewed` (half in East US, the rest uniform).
- Load: arrivals offer 50% or 90% of the cluster's CPU against blind placement's expected occupancy.
- Policies, over the fast path's own sample (`FitIndex::sampled_fit`): **blind** (sampled best fit),
  **proximity** (least predicted RTT read as an upper bound, `rtt × (1 + 2·mean error)`; best fit
  breaks ties), **tie-break** (best fit, proximity only among nodes whose free capacity ties),
  **oracle** (least true RTT).
- Goodput: workloads finished by the horizon (the last arrival), per second. Completion time: arrival to
  end, every workload run to its end.

Virtual time: the numbers are a function of the seed and the code, not of the host's load.

## The run of record

- Host: fedora, `kiln-a094a12d.attlocal.net`, Intel Core i9-14900HX, Linux 6.19.10-300.fc44; release
  (`cargo bench` profile).
- Commit: `f1b1ec0` (cp/t7-sched-g3). Date: 2026-11-05, 03:04Z. Unit:
  `hex-sched-g3-bench-20261105T025724Z` (CPUs 16-31, exit 0). Load average at the start 32.4 (noisy;
  it doesn't change virtual-time results), co-tenants `hexbench-oci-623847`,
  `issue556oss-control-plane`, kiln buildkit and minio containers.
- Coordinates: median relative RTT error 9.0-9.7% over 20,000 pairs (Vivaldi's own evaluation: 11%,
  PF[1]); a node's own region predicted nearer than another's in 19,971-19,988 of about 20,000 trials.

## Proximity against blind, per seed (goodput gain %, p99 completion cut %)

```
step   10 ms load 500‰ Uniform: (+11.5, +93.1) (+11.6, +93.6) (+11.2, +92.9) (+11.1, +93.3) (+11.9, +92.7)
step   10 ms load 500‰ Skewed: (+9.9, +91.8) (+10.5, +92.3) (+10.2, +91.0) (+10.8, +92.0) (+9.9, +91.5)
step   10 ms load 900‰ Uniform: (+18.0, +92.9) (+18.4, +93.4) (+17.5, +93.7) (+18.0, +93.3) (+18.6, +93.4)
step   10 ms load 900‰ Skewed: (+15.2, +89.6) (+16.3, +88.0) (+15.1, +88.2) (+16.5, +88.6) (+14.9, +87.7)
step  100 ms load 500‰ Uniform: (+6.7, +58.8) (+6.9, +58.7) (+6.5, +59.3) (+6.7, +58.8) (+6.8, +59.1)
step  100 ms load 500‰ Skewed: (+4.8, +48.8) (+4.9, +46.4) (+4.8, +46.0) (+5.5, +47.0) (+4.7, +43.3)
step  100 ms load 900‰ Uniform: (+10.5, +61.3) (+10.4, +59.3) (+10.0, +58.9) (+10.1, +59.2) (+10.7, +58.5)
step  100 ms load 900‰ Skewed: (+7.0, +45.3) (+7.2, +41.8) (+6.9, +41.2) (+7.6, +43.0) (+7.0, +38.8)
step 1000 ms load 500‰ Uniform: (+1.3, +13.4) (+1.3, +10.9) (+1.3, +12.8) (+1.5, +11.4) (+1.5, +15.5)
step 1000 ms load 500‰ Skewed: (+0.9, +7.9) (+0.9, +10.7) (+1.0, +7.6) (+1.2, +8.4) (+1.0, +6.8)
step 1000 ms load 900‰ Uniform: (+2.0, +12.3) (+2.2, +12.3) (+2.1, +14.2) (+1.9, +11.8) (+2.3, +12.7)
step 1000 ms load 900‰ Skewed: (+1.3, +5.7) (+1.3, +8.8) (+1.2, +8.2) (+1.2, +7.1) (+1.3, +7.9)
```

## Every policy, per seed (goodput /s, p99 completion s; mean true RTT to home, ms)

| step ms | load ‰ | homes | seed | coord err | blind | proximity | tie-break | oracle | RTT b/p/t/o |
|---|---|---|---|---|---|---|---|---|---|
| 10 | 500 | Uniform | 1 | 9.1% | 166.2, 227.9 | 185.3, 15.7 | 173.3, 170.8 | 185.4, 14.4 | 143.3/0.8/90.8/0.2 |
| 10 | 500 | Uniform | 2 | 9.7% | 166.6, 230.4 | 185.9, 14.7 | 174.8, 156.0 | 185.9, 14.2 | 143.7/0.7/88.9/0.2 |
| 10 | 500 | Uniform | 3 | 9.5% | 156.3, 239.2 | 173.8, 17.0 | 163.4, 163.6 | 174.0, 15.9 | 144.0/0.9/88.1/0.2 |
| 10 | 500 | Uniform | 4 | 9.3% | 157.8, 236.1 | 175.3, 15.9 | 164.4, 167.8 | 175.4, 14.8 | 143.8/0.8/88.5/0.2 |
| 10 | 500 | Uniform | 5 | 9.0% | 171.2, 213.5 | 191.5, 15.5 | 179.5, 164.7 | 191.6, 14.3 | 144.2/0.8/88.4/0.2 |
| 10 | 500 | Skewed | 1 | 9.1% | 184.1, 183.0 | 202.5, 14.9 | 192.0, 120.5 | 202.6, 14.2 | 119.3/0.9/69.0/0.3 |
| 10 | 500 | Skewed | 2 | 9.7% | 188.1, 190.7 | 207.9, 14.7 | 196.7, 124.3 | 208.1, 13.3 | 119.0/1.5/67.8/0.4 |
| 10 | 500 | Skewed | 3 | 9.5% | 170.2, 211.5 | 187.6, 19.0 | 177.4, 135.2 | 187.9, 16.7 | 121.2/1.7/70.4/0.5 |
| 10 | 500 | Skewed | 4 | 9.3% | 192.5, 180.5 | 213.3, 14.5 | 201.0, 116.7 | 213.5, 13.4 | 118.7/1.2/68.4/0.5 |
| 10 | 500 | Skewed | 5 | 9.0% | 183.2, 189.1 | 201.3, 16.1 | 190.7, 135.0 | 201.6, 14.9 | 118.6/1.4/69.3/0.5 |
| 10 | 900 | Uniform | 1 | 9.1% | 280.1, 223.6 | 330.6, 15.8 | 300.5, 150.5 | 330.8, 14.4 | 144.0/0.8/85.5/0.2 |
| 10 | 900 | Uniform | 2 | 9.7% | 280.4, 221.0 | 332.0, 14.7 | 302.8, 162.3 | 332.3, 14.2 | 143.5/0.7/84.2/0.2 |
| 10 | 900 | Uniform | 3 | 9.5% | 264.0, 272.5 | 310.3, 17.1 | 282.4, 161.0 | 310.6, 15.9 | 142.8/0.9/85.2/0.2 |
| 10 | 900 | Uniform | 4 | 9.3% | 265.6, 237.3 | 313.3, 15.9 | 285.3, 154.5 | 313.6, 14.9 | 145.0/0.8/84.7/0.2 |
| 10 | 900 | Uniform | 5 | 9.0% | 287.8, 236.7 | 341.4, 15.5 | 310.8, 154.3 | 341.9, 14.3 | 143.5/0.8/85.2/0.2 |
| 10 | 900 | Skewed | 1 | 9.1% | 311.4, 191.9 | 358.8, 20.0 | 330.6, 123.5 | 358.9, 20.6 | 117.9/4.6/71.6/4.0 |
| 10 | 900 | Skewed | 2 | 9.7% | 316.6, 196.3 | 368.2, 23.5 | 339.2, 125.3 | 368.6, 21.8 | 118.9/5.6/69.3/4.8 |
| 10 | 900 | Skewed | 3 | 9.5% | 289.0, 209.8 | 332.6, 24.8 | 308.9, 141.9 | 333.2, 23.7 | 120.5/4.9/67.9/4.1 |
| 10 | 900 | Skewed | 4 | 9.3% | 324.6, 185.7 | 378.0, 21.2 | 347.0, 124.6 | 377.6, 21.9 | 119.0/5.2/70.0/5.0 |
| 10 | 900 | Skewed | 5 | 9.0% | 310.7, 189.3 | 357.0, 23.3 | 329.4, 138.0 | 357.6, 21.9 | 118.4/5.6/69.4/4.4 |
| 100 | 500 | Uniform | 1 | 9.1% | 104.7, 347.2 | 111.7, 143.1 | 107.7, 281.0 | 111.8, 142.2 | 144.6/0.8/89.3/0.2 |
| 100 | 500 | Uniform | 2 | 9.7% | 105.2, 337.6 | 112.4, 139.4 | 108.1, 257.4 | 112.4, 139.2 | 144.2/0.7/91.9/0.2 |
| 100 | 500 | Uniform | 3 | 9.5% | 98.6, 388.0 | 105.0, 158.0 | 101.2, 308.1 | 105.1, 156.3 | 144.6/0.9/91.4/0.2 |
| 100 | 500 | Uniform | 4 | 9.3% | 99.5, 355.7 | 106.2, 146.7 | 102.1, 301.4 | 106.2, 145.7 | 145.2/0.7/91.5/0.2 |
| 100 | 500 | Uniform | 5 | 9.0% | 108.2, 343.1 | 115.5, 140.2 | 111.0, 280.0 | 115.5, 139.9 | 143.6/0.8/90.1/0.2 |
| 100 | 500 | Skewed | 1 | 9.1% | 111.6, 311.1 | 117.0, 159.3 | 114.2, 243.3 | 117.0, 159.8 | 124.1/21.1/75.7/20.3 |
| 100 | 500 | Skewed | 2 | 9.7% | 113.6, 305.9 | 119.2, 163.9 | 116.0, 241.9 | 119.1, 164.1 | 126.7/20.9/77.2/21.0 |
| 100 | 500 | Skewed | 3 | 9.5% | 103.2, 365.7 | 108.1, 197.5 | 105.5, 284.6 | 108.1, 197.4 | 125.4/19.6/76.0/19.7 |
| 100 | 500 | Skewed | 4 | 9.3% | 115.7, 298.1 | 122.1, 158.1 | 119.0, 225.3 | 122.1, 156.1 | 124.4/21.4/76.1/20.8 |
| 100 | 500 | Skewed | 5 | 9.0% | 110.7, 316.7 | 115.9, 179.7 | 113.3, 270.6 | 116.1, 173.8 | 124.7/20.9/76.7/20.0 |
| 100 | 900 | Uniform | 1 | 9.1% | 174.5, 369.6 | 192.9, 143.1 | 182.4, 267.6 | 193.0, 142.3 | 144.5/0.8/85.3/0.2 |
| 100 | 900 | Uniform | 2 | 9.7% | 175.8, 341.8 | 194.1, 139.1 | 183.6, 260.0 | 194.3, 139.3 | 145.5/0.7/88.1/0.2 |
| 100 | 900 | Uniform | 3 | 9.5% | 165.2, 383.1 | 181.7, 157.4 | 172.2, 284.2 | 181.8, 156.2 | 144.3/0.9/87.6/0.2 |
| 100 | 900 | Uniform | 4 | 9.3% | 167.2, 359.8 | 184.0, 146.8 | 174.2, 292.3 | 184.1, 145.9 | 145.2/0.8/87.7/0.2 |
| 100 | 900 | Uniform | 5 | 9.0% | 180.3, 339.9 | 199.6, 140.9 | 187.9, 276.3 | 199.8, 139.8 | 144.3/0.8/88.3/0.2 |
| 100 | 900 | Skewed | 1 | 9.1% | 186.8, 312.3 | 199.8, 170.8 | 193.4, 242.4 | 199.6, 171.1 | 125.6/28.7/77.7/28.9 |
| 100 | 900 | Skewed | 2 | 9.7% | 189.7, 306.2 | 203.4, 178.3 | 196.7, 252.4 | 203.4, 176.9 | 125.3/28.8/77.3/29.5 |
| 100 | 900 | Skewed | 3 | 9.5% | 172.8, 362.5 | 184.7, 213.3 | 178.9, 270.2 | 184.3, 207.4 | 126.7/26.9/76.5/27.8 |
| 100 | 900 | Skewed | 4 | 9.3% | 193.3, 299.9 | 208.0, 170.9 | 200.6, 227.6 | 208.0, 168.7 | 126.8/30.3/78.5/29.8 |
| 100 | 900 | Skewed | 5 | 9.0% | 185.5, 316.4 | 198.4, 193.7 | 192.0, 265.3 | 198.4, 194.6 | 126.0/29.0/79.3/29.3 |
| 1000 | 500 | Uniform | 1 | 9.1% | 22.4, 1641.0 | 22.7, 1421.5 | 22.5, 1549.7 | 22.7, 1420.2 | 146.8/0.8/89.6/0.2 |
| 1000 | 500 | Uniform | 2 | 9.7% | 22.5, 1556.6 | 22.8, 1386.9 | 22.6, 1487.9 | 22.8, 1386.3 | 145.5/0.8/91.3/0.2 |
| 1000 | 500 | Uniform | 3 | 9.5% | 21.1, 1789.0 | 21.4, 1560.3 | 21.2, 1706.6 | 21.4, 1560.3 | 145.6/1.0/90.8/0.2 |
| 1000 | 500 | Uniform | 4 | 9.3% | 21.3, 1642.6 | 21.6, 1454.7 | 21.4, 1605.1 | 21.6, 1454.5 | 146.6/0.8/90.8/0.2 |
| 1000 | 500 | Uniform | 5 | 9.0% | 23.2, 1650.3 | 23.5, 1394.7 | 23.3, 1556.5 | 23.5, 1394.5 | 146.4/0.8/91.5/0.2 |
| 1000 | 500 | Skewed | 1 | 9.1% | 22.7, 1530.6 | 22.9, 1409.7 | 22.8, 1477.5 | 22.9, 1407.6 | 131.0/29.6/80.8/29.9 |
| 1000 | 500 | Skewed | 2 | 9.7% | 23.1, 1489.9 | 23.3, 1330.4 | 23.1, 1419.4 | 23.3, 1318.5 | 129.7/30.1/79.5/30.3 |
| 1000 | 500 | Skewed | 3 | 9.5% | 21.0, 1775.1 | 21.2, 1640.5 | 21.1, 1746.2 | 21.2, 1645.5 | 130.8/27.9/80.4/28.1 |
| 1000 | 500 | Skewed | 4 | 9.3% | 23.6, 1430.9 | 23.8, 1310.1 | 23.7, 1369.9 | 23.8, 1291.4 | 131.4/29.3/79.9/30.2 |
| 1000 | 500 | Skewed | 5 | 9.0% | 22.5, 1543.6 | 22.7, 1438.2 | 22.7, 1523.2 | 22.7, 1437.4 | 130.5/29.9/82.0/29.0 |
| 1000 | 900 | Uniform | 1 | 9.1% | 37.4, 1619.1 | 38.1, 1420.2 | 37.7, 1538.1 | 38.1, 1420.2 | 145.7/0.9/87.7/0.3 |
| 1000 | 900 | Uniform | 2 | 9.7% | 37.4, 1582.1 | 38.2, 1386.8 | 37.8, 1493.8 | 38.2, 1386.7 | 147.0/0.8/89.3/0.3 |
| 1000 | 900 | Uniform | 3 | 9.5% | 35.3, 1819.1 | 36.0, 1560.3 | 35.6, 1709.3 | 36.0, 1560.3 | 146.1/1.1/91.0/0.3 |
| 1000 | 900 | Uniform | 4 | 9.3% | 35.5, 1648.4 | 36.2, 1454.7 | 35.8, 1582.3 | 36.2, 1454.4 | 145.6/1.0/87.9/0.4 |
| 1000 | 900 | Uniform | 5 | 9.0% | 38.5, 1596.9 | 39.4, 1394.8 | 38.9, 1512.4 | 39.4, 1394.5 | 144.9/0.9/89.4/0.3 |
| 1000 | 900 | Skewed | 1 | 9.1% | 38.0, 1509.7 | 38.5, 1423.1 | 38.2, 1462.6 | 38.5, 1423.0 | 129.9/38.5/84.2/37.2 |
| 1000 | 900 | Skewed | 2 | 9.7% | 38.4, 1482.6 | 38.9, 1352.2 | 38.7, 1427.4 | 38.9, 1358.6 | 129.9/38.5/83.6/38.7 |
| 1000 | 900 | Skewed | 3 | 9.5% | 35.2, 1798.8 | 35.6, 1651.9 | 35.4, 1721.2 | 35.6, 1692.5 | 130.8/36.7/83.5/36.8 |
| 1000 | 900 | Skewed | 4 | 9.3% | 39.1, 1423.8 | 39.5, 1322.8 | 39.3, 1375.3 | 39.5, 1322.7 | 131.4/40.9/86.1/38.7 |
| 1000 | 900 | Skewed | 5 | 9.0% | 37.6, 1565.4 | 38.1, 1441.5 | 37.9, 1498.5 | 38.1, 1451.3 | 130.6/38.7/85.2/39.0 |

## Reading it

- Proximity is within 1% of the oracle's goodput in every study: at 9-10% median error the
  coordinates rank as well as the truth does here. Where homes are skewed the oracle's p99 is sometimes
  a little worse than proximity's (it packs the near region harder).
- The tie-break keeps about a third to a half of the gain: best fit's ties are common in a cluster with
  room, but proximity among them alone leaves most placements cross-region.
- What the gain rests on (the model, not measured on Hex's fleet): each workload talks to one home, its
  round trips sequential; bandwidth is not modelled (A2's bandwidth gap stands); the WAN's jitter and
  heights may be kinder than a real WAN (H12's falsifier). The full log is on fedora at
  `~/.cache/hex-t7/sched-g3/out/hex-sched-g3-bench-20261105T025724Z.log`.
