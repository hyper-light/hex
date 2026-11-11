# Alternate artifact builder: exact 4a/14 inputs on distinct 00aad

This is a source-only proposal for ROOT review. No SDK files were copied or
installed, no namespace was executed, and no kernel/bootstrap/image build,
VM, GPU or driver operation started.451 remains reserved through the LB sweep
and queued focused run. This lane issued no command on451 during this check.

## Distinct host evidence first

The LB peer's existing contemporaneous receipts are copied, with original
paths/hashes, into `alternate-host-identity.receipt.json`. They show:

| Observation | 451 | 00aad |
| --- | --- | --- |
| normalized machine-ID SHA256 prefix | 098e190424c5 | 01685e36c9b6 |
| normalized boot-ID SHA256 prefix | b888bfd52a5b | ae706cbc1213 |
| memory, KiB | 128893460 | 62831872 |
| permanent wired NIC address SHA256 prefix | 1adcabc363f4 | fe3f8dcb6584 |
| WiFi PCI vendor/device | 14c3:6639 | 14c3:7927 |
| kernel | 7.2.9-2-cachyos | 7.3.3-1-cachyos |

Our normal admitted00aad read-only probe independently matches its full machine
and boot hashes. This is strong independent-host evidence, beyond runner names
or labels. DMI UUID/serial reads were denied; no privileged/firmware attestation
or proof excluding hidden virtualization is claimed. The original LB receipt
hashes are2c18512f… and4ca312f4…; exact values and raw observations are retained.

## Existing controls were positively checked

One new owned user scope, `hex-native-fc-alt00aa-delegation1`, ran only a bounded
controller/identity observer under admitted1CPU/4GiB/12GiB needs. It returned0:

```text
cpu.max         100000 100000
memory.max      4294967296
memory.swap.max 0
pids.max        256
nice            10
sole PID        3414668, birth30416089
```

The scope guard was15s, outer20s and caller30s. The subsequent receipt reports
the exact PID absent, cgroup path absent, LoadState=not-found and
ActiveState=inactive. The ordinary runner ancestry exposed only CPU, but the
existing user manager's app.slice supports the complete controls. No existing
delegation, service, security, permission or host configuration was changed.
The exact successful scope command and cleanup are in
`alternate-00aad-probes.raw.json`; it is the method proposed for ROOT's future
unique build scope. It is not kernel/VM/GPU resource-return evidence.

## Exact content to transfer

The original source is signed4a782bd04d326b049d383980945825c23cca1c0e with
handoff14c7716ebfa316592b56208db3ef0eb7b2f15507. Original cohort SHA256 is
`cbb3faa4851200a3f3bc0f61ce8a4c3f38a678c06274ac1485cd86a9e938c701`;
original bundle seal is
`af60f4f05846963d3eaf0a674207160a01e8dedced370f93b2083b409c774a60`.
Neither original file nor driver is changed. The transfer manifest was derived
with `git show` at that exact source; floating owner files are not inputs.

`alternate-00aad-transfer-plan.json` lists every destination, byte count,
content SHA256, original provenance and chosen acquisition source. It covers
7929 materialized members and7349 distinct content objects,834,588,027 bytes
before materialization/metadata/build output. The full4a SDK is7869 logical
input paths,436,049,753 bytes, deduplicating to7301 objects/416,325,153 bytes.
The retained image inputs keep the original kernel/config, all11 source-derived
firmware names, ICD/libraries, witnesses/shaders and exact old Hex. These are
input byte counts, not RuntimeCost, VM memory or total build-space guarantees.

A metadata-only stdin comparison hashed the actual00aad paths against the
complete4a SDK and45 payload/ICD/Hex entries:

| Input class | Exact hash matches | Absent/different | Original bytes requiring another source |
| --- | --- | --- | --- |
| SDK,7869 paths | 6252 / 180297807B | 1617 | 255751946B |
| payload/ICD/Hex,45 paths | 30 / 47419474B | 15 | 223214151B |

Current host GCC is package-3, executable585673b6…, rather than4a package-1,
c348a945…. Host Perl, Python and systemd-run differ; libelf is0.197 rather
than0.196. The plan does not replace those4a inputs with current host versions.
Rust1.99.1 is installed alongside the default1.98.1; neither is selected or
used by this GCC/kernel recipe. No toolchain default changed.

Object sourcing has a deterministic order; all sources are rehashed before and
during copying. The current manifest resolves the unique objects as follows:

