# Assigned create completion ownership

Source-first successor to8a29dfd30fa9131a94a977e7c87a8b616fed77d2.
AC-T7.4.1 / T-T7.1.2. Scope remains the assigned create section in existing
windows/oci/sys.rs and its existing OciBox/Holding lifecycle, with focused light
checks only. No new OS import/call, thread, manager, cache or holder authority.

Exact8a29 actually passed16 selected native Windows units (15 assigned plus one
existing Catalog filter match),0 failed/ignored,56 filtered. Its full Runtime
Clippy failed101 only on two mem_forget calls. Preserve that raw failure and the
earlier57 name-collision failure; neither becomes a passing gate.

Microsoft's [operation completion sample, pinnedb9a2dc8896baacb401ab7dfef2e7e06e2216e4da](https://github.com/MicrosoftDocs/Virtualization-Documentation/blob/b9a2dc8896baacb401ab7dfef2e7e06e2216e4da/virtualization/api/hcs/Reference/OperationCompletionSample.md)
explicitly closes an operation from its terminal callback. [HcsCreateOperation](https://learn.microsoft.com/en-us/virtualization/api/hcs/reference/hcscreateoperation)
defines that callback as completion, not a cancellation receipt. Queries must not
close a registered operation before its callback: that could discard a queued
callback and its context. The existing wait call with zero patience obtains the
result in the callback; no new GetOperationResult import is added.

One per-create context owns the operation, System, stable UTF-16 input vectors
and typed NotStarted/Active/Terminal/Released result. It has exactly two physical
receipts: existing PendingCreate in OciBox, and HCS's one registered completion.
Only that native lifetime is shared; Holding remains the sole claim ledger.

Stable Rust1.99.1 [Arc::try_new remains experimental](https://doc.rust-lang.org/1.99.1/std/sync/struct.Arc.html#method.try_new).
An additional infallible Arc allocation would not satisfy R01. The adapter instead
fallibly allocates exactly one concrete context with Layout::new and typed null
refusal. It is not a reusable smart-pointer abstraction. Its checked atomic
receipt count starts at2 because there are two physical owners, never clones or
wraps, and its last receipt drops/deallocates that exact layout. UTF-16 vectors
reserve their checked encoded lengths plus the required NUL before native create.
Per-create memory is that one concrete layout plus its admitted inputs; outstanding
creates remain bounded by the existing Holding admission/MAX_BOXES and durable
claims. No measured performance or leak-free claim is made.

The callback sets its terminal fact and uses try_lock. If it arrives inline before
CreateComputeSystem returns, or while a query owns the context, it never waits on
that thread. The live runtime owner observes completion after the call returns,
or closes it at final context drop. Otherwise the callback observes the result,
closes the operation once, and releases its native receipt. A query may consume
a terminal result/System first, but leaves the registered operation/context alive
for the callback. Dropping Runtime releases only its own receipt; a later native
completion still owns the context and releases it. A synchronous refusal before
the operation starts explicitly closes it and returns the unused callback receipt.
These transitions do not infer physical absence from cancellation or handle drop.

The focused fake-call controls cover query-before-callback, cancellation refusal,
inline callback while the creator owns the mutex, Runtime drop before callback,
synchronous refusal, and one exact close/context deallocation. The existing actual
Runtime/capacity/holder fixture retains overlap refusal and sibling progress before
terminal failed-create cleanup. Its HCS calls are synthetic; only ordinary empty
Jobs/private files are real. No HCS/container/GPU/VM/WMI/keeper campaign is run.

New callback transitions are source-first at this checkpoint. ROOT owes real HCS
callback ordering, synchronous-error/late-callback behavior, native fault/mutation
proof, physical hardware effects, seven-target/all-device integration, costs and
full gates. A never-completing HCS operation retains its native completion receipt
and Pending marker; no limit/wait is enlarged and no capacity is published free.

Exact4d215cce Windows cross check type-checked the callback successor, then
failed101 solely because Rust1.99.1 deprecated Atomic::fetch_update in favor of
try_update. Its original light-v8 log is retained. The successor uses the
compiler-specified name with the same checked subtraction; no lint waiver,
ownership change, larger wait or limit is introduced. Native callback controls
and final selected Clippy/CLI checks remain pending at this source signature.
