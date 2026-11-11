# One managed report preflight

`hex_runtime::devices::check_managed_report(Option<&Catalog>, NativeRecord,
&ManagedLaunch, writer_present, absolute_deadline) -> Result<Instant, Refusal>`
is the shared pre-effect helper for Node and Recorded consumers. It validates
the original record, actual routed backend, Creating stage, absent init, canonical
scope, request/writer parity, complete catalog-selected PhysicalGpu set and finite
remaining deadline. The returned deadline is the minimum of caller and SDK budgets.
It reads SDK getters, never reparses argv or mistakes literal argument values for
native flags. The existing Node delegate calls this same implementation once.

CPU/Firecracker launches with no Shards report require no catalog and allocate no
catalog or selection vector. A GPU report requires Some(actual_catalog), rather
than constructing an empty authority for a legacy CPU-only Recorded. GPU selection uses a fallibly collected/sorted transient
index and exact full-set comparison. It never chooses another device or treats
metadata as a provider's current native correspondence. L2 retains that actual
initialized identity join; this guard cannot make an unverified LUID or catalog
entry into native authority. The helper changes no writer flags or FD ownership;
the native launcher remains responsible for its final owned duplication and SDK
suffix rendering. No wrapper owner, ledger, cache, polling or timer is introduced.

The existing unrun scope/writer-parity control from the Node checkpoint moved
without expectation changes to `devices::report_tests`; it remains UNRUN, not a
new pass or a repeated run. The whole combined helper and OS-specific carriage
still require joined compilation, focused source controls, actual descriptor
transfer/cleanup and guard-removal proof owned by ROOT. No build/test/native job
was dispatched by this lane. L3 owns the Recorded call site and platform carriage.

Policy0a6d91b3 / rulesdf2 applies. R01/R04/R06/R07/R13: fallible buffers, explicit
typed identity/refusal and one checked metadata boundary. R02/R03/R05/R12: reuse
the exact SDK report contract, canonical NativeRecord, immutable catalog and
existing deadlines; no new constants, framework or authority. R08/R14: no measured
performance/allocation claim. R09/R10: source-first required integration, with
unrun proof explicit. R11/R15/R16: shared cross-platform helper, implementer-only
scope, no native campaign, foreign changes or runner/credential/service actions.