| Chosen source | Objects | Bytes | Current status |
| --- | --- | --- | --- |
| exact matching00aad file | 5734 | 204159607 | hash-inventoried; not copied |
| exact member in retained signed witness/support archive | 4 | 52132 | member hash checked; not extracted |
| exact pinned public URL from4a | 14 | 157566865 | same kernel/firmware/WHENCE/license/bc inputs; not downloaded here |
| exact bc ELF extracted from pinned package | 1 | 96968 | future extraction only, no package scripts |
| original4a byte export from451 | 1596 | 472712455 | HELD through sweep plus focused queue |

The retained archives include the nested support bundle and exact shader/
witness/PCI-observer members; archive and member hashes are recorded in
`alternate-00aad-retained-archive-inputs.json`. The original full retained Hex
and some SDK/RADV/LLVM bytes are not in those small archives. They remain
explicit export requirements. Original installed451 file paths are provenance
locations, not immutable bodies: a future export must match the original4a
hash before/while reading. If the old bytes have changed, that object remains
unavailable until an exact matching retained/public input is found. A newer
package is never accepted by name/version resemblance. No export has begun.

All transfers use normal Kiln admission and source-pinned `--no-sync` archives
or stdin. No SSH, host install, default change, shared cache mutation or foreign
file deletion is part of this plan. A sender inventory and receiver manifest
must agree on the exact selected object set, raw transfer hash and per-object
hashes. The packed-transfer hash cannot exist yet and is not fabricated.

## Concrete private SDK and observer adapter

Future owned root:
`/home/adalundhe/.cache/hex-native-firecracker/amd-root-build-alt00aa-root01`.
It must be absent before the attempt; successors use new names. Import objects
into `input/objects/<sha256>` with exclusive writes, streaming hashes and
read-only final mode. Materialize only manifest-listed regular files. Every
SDK `/usr/...` input retains that exact name below `input/private-sdk/usr/...`.
No host `/usr` is mounted into compiler phases. The bc executable is extracted
from its pinned package into owned `tools/bin`, preserving the4a method.
Input modes clear setuid/setgid; ELF or shebang executables are0555, other
regular input bytes0444. No mode or permission on a host input is changed.

`alternate_00aad_adapter_contract.py` is the concrete source for the changed
boundaries. It has no build entrypoint or import-time effects. ROOT must join
these boundaries into a separately sealed successor of the original driver
before execution; this contract does not claim that successor already exists.
The original4a driver cannot run unchanged here: it binds451 admission,
installed host paths and two specific GPU sysfs observations.

The planned successor changes only these boundaries:

1. The launcher accepts only exact00aad admission and its pinned machine/boot/
   kernel/UID. Use `alternate-00aad-host-controls.json` for the build-host
   observer/launcher:2379 file pins,98,093,609 logical bytes, including45 ELF
   closure entries and2333 Python auxiliary files. Verify them before/after;
   fail if changed. These controls do not replace compiler-invoked4a Python,
   Perl, GCC or libelf.
2. Resolve original nonbundle source rows by exact object hash. Verify the
   entire materialized SDK name set and hashes before/after. Do not probe
   original absolute source paths as a fallback. Keep all original source
   paths/hashes as provenance labels in the final manifest.
3. Every old make/GCC/readelf compiler phase uses the exact argv construction
   in the adapter contract. Existing bubblewrap0.13.1 is pinned at SHA256
   `54e8408bd5b325c17926dd2c2420e6f28086286fd5bf99b397f9b681be3dada0`.
   Its per-phase filesystem exposes only private4a `/usr`, derived `/bin`/
   `/lib` aliases, the owned work tree, read-only inputs/source, fresh proc/dev
   and the4a environment. User/PID/network/IPC/UTS namespaces are required;
   capabilities are dropped. No host GPU/KVM/VFIO/sysfs/device tree, network
   namespace or host `/usr` fallback is supplied. Namespace execution is
   UNRUN: version/help/kernel settings are prerequisites only. Fail hard if
   it cannot be created; no `--try`, weakened flags or security changes.
4. The outer host driver retains the real phase bwrap PID/birth/process group,
   whole-scope guard, exact waits/reaping, cgroup member check and all raw
   errors. PID1 inside this compiler namespace is bwrap's reaper, never Hex
   or a VM. The host observer records only00aad build-machine identity and
   controls. It does not read GPUs or label00aad as the selected native host.
   The immutable target AMD79:00.0 evidence remains451-only and held.
