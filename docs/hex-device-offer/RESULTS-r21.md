# Original r21: actual candidate red and fixture lint failure

Executed source `c5e806edb0b91395af14a0575b6f74a69979a5dc`, fixed production
`07c68e378b741e3ce6008e1cd612b5e9d9bc3ed2`. Job r21 terminal exit1.
Node lib compilation0; **6selected,5passed,1failed,0ignored**. Production-lib
Clippy0; lib/tests Clippy101 from ONE fixture clone_on_copy warning; rustfmt0.
The next source removes only the unnecessary `room.clone()` (CapacityResources
is Copy). That correction has not yet been executed at this record's checkpoint.
No production fix or changed candidate expectation is included.

## Actual pure observations

The first exact test `late_refusal_does_not_publish_partial_model_interning`
failed101 at the final unchanged-cluster assertion. Runtime Capacity contains a
valid named block1:3 input then GPU PCI12345678:01:02.3 with offered memory1048577
versus synthetic measured1048576. The actual bridge returns typed
Invalid{what:runtime capacity}; Facts/report values remain unchanged. The cluster
model table changes from `[existing-model,nvidia-geforce-rtx-5060-ti]` to the same
table plus `new-block-model`. All other cluster fields are equal in the raw snapshots.
The separate repaired copy, with GPU size1048576, accepts exactly Block+GPU.
The original failed input/cluster snapshots remain intact, not overwritten by repair.

This establishes the pure bridge's error-side model-table update. Classification
and any minimal production fix belong to ROOT. No Service command, publication,
durable row write, native attachment, real GPU access or security effect was executed.
The requested candidate invariant is not silently changed to make this result green.

Other five exact tests passed:

- Native-order keys275 and20015998304531 retain exact model/memory/load; the extra
  telemetry-only GPU gets no offer. Both inputs and full cluster stay unchanged.
- No reports, unknown capacity and empty capability yield three exact empty offers.
- Eight distinct typed Invalid cases preserve full inputs/cluster; eight repaired
  copies accept two intended offers.
- Unheld/current namespace offer is available and existing binder returnsSome([0]);
  foreign namespace/prefix-near-miss/caller ID are unavailable and binder returnsNone.
- Linux/Windows OCI Facts inputs accept two GPU offers each; two native reports
  mislabelled Firecracker/Shards are typed-refused without model-table mutation.

ALL measurements above are synthetic input values in real production types.
No hardware/platform support, actual recovery or native resource claim follows.

## Exact execution and lossless evidence

Runner d1323898b255a3ed435eb83fa0f7b0cf; macOS26.4 arm64; Rust/Cargo1.99.1.
Admitted cpus1/mem4g/disk12g; nice-n10 (actual20 from inherited10), jobs1/threads1,
no-sync. NEW root `~/.cache/hex-device-offer-01a1c640-c5e806ed-v1`, with separate
source/target/cargo/proof. Only registry data was APFS-cloned; no cargo credentials
or configuration copied. No prior/root/sibling target/source/cache was changed.

Input archive7686034B,1708regular members, SHA-256
`15ebda5970b262b11055505c867db0626c08e7a6181884e373b6892f9c081b55`.
Pre-transfer local free218557188KiB, pre-fetch218636724KiB, both above30GiB.
All archived source files were checked before execution and after checks. The only
source difference is the recorded PRIVATE scheduler/tests.rs registration:
beforeSHA `44880daeca4b965320e6314e0d59786cd1533f712007ae4c14c1388384a230a3`,
privateSHA `15138583a472477840ccc3c97bdd44225e58f921451e249d386129996b72db2b`.
The actual worktree/shared parent remains untouched. Binary SHA-256
`a0bfaaa926334cc07e2e2415f1bdee543b4c2147f0fba435ee3431391a636f27`.

[Lossless proof](proof-v1.tar.gz):232738B/54safe regular members, SHA-256
`e6b568438963ad895ebd4c3e1e71a56e5dddeacdd90fd89f3db4bb0f6eae6f79`.
Every original member size/hash, source byte match, private registration, exact
selected count and selected child wait/reap is verified by [receipt](proof-receipt.json).
Original libtest EOF whitespace stays compressed, never normalized. The archive
contains full raw logs/snapshots/commands/PIDs/exits, original driver, source manifest,
postcheck hashes, private parent bytes and exact relevant source files.
[All original source/log hashes](source-and-logs-r21.json).

Original red log `proof/test-0.log` SHA-256
`ff91c8b440d10c6209674301df1b243141dec199cd6afb34f91f20aeaca2d3fb`.
Clippy failure `proof/clippy-tests.stderr` is retained unchanged with its exact
clone_on_copy diagnosis in the same proof. Node production Clippy was0; the
fixture test classpath was not green. No whole-run green claim is made.

Commands and their exact executable paths/argv are in the original proof:

- cargo test -p hex-node --lib --locked --offline --no-run --message-format=json
- Compiled binary --exact scheduler::tests::device_offer_tests::NAME --nocapture
  --test-threads=1, six exact names verified against the compiled test listing.
- cargo clippy -p hex-node --lib --locked --offline --no-deps -- -D warnings
- Same command with --lib --tests for the test classpath.
- Scoped leaf rustfmt --edition2024 --check.

Owned children completed/waited: Cargo38630; six tests38982–38987;
Clippy38988/39039; rustfmt39052. No timeout/kill needed. No fixture sockets,
files, boxes, physical holders or native children were created by these six tests;
their local input state ends with each reaped test process. Source/targets/cache/
binary/raw proof remain frozen. No unrelated deletion/runner/install/VM action.
ROOT still owes candidate classification, production changes if assigned, parent
registration, combined/full gates and actual supported-platform/native proof.
