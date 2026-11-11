# Exact assignment on borrowed Windows volumes

AC-T7.4.1 / T-T7.1.2. Source-first continuation after e75121e4. ROOT owns real
HCS/GPU effects, native fault/mutation proof, all-platform integration and gates.
Current Ada instruction requires valid devices on Windows/Linux amd64 and arm64
and macOS arm64. An OS-based refusal does not complete that feature.

## Contract and sources

The existing HCS adapter already distinguishes an owned scratch folder from the
caller's mounted root volume. OCI [v1.2.1 root configuration](https://github.com/opencontainers/runtime-spec/blob/v1.2.1/config.md#root)
requires a Windows volume GUID root, and Microsoft's [HCS Storage schema](https://learn.microsoft.com/en-us/virtualization/api/hcs/schemareference#storage)
accepts the scratch path. The existing DeviceInstance renderer and its pinned
hcsshim7d4e89bc source record remain unchanged.

The same native BoxRecord now records typed OwnedFolder/BorrowedVolume ownership
and Created/Pending native creation. OwnedFolder and Created are legacy defaults
and omitted from JSON, preserving empty legacy bytes and MAX_RECORD. The complete
new fields share the already-derived MAX_CONFIG_BYTES allowance; checked total
record_bound remains MAX_RECORD + MAX_CONFIG_BYTES. No bound is increased.
Borrowed records cannot own created paths. Recovery checks ownership and the exact
borrowed root against the current spec before native adoption, in addition to the
existing exact DeviceId/LocationPath/catalog/system-id check.

Borrowed preparation writes its existing marker before HCS create, using the
caller volume unchanged. It performs no layer activation, preparation, deactivation
or deletion of borrowed storage. Explicit cleanup removes only Hex's marker/next
after physical termination or an owned terminal failed-create result. A wrong
ownership record refuses without changing durable bytes. Assigned EndsWithNode
boxes also retain their marker when dropped; closing a system handle is not a
physical-removal witness.

Microsoft's [create reference](https://learn.microsoft.com/en-us/virtualization/api/hcs/reference/hcscreatecomputesystem)
distinguishes starting an operation from its final result. Its [wait reference](https://learn.microsoft.com/en-us/virtualization/api/hcs/reference/hcswaitforoperationresult)
defines timeout as incomplete. The [cancel reference](https://learn.microsoft.com/en-us/virtualization/api/hcs/reference/hcscanceloperation)
currently documents E_NOTIMPL; asking to cancel therefore never proves cancellation.
The [close reference](https://learn.microsoft.com/en-us/virtualization/api/hcs/reference/hcscloseoperation)
describes releasing tracking resources, not physical-system absence.

Only the assigned-create section of the existing sys adapter is extended. It uses
the already-loaded create/wait/cancel/close calls and existing patience, retains
its operation, system and fallibly reserved stable UTF-16 inputs, and closes the
operation once only on a terminal result. Explicit delete makes one zero-patience
terminal observation; no periodic task, wait inflation, new OS import, duplicate
holder ledger or global pending registry is added. Cancellation refusal remains
typed and retained. The existing OciBox in Holding owns a pending create even
when Runtime.create returns a typed failure. Known preparation failures have a
separate cleanup state and never open a pre-existing same-textual-id system.

Pending markers survive recovery even if opening that ID currently finds nothing.
After a process crash the operation handle cannot be reconstructed from disk. A
live system can be ended by its exact recorded ID; an absent Pending system stays
uncertain until terminal cancellation/absence is established. This is deliberately
not presented as proof that every native failure is already recoverable.

The initial8a29 Drop used process-lifetime retention. Its actual Clippy check
refused both mem_forget calls; this remained an implementation gap despite its
16 passing native selected units. No waiver was added. The successor replaces
that mechanism with the same operation owner's native completion receipt, as
[completion-ownership.md](completion-ownership.md) records. An incomplete create
remains owned until its actual callback, including after Runtime drops. Its
terminal callback can close and release the operation/System/input allocation;
an unfinished native operation still retains its bounded resources and Pending
marker. ROOT must validate and measure the actual HCS failure paths. No source
observation here is called a reproduced native bug.

## Focused evidence and remaining work

The original ce99413f reclaim fixture actually failed0/1 with one forbidden
cleanup callback; e75121e4 passed that unchanged case and its map-lock/receipt
fixture. The latter run actually passed9 total (eight assigned Windows plus one
existing shared Catalog filter match), none failed/ignored,56 filtered. Runtime
Clippy101 found only items_after_test_module; moving the module to the end fixes
its cause. Original raw files remain under reclaim-red-v3/reclaim-green-v4.

New source pairs actual borrowed scratch preparation/marker bytes with counted
cleanup callbacks and wrong ownership; pending-marker recovery/end with absent
versus terminal observations; fake existing HCS calls with retained inputs,
cancellation refusal and one terminal close; actual Runtime/Holding/capacity
callbacks over that fake operation with overlap refusal, unchanged pending marker,
an independent ordinary empty Job, terminal cleanup and holder release. These are
source-first, not yet executed at this checkpoint. No HCS, GPU, child program,
keeper, VM, WMI, hardware or native campaign is launched by those selected tests.

Exact57dc6540 subsequently ran HCS50 passed/0 failed/0 ignored/282 filtered and
core native all-target Clippy0. Windows crosscompile failed101 before Windows
tests: the new enum collided with the existing CreateOperation FFI type alias.
The15 compiler diagnostics are the naming collision and its cascades, retained
unmodified as light-v5. The successor renames only the new state enum to
CreateOperationState. Core HCS bytes/dependencies are unchanged.

The successor also applies the existing cleanup system-identity check when claims
remain declared but native binding metadata is missing. Its counted cleanup
callback fixture requires zero native end calls, unchanged marker/sibling bytes,
and the retained holder when that record names a different system. This is a
feature-contract regression; it is not a claimed native HCS bug reproduction.

R01–R07/R12/R13: typed state/ownership at the existing seams; fallible temporary
storage and checked derived metadata bound; no shared parser/manager, syscall
import or production limit change. R02/R03: sources above and user-specified
ownership contract, not a measured tuning number. R09/R10: callback red/green
separately attributed; original failures and bytes preserved. R08/R14: no speed,
allocation-free, hardware, leak-free or 4–5x claim; ROOT owns comparisons/profiles.
R11/R15: focused private1CPU/4GiB/12GiB nice10/jobs1/threads1 only; all real Windows
HCS/GPU effects, fault cleanup, supported CPU/OS/device combinations, seven-target
and full gates remain ROOT-owed. R16: local30GiB floor checked before transfers,
all original sources/archives/targets/evidence preserved, only owned handles used.
