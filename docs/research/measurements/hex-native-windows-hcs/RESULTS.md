# Assigned native Windows/HCS validation — ongoing

Coordinator `01a1aa77778471339bea3beff90d6bbd`; assigned adversarial tester
`01a1c8aa921b72c89b7272fba2926a9c`. Policy fully loaded at Shards
`0c598386ca1f7c65c6e0634dba77983e4abffff2`, rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
ROOT explicitly ACKed/released before the first edit or native dispatch.
Ownership is only NEW files in this directory and NEW `native_windows_*` test
leaves in `crates/hex-cli/tests` / `crates/hex-gpu/tests`. Production fixes,
integration/main, audits and profiling/benchmarks remain with ROOT/L3/L1.
No new agent, goal or plan was created. Windows ARM VM remains paused.

Production source is exactly signed ROOT candidate
`2c8855b220a36c57685e456a7b8c98d8368f7899`, not main or final gated. Fixture-only
successors `085d24797518f388e936be485b75fc92c91f1f6f`, `ed6d3e54`, and
`985d7767232780f74ac95b2e2aa966130730b34d` retain that production cohort. New ROOT production cohorts are not silently substituted.

Current policy `0a6d91b3de0f10a8397868aa3794adab1e09afdc` was fully reread after context loss; all four binding policy files are byte-identical to the acknowledged `0c598386` revision (`policy-byte-equality.json`). The receipt identifies policy loading, not completion.

All native work is on Jupiter `f766f09294f2e17e3b8a631ffd4e6a4b`, admitted
1 CPU / 4 GiB / 12 GiB for builds/execution and the explicitly reviewed 32 GiB disk envelope for full-image preparation, BelowNormal, jobs1 / testthreads1, no-sync immutable
archives and unique source/target/cargo/temp directories. No laptop build/test,
Fedora, runner/driver/service/security change, installed helper or host DLL copy.

## Actual host GPU output

`hostv2` built the original ignored `completion_windows_compute` native x64 test,
Cargo exit0,45.85s, peak owned job commit1,328,148,480B. Its guard initially queried
active process accounting immediately after root-process signal and observed2;
it refused dispatch and retained that failure. Kill-on-close applied only to its
owned creation-time job. The corrected guard waits on a finite completion-port
notification and then independently queries accounting. Build-only residual
compiler children are explicitly terminated in their exact owned job after Cargo
returns; this is recorded, not presented as natural child exit.

`hostv3` executed the original binary, one selected test passed,0 failed/ignored,
0.24s. Exact OS-enumerated adapter-arrival `1ca` interface → KMT full LUID
`0000000000014c4d` → DXGI → explicit D3D12 hardware device, nodes1, flags0.
The real shader wrote one4096B native page: all1024 output words matched the CPU
oracle after a finite5000ms fence wait (wait0,value1,device healthy).
Binary SHA256 `4fdd12df17fbf9e97b8be3ea68fb22344294cbe2f71c8f36de14c8a94034f6e9`.
Raw stderr SHA256 `523b6b3409bb60197a275fe526266a1f50f26be4d17840de0d580169c1ea8d93`.

The60s whole-process guard observed natural exit0 and active job processes0;
peak job commit49,905,664B. KMT1/1 and event1/1 wrapper closes succeeded. These
wrapper counters and the COM RAII message do not establish complete driver/COM
leak attribution. The in-driver temp count1 was transient Add-Type compiler
storage: independent post-driver enumeration and collection found an empty owned
temp directory without deleting any artifact. Both host source trees'2305 files
match their exact Git archives byte-for-byte (`host-source-verification.json`).

## Real negative and mutation controls

`controlsv2`, original production/test source2c8855, actually ran three executable
cases. Wrong expected LUID14c4c refused at KMT identity against actual14c4d,
exit101, KMT1/1 closed, events0/0, no DXGI/D3D12 selection/fence/output.
In a private source only, removing precisely the embedded shader's required
output store completed a real GPU fence but failed the independent oracle:
index0 expected1, actual4294967295. Restoring exact original source bytes and
rebuilding produced a real1024-word green result. Every executable job ended
with activeAfter0 and no timeout. The original `native_tests=0` field in this
control driver is a counter defect; phase receipts show3 actual executable
invocations and2 real GPU dispatches. Its successor increments that field; raw
originals are never edited to conceal it.

