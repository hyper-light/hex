# Recovery payload guard: coordinator validation

ROOT integrated the reviewed fixtures and guard as
`f3f19c07e658dcd8097bba3dbb2bcbc484d9d8d7`. The [record](coordinator/record.json)
contains exact commands, source and log hashes, original failures, all R01–R16
items and remaining checks. The implementing agent's preceding record remains
revision-specific; its formerly owed ROOT runs are resolved here individually.

The combined native macOS node suite passed **139 tests**. Native node Clippy
passed on macOS and Windows; the native Windows suite passed **133 tests**.
Removing only the payload guard independently makes the start-intent and
equal-start specification cases fail for their intended reasons. Each case
cleans its simulated boxes before assertion; restoring the exact bytes passes.

ROOT also exercised actual OCI boxes inside two owned ARM Linux guests. With
the old host, conflicting owner `start=false` and runner `start=true` records
boot successfully and the actual init becomes `Running`. With the fixed host,
boot refuses as `Damaged`, the actual box remains `Created`, and durable record
bytes stay unchanged. Both cases demonstrate sibling work after the observation,
matching-record recovery, explicit box deletion, an empty physical baseline and
the exact owned guest's `Stopped` outcome. Independent readback joins all 133
ledger rows, namespace/key identity, config, artifacts, receipts and backend reaps.

The host was rebuilt at f3; guest, static Linux binary, probe and initrd were
intentionally retained from 8767. All eight wire inputs are byte-identical.
Shards and kernel pins did not change. These are functional OCI results, with
no performance, exploit, Firecracker, WHP or device-attachment claim. Windows ARM
remains paused and unverified. Final Linux gates/root lab and the full scheduler
goal remain open.

The `coordinator/` drivers are the exact executed/collection sources. Their cache
paths are recorded inputs, not installed product requirements. The native verifier
uses the repository's `bench/sched/runtime-pools/census/verify_cold_census.py` and
`verify_census.py`, whose bytes were independently matched to its retained cache
copies; hashes are in the record. Preserve original evidence before selecting
fresh private source, proof and target paths for another invocation. Caller
resource and outer cancellation budgets are explicit fixture inputs.
