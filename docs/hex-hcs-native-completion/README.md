# ROOT-only actual HCS create completion fixture

AC-T7.4.1 / T-T7.1.2. Branch cp/t7-hcs-native-completion starts exactly at
bd80d9c42d656c5ac2445054b4042cee1026e225, preserving the final Windows source
edffbd22e0d45e2237875949aa2fb47c23dfca2a and its complete raw evidence branch.
The only sys.rs change is one cfg(all(test,windows)) path registration. All
production functions/imports/state are unchanged. No extra fake controls, thread,
manager, production metric or dependency is added.

Initial full policy readback: Shards33df95b08ad8f95bddeaa1c48acf0c64386e0eac,
rules SHA256df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3.
Implementer01a1c643 owns NEW assigned_native_tests.rs, that one registration and
this docs/hex-hcs-native-completion directory only. ROOT acknowledged the exact
scope and test-only self-handle counter before edits. Hex CLAUDE/HANDOFF and
current Shards AGENTS/rules/evidence/CLAUDE plus g121 handoff were fully read.
After resume, the full current a920c85b policy/plan and handoffs were read again;
the rules hash is unchanged. Exact file hashes/roles/paths are in policy-resume.json.
No children/goals/plans/campaigns are used.

## Required ROOT inputs

The test is ignored by default. Missing input fails; it never silently skips or
supplies a default. ROOT alone invokes it, with an external guard that covers the
explicit completion budget and the existing bounded native cleanup calls.

| Variable | Required ownership/meaning |
| --- | --- |
|HEX_HCS_NATIVE_ID|Fresh private valid BoxId. Preflight must find no system by this ID; an existing one is never terminated.|
|HEX_HCS_NATIVE_ROOT|Absolute existing private evidence directory owned exclusively by ROOT. A fresh `<id>.completion` directory is required.|
|HEX_HCS_NATIVE_CONFIG|Absolute regular UTF-8 HCS JSON file canonically beneath that root. Read ceiling is existing MAX_CONFIG_BYTES.|
|HEX_HCS_NATIVE_STORAGE|Exact already-prepared private storage path ROOT owns, equal to Container.Storage.Path and currently a directory. The fixture never prepares, deactivates or deletes storage.|
|HEX_HCS_NATIVE_BUDGET_MS|Explicit positive finite DWORD milliseconds; no zero/INFINITE sentinel or default.|

ROOT must supply a Container document, not VirtualMachine/HostedSystem, and
ShouldTerminateOnLastHandleClosed=true. The document is passed unchanged; it is
retained privately before effects and never printed. Required caller ownership
of any mounted volume/layers is a precondition; the fixture adds no host probe,
installer, mount operation or hardware/configuration mutation.

ROOT's invocation after its own native setup/admission is:

```text
cargo test -p hex-runtime --lib --locked windows::oci::sys::assigned_native_tests::assigned_native_create_completion_and_owned_cleanup -- --exact --ignored --nocapture --test-threads=1
```

Implementation agents must never execute this command. Their check is Windows
cross-target Clippy for the test source only, on an approved private light runner.

## Actual facts the fixture will record

The real private create_system_owned is called once with Duration::ZERO. This is
the boundary input, not a guarantee that Pending will be observed. Immediate
terminal and Pending are recorded separately. If needed, one existing native
operation wait uses ROOT's positive input budget; there is no poll, sleep, retry
or guessed deadline. Cancellation's typed result is retained, never treated as
completion. No production wait/limit is changed. The cleanup API's existing
operation may wait once and again after cancellation; ROOT's outer guard must
cover those existing calls as well.

After a terminal result, only the returned system or the exact private ID proven
absent before create can be cleaned up. A native cleanup refusal is uncertainty,
not success. The test requires an actual own system observation and later actual
absence, observed native callback completion, released callback receipt, no held
operation/inputs/System in the context, exactly one runtime receipt before dropping
it, and no replayed terminal result. If callback delivery is not observed at the
required cut, it reports uncertainty rather than sleeping for a guessed interval.

The private report retains stage/result/HRESULT/cleanup facts and the known ID;
HCS documents, private paths, environment values and HCS error-document content
are not printed. Pending outcomes keep that report and the native completion
owner; they fail this fixture and are never promoted to physical absence or free
capacity. Caller storage and original evidence remain untouched.

Self-process handle counts are taken at entry, after the necessary absent-ID
preflight and after cleanup. They are aggregate observations of this test process,
not a global OS snapshot or attribution of every handle. No arbitrary permitted
delta, allocator-free claim or leak-free assertion is invented. Context close and
last-receipt observations plus the actual physical absence witness are distinct
from those counts; allocator/native resource profiling remains ROOT-owed.

