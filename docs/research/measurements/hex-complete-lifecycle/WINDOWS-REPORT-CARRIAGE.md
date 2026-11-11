# The same native launcher's optional private report writer

AC-T7.4.1, source only. The exact SDK dependency is
`5ba9ca5022357807c7b70a9ffe31534475a4cc31`; ROOT has not yet accepted/vendored or
compiled that cohort. The shared guard is L1
`c480541b8469cecc449337e622e9dc50eb511f0e`, consumed by Recorded at
`1c936577`. The VMM's child-side Windows adoption is full Shards
`b128aaa5ca2e7a9da4d140f206610a952b1d57d5` (parent `1108260c`): it clears
inheritability before starting helpers and transfers the same owned writer only
through the named VMM grant. These dependencies are source attribution, not
native proof.

The canonical transaction checks the actual retained Box/Pool record, sole Store
claims, immutable request scope/full selectors, request/writer parity and the
minimum of the original deadlines before its native callback. Windows also calls
the same shared guard at its public native launch entry, using its existing
catalog, so direct embeddings do not bypass the check. CPU None does not need a
catalog selection vector or report writer.

The final Windows launcher creates a private same-process `DuplicateHandle`
copy of the owned report writer with inheritance enabled on that copy. It leaves
the API's original handle flags unchanged. The SDK renders its four report words
from this final duplicate's actual value; the prepared base arguments contain no
parent raw handle. The existing suspended `CreateProcessW` receives exactly its
three standard handles and this one copy through `HANDLE_LIST`, together with the
same atomic `JOB_LIST`. Both attribute arrays remain alive through attribute-list
destruction. After successful creation, both parent report copies close before
birth observation/publication; the child holds its own inherited copy. Neither
writer is handed to the passive keeper. No new Process row, waiter or ledger is
introduced. Allocation/argument preparation precedes creation of the native
controller. Existing unrelated stdio inheritance behavior is unchanged.

Alternatives considered: mutating the caller's original inheritable flag would
change shared caller state; serializing its raw value before a transfer would
name the wrong handle; duplicating into an already-running unbounded controller
would break the existing atomic job envelope. The private duplicate plus the
existing attribute list preserves the owned native launch order.

The primary contracts are Microsoft's [process inheritance](https://github.com/MicrosoftDocs/win32/blob/3aee48ac3f48e187d17bdfd56951741737331749/desktop-src/ProcThread/inheritance.md),
[DuplicateHandle](https://github.com/MicrosoftDocs/sdk-api/blob/f45aa8129cb14704dd80402ce580fcb04c3505db/sdk-api-src/content/handleapi/nf-handleapi-duplicatehandle.md),
and [UpdateProcThreadAttribute](https://github.com/MicrosoftDocs/sdk-api/blob/d43984d7c322f7d7d35704d6d1ad36dc3539703b/sdk-api-src/content/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute.md).
The first documents same-value inheritance; the second distinguishes a duplicate's
target-process value and inheritable flag; the third requires inheritable list
members and attribute values alive until list destruction. Exact source byte
hashes are in `windows-report-carriage-source.json`.

New source controls, all UNRUN by L3:

- Recorded report2: actual Store/granted pool and counted native callback; scope,
  missing/extra writer, partial/different selector sets refuse without callbacks
  or retained-byte changes; the exact set reaches the callback once with its
  original deadline. CPU None reaches the same callback without a catalog.
- Windows managed command-line1: literal CPU CRT golden remains unchanged with
  an empty suffix; exactly four explicit report words append afterwards.
- Windows sys report-copy1: a small pipe/handle control, no VM/job/controller;
  the rendered value is the actual private duplicate, original flags stay equal,
  and the same pipe reaches EOF after all owned writers close.

ROOT owns matched SDK/devices/consumer compilation, these targeted controls,
actual child/VMM-only inheritance and forbidden-effect observations, full native
GPU/scope cleanup, fault/mutation and resource/profiling gates. The Mac SCM_RIGHTS
consumer is separate unfinished source; no report or persistent-helper completion
is claimed here. No L3 build/test/native campaign was run for this checkpoint.
