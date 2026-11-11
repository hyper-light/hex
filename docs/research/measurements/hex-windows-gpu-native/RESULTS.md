# AC-T7.3.1: actual Windows AMD telemetry

ROOT coordinator01a1aa77778471339bea3beff90d6bbd. Current policy Shards33df95b0,
R01–R16 rulesSHA256df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Exact source9190738c332a442c91719aaa7857f9c3673a2769; native Windowsx86_64
Jupiter, admitted1CPU4GiB12GiB/privatetarget/cache, BelowNormal priority.

Actual hardware test:1 passed,0 failed,0 ignored, no skipped device. AMD Radeon(TM)
8060S Graphics at PCI0000:c4:00.0; ADL driver25.20.32.06 reports4294967296memory
bytes and two samples of busy0%, used82837504bytes. The independent OS query
names the same model/PNP device; WMI's32bitAdapterRAM is not used for capacity.
The Rust allocator's existing sampling assertion passed; native driver/kernel
allocation and leak costs were not measured.

Cargo323604 and localCLI58418 waited/reaped, private temp0. The independent
collector rehashed all1848post-source files against the exact signedGit archive,
verified rawstdout/stderr hashes and actual non-skip counts. Full commands,
PIDs, platform/toolchain and hashes are in result.json/receipt.json. No HCS
system, VM or container was created by this telemetry check.

Original preparationv1 stopped before Cargo at a stale source-hash receipt copied
from the Windows lane: its marker test leaf differs in the combined API source.
The corrected v2driver validates its own complete immutable archive/source hashes;
production bytes and hardware test did not change to make it pass. First independent
collector also failed at PowerShell foreach token spacing before any reads; v2
corrects only that syntax. Original failed logs remain in ROOTprivatecache, not
reported as native checks. No evidence/source deletion.

This is real GPU inventory/sampling on Windows, not assignment, DirectX/CUDA/ROCm
compute, exclusive-access, container/VM/recovery/cleanup validation, WindowsARM64
native proof, full portability, or comparative performance. Those remain owed.
