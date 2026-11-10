# AC-T7.4.1: supported device integration checks

Coordinator Hex 01a1aa77778471339bea3beff90d6bbd. Current Shards policy33df95b0,
R01–R16 rules SHA256 df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Implementers own scoped light checks; ROOT owns native effects, audits, integration,
profiling, benchmarks and main. Platform-based unsupported responses remain gaps.

The lossless proof archive is SHA256 0bf4601d3d151b42ad9460c6ce3425c99c6d1b0489895df69cafc400c5f0564b.
The independent collector rehashed every post-check source file against its signed
Git archive, verified raw log hashes and exits, and checked each Cargo child reaped
and each private temporary directory empty. receipt.json retains commands, PIDs,
versions, archive/source manifests and actual results, including failures.

| Exact source | Actual result and scope |
| --- | --- |
| bd8e4152 | Capacity routing model: 1 passed, 1 failed; native device pool hidden. |
| e6c324f6 | Same model: 2 passed; native runtime Clippy clean. Linux mixed-vendor physical fixture cross-Clippy clean; fixture not executed. |
| 533dc2d1 | Supported backend model: 2 passed, 1 failed; Windows ARM64 rejected. Original unused mock socket thread also printed EOF panic; fixed in later source and preserved in this raw log. |
| 0ba6a60f | Routing/backend 3 passed and actual offer helper 6 passed, no panic text; Mac native and Windows ARM64 node Clippy clean. |
| c2da38cc | Offers 6 passed; actual Apple hardware test 1 passed, no skipped device; Mac native and Windows ARM64 node Clippy clean. Facts filter selected **zero** tests and Linux node Clippy failed (missing gpu_facts import in Linux-only test module); whole run exit1. Corrected source7adf299e moves this regression to an all-target test module; its results are pending separately. |

Actual native hardware: Mac Studio M3 Ultra, macOS26.4/Darwin25.4.0,
Metal registryID4294968831, recommendedMaxWorkingSetSize83494174720 bytes.
Both samples returned allocated-memory2117156864 bytes and busy0%. This is
native inventory/counter evidence, not compute, binding, isolation, runtime/guest
attachment, restart cleanup, allocation-free or performance evidence. The Metal
budget is not dedicated VRAM or total host memory. Native CF allocations are not
measured by the fixture's Rust allocator.

All seven targets, major Linux environments, actual Linux/Windows device effects,
Metal compute and guest attachment, recovery/mutations, resource/fault accounting,
comparisons/profiles, and final combined gates remain owed. The Linux runner
admission guard currently reports no 1CPU4GiB room on either approved Linux host;
Fedora remains forbidden. No guard bypass or unrelated process termination.