Original fixture/transport failures are retained: sandbox denied the first local
Kiln socket connection (no remote effect); approved escalation connected normally.
`controlsv1` stopped before native work at PowerShell's reserved `$Host` variable.
`hcsv1` ran0 native HCS cases because its new fixture failed compilation at stale
inherited Entry.binding/DeviceHolder expressions plus an unfinalized hash value.
Only the NEW owned fixture was corrected; no production defect is claimed.

## HCS and full image prerequisites

Nano Server2025 layer1 exists with blank.vhdx and ntdll26100.1, but actual read-only
file checks prove gdi32.dll,dxgi.dll,d3d12.dll,d3dcompiler_47.dll absent. Per ROOT's
explicit steering, Nano selected-GPU application execution remains0; no repeated
loader failure is used as a Hex/GPU defect. `hcsv2` hit its original900s build deadline, exit1460, owned job reaped/active0;
its four recorded build PIDs were independently absent. The one source-identical
DEV/TEST debug0 diagnostic `hcsdiag1` also hit900s, exit1460, reaped/active0,
peak job commit1,105,162,240B, native tests0; its recorded four PIDs were absent.
Neither timeout establishes a PDB, linker or production cause.

The single approved existing-LLD diagnostic `hcsllvm1` built exact `ed6d3e54`,
archive `40cebb529fd46e59256feced0a67c4b5a1d9c050444858ff2930735dfca40157`.
It used installed Rust LLD SHA `53d7ea9474ddbb0f0eda351f2dace3259e77f1175537f64d321c232bc8d29b68`,
`RUSTFLAGS=-C linker-flavor=lld-link -C target-feature=+crt-static`, debug0;
this is a distinct linker/flags/binary cohort, not a production fix or a cause proof.
Build exit0,144 owned processes, peak commit1,069,109,248B; the guard explicitly
terminated remaining owned compiler children after Cargo exit, then reaped/active0.
Cargo emitted one `fresh:false` test/bin artifact with test profile and correct
manifest/source. Native canonical target identity matched the initially empty
owned target; PE machine was AMD64. All2314 source files had the same before,
after-build and after-execution manifest SHA
`4eccc885a65b2839ec45c17fd2aaecaf9e70bdd003d386d7f70353496b21da86`.
Test executable SHA `d459c4803f79aa238ccf53514fd76e1c92d32df4e9113f06b56298d7e1f7dd38`.

The first controller passed the extended `\\?\C:` canonical path through
`cmd.exe`; it exited1 with empty test stdout and “The system cannot find the path
specified.” Its raw `native_tests=1` is an attempted invocation, not actual test
execution: actual tests/HCS0. `execute_hcs_cpu.ps1` then verified the ordinary DOS
alias resolved to that same canonical fresh artifact, PE and hash, without rebuilding.
The actual `--list` output verified the exact CPU test exists. The native CPU HCS
case executed once under180s: fail2.12s, exit101, source unchanged, job reaped/active0,
peak12,414,976B. Create succeeded; start returned
`Os { op: "starting a program in a container", why: "Hex's launcher ended without replying" }`.
Raw stderr SHA `f8448cff2e03a3bf75a20feb2b84c5da383f50523a3cb2bdc0b76287f6814a4b`.

Independent exact cleanup at04:49:05Z: HCS ID `wd-hexdeviceuopdHC-cpu-recovery`
returned0x8037010e/null (absent), scratch and native marker were absent, and the
private state contained no box record. Keeper7828 with recorded native birth
134388458780649742 and every recorded guard/root PID were absent. The retained
keeper.owner and bundle config have exact raw hashes. **Evidence gap:** this
fixture's failure cleanup had already removed marker/store rows before external
collection, so their earlier bytes were not retained and cannot be reconstructed.
No persistence-before-effects or restart/recovery pass follows from this failure.

