# CDI into the existing OCI renderer

POLICY_READ: implementer Kiln `01a1c640e6ed710d8a8c919b4aa1fc8c`, coordinator
`01a1aa77778471339bea3beff90d6bbd`. FULL current Shards origin/main AGENTS.md,
docs/development-rules.md R01–R16, development-evidence.md, CLAUDE.md and
managed-runtime-plan.md plus /Users/adalundhe/g121-handoff.md read before edits;
re-read after context loss. Revision `cbd39284937d024640c56db6f8d8ef2bd9b0ee24`
verified equal to remote main. Rules SHA256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Hex CLAUDE/HANDOFF read fully: hashes
`8708b80967e5517ae9137eb642600668ac02d164a4c2145ad8462a37889d424e` /
`9b780baaa9ff3dc1d491624d99a217ceb7a2b7cc913c1f56e0598f6d8f8694c1`.
User scope ACK persists after readback; stable receipt key
`cdi-oci-render-policy-cbd-df2-v1`. This receipt is not proof of compliance.

Branch `cp/t7-cdi-oci-render` starts at EXACT
`d4d96083c83f88ff3e9b9447598a3b95f461b680`. Tested CDI source `9cb5953c` and
previous branch `cp/t7-cdi-device-edits` remain preserved. Own only bundle.rs
render/device sections, NEW bundle/device_edits.rs and its focused tests, and
this measurement directory. No edits to cdi.rs, lib.rs, shared API, runtime or
catalog. ROOT owns combined registration/API/native/OS, audits, mutations,
E2E/VM/heavy/gates, profiling/benchmarks and merges. This scope is implementation
and light checks only. No children, goals, plans or automatic dispatch.

## Exact interface

```rust
pub fn render_with_devices(
    spec: &OciSpec,
    at: &Place<'_>,
    edits: &crate::cdi::Edits<'_>,
    resolved: &[ResolvedNode<'_>],
) -> Result<Bundle, Refusal>;

pub struct ResolvedNode<'a> {
    pub node: &'a crate::cdi::DeviceNode,
    pub kind: crate::cdi::DeviceType,
    pub major: u32,
    pub minor: u32,
    pub file_mode: u32,
}
```

ResolvedNode borrows the EXACT selected node, one per selected operation in its
original order (including path replacements). The caller owns actual host stat
and its identity/time-of-check implications. `node` must reference the corresponding
selected object; copied/reordered objects refuse before output. `kind` must be
stat-derived Block/Char/Fifo, numbers are the unsigned unix Major/Minor outputs,
file_mode is stat.Mode with file-type bits removed (Linux permission/special bits
only). Debug redacts all fields. No stat, path lookup, IO or hook execution occurs
here. Fully specified CDI nodes still require a caller resolution receipt in this
interface; Go allows stat failure for minimally specified nodes. ROOT owns any
future explicit unavailable-stat variant and its policy.

`render` delegates to this SAME renderer with empty edits/resolutions. Additions
modify its typed Config, never a second JSON tree or renderer. Empty edits must
preserve successful Bundle bytes exactly. Selected netDevices/intelRdt refuse as
Refusal::Platform BEFORE rendering; no ignored native fields are emitted.
Resolution/count/type/mode/field errors use Refusal::Invalid; allocation/config
budget exhaustion is explicit. Existing Refusal carries String presentation,
so fixed field/cause/count-only messages bridge that API without private payload
or string-matching control. No shared error API is changed.

## Pinned implementation decisions

CDI source revision `98a7d735d9592c1ccca0130e06b0709f4b65ecbf` (SPEC v1.1.0):
[Apply/Append](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/pkg/cdi/container-edits.go),
[Unix fillMissingInfo](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/pkg/cdi/container-edits_unix.go),
[OCI conversion](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/pkg/cdi/oci.go),
[go.mod](https://github.com/cncf-tags/container-device-interface/blob/98a7d735d9592c1ccca0130e06b0709f4b65ecbf/go.mod).
Exact dependency: runtime-tools `v0.9.1-0.20251114084447-edf4cb3d2116`,
[generator](https://github.com/opencontainers/runtime-tools/blob/edf4cb3d2116/generate/generate.go).

- Go minimally specified means type nonempty and (major nonzero OR fifo).
  Preserve its type/numbers; fill absent file mode from the resolution. Otherwise
  fill absent type, require equal host type and fill BOTH major/minor if major0;
  fifo preserves its own numbers. Explicit UID/GID, including0, take precedence;
  absent IDs inherit nonzero process IDs, else stay absent.
- Remove then add device by container path; append exact allow rules for final
  b/c nodes only (default access `rwm`). Retain earlier grants as Go does;
  path replacement does not revoke earlier rules. No broad wildcard grants.
- Mount removal deletes FIRST exact destination, append new mount, then stable
  sort ALL mounts by separator count of Linux-cleaned destination. Parent/child,
  equal-depth and repeated-destination semantics are preserved. Linux lexical
  cleaning is used on every host because this renderer emits Linux OCI bundles.
- Hooks append in their six respective OCI phases with path/argv/env/timeout.
  Additional GIDs skip0 and deduplicate against existing process groups.
- IMPORTANT pinned env quirk: NewFromSpec/createEnvCacheMap uses complete existing
  `KEY=value` strings as keys; addEnv queries `KEY`. Thus first CDI edit appends
  beside an existing key, later same-key CDI edits replace the appended slot.
  Match this exact pinned behavior without introducing an env cache. This is a
  source-derived compatibility limit, not a claimed production defect or fix.
- MAX_CONFIG_BYTES is the existing runtime read bound, used for added input/work
  and final encoded output. Fallible vector additions and checked counts; no new
  arbitrary production bound, cache, registry or resource manager.

Source hashes: Apply `5bc83f07a076ae4d781ae0190c64216629dfe34d00d700ddf034ba4f0785266e`,
Unix `866d5314df8f381305e9950023727c44f7264d5fe0d25a841105734c2ffbb8cf`,
conversion `59d5cc60c8126e27c9b73658efa855da8a8d2561540f551290f37c4ed83291f2`,
generator `113572d199e3d3421eb92b264d06eed112f19aa64654d75efbfa681c5f689fae`.

## Verification scope

SOURCE ONLY until actual selected tests run. Focused pure renderer/CDI tests and
core lib/test Clippy only on d1323898b255a3ed435eb83fa0f7b0cf,
cpus1/mem4g/disk12g, nice10/jobs1/testthreads1, NEW private source/target/cache,
precise no-sync transfer. lib.rs cdi registration only in that private copy;
no unregistered/zero-filter result counts. No native attachment/enforcement,
stat race, hardware, OS portability, full gate, performance or complete-live-device
claim. ROOT owes combined registration/caller review and all those real runs.
Local free-space observation27,239,680KiB (25.98GiB) reported ROOT; no local build
or test, no other-lane cleanup, large transfers held below required30GiB.
