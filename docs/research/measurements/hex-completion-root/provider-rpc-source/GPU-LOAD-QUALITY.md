# GPU load quality at the actual admission seam

AC-T7.4.1. The original intended pure control failed0/1 at real
gpu_facts -> Facts::node -> attachment offers -> bind, selecting GPU0 after
Unread. `UNREAD-ORIGINAL-RED.md` retains the exact source/body, raw log/binary
hashes, admitted scope and resolved delayed-v5 first-write fence. No native GPU
reproduction or effect is claimed.

The narrow remedy makes the internal `GpuLoad.busy_per_mille` and
`memory_used` independent Options. Default means unknown; native read failure or
missing sample remains None, while a known zero remains Some(0). Usable memory
and the other counter survive independent failure. Invalid busy units do not
become a clamped valid sample. Device::gpu is explicitly the simulator's idle
constructor and supplies Some(0); a native observation does not use that
constructor to fill counters.

Fresh GPU matching requires both observed counters, and memory_free is optional.
The single existing picker ranks only known readings; unknown earns no idle
credit. CPU-only asks need no GPU readings. Original held identities and immutable
allocations are unaffected: recovery's exact-device validation continues to use
the persisted identity/model/total, without a load-based picker or new holder.
Known memory used above total is retained; free is floored at zero without
rewriting the observed pressure.

Old numeric serde values decode as Some; absent/null counters decode as None.
Existing DeviceInfo, Capacity and native PhysicalReport/wire shapes are unchanged.
The peer offer's existing serialized GpuLoad now carries explicit null for unknown
readings; ROOT owns any external compatibility/integration handling. Root's
`hex-sim/tests/needs.rs` has one numeric GpuLoad literal needing Some wrappers;
that file is outside L1's narrow classes-literal grant for these new edits.

The original targeted control is kept with only Option assertion/input syntax
and explicit independent-None checks. A bounded new Index control holds an exact
GPU, loses telemetry on both GPUs, verifies the original holder survives and
fresh GPU admission refuses, verifies CPU progress, then releases the original
assignment and restores a known sample. These source controls are UNRUN at this
checkpoint; green will name its exact source/cohort and selected counts.

No resource/performance/native leak claim follows. Root owns real provider
effects, forbidden-grant/mutation, native held recovery, seven-target/full checks
and R01/R14 allocation profiling. The source adds no inventory cache, provider,
holder ledger or picker. Split-process Service must still consume the native
physical report through the existing runtime RPC; that consumer is the next
required integration increment.
