# Two bounded controls on the frozen f3 consumer

AC-T7.4.1, R09/R10. Implementer L1; full policy read at current
`0c598386ca1f7c65c6e0634dba77983e4abffff2`, rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Production is the unchanged signed `f3f99e71304a780e2a75fd143895396dbd8f2d6a`.
These test-only additions inspect concrete legal inputs through the SAME Index.
They are **source-first, unrun intended-red controls**. Neither observation is an
executed defect, and no production remedy or native effect is included.

`a_prepared_gpu_backing_does_not_hide_an_eligible_native_oci_route` supplies one
native Linux OCI route and one same-image ShardsDedicated GPU route whose exact
observer is Prepared. Both masks name the SAME observed GPU; the typed API permits
these distinct RouteKeys and backing topology. Only the native route is currently
eligible. The original payload asks for one GPU and explicit Linux/amd64. Expect
CPU progress, GPU admission through Native, and the same free resources after
take/give. The source hypothesis is that `Routes::chosen` rejects multiple platform
matches before checking current grant/physical eligibility. No first-offer fallback
or second picker is proposed by this control.

`a_prepared_snapshot_cannot_roll_back_a_granted_backing_or_its_claims` initializes
a Prepared exact GPU backing, grants it through the existing Index, then delivers
the original delayed Prepared snapshot. Expect typed refresh refusal and unchanged
host resources/physical reservation; repeated grant must retain the exact tuple.
The source hypothesis is that Granted-to-Prepared is absent from the rollback
guard, allowing a stale snapshot to erase the debit/holder. A capacity response can
be delayed across a local grant in the split scheduler/native-owner topology; it
is not a native release receipt. No phase-policy remedy is included before result.

The earlier pending-GPU refresh/handoff control in f3 remains unchanged and unrun.
The original three route reds remain frozen in the earlier `40f+1740` cohort, not
relabelled as these results. Scoped installed-toolchain rustfmt and diff checks
passed. ROOT is composing the matched source/literal cohort before focused light
execution; there is no new dispatch, local Cargo/build/test or native campaign.
