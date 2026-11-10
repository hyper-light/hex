# Shards API seam

Repository: https://github.com/hyper-light/shards
Revision: `33df95b08ad8f95bddeaa1c48acf0c64386e0eac` (main).

The API, ABI, AF_UNIX transport and test-directory crate, license, formatting,
lint configuration and binding policy are copied exactly from this revision.
Cargo.toml is Hex's four-member API-only workspace scaffold (listed as own in
vendor/MANIFEST.toml), retaining upstream package/dependency/lint settings.
No VMM/kernel binary or pin is changed. The former provisional c55 vendor and
Hex borrowed patch are preserved by prior commits; upstream now owns the tested
borrowed implementation. See docs/research/measurements/hex-shards-api-33df.

## Recorded changes

| File | Change | Why | Patch |
| --- | --- | --- | --- |
| crates/api/src/lib.rs | Recognize Windows ARM64 WHP and update its backend regression | Existing VMM implements hv/whp_arm64; supported API must reach it, then let it check real host capability. | 0001-recognize-existing-windows-arm64-whp-backend.patch |
| crates/api/src/{lib,client,sim}.rs and API fixtures | Add GuestSpec.gpu and forward existing --gpu only when true | Expose the existing shared Venus path through the public seam; physical binding still requires runtime mapping. | 0002-expose-existing-shared-venus-gpu-enable-through-guestspec.patch |
| crates/api/src/conformance.rs | Keep only Intel Mac in the explicit excluded-host fixture | Public conformance must agree with Windows ARM64 WHP recognition; actual regression df9f6633 failed. | 0003-align-backend-conformance-with-supported-windows-arm64-whp.patch |

Five exact upstream Rust fixture sources referenced by API include!/path attributes are also copied so the API test targets compile without disabling or stubbing their modules. Their paths are in vendor/MANIFEST.toml; the upstream revision is unchanged.
| crates/api/tests/hex_borrowed_contract.rs | Add ignored coordinator-owned public lifetime and repeated Windows handle checks | Validate the current borrowed contract without reviving the obsolete c55 implementation. | 0004-exercise-public-borrowed-process-lifetime-and-windows-handle.patch |

| crates/api/tests/hex_borrowed_contract.rs | Exercise finite overflow, target death during handshake and exact readiness observations. | AC-T7.4.1 adversarial regression coverage; production API unchanged. | 0005-exercise-finite-overflow-and-post-handshake-target-death.patch |
| crates/api/src/conformance.rs | Check the sole excluded Intel Mac directly. | Native direct API Clippy at17c reproduces single_element_loop after0003; semantics unchanged. | 0006-check-the-sole-excluded-host-directly.patch |
| crates/api/tests/hex_borrowed_contract.rs | Place test safety invariants directly at nested native calls. | Native0116 Clippy requires the existing explanation immediately before unsafe; no behavior changes. | 0007-place-test-safety-invariants-at-each-native-call.patch |
