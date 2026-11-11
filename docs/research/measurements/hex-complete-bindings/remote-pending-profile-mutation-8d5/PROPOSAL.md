SOURCE ONLY. This is one proposed R10 guard mutant against fixed Hex 8d5bb0b4eb6a175532d73d889dc472b721660b41.
The production worktree and old tests remain untouched. No patch application,
Cargo invocation, mutation execution, native call or runner dispatch was performed
by L1. ROOT owns the two admitted executions and their exact process/deadline/raw
receipts:1CPU/4GiB/12GiB, nice10/jobs1/threads1, separate fresh private targets.

The only hunk retains evaluation of `original.check_backing(backing).is_ok()`
in the Released dedicated branch, assigns its Boolean to `_profile_checked`,
then omits enforcement in the return. The underscore binding is deliberate so the
value remains evaluated without a compiler-only unused-variable failure. There
is no added lint allowance, fallback profile, DTO, state owner or physical grant.
Its compilation is unverified until ROOT runs it.

The unchanged exact unit is `index::route_offer_tests::committed_pending_recovery_keeps_receipt_charge_and_claim_bounds`. Missing/foreign receipts, missing or malformed
charge and host/resource checks still execute. Its otherwise-valid foreign cost
profile has a different identity, but an exact P receipt and sufficient room.
The mutant should therefore accept that case and fail the existing assertion
with `foreign profile`, before later cases. A compiler error, selection0, a failure
at another precondition or a timeout is UNVERIFIED for this mutation purpose.

ROOT should require mutant1selected/0pass/1semanticFail/0ignored and restored
1selected/1pass/0fail/0ignored, using the SAME unchanged unit and installed toolchain.
These are expectations, not observed results. Baseline ROOT8d5 already has3/3
focused passes and hex-sim Clippy0; this proposal does not replay them or prove
native GPU identity, execution, full-scope release or any native forbidden effect.

Exact content identities:

| Object | Bytes | SHA256 |
| --- | ---: | --- |
| Restore `crates/hex-sim/src/index/routes.rs` | 28134 | `cda46b5fee827aaab61a9e2fcf7f1e9b7085c596017cbd7de3d3d2ebabb399fc` |
| Proposed mutant file | 28151 | `e9fbf5d8aafdf9b12755a1d07f91ce21842a61a4beb53a9cd098ba267033b1c6` |
| Unchanged `crates/hex-sim/src/index/route_offer_tests.rs` | 49835 | `795f0d8ee79eaa8226905c5dc22bdc142942be1f8f1ab03cbfb3a48142a9a614` |
| Exact one-hunk fixture | 641 | `af92e293ddf6b7434c3a4b9e1cbc775586f09c1d8277438c1ec7a55266b7798d` |

ROOT command recipe below assumes its fresh, source-verified private8d5 tree,
this exact fixture copied outside production source, an installed fixed cargo,
and already-admitted cgroup/deadline guards. The variables must name ROOT-owned
absolute private paths. No runner/service/toolchain setup is part of this recipe.
Retain each Cargo exit/PID/count/log and cleanup receipt with the existing ROOT
driver; semantic failure must be checked independently of exit101.