[GetProcessHandleCount](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesshandlecount)
takes HANDLE and writable DWORD output and returns BOOL. [GetCurrentProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getcurrentprocess)
returns a pseudo handle which is not closed. These two test-only calls were
explicitly approved. The [pinned Microsoft callback sample](https://github.com/MicrosoftDocs/Virtualization-Documentation/blob/b9a2dc8896baacb401ab7dfef2e7e06e2216e4da/virtualization/api/hcs/Reference/OperationCompletionSample.md)
permits terminal callback closure. The existing callback/ownership implementation
is reused exactly, with no replacement function pointers or synthetic HCS calls.

## Evidence status

Source-first479893721cad81bc89b8193540ebef5d9bd74bcb actually failed Windows
test compilation with exit101/E0308: the new leaf passed String rather than &str
to BoxId::new. Signed successor eefaa97dfefab0aa2228dbafcc61d71431ae7aee changes
only that call in the leaf; all production functions remain unchanged.

That exact successor passed this compile-only command on private d132, using the
already installed x86_64 Windows std and Rust1.99.1:

```text
nice -n 10 cargo clippy -p hex-runtime --tests --locked --target x86_64-pc-windows-msvc -- -D warnings
```

Actual exit: **0**. One ignored fixture is compiled from source; **executed tests:
0, native invocations: 0**. No HCS/GPU/container/VM/keeper/WMI/native fault or
mutation campaign ran. Compilation does not prove native callback delivery,
close/allocator behavior, physical attachment, cancellation or cleanup.

| Retained run | Actual outcome | Original log SHA256 |
| --- | --- | --- |
|47989372/v1|Compiler101, E0308 in new test leaf|fe62006e132720c5376c32bd611e4777afb6078efdf99ab0a057fd17ae1da1ac|
|eefaa97d/v2|Windows test Clippy0; no execution|b9d2480d30c42230600e142bc73002f6ff9f0054e7d21d2ca51fc09d167b5c4f|

Final leaf SHA256 is64726bf2b70990706ef9faf8b699b1b19ae223b8b3f3cf6702e5ae157634909f;
sys.rs with registration ise7b7372f9397c616c947891f652eecbd8bbb4824c18fdf4e672b468934b658ab.
Stripping only the exact registration restores edff sys.rs byte-for-byte,
SHA256092a4ac23ca04169e8d3f4197b51ad8069b189af6a8f6d22c18992097fb455b7.
The original failure and successful compile retain all14 original files in
raw-evidence.tar.gz, SHA256ed52112260bf8e5f2949fa4197e988774381fa1f89bfcae772ce4f107edb5753.
raw-index.json records each original size/hash; verify_evidence.py reads those
bytes and source identities without building or executing a test. Complete
commands, source archives, runner/toolchain, resource envelope and results are in
verification.json and the raw archive. ANSI/line endings are preserved.

Both runs used fresh private source/target/cache/temp paths, 1CPU/4GiB/12GiB,
nice10/jobs1/threads1 and no-sync. Exact Cargo children54028/54622 were reaped,
private temp entries0; local CLI32405/50538 exited1/0 respectively. No active jobs
or native systems belong to this scope. Remote sources/targets/caches and all
originals remain preserved; uncompressed collected originals are at
~/.cache/hex-lead/transfers/hcs-native-completion-01a1c643-originals-final.
The bounds/HCS/API/final-Windows refs remain unchanged. Local free space stayed
above30GiB before each bulk transfer/collection; no cleanup removed other data.

ROOT owns every actual invocation and all physical, callback-order, fault,
mutation, all-platform and full gate conclusions, plus profiling/benchmarks and
merges. ROOT's independent51-HCS/API-u64 and WindowsARM CLI integration checks
are not duplicated or attributed to this fixture. Its older API base does not
change production or claim a device-key authority restriction.

R01/R04/R06/R07/R13: typed outcomes and bounded ownership at the existing seam;
production code is unchanged. R02/R03/R05/R12: official SDK/async sources above,
caller budgets and existing read bounds, no new default/cache/manager/poll.
R09/R10: no native reproduction claim from compilation or prior fake tests.
R08/R14: no cost/leak-free/comparative claim; ROOT owns profiles/benchmarks.
R11/R15: compile-only scope, actual required platforms/hardware remain unverified
by this fixture until ROOT executes them. R16: private sources/targets/cache, local
30GiB floor and all prior raw/branches preserved; no unrelated process/data touched.
