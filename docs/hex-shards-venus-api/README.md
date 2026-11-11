# Existing shared Venus enable at the Shards API

Signed source-first3f now has [actual focused results](RESULTS.md): API harness
compile0 and4selected/4pass/0fail/0ignored, with both earlier preparation failures
retained. This exposes the existing shared Venus enable bit. **It does not select,
reserve or exclusively attach a physical GPU.** All supported-platform attachment
and exact host/guest identity mapping remain required.

## POLICY_READ and scope

Implementer Kiln01a1c640e6ed710d8a8c919b4aa1fc8c; coordinator
ROOT01a1aa77778471339bea3beff90d6bbd. Branch cp/t7-shards-venus-api from EXACT
`0ba6a60f582d461e65673ad736b2ac658cee9b72`. Preserved cp/t7-device-offer at2cf4410f
and every earlier source/raw ref. No native jobs or unrelated source mutations.

Fully read current fixed vendor/shards AGENTS.md, docs/development-rules.md R01–R16,
docs/development-evidence.md and CLAUDE.md. Also verified current Shards origin/main
`33df95b08ad8f95bddeaa1c48acf0c64386e0eac` AGENTS/rules/evidence against canonical
g121 checkout; no older branch waives current policy. Rules SHA-256
`df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3`.
Full current ROOT Hex CLAUDE.md SHA-256
`358e8baa67af459463e2c8e06e90f106bcfdbe15a820875a1a37700703b953a0` and current
Hex HANDOFF-codex.md SHA-256
`8173b744fbc5dece1fb1fd130472589550f9426cbac19e42d7c37a8a6d520996` read.
POLICY_READ stable key shards-venus-policy-33df-0ba6-v1; ROOT explicitly ACKed
the exact API-only scope before edits. Managed plan/g121 handoff at33df were fully
read in the preceding scope; their unchanged open obligations remain in force.

Own ONLY vendor/shards/crates/api GuestSpec/arguments and their spec fixtures,
plus this directory. ROOT owns outside-Hex GuestSpec literals, GuestConfig wrapper,
vendor patch0002/metadata, physical selector/mapping, VMM/kernel/upstream-main
changes, actual native/VM effects, heavy checks/audits/profiles/benchmarks and merges.
Existing0001 Windows ARM64 backend_of code is byte-preserved. The unrelated stale
Windows ARM conformance expectation was reported; ROOT explicitly owns its separate
reproduction/fix. No backend/platform refusal is accepted as overall completion.

Explicit Hex light-only scope: no laptop builds/tests/benchmarks, no hardware,
VM/native/E2E/mutation/sweep/full gate/campaign, no children/goals/plans. Focused
API units/compile only on approved d1323898b255a3ed435eb83fa0f7b0cf with
cpus1/mem4g/disk12g,nice10,jobs1/threads1,NEW private source/target/cache,no-sync.
Preserve other lanes, snapshots, targets, guests and jobs. Local30GiB floor before
transfers; stop only exact recorded owned handles/PIDs; no secrets/install/restarts.

## Exact interface and behavior

```rust
pub struct GuestSpec {
    // Existing fields unchanged.
    pub gpu: bool,
    // Existing fields unchanged.
}
```

`gpu:false` emits the exact prior argument list. `gpu:true` appends exactly one
`--gpu` to the same existing arguments Result path. Both Init::Executable and
Init::Initrd retain their original flags, disks, shares, vsock and console values.
The bit does not change CPU/memory validation, backend selection or an allowlist.
It is a required Rust struct field; existing full API spec fixtures setfalse and
struct-update fixtures inherit it. ROOT adapts callers outside this API crate.
SimHost retains the complete requested GuestSpec through the existing clone path.

Primary source: Shards33df
[vm_run.rs CLI](https://github.com/hyper-light/shards/blob/33df95b08ad8f95bddeaa1c48acf0c64386e0eac/crates/shards/src/vm_run.rs#L48)
documents Vulkan through shared Venus; its parser line474 maps --gpu to cfg.gpu=true.
Those exact source lines were read locally using git show origin/main. No new GPU
framework, VMM, renderer, manager, binary, resource default or device selector is added.
The existing argument builder gains at most one fixed five-byte CLI token; its
existing argument allocation contract is retained. This is not allocation-fault,
native GPU isolation, rendered output or full vendor toolkit validation.

Focused checks planned after the signed source checkpoint:

- Existing exact false argument list, including both init forms.
- New true case equals the whole false list plus one --gpu, for both init forms.
- Existing bounds fixture for both bool states, including zero CPUs/odd/low memory.
- New simulator receipt for both bool states: full spec equality after ordinary
  model power-off/stop/drop, explicitly not a hardware effect.

Compile the owned API unit/integration harnesses only; never execute real.rs,
native start/attach scripts, measurements, fault/descriptor campaigns or the whole
conformance suite. The known unrelated Windows ARM conformance mismatch remains
ROOT-owned. Tests use existing fixture values/min_memory_mib and existing learned
PATIENCE; no guessed waits or limit increases. Actual selected counts/raw logs/
source hashes/owned cleanup and limitations will follow in RESULTS.md.

R01/R04/R06/R07/R13: minimal boolean/argument branch at existing Result seam;
typed checks retained, test assertions only, explanatory shared/exclusive boundary.
R02/R03/R05/R12: pinned existing Config/CLI semantics, one path, no new tuning,
cache/wait/manager/adapter. R08/R14: no performance claim; ROOT owns profiling.
R09/R10: requested missing API exposure, source-first not a native bug reproduction;
original states and exact evidence preserved. R11: pure/model checks cannot prove
all-OS effects; Linux GNU/musl and Windows amd64/arm64, macOS arm64 remain required,
Intel Mac excluded. R15/R16: scoped signed implementation/light checks only,
ROOT integration/merges and resource/evidence isolation retained.
