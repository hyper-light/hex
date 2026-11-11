# Current Creating/history/resource fixture port

Source-only checkpoint `42f2c8031365f2e2f6297a2936838162039b827f` has exact parent
`d1ed4e1cf54d8d0a22da931d55832e4b38b202c9`. It changes only
`crates/hex-cli/tests/completion_backends.rs`, through 15 selected hunks
(610 insertions, one import-line replacement). The parent and resulting leaf
hashes, individual hunks and unchanged-region hashes are recorded in
`current-creating-port-source.json`.

The source was ported from the existing `c18e4482` instrumentation as carried
unchanged in `b0a168a6`. It was applied as selected diff hunks onto the current
leaf, not by replacing it with an old blob. Older peer-barrier and factory-pin
assertion hunks were omitted. Existing `BackingPhase`/`DeviceHolder` imports,
Box-versus-Backing handling, EOF readers, one exec deadline, original oracle
callbacks and the entire test-case tail are preserved. The latter includes the
three pipe controls ROOT already ran at `7022312e`; none was repeated here.

## Producer semantics

The existing notification observer arms before scheduler Submit. Linux uses the
existing safe rustix inotify seam, one instance for the private Store root and
its shards, plus the existing owned stop pipe. Other targets use the already
source-pinned notification helper protocol. There is no periodic scan: bounded
Store scans occur on initial readiness and change notifications.

The observer retains bytes only from a canonical `runtime_record` path with a
matching class, actual `Phase::Creating`, `init == None` and an actual native
scope. The result carries the original bytes, size and SHA256, together with the
128-bit scope and immutable scope/spec commitment. The matching BoxId is selected
after Submit returns. A missed window or observer error produces an explicit
unobserved Creating entry with a reason and no invented record or scope fields.

The separate binding observation reads `Phase::Ready`. Both restart observations
also name their actual Ready phase. History uses the existing oracle's ordered
stage census: binding, creating, scheduler-recovery, runtime-recovery. That array
order is not claimed as a clock trace. Raw phase/init bytes distinguish the
independently captured Creating window from the Ready origin. A Ready read is
never relabeled as Creating; a miss cannot pass the complete phase oracle.

The resource snapshots query the same backend's Capacity, PhysicalReport and
list RPCs. They preserve actual typed Box/Backing holders and original allocation
data. These API observations do not prove driver restoration or whole-scope
termination. A separately supplied native resource producer must provide those
facts; its absence yields `observed: false` with an explicit reason. No holder,
inventory, manager or native resource producer is created by this fixture.

The before snapshot is taken after service readiness and before Submit. Scope
history is persisted after initial observations and each restart. The after
snapshot is taken after the existing cleanup observer succeeds, while owned IDs
and history are still available, before clearing them and stopping services.
The Creating observer is stopped/joined before ordinary cleanup on failure too.
Failures retain uncertainty; source review does not establish a timed filesystem
read, worker join, native termination or successful cleanup.

## Required ROOT integration inputs

This scoped patch does not change `prepare.py`, `run.py`, helpers, Cargo or gate
code. The existing `c18e4482` contracts must be supplied in ROOT's matched cohort:

- Existing `record_root` must identify the private Store under the fixture scope.
  Existing `wait_ms` and `byte_limit` remain positive caller inputs. New-to-this-
  leaf inputs `creating_entry_limit`, `creating_event_limit` and
  `creating_notification_bytes` must be explicit positive integers. There are no
  defaults or larger caps. The event limit applies to each notification drain;
  the existing observer absolute deadline bounds successive waits.
- On non-Linux targets, `creating_notifications` supplies the pinned executable
  `{path, sha256, args}` for the existing ready/changed line protocol. Relevant
  source-pinned helpers are `creating_events.py` and `creating_events.ps1` at
  `c18e4482c6cf6f5482f20d124c4d8a52d5a8f671`; exact hashes are in the receipt.
  They were not copied, changed or executed here. Linux needs neither helper.
- Optional `native_resources` is the existing pinned host-command shape. Its
  argv can use `@PHASE@` for `before`/`after`. A claimed observation must use
  `hex-completion-native-resources-2`, match source/backend/phase and the native
  authority, and carry driver bindings, exact terminal scope/commitment receipts,
  owned native processes and artifacts. Missing facts remain unverified. The
  fixture observes BoxRecords; it does not invent a PoolRecord observer or turn
  a removed Box claim into release of a native backing.
- ROOT's runner must set these output paths under its retained run directory
  (the code below is the existing `c18e4482` environment contract, not an executed
  command):

```python
env.update(
    HEX_COMPLETION_RESOURCES_BEFORE=str(root / "resources-before.json"),
    HEX_COMPLETION_RESOURCES_AFTER=str(root / "resources-after.json"),
    HEX_COMPLETION_SCOPE_HISTORY=str(root / "scope-history.json"),
    HEX_COMPLETION_SCOPE_EVIDENCE=str(root / "scope-records"),
)
```

The history file and raw scope-record directory must have the same parent.
ROOT's runner/gate must retain and hash the snapshots, history and referenced raw
records. Existing `c18e4482` prepare/run/oracle sources and hashes identify these
interfaces; this patch does not import their other changes or register checks.

## Results and remaining work

Only source comparison, hashing, signed Git operations and whitespace review
were performed. The observer block and both resource/history helper sections are
byte-identical to `c18e4482` and `b0a168a6`. Protected current EOF, exec deadline,
oracle, holder and test-tail sections are byte-identical to the exact parent.

Tests, Cargo/Clippy, VMM API requests and native/VM/GPU executions: **zero**.
ROOT owns the matched compile and review of the actual observer code. No current
native acceptance, Creating-window capture, physical resource restoration,
terminal-scope, renderer or whole-workload EOF coverage is claimed. Earlier
`7022312e` pure passes remain attributed to that selective cohort.
