# The recorded microVM workload (G2 step 3, AC-T7.4.3)

Hex's scheduler places microVMs (Ada, 2026-11-06: "You are NOT Kiln's load balancer for jobs, CI, and
general work. Your scheduler is for *microvms*."). Its recorded workload is a day of serverless sandbox
launches from a production cloud: `huawei-r5-day30.trace`, and the same day with Huawei's keep-alive
added, `huawei-r5-day30.keepalive.trace`.

## Source and licence

Huawei Cloud's public serverless traces 2025, Region 5, day 30, from A. Joosen, A. Hassan, M. Asenov,
R. Singh, L. Darlow, J. Wang et al., "Serverless Cold Starts and Where to Find Them", EuroSys 2025
(doi:10.1145/3689031.3696073; arXiv 2410.06145), released at https://github.com/sir-lab/data-release
under the Creative Commons Attribution 4.0 International licence (CC BY 4.0,
https://creativecommons.org/licenses/by/4.0/). The traces here are converted from it; the change is the
conversion `huawei_trace.py` describes. The raw files are not committed. `huawei_trace.py` refuses any
file but these:

| file | from | sha256 |
|---|---|---|
| cold starts, all 31 days (`R5.zip`, 54 MiB) | https://drive.google.com/file/d/1ah7nj5fyedMpOeaFgNVA7oMx3815D8BZ | `dadea4f0e5df629f016bb30144baf97d996f93ac487a161f713fb7f5f05e8bf0` |
| requests, day 30 (`R5.zip`, 257 MiB) | https://drive.google.com/file/d/102AFIHkEQHuvG8-hZeWHVFqXeeI-a8xS | `5825f74443cf6e4f5df264f8c94d439fc9eae576399f9786a50ca4cb3316e60d` |

Downloaded 2026-11-06. Rebuilt byte for byte by:

```
huawei_trace.py R5-coldstarts.zip R5-requests.zip huawei-r5-day30.trace
huawei_trace.py R5-coldstarts.zip R5-requests.zip huawei-r5-day30.keepalive.trace --keep-alive-s 60
```

## Why this trace

It is the only public trace found with one row per sandbox launch: each cold start creates a pod, and
the row names the pod's resource pool (its CPU and memory) and its owner.

- Azure Functions 2019 (Shahrad et al., ATC 2020) gives per-minute invocation counts and per-app memory
  percentiles: no instances, and no CPU.
- Azure Functions 2021 (Zhang et al., SOSP 2021) gives per-invocation durations: no memory, CPU or
  instances.
- Azure's 2020 packing trace holds full VMs, not sandboxes. Shards keeps no launch records yet.
- Region 5 is the smallest complete download. Region 1's request logs keep only the top 100 pods per
  function, and Region 3's are a subsample, both of which would bias lifetimes. Regions 2 and 4 are 22 GB
  and 5 GB.
- Day 30 is the only day with request logs, which are what give each sandbox's lifetime.

## What the sandboxes are, and how each maps to a microVM request

They are Huawei's serverless pods, which may be containers rather than microVMs. Hex takes each as a
microVM request by its shape: when it launched, how long it lived, its CPU and memory, and its owner.

| microVM request | from the trace |
|---|---|
| arrival | the cold start's `time` (seconds, read exactly), from the day's first, in ns |
| lifetime | cold start to the end of the pod's last request of the day (`time_worker` + `totalCost_worker`, by `podID`); in the keep-alive trace, plus 60 s |
| vCPU | the pod's resource pool, named in `podID` (`pool24-600-512`: 600 millicores), as millicores |
| memory | the same pool's memory (`512` MB), as MiB |
| devices | none: the trace names none |
| owner | `userID`, a dense index by first launch, each of weight 1; band 0, no gang, no group |
| kind | plain: the trace has no builds or benchmarks |

- **Lifetimes.** Without a keep-alive they are a lower bound: a pod outlives its last request until it
  is deleted, and the trace doesn't record when. Huawei documents the rule: "The pod then waits for
  additional requests for a designated keep alive time. In our system, this time is set to one minute by
  default. If the pod receives no requests during this time, it is deleted" (Joosen et al. §2). So the
  keep-alive trace adds 60 s to each, which gives the pod's whole life.
- **Hosts.** The trace names clusters, not hosts. The nodes are EC2 m5d.metal hosts, the host the
  Firecracker paper measured on: "two Intel Xeon Platinum 8175M processors (for a total of 48 cores with
  hyper-threading disabled), 384GB of RAM" (Agache et al., NSDI 2020 §6). That makes each node 48,000
  millicores and 384 GiB, with no GPU. There are 10 of them, so each of the 16 seeds of
  `AC-T7.4.3` packs about 600 sandboxes to 90% from its own stretch of the day.

## What it holds

25,345 cold starts on day 30. 48 name no resource pool in their pod ID and 2 are a second cold start of
a pod already launched; both are counted and left out. That leaves 25,295 sandboxes of 339 owners and
468 functions, every one with requests in the day's table. 69 lifetimes come out under 1 ms, because the
two logs' clocks differ slightly, and are run for 1 ms. Every rule is counted in each trace's header.

| CPU (millicores) | memory (MB) | sandboxes |
|---:|---:|---:|
| 300 | 128 | 16,937 |
| 400 | 256 | 2,868 |
| 1,800 | 2,048 | 1,184 |
| 600 | 512 | 1,157 |
| 3,400 | 4,096 | 894 |
| 6,600 | 8,192 | 480 |
| 800 | 768 | 477 |
| 2,600 | 3,072 | 428 |
| 3,000 | 3,584 | 150 |
| 1,000-2,200 | 1,024-2,560 | 720 (five pools of 144 each) |

Lifetimes without the keep-alive: p50 0.32 s, p90 61 s, p99 770 s, the longest 14 hours. CPU binds:
the mean sandbox asks 0.72 cores and 0.65 GiB, about 1.1 cores per GiB, against a host's 48 cores to 384 GiB
(0.125 cores per GiB).

## What would make it wrong

- Huawei's pods are a stand-in for the agent microVMs Hex will run. Hex's own launches may be larger,
  longer and come with devices, none of which this day has. GPU placement and the
  benchmark-beside-build rule are tested on generated clusters instead (hex-sim.md H9).
- The pool's limits are taken as the microVM's request. A platform that packs by usage rather than
  limits would see smaller requests.
- One region and one day; another region's mix could pack differently.