5. Preserve4a firmware/config/security checks, V=1/jobs1/O= commands, static
   bootstrap flags/source, image retention grammar, input/output hashing and
   null profile. Keep ROOT's matched producer/current ABI requirement open.
   Emit an explicit source diff, successor driver hash and supplemental seal
   binding this plan, host controls and original4a/14 hashes. ROOT reviews
   that concrete source cohort before the heavy dispatch; no floating overlay.

Bubblewrap's own documentation explains that filesystem exposure/policy comes
from its caller's arguments; its presence alone is not an isolation proof.
The installed help SHA256 is2fa5163e… and every proposed option is in that help.
The exact argument source is reviewable, while actual namespace behavior still
requires a bounded preflight after review.
[Pinned upstream source](https://github.com/containers/bubblewrap/blob/v0.13.1/bubblewrap.c),
[upstream explanation](https://github.com/containers/bubblewrap/blob/v0.13.1/README.md).

## Finite ROOT sequence; not dispatched

After precise-plan/source review, ROOT may materialize the exact objects that
are available without451. The missing original-byte export remains held until
both campaign-ended receipts. No compiler command is authorized by this plan
alone. Missing/mismatched objects refuse before any compile.

The future one-attempt scope command is the already successful scope method
with new exact unit/root and a3300s whole-scope guard:

```text
needs linux/amd64,cpus=1,mem=4g,disk=12g, on00aad, --no-sync
timeout --signal=TERM --kill-after=5s 3350s
systemd-run --user --scope --quiet --unit=hex-native-fc-amd-alt00aa-root01
  --property=CPUQuota=100% --property=MemoryMax=4G
  --property=MemorySwapMax=0 --property=TasksMax=256
  --property=RuntimeMaxSec=3300s nice -n10
  /usr/bin/python3 -B EXACT_REVIEWED_SUCCESSOR --admission EXACT_REVIEWED_ADMISSION
caller timeout3360s
```

The successor/admission paths must be real sealed inputs, not the contract
module. The55-minute scope includes materialization, namespace preflight,
configuration, compilation, assembly and receipt work: no per-stage clock
reset, retry, timeout inflation or different envelope. Before foreign compiler
exec, require actual CPU100%/memory4GiB/swap0/pids256/nice10 receipts again.
The artifact tmpfs cap remains512MiB, not an accepted VM budget or RuntimeCost.

Namespace preflight checks pinned GCC specs/paths and library resolution from
the private view without compiling; require the existing specs SHA256
c0ab03f7… and all SDK/name-set checks. Any unsupported namespace/interpreter/
library path leaves a retained failed preparation. It is not a reason to use
host GCC/libelf. A reviewed attempt then preserves each raw phase and source/
binary/config/input/output hash, whole-root file allocation/headroom, process
reaps/absence and exact scope empty/gone. Kernel/source/target/temp/cache,
exports, failed partial products and all old raw remain owned and preserved.
The actual fit within12GiB is unknown; per-phase whole-root checks and current
Kiln admission/headroom remain required, without a claimed metadata hard quota.

## Attribution and remaining work

The full byte comparison passed after correcting Kiln CLI shell quoting; the
initial syntax failure is retained separately. An earlier read-only dependency
collector hit its counted queue bound because duplicate edges were enqueued;
deduplication fixed the collector under the same unique-file/time bounds.
Neither failure is a production defect or a native GPU/driver result. Their
commands/exits/raw are retained in the new probe/comparison files.

Source-only AST/JSON/content/manifest consistency checks are appropriate here;
no functional tests, compilation, benchmark, profiling or full gate is claimed.
R01/R04/R06/R07/R13 apply to the reviewed fixture boundaries; R02/R03/R05/R12
are served by exact source/identity/content evidence and finite private scope,
without a second scheduler or holder ledger. R09/R10 retain all observations
and failures without calling source reasoning a live defect. R08/R14 remain
ROOT's unrun profiling/calibration work. R11 is Linux amd64 artifact preparation
only, with all other platform/native cases unverified. R15/R16 preserve role,
source/raw, current7d7e policy (all four blobs byte-identical to ACKed0a6/df2),
runner reservations and no main merge/push/new agent/goal.

Current alternate status: distinct host and correct existing scope verified;
exact SDK relocation/namespace execution and build are not yet performed.
Current native status: CPU4/7dee passes stay separate; VFIO, actual APU guest
driver/identity/checked compute/recovery/cleanup, current ABI/provider/Store and
accepted Firecracker RuntimeCost remain open. No VM/GPU run is part of this plan.