**Procedural failure retained:** the executor's initial pre-sign `git diff --no-index
--check` returned1 for the new-file difference, stopping signing; dispatch was not
gated on that exit. Exact driver SHA `28756a35587f0615f25fbea59cb5f73f15e3a461df0912a00c7f35e6b30a4fb3`
was signed unchanged afterward in `3ba99780`. It must not be described as signed
before dispatch. ROOT was notified immediately; subsequent dispatches explicitly
gate on successful signing. Original transport/command chronology is retained.

Microsoft's [GPU container requirements](https://learn.microsoft.com/en-us/virtualization/windowscontainers/deploy-containers/gpu-acceleration)
exclude Nano/Server Core for DirectX; the [base-image guidance](https://learn.microsoft.com/en-us/virtualization/windowscontainers/manage-containers/container-base-images)
names Windows Server as GPU capable. A real pinned Windows Server amd64 image
manifest was obtained and hash verified, not assumed available:
`mcr.microsoft.com/windows/server@sha256:a62109569303cfe0ab49b1618383615f0477cdc285529b98d9b7085902d0e745`,
OS26100.33822/win32k. Compressed layers total6,657,199,971B. Existing measured
source+target+cargo footprint is1,839,757,359B. The no-blob-file meter verified both compressed SHA256 digests and both
uncompressed diff_ids under the original390s/layer and850s outer budget.
Uncompressed tar extents were10,546,981,376B and5,583,841,280B; combined compressed
and tar payload22,788,022,627B. Adding measured source/target/cargo1,839,757,359B
made24,627,779,986B, exceeding12GiB; ROOT explicitly approved32GiB admission.
This envelope is not proof of NTFS allocation.

`imageprep2` used only the pinned existing containerd/ctr binaries, fresh private
state/pipe/controller and exact image digest. Before every ctr action it checked
`GetNamedPipeServerProcessId` against the recorded live held server PID/birth.
Its original1000s pull/import deadline expired; the1100s owned job exited1,
reaped/active0, peak246,235,136B. The recorded server16012, ctr6444 and job2528
were independently absent afterward. Original ledger omitted ctr terminal exit
because its timeout branch threw before recording it; that missing receipt remains
explicit. No deadline or resource increase and no unchanged pull retry occurred.

A legacy .NET allocation walk failed at an actual264-UTF16-unit WindowsApps path.
The exact-path probe reproduced legacy error3 and extended Win32 attributes0x2016.
The first Win32 replacement had a WIN32_FIND_DATA packing error (lost two filename
characters), retained as `retainedmeter1`. The corrected four-byte FILETIME
alignment in `retainedmeter2` measured17,909,705,904 allocated bytes: regular files
17,810,017,456B plus directory allocation99,688,448B,180898entries,53054duplicate
file identities,367maximum UTF16 units. Native FileIdInfo volume+128-bit ID
removed hardlink duplication; reparse objects were opened without following.
Extra named streams and volume-wide MFT metadata are not attributed. All four
cached manifest/config/layer files were independently checked for exact lengths
and SHA256 digests. This measures retained partial input, not disk returned.

Stopped databases were copied read-only and verified through their bbolt version2
meta-page checksums. Snapshot1 is committed at base diff-id7dd5847c...; snapshot2
is active at tail diff-id4c77a873..., parentbase, targetchain49639eb7.... The image
record was not yet published. Original DB hashes:
`391ce09fe56632a48f9af17c0a1b51311f1971396773c2363a328aba18ae5d8b` and
`1874a57aa5915be302f83318873c9c4c57d3bea9a76c3e0ef466963339fc6eef`.
These metadata and verified blobs do not establish a completed full image.

The concrete continuation proposal is signed `53f605ac`:
`prepare_cached_image_body.ps1` SHA `abf9d137ff53a1826394eb4a0d6067462888e1c831202349b76e82164d34015c`,
controller `prepare_cached_image.ps1` SHA `0fe01b9b25180a13a2327dba74e7c1220b6527d26975df9eee4f532ad3c9ae3a`.
It preserves original metadata/inactive snapshot2 and streams only verified cached
content through local OCI import, requiring reuse of committed snapshot1, the exact
two-layer head, actual DLLs, allocated-byte checks and exact owned server reaping.
The original32GiB/1100s job/1200s outer/1000s import limits remain. No registry
fetch, full tar disk copy, install or shared service/driver change. It was initially held
for review, then ROOT explicitly released the exact cached-only body/controller once
as an operational resource admission, without a hard NTFS quota requirement. The
actual result follows below. Actual installed ctr has no
`images unpack` command; that availability refusal is retained. Its supported
`images import --local` is the proposed changed path. Harmless `cachedpreflight2`
validated both scripts' AST, compiled pump, real USTAR flags and byte-exact3072B
forwarding, independent SHA equality and deliberate3071B overflow refusal. It
started no server/import/HCS workload. The first preflight's nested-error matching
failure is retained, with the corrected full inner exception in the successor.

## Actual independent native API and image-loader controls

`cpucallback3` used the actual HCS operation callbacks and Nano CPU process:
create/start were immediately Pending, each completed once with HRESULT0; the
CPU output was exactly `native-hcs-cpu-checked-output\r\n`. Recorded child11652,
birth04:15:58.1076896Z, native exit0/reaped. Terminate callback0, exact system absent,
Unprepare's existing FilterHeld0x801f0010 classification followed by Deactivate0
and native mount query0x80070037. Cleanup errors empty. Earlier process-observer
race and PowerShell null ExitCode failures remain separate. All three exact system
IDs were independently absent and their scratch detached; retained copies remain
as inputs/evidence. This is native API prerequisite proof, not Hex Runtime/Store,
keeper/recovery or selected GPU proof.

The real fresh Hex CLI launcher from the LLD cohort was
`C:\Users\corph\.cache\hex-native-windows-hcs\hcsllvm1\target\debug\hex.exe`,
SHA `ba4ef610609009d64aa1245ba98554924c1ea1df0e20e30daa07164a42135225`.
The actual HCS launcher entrypoint is `C:\hex\hex.exe oci-launch`; its default
process document uses cwdC:\, stdin/stdout pipes and no stderr pipe. Actual image
import preflight alone was not used to declare loader failure: Nano API contracts
may resolve without ordinary System32 files. `launcherloader1` mapped that exact
binary read-only and executed `oci-launch <NUL`, expecting Hex's documented EOF
parser refusal2 if loadable. Captured actual child errorlevel was
-1073741515/0xc0000135 (`STATUS_DLL_NOT_FOUND`), stdout/stderr empty, so Hex's parser
was not reached. Raw CLI SHA `c404b2d37c98daa13bfbfae4bc9bed691773d59e810c43ffccae04f1526494fa`.
The wrapper PID16708/birth04:52:26.4011010Z was held until native exit0/reaping;
the Hex child PID itself was not retained and is not invented. Exact HCS system
`native-hcs-loader-302ffbfe3806414d900d93876f5a374a` was absent before/after;
callbacks/termination0, scratch detached, cleanup errors empty. Full source,
command, binary/import view and raw receipts were sent to L3/ROOT. The Hex CPU
startup result is supported as an image prerequisite failure; no lifecycle source
fix or production defect is claimed. No unchanged Nano or host GPU rerun follows.

Raw logs and native receipts are retained byte-for-byte in deterministic gzip tar
archives with per-member bytes/SHA256 and archive SHA256; originals remain in
place and previous archives are never overwritten. `.gitattributes` does not
suppress source diffs. Missing/counter/collector failures remain explicit.

One physical GPU cannot establish a distinct-GPU negative: **UNVERIFIED**. No
foreign identity, ordinal/model fallback or topology is invented. HCS selected
compute, overlap, persistence-before-effects, borrowed-volume marker, Pending
callbacks, unchanged physical identity through recovery, mismatch byte preservation
and physical/resource release remain unfinished until actual run receipts exist.

The `985d7767` fixture now retains lossless marker/store byte maps before start
and before failure cleanup; it also includes the borrowed-volume extension and a
separately named selected-only case whose CPU exclusion remains unverified.
It was actually compiled and executed as described below. The older `ed6` binaries
and their original missing-state evidence gap remain unchanged.

## Released cached import and actual full-image native checks

`cachedimport3` dispatched the reviewed `abf9` body / `0fe01` controller exactly once,
using the reviewed 1100s owned job, 1200s outer and 1000s shared archive/import limit.
All six checked pipe observations matched live server16416 and native birth
134388480816347837. No pull/download, full-tar disk copy, original snapshot removal,
VHD framework, host install/service/driver/security/quota change occurred.
The producer streamed6,657,207,296B, SHA
`828d0788bbf5869db1c87e26e0bb741b128740698d00e020ec7f59fe01294cca`.
Producer and actual `ctr images import --local` both exited0/reaped. The native
head `sha256:49639eb7e32ce1d8a18cc2504b183a024475038fcba89f069a50751e8eef5a8d`
committed with top snapshot3 and the original committed base1. Inactive snapshot2
was retained. The temporary path-read snapshot4 alone was removed normally.

The original controller nevertheless exited1 after import: its PowerShell
`@($parentJson | ConvertFrom-Json)` counted the decoded array as one wrapped object.
The same immutable native mount receipt decoded directly has two strings. Original
stderr SHA `9c45f9db02b5ca38f8ec79c5b0836da6ed65e89a1debbe8c3c9db6c675c8ac00`
and parent receipt SHA
`360bd58c4e944a592bc5a297ab32488386922476f30a171d451cb68085ac2e95` are retained.
This controller failure was never edited into success and no import was repeated.
Original job17112 was not timed out, reaped/active0,21 processes, peak623,607,808B;
all recorded server/producer/ctr/controller processes were independently absent.

The first separate verifier reached its90s outer deadline with no recorded remote
PID: that remote process cleanup remains uncertain. Verifier2 recorded PID500,
then timed out before its first result; PID500 was independently absent afterward.
Instrumented verifier3 localized the same90s timeout to depth25 JSON serialization:
Windows PowerShell `Get-Content -Raw` decorated the native log string with provider
and drive object graphs. The bounded startup control succeeded; a plain
`File.ReadAllText` returned identical text without that graph. Verifier3 PID488
was independently absent after its original deadline. The signed correction kept
all deadlines and original raw files. `imageverify4` then exited0, confirming the
actual native head/base paths,10 content hashes across the five explicitly required
DLL names, original cleanup and post allocation. Its raw result SHA is
`7ea4fec0d58399004a02737b2b47e59bf32ece71fbf44c0f670d084601d2e57d`.
This establishes prepared inputs, not transitive loader completeness or GPU execution.

Whole charged-root allocation before the released import was23,026,800,200B and
host free space174,953,762,816B. One observation after archive IO completion was
34,061,086,376B (298,651,992B below32GiB); it is not a peak guarantee. Import-terminal
allocation was28,686,745,704B. After the new985d build/native cases, the complete
root measured29,813,317,856B, with observed4,546,420,512B headroom and host free
167,215,620,096B. Every measurement includes retained source/target/cargo/incoming,
controllers, image, old failed snapshots and native scratch. Extra named streams
and global MFT/security/journal overhead remain unattributed; none is called a hard
quota, exact peak or leak-free result. ROOT's clarification is recorded as a review
clarification, not a policy waiver.

Original stopped DB checkpoints were copied byte-for-byte and remain
`391ce09fe56632a48f9af17c0a1b51311f1971396773c2363a328aba18ae5d8b` /
`1874a57aa5915be302f83318873c9c4c57d3bea9a76c3e0ef466963339fc6eef`.
Current committed snapshot DB SHA is
`9a909f9be9efc4724d88a0503c59313a298435c4c64b66b3222910f8f466b3e5`;
current containerd DB SHA is
`171a89efeb6d7dbf74ddf0851e8a521553ea6be5d7e91ebde1ecbb0ab21795a7`.
The raw before/after DBs are in `runs/committedmeta1/raw/` and its lossless archive.
No new server/import was used to collect them.

`hcsgpu1` used the original fresh `ed6`/d459 artifact on the prepared image. It
failed before first HCS create because this new fixture had used `Recorded::open`
without the native provider required by `capacity`. L3 confirmed the existing
production CLI contract. Only the new fixture was corrected to use the same
`facts::shared_prober` and `Recorded::open_with_prober`; original stderr
`8a8e35bc536e25b59aa684dd8b5d2fa94f5274cffc20170070e38122d60dcd74` remains.

`hcsfixed1` compiled immutable985d/archive
`a46ce6594ef3845316a081a136b3388858520b96702fc1603d7fa725a3081e66`.
Fresh:false test binary SHA
`aaef9dedc0cda720e635dccd6ada7e0d79bc1c7868930f76d6f965d9b999abcf`,
fresh:false CLI SHA
`cb6e9f0f72bb31bb33d774b019ad3bb8eed3d0621db8787fc36f6dae97be8828`;
source manifest `33aaeb056dd0c96594f384882ba47b65110d4e8da98cca81113b3813f128e5a7`
remained identical before/after build and native execution. Empty target identity,
manifest, target source, test profile, canonical path and AMD64 machine were checked.
Build job3900 exited0,144 processes, peak1,068,699,648B, reaped/active0 after explicitly
ending residual compiler children. No linker/PDB cause or performance claim follows.

`hcsgpu2` executed the independently named selected-only case once. It now reached
the real provider but failed before HCS create: the exact production2c facts adapter
explicitly excludes Windows AMD until its native correspondence join is implemented.
The refusal was `an initialized route has no unambiguous native physical identity`.
L1 received original raw stderr SHA
`c53cd11b31d0b45e3838a9673834997f4998fbd74673030ded0bb1eff17a4bef`
and the exact source/binary/input receipts. No guard was removed and no fake provider
was added. The new failure-state retention captured three original state files,
SHA `a762a10a4690472d0d52c20b2bafd19d323e7f6fa4f9892d3d04a6a811cec68c`.
Job14384 exited101, reaped/active0, temp0, source unchanged. This is not a GPU pass.

The independent full-image ordinary CPU case `hcscpu2` used unchanged ed6/d459:
HCS create succeeded, then Start failed0x80070287 before the launcher. Independent
native API case `fullimagecpu1` bypassed Hex Runtime/Store/keeper/launcher and
reproduced the same native start failure. Both create and start were Pending and
completed once; the start result has empty ErrorMessage and Attribution Start.
The [Microsoft WinError.h documentation](https://learn.microsoft.com/en-us/windows/win32/debug/system-error-codes--500-999-)
names low word647 `ERROR_FAILED_DRIVER_ENTRY`; it does not identify which driver
or its cause. Host is Windows11Pro26200.9974/25H2, image26100.33822. Version numbers
alone are not a demonstrated cause. Native API cleanup retained terminate-call
0x80370110 and an operation-context explicit-close gap until process exit; system
absence, Unprepare0/Deactivate0 and detached mount0x80070037 were observed.
Original CPU fixture marker/store bytes had already been deleted by its older
failure cleanup; that gap is not retroactively repaired.

`fullimagegpu1` then changed only the independent native capability control to
actual HCS2.2 exact `DeviceInstance` / recorded LocationPath, with the same pinned
4fdd oracle ready to run. Real prepare succeeded, but create completed Pending /
one callback / closed with0x80070003, Attribution Construct, empty ErrorMessage.
No compute process ran. System absent before/after, Unprepare0/Deactivate0,
mount0x80070037 and cleanup_errors[] were recorded. This is an actual native
assignment refusal, not Hex recovery or GPU execution proof. No fallback was used.

Required requery `currentidentity1` at06:12:36Z independently confirmed the SAME
actual CM1ca interface / full KMT14c4d (open0/close0,1/1), PNP, driver and03:04 boot.
Actual PNP LocationPaths still include `PCIROOT(0)#PCI(0801)#PCI(0000)`;
LocationInfo String is exactly `PCI bus 196, device 0, function 0`, BusNumber196,
Address0. ContainerId is the all-FFFF sentinel, not a GPU UUID. Raw query SHA
`0fea22a68ba8a8e7506a06c69b264ec91ac6e16900188877321664214a796021`.
The existing value and [Microsoft's segment property documentation](https://learn.microsoft.com/en-us/windows-hardware/drivers/pci/obtaining-pci-segment-values)
were sent to ROOT/L1 without another probe or an invented segment. Scoped existing
event queries returned no events over each owned scope's actual creation-to-terminal
interval; no channel was enabled. The native startup/construct causes remain unclassified.

`absence3` independently found all three recorded keepers, original import server,
outer controller and selected native-control process absent by recorded PID/birth.
All three recorded HCS IDs returned0x8037010e/null. All retained completed guard
receipts were reaped with activeAfter0. Stop calls0. Retained files/scratch are
intentional evidence; no arbitrary cold allowance or full driver/COM leak proof
is claimed. HCS compute, overlap, persistence-before-effects, borrowed-volume,
actual owner-process crash/restart, mismatch preservation and GPU resource release
remain UNVERIFIED until real passing workload receipts exist.

## Separate ROOT-requested changed-source light validation

ROOT requested only four `windows::preflight::tests::` private-metadata tests and
Node/CLI all-target compilation on exactc579. Runtime Clippy was explicitly excluded
because its unchanged lint failures were already reproduced elsewhere. No HCS,
GPU, WMI or native-job creation is performed by these four test bodies.

`winpreflight1`, archive
`c276dca9b15ad4abd41714050b4d0e3a3da2f1f491dd1dfee9151e0f84cfd3cb`, failed native
Windows unit-lib compilation101 with nine errors, so actual selected tests0.
Original stdout SHA
`4c477e72c4936d16ac128df4b2e2a2b5492e8411b0e967cf78d8e2e4166bd791`.
The separate requested Node/CLI compile `nodeclic579` confirmed three errors in
production: missing Read import at windows/oci/mod.rs661, untyped marker Option at
windows/preflight.rs260 and removed DeviceInfo.classes at windows/mod.rs1854.
Original stdout SHA
`63103de7417f0e93c5ef810e0506f057c1987f135c66eebbedec5e4ba12ce944`.
Both jobs were reaped/active0, no timeout, source unchanged; no test/native workload
or executable acceptance followed these compiler failures. L3 returned signed
`93abeaae82d97aaf64271e8fc29908538788bbb8`, exact parentc579 and only four Windows
source files plus its evidence, correcting the nine reported causes without disabling
oracles. Immutable93 was already transferred when the later joined ROOT585 instruction was
read; ROOT explicitly said to finish an already-active93 without mutation/restart.
It remains attributed to93, not585. Its archive is
`194b9647a3812c7ff44bb6362c8a6888fa28636cb289ec6ba25ac594f956bf73` and fresh runtime
unit-test binary is
`7f4c88f212ecc2861b50887699040e9d620c460ee70acfd6e2620cc673825d23`.
Compilation passed. Exactly four tests actually ran: three passed, one failed in0.07s.
The Pending test attempted to serialize a marker without the bindings its current
schema requires, so `hcs::write_record` rejected it before preflight. Original stderr
SHA `b51a7a087796bd7113781b4e8b3218d7a1dfd44e9441dc0cd38304ae2131cadb`.
L3 received this fixture cause; it was not disabled or called a native failure.
The raw legacy native_tests0 counter is not the actual unit-test count; the exact
four names, actual_selected_test_count4 and raw stdout show3pass/1fail. All GPU,
HCS and WMI effects from those test bodies remain0.

The separate same93 Node/CLI all-target compile passed the runtime errors, then
failed at ROOT-owned Windows CLI node.rs522: BackingOffer has no check method.
Original stdout SHA
`c0a6f9e559d0d13fc55f8b0682b0d06099b20e348d054102f64c28dad3ddcbfe`.
ROOT received the exact consumer error; no patch or unchanged retry was made here.
Every93 job ended reaped/active0 with unchanged source manifest
`29d0d11df8cf1225a828d801b2ae063c36d4c6b2d92b0fe383bd1f3842d076e5`, temp0.
This is separate from fixed2c native GPU evidence and from a ROOT integrated or
final-gated candidate.

After c579 retention, complete charged-root allocation measured30,837,029,928B,
with3,522,708,440B observed headroom before the93 transfer. This is another
operational observation with the same explicit metadata/peak uncertainty.
`absence4` additionally confirms the completed93 jobs are empty/reaped and the
recorded HCS/keeper identities remain absent, without stop calls.

After93 retention, `wholepost3` measured the complete charged root at
31,980,355,120B, with2,379,383,248B observed headroom under32GiB and host free
165,058,592,768B. Its receipt enumerates all70 charged child paths, including the
original image, inactive snapshot2, source/target/cargo/incoming and controllers.
No old snapshot or evidence was removed to obtain this result. The measurement
finished at06:42:55Z; it is not a peak or filesystem-quota proof.

The separate existing HCS Basic property query succeeded at06:38:06Z and returned
SupportedSchemaVersions[{Major:2,Minor:11}], with checked LocalFree success.
Query DLL SHA `13b43eada2504adb48650f381a2cc529bc300da679d5d29e24bc57c7dccc060d`;
actual System32 computecore SHA
`e3202a3f676a815c0e96a12dfb33364d0486beb293a7298127f1ed78d3de93ab`.
This records service metadata and does not identify the startup/assignment cause.

ROOT's next SDK19 pipe/late-completion work is explicitly held pending its corrected
pin and original-function-only overlay/hashes. No SDK19 test or improvised mutant
has been dispatched. Read-only archive inspection of the held3d31 source measured
28,029,358B compressed /54,528,000B tar /2,483 files with52,365,327 logical bytes.
Its target/cache peak is unmeasured. Two previous full-runtime cohort allocation
deltas would bring the retained working-set estimate to34,267,005,504B, leaving
only92,732,864B under32GiB before unknown staging/metadata/build peak. ROOT received
these actual inputs and a40GiB operational-envelope proposal before any transfer;
that proposal is not an admitted envelope or measured SDK19 minimum.

L3 supplied signedc0a78 correcting only the failing Pending marker fixture, and
L1 supplied signed204c for the native AMD correspondence bridge. Neither is a
passing native result here. The joined bridge pin and held SDK19 ordering correction
remain separate prerequisites; no floating owner files or improvised overlay were
copied. The already passing three preflight cases will not be repeated unchanged.

The next single-test driver is prepared in `preflight_pending.ps1`, with exact
c0a78 source and the original93 red hashes in `pending-delta-source.json`. It has
not been transferred, built or executed. Its source CLAUDE differs from fixed2c;
both full files were read, and that difference is explicitly retained. c0a78's
actual parent is335fca8, not93: its complete tree differs from93 across57
production/dependency files. A coherent ROOT joined pin is required before this
draft is eligible for dispatch; the narrow commit is not treated as a complete93
successor. ROOT and L3 received this exact source discrepancy.

The first evidence staging attempt was not executed. Automatic approval review refused
to execute its staging command because the session token could not be refreshed
after sign-out/account change; this was a review failure rather than an unsafe
determination. The command did not run, and no bypass was attempted. User and ROOT
were informed. After the user's Continue, normal approval review resumed and the
same scoped staging command exited0. All raw/source/archives remain in place.
`approval-review-auth-failure-v1.json` records the original failure independently
from the native failures and SDK19 source hold; its resolution is recorded in
`approval-review-restored-v1.json`. The final commit itself establishes signing.

## R01–R16 account at this checkpoint

R01/R04/R06/R07/R13: no production edit; test-only explicit failure/cleanup and
source-pinned real-output checks. Final focused native Clippy remains pending.
R02/R03/R05/R12: existing Runtime/Recorded/Store/keeper/catalog seams; no new
picker/holder/cache/manager. Caller budgets are explicit; native event waits and
source pinning are recorded. Job completion is followed by accounting, and a
missing notification fails under the original finite caller guard.
R08/R14: no speed, leak-free, allocation-free or competing-baseline claim. ROOT
owns all profiling/benchmarking and full audits.
R09/R10: original failures and real original/mutated/restored GPU behavior retained
separately from compile and prerequisite failures. Hex HCS execution/recovery has no passing claim; independent native API controls are separately labelled.
R11: actual x64 Jupiter only; ARM paused, one-GPU hardware limitation explicit;
all remaining seven-target/final gates are ROOT owned, unrun by this lane.
R15/R16: explicit policy ACK, bounded assigned native scope, signed test-only
checkpoints, original source/refs/raw preserved, exact owned jobs only, no merges
or pushes. Local free space was above30GiB before each archive transfer. The released cached import, actual native refusals and separate light-check compiler reds are retained; changed-source checks and remaining native execution continue; this is not readiness.