```sh
: "${profile_source:?fixed8d5 private source directory}"
: "${profile_patch:?exact profile-enforcement.patch}"
: "${profile_cargo:?fixed installed cargo binary}"
: "${profile_rustc:?fixed installed rustc binary}"
: "${profile_mutant_target:?fresh private mutant target}"
: "${profile_restored_target:?different fresh private restored target}"
: "${profile_cache:?ROOT private Cargo cache}"
: "${profile_tmp:?ROOT private temporary directory}"
test "$profile_mutant_target" != "$profile_restored_target"
test ! -e "$profile_mutant_target"
test ! -e "$profile_restored_target"
printf '%s  %s\n' 'af92e293ddf6b7434c3a4b9e1cbc775586f09c1d8277438c1ec7a55266b7798d' "$profile_patch" | sha256sum --check -
printf '%s  %s\n' 'cda46b5fee827aaab61a9e2fcf7f1e9b7085c596017cbd7de3d3d2ebabb399fc' "$profile_source/crates/hex-sim/src/index/routes.rs" | sha256sum --check -
printf '%s  %s\n' '795f0d8ee79eaa8226905c5dc22bdc142942be1f8f1ab03cbfb3a48142a9a614' "$profile_source/crates/hex-sim/src/index/route_offer_tests.rs" | sha256sum --check -

git -C "$profile_source" apply --no-index --check "$profile_patch"
git -C "$profile_source" apply --no-index "$profile_patch"
printf '%s  %s\n' 'e9fbf5d8aafdf9b12755a1d07f91ce21842a61a4beb53a9cd098ba267033b1c6' "$profile_source/crates/hex-sim/src/index/routes.rs" | sha256sum --check -
printf '%s  %s\n' '795f0d8ee79eaa8226905c5dc22bdc142942be1f8f1ab03cbfb3a48142a9a614' "$profile_source/crates/hex-sim/src/index/route_offer_tests.rs" | sha256sum --check -
cd "$profile_source"
env RUSTC="$profile_rustc" CARGO_BUILD_JOBS=1 RAYON_NUM_THREADS=1 OMP_NUM_THREADS=1 \
  CARGO_TARGET_DIR="$profile_mutant_target" CARGO_HOME="$profile_cache" TMPDIR="$profile_tmp" \
  nice -n 10 "$profile_cargo" test -p hex-sim --lib --locked \
  index::route_offer_tests::committed_pending_recovery_keeps_receipt_charge_and_claim_bounds \
  -- --exact --nocapture --test-threads=1
```

ROOT must record the intentional unit failure and continue restoration through
its existing driver's error/finally path; shell `set -e` must not skip restoration.
If any mutation-phase step fails early, first verify the actual file hash before
choosing reverse application. Do not blindly overwrite an unexpected state.
The following restoration assumes the exact proposed mutant hash is present:

```sh
printf '%s  %s\n' 'e9fbf5d8aafdf9b12755a1d07f91ce21842a61a4beb53a9cd098ba267033b1c6' "$profile_source/crates/hex-sim/src/index/routes.rs" | sha256sum --check -
git -C "$profile_source" apply --no-index --reverse --check "$profile_patch"
git -C "$profile_source" apply --no-index --reverse "$profile_patch"
printf '%s  %s\n' 'cda46b5fee827aaab61a9e2fcf7f1e9b7085c596017cbd7de3d3d2ebabb399fc' "$profile_source/crates/hex-sim/src/index/routes.rs" | sha256sum --check -
printf '%s  %s\n' '795f0d8ee79eaa8226905c5dc22bdc142942be1f8f1ab03cbfb3a48142a9a614' "$profile_source/crates/hex-sim/src/index/route_offer_tests.rs" | sha256sum --check -
cd "$profile_source"
env RUSTC="$profile_rustc" CARGO_BUILD_JOBS=1 RAYON_NUM_THREADS=1 OMP_NUM_THREADS=1 \
  CARGO_TARGET_DIR="$profile_restored_target" CARGO_HOME="$profile_cache" TMPDIR="$profile_tmp" \
  nice -n 10 "$profile_cargo" test -p hex-sim --lib --locked \
  index::route_offer_tests::committed_pending_recovery_keeps_receipt_charge_and_claim_bounds \
  -- --exact --nocapture --test-threads=1
```

After restoration, ROOT should compare its original whole-source manifest,
not only the touched file/test hashes; retain any difference rather than clean it
silently. Other old passes are not part of this two-phase R10 purpose. Actual
native effects, supported VM/platform coverage, allocation/profile audit and
full gates/main remain separately ROOT-owned and unverified by this fixture.
