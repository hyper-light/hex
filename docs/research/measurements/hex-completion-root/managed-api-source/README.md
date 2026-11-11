# AC-T7.4.1: native-managed Shards API intake

ROOT applies only the six API/ABI paths from signed source
`b19c86d26c3d4c9f029118084f0461305808a4ad` in the isolated upstream API worktree.
Its API-only chain is b19 -> e761 ->42c3193e ->36d9254f ->327424a1 ->0342.
The delta starts at42c, already represented by Hex vendor patch0009. VMM,
renderer, carried binaries and the upstream main pin are not replaced here.
`intake.patch` and `source.json` retain the exact input and hashes.

PreparedBoot performs checked preparation; ManagedLauncher supplies the same
native receipt owned by the canonical native scope. The API retains its existing
output/drain/watch protocol and fallibly allocated protocol ownership.
ManagedGuest::try_clone counts API lifetime handles separately from its worker
references. ShardsLink::managed stores that value directly, without allocating a
second Arc or process record. Existing ShardsLink::new remains available.

An unpublished controller has a fallible identity getter, never a pid/start-zero
substitute. Process exit is a distinct fact from the complete native envelope's
Released outcome. ManagedProcess::wait_for returning Some requires Released;
an error or surviving helper must not free scheduler/device reservations.
Native producers and recovery still have to establish those facts.

The shared ABI adds format2 GPU probe receipts: exact selector/UUID plus optional
complete PCI address and valid native LUID bytes. No ordinal, model-name match or
invented PCI segment is introduced. The ABI identity changes and all carried
host/guest/probe artifacts must be rebuilt together before native use.

The public borrowed connector and absolute-handshake function bytes were compared
and are identical to the pre-intake Hex copy. All previous backend, borrowed and
test patches remain. New vendor patch0010 records only this incremental API/ABI
delta. The existing test doubles expose their actual model terminal state or
typed unavailable probe; wrappers around real Shards delegate the probe. Exact
test compatibility spans are listed in `test-compatibility.json`.

This checkpoint is source integration, not compilation or native proof. The
implementer checks its signed SDK closure separately. ROOT still owes the coherent
native launch/attach consumers, original-allocation projection, runtime provider,
process/helper release, GPU joins/compute and final platform/full gates. The
standalone probe launch and legacy spawn path must not be confused with proof of
a bound native VM launch.

The initial local vendor-record invocation used system Python3.9 and refused
before recording because tomllib requires3.11. It changed no source. Reissuing
with the already installed Homebrew Python succeeds; no interpreter installation
or runner change was made. Policy revision/roles remain ca3ab10c / df2acf47 as
recorded in the current ROOT receipt.

## Fallible native receipt intake

Signed upstream API delta219f->eee80c12 changes only managed ownership and the stable checked atomic operation name. ROOT applies that delta while preserving all earlier Hex patches. The minimal dependency overlay enables existing triomphe0.1.17 unsizing and exact unsize1.1.0 (8,060B crate/checksum4fa7a7a7...). It is a Hex vendor candidate change; the peer upstream manifests and main remain untouched. Exact source/lock hashes and unchanged borrowed function hashes are in fallible-receipt-intake.json. The reviewed private-workspace seed and its precise delta are retained in managed-receipt-Cargo.lock / managed-receipt-seed.json. See docs/design/managed-native-receipts.md for alternatives and remaining allocation/native proof. This intake is SOURCE ONLY until its own checks finish.
