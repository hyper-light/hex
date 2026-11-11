# Preserved index-only recovery draft

This is a source-only checkpoint, not a validated production candidate. ROOT's
subsequent physical-identity contract requires the follow-up to distinguish
physical `(DeviceKind, key)` claims from synthetic slot claims. This intermediate
draft is retained rather than discarded; no build or test has executed it.

The draft serializes current `Index::holds` before `Told::Create`, temporarily
decodes exact indices during recovery, reserves them through the existing index,
stages recovery row edits until validation succeeds, and exposes
`Node::assigned_devices(Key) -> Option<Held>`. `Ran` carries no parallel `Held`.
Its index-only format is insufficient to detect same-shaped physical replacement
or retain a physical device through a harmless catalog reindex.

Original collision red remains signed as 44a2df3b/33638073: 0 passed, 1 failed,
158 filtered; log SHA256
`6cbaa37df4769f9a3a4471561eaff2b9c52f5c0585161f3fa4f4b1db9c9b8a52`.
The additional transient control at signed 90230466 executed against original
peer/index source, before this draft: 0 passed, 1 failed, 159 filtered. Its four
owned/runner-only memory/hot cases refused recovery and preserved all bytes;
repair/cleanup completed in each case before the final assertion. Log SHA256
`32d31800b3761033e9b8fadd0eabbaa444c6f46345f3de62dc245f0eb609fe5d`;
raw proof is retained in the private `device-recovery-impl-01a1c640-90230466-before-v1`
runner and local transfer directories. Neither red is a native GPU effect claim.

Policy: full Shards cbd39284937d024640c56db6f8d8ef2bd9b0ee24, rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Implementer 01a1c640e6ad7127bcf34212f474d37c; ROOT coordinator
01a1aa77778471339bea3beff90d6bbd. Current expanded ownership is Device metadata in
model.rs, durable-recovery sections in peer.rs/index.rs, the lane-only leaf, and
this evidence directory. All execution is confined to approved d132 private
1CPU/4GiB/12GiB light scopes, nice10/jobs1/threads1. ROOT owns registration,
integration, mutation/native/physical-attachment proof, gates and merges.
