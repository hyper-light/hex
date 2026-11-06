# The recorded microVM workload (G2 step 3, AC-T7.4.3)

Hex's scheduler places microVMs (Ada, 2026-11-06: "You are NOT Kiln's load balancer for jobs, CI, and
general work. Your scheduler is for *microvms*."), of three kinds: OCI containers, Firecracker microVMs
and Shards microVMs ("we work with OCI containers, Firecracker, and Shards Microvms", the same day). Its
recorded workload, the one `AC-T7.4.3` measures packing on ("a recorded workload of OCI containers,
Firecracker microVMs and Shards microVMs", T7's plan, version 4), is a day of serverless sandbox launches
from a production cloud, each made a request of one of those kinds, on two node models:
- `huawei-r5-day30.trace`, the main one: 10 EC2 m5d.metal hosts, each running all three kinds;
- `huawei-r5-day30.fleet.trace`, beside it: Hex's own fleet's host types, five of each, each running the
  kinds its OS runs, so that where each kind may land matters.

Each has a twin with Huawei's keep-alive added (`.keepalive.trace`).

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
huawei_trace.py R5-coldstarts.zip R5-requests.zip huawei-r5-day30.fleet.trace --nodes fleet:5
huawei_trace.py R5-coldstarts.zip R5-requests.zip huawei-r5-day30.fleet.keepalive.trace --keep-alive-s 60 --nodes fleet:5
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

## What the sandboxes are, and how each maps to a request

They are Huawei's serverless pods, which may be containers rather than microVMs. Hex takes each as a
request by its shape: when it launched, how long it lived, its CPU and memory, and its owner. Its kind,
which the trace doesn't record, comes from its owner (below).

| request | from the trace |
|---|---|
| arrival | the cold start's `time` (seconds, read exactly), from the day's first, in ns |
| lifetime | cold start to the end of the pod's last request of the day (`time_worker` + `totalCost_worker`, by `podID`); in the keep-alive trace, plus 60 s |
| vCPU | the pod's resource pool, named in `podID` (`pool24-600-512`: 600 millicores), as millicores |
| memory | the same pool's memory (`512` MB), as MiB, plus its kind's overhead (below) |
| devices | none: the trace names none |
| owner | `userID`, a dense index by first launch, each of weight 1; band 0, no gang, no group |
| kind | an OCI container, a Firecracker microVM or a Shards microVM, its owner's (below) |
| role | plain: the trace has no builds, benchmarks or agent sessions |

- **Lifetimes.** Without a keep-alive they are a lower bound: a pod outlives its last request until it
  is deleted, and the trace doesn't record when. Huawei documents the rule: "The pod then waits for
  additional requests for a designated keep alive time. In our system, this time is set to one minute by
  default. If the pod receives no requests during this time, it is deleted" (Joosen et al. §2). So the
  keep-alive trace adds 60 s to each, which gives the pod's whole life.

## Each request's kind: its owner's

- **The rule.** All of an owner's sandboxes are one kind. The trace records no kind, so owners are split
  to give each kind a third of the day's launches: the owner with the most launches first, each to the
  kind with the fewest so far, ties OCI, then Firecracker, then Shards (`classes_by_owner`). OCI gets
  8,432 launches of 107 owners, Firecracker 8,432 of 117, Shards 8,431 of 115.
- **Why by owner.** A runtime is an isolation choice made for a workload, not for each launch: a
  Kubernetes pod names its runtime class in its spec (`runtimeClassName`; Kubernetes, "Runtime Class",
  https://kubernetes.io/docs/concepts/containers/runtime-class/), and AWS chose Firecracker for whole
  services, Lambda and Fargate (Agache et al., NSDI 2020, abstract). An owner's sandboxes here are its
  functions' pods.
- **Why thirds.** Nothing in the trace favours one kind, and Hex records no launches of its own yet.
  The split matters only to the mix: each kind alone runs the whole day as that kind
  (`hex_sim::packing::alone`), so its measure doesn't depend on the split.

## Where each kind may land, and what it costs there

There are two node models, neither with background load (the lead, 2026-11-06: the first stays the main
one, the second sits beside it to exercise where each kind may land).

- **EC2 m5d.metal, the main one.** 10 hosts of the host the Firecracker paper measured on: "two Intel Xeon
  Platinum 8175M processors (for a total of 48 cores with hyper-threading disabled), 384GB of RAM"
  (Agache et al., NSDI 2020 §6), so 48,000 millicores and 384 GiB each, with no GPU. Each is Linux x86_64
  with KVM, so each runs all three kinds, and every request may use every node. 10 hosts make each of
  the 16 seeds of `AC-T7.4.3` pack about 600 launches to 90% from its own stretch of the day.
- **Hex's fleet, beside it.** Hex's own hosts, from kiln's runner facts (2026-11-05, `kiln_workload.md`),
  five of each: each host's threads as millicores, its memory, its GPUs, and the kinds its OS runs.

| host | OS | threads | memory | kinds |
|---|---|---:|---:|---|
| adacachy | Linux x86_64 | 32 | 122.9 GiB, 1 GPU | OCI, Firecracker, Shards |
| fedora (`kiln-a094a12d.attlocal.net`) | Linux x86_64 | 32 | 62.5 GiB | OCI, Firecracker, Shards |
| jupiter | Windows x86_64 | 32 | 123.6 GiB | OCI, Shards |
| 7 Macs (Mac Studios, MacBook Pros) | macOS arm64 | 14-32 | 36-128 GiB | Shards |

- **OCI containers** run on Linux (Hex's `oci` class) and on x86_64 Windows over HCS (native step 4),
  not on macOS, where Hex has no OCI class yet.
- **Firecracker microVMs** run on Linux with KVM only.
- **Shards microVMs** run on KVM on Linux, Hypervisor.framework on arm64 macOS and WHP on x86_64 Windows
  (Shards' own API draft, hyper-light/shards `a0cced87`, `crates/api/src/lib.rs:172-178`). Its guest
  floor, 128 MiB on x86_64 and 64 MiB on arm64 (`lib.rs:188-191`), never binds: the smallest pool is
  128 MiB.
- **GPUs.** Only an OCI container can be given one (`hex_sim::model::Class::takes_gpus`;
  scheduler-architecture.md S3). Mainline Firecracker passes no device through. Shards' VMM gives a guest
  a shared GPU through Venus (virtio-gpu, on Linux amd64, macOS arm64 and Windows amd64) or, on Linux, a
  whole PCI function by VFIO, but shards-api, Hex's one seam to it, names no device to ask for
  (`docs/research/scheduler/agentic-workloads.md` [58]-[60]). This day asks for none; the rule is tested
  on generated clusters (hex-sim.md H9).
- **Overheads**, added to each request's memory (an `overhead` row each): OCI 0.42 MiB, Hex's own
  measurement of an idle container (`bench/competitive/RESULT.md:95`); Firecracker 5 MiB, its
  specification's bound on the VMM's memory, tested on every pull request, and the NSDI 2020 paper's
  "memory overhead of less than 5MB per container" (`docs/research/runtime/substrates-and-sandboxes.md`
  §3.3); Shards by guest size, 2.26 MiB beside a 128 MiB guest to 4.77 MiB beside an 8,192 MiB one, its
  VMM's own PSS plus the kernel's memory per guest, measured on fedora under KVM with the build native step
  5 runs (`shards_overhead.md`), one `overhead` row per size. No source gives a resting CPU cost for any of
  the three, so none is charged.

On the fleet, OCI containers may use 15 of the 50 nodes (480 cores), Firecracker microVMs 10 (320
cores), and Shards microVMs all 50 (1,105 cores). On either model, a pack's load is measured over the
nodes its requests may use.

As context, not of record, `packing.md` also reports the fleet as it is, one of each host: the fleet
trace's first copy, the nodes `--nodes fleet:1` writes. There Firecracker's whole sub-cluster is two
hosts, so a seed can turn on one node's state, which is why the fleet of record is five copies.

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
the mean sandbox asks 0.72 cores and 0.65 GiB, about 1.1 cores per GiB, against an m5d.metal's 0.125
cores per GiB (48 to 384) and the fleet's 0.29 (1,105 cores to 3,765 GiB). By kind, the mean OCI request asks 0.61 cores and 0.51 GiB, the mean
Firecracker one 0.47 and 0.34, the mean Shards one 1.09 and 1.11.

## What would make it wrong

- Huawei's pods are a stand-in for the agent microVMs Hex will run. Hex's own launches may be larger,
  longer and come with devices, none of which this day has. GPU placement and the
  benchmark-beside-build rule are tested on generated clusters instead (hex-sim.md H9).
- The kinds are given, not recorded. A real split far from thirds, or by function rather than owner,
  would change the mix's measure (not each kind's alone).
- Shards' overhead is measured on idle guests, on one host, without a network or shares
  (`shards_overhead.md`, what it doesn't show). A busy guest, or one with Shards' network or share
  process beside it, costs its host more.
- The pool's limits are taken as the request. A platform that packs by usage rather than limits would
  see smaller requests.
- The nodes are two models: m5d.metal, where every kind fits everywhere, and Hex's fleet today, five
  times over. A fleet of other shapes, or with more Linux hosts, would change where each kind fits.
- One region and one day; another region's mix could pack differently.
