# Pool Store and Turns shell checkpoint

L3 implementer, ca3/df2; AC-T7.4.1; SOURCE ONLY, no build/test/native dispatch. Dependency: core d83a1eb097088b2b14033f6159f11cf40c0da22d +27f6c910 invariant successor, API73547/ROOT3d. The Recorded base for the next transaction consumer is immutable ROOT6e83bcc5eb35b9bc02a1b17936e72d4a41556462 plus signed f58fe9cd projection. No Root CLI/manifests/vendor/DTO code is changed here.

Store exact pub(crate) signatures:

- prepare_pool(pool:[u8;32],execution:Execution,cost:RuntimeCost,devices:&[DeviceId])->Result<PoolRecord,Refusal>
- read_pool(pool:&[u8;32])->Result<Option<PoolRecord>,Refusal>
- grant_pool(grant:&BackingGrant)->Result<PoolRecord,Refusal>
- put_pool(record:&PoolRecord)->Result<(),Refusal>
- remove_pool(key:&BackingKey)->Result<(),Refusal>
- all_pools()->Result<Vec<PoolRecord>,Refusal>

The SAME Store uses its private .pools namespace, full fixed configuration hash filenames, the SAME existing Turns, and one shared fresh_scope() OS fill for Box/Pool. Initial preparation holds the real configuration key before there is any generation to name. It refuses an unreadable existing row, reuses an exactly matching row without regenerating scope, and rejects different configuration/claims. An exclusive create_new name reserves absence; successful initialization then publishes complete Prepared through the SAME scoped durable publication helper used by native-scoped boxes. A partial reservation/publication remains uncertainty and has no grant/native-effect permission. There is no absence-driven reclamation. Updates preserve exact key/identity/phase through PoolRecord validation. Removal requires the SAME generation's Released row and leaves any replacement untouched.

Scoped publication encodes and fallibly prepares paths first, writes/flushed a temporary file, then atomically publishes it; Windows uses the already reviewed MoveFileExW write-through helper. Unix flushes the actual parent directory (including pool namespace); the fresh Unix namespace's parent is also synced. Windows write-through is NOT claimed as a live power-loss proof. Existing unscoped Box writes retain their old code. Primary native durability basis and proof limits are in native-scope-producer.md and macos-managed-source.md. No new unsafe/API/dependency is introduced.

Turns stores OwnerKey in the SAME fallibly reserved sorted vector. Existing box methods remain wrappers over its single take/try-hold algorithm. New try_take_pool(key,devices), try_take_pool_configuration(pool,devices), take_pool(key), take_pool_configuration(pool) serialize all generations of the SAME configuration and conflict with box operations on the SAME selected physical IDs. Active method holds are ephemeral exclusion only, not reservation/inventory state. The same learned patience/cadence/bound is retained. Release matches its own held receipt, and native effects never keep this map lock.

All pool file reads reserve the existing MAX_RECORD_BYTES+one limit BEFORE reading and reject oversized/truncated/unreadable/mismatched rows. Namespace scans are counted: configured slot budget, first publications admitted concurrently by that same Turns bound, and their at-most-one temporary file (3*TURNS entries). This scan bound is not a native/resource grant allowance. Backends must reserve in the SAME bounded native configuration owner before exposing preparation; Index alone grants resource allocation. Existing serde JSON parse/diagnostic allocation gaps remain explicitly unproved, not allocation-safe completion.

Three new focused controls are supplied UNRUN: preparation retry cannot call the fresh-scope source and readback keeps the original durable record; real private-file lifecycle refuses old-grant/old-cleanup after a deterministic same-pool replacement; box and two pool generations share one physical device/turn exclusion. Tests inject only fresh identity bytes through the private helper while executing actual Store/Turns/file code, never a VM/native controller stand-in. No original-source runtime bug/native pass is claimed.

Next required composition: Books.prepare_pool pure publication under canonical method/device turns and all Box/Pool claim conflict validation; Books.with_pool_transaction(fullKey,devices,callback) and SAME concrete transaction/NativeRecord native callbacks; Node/Recorded provider+DeviceHolder Box|Backing join; real Windows/macOS/Linux native owner consumers; L2 exact boot receipt; ROOT wire/CLI/native/fault/mutation/profile/portability/full gates. These Store helpers alone do not advertise initialization or complete that integration. Cold Mac cleanup and aggregate CPU enforcement remain separate necessary work.
