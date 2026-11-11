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

| crates/api/src/{lib,client,sim,physical}.rs and API fixtures | Carry exact native GPU identities through GuestSpec, validate the set, and forward canonical --gpu-device arguments. | AC-T7.4.1: reviewed upstream source36d9254 provides the typed physical-selector seam while all existing borrowed and backend patches remain intact. | 0008-carry-exact-native-gpu-selectors-through-the-public-api.patch |

| crates/abi/src/{lib,gpu}.rs and crates/api/src/{physical,client}.rs | Share one core+alloc physical identity/parser type between API and VMM, retaining OS argument encoding in API. | AC-T7.4.1: Shards ROOT requires one actual type, not copied parsers; reviewed source42c3193 changes ABI identity and requires a matching rebuilt cohort. | 0009-share-exact-physical-gpu-identity-and-validation-through-abi.patch |

| crates/api/src/{lib,client,sim,physical}.rs and crates/abi/src/gpu{,/probe}.rs | Expose prepared/managed boot over one native receipt, positive process versus full-scope terminal facts, and exact GPU probe metadata. | AC-T7.4.1: signed API-only source b19c86d26c3d4c9f029118084f0461305808a4ad permits native bound launch without a second process record; physical joins retain UUID, complete PCI and valid raw LUID. Borrowed connector and handshake bytes remain unchanged. | 0010-expose-native-managed-receipts-and-exact-gpu-probe.patch |

| crates/api/src/lib.rs | Retain typed native scope operations/errors before controller identity publication. | AC-T7.4.1: signed bb763aa3b175b755c8b85c2115722d6ad749088b permits cleanup of the same known scope without fabricating pid/start values. | 0011-preserve-typed-native-scope-errors-before-controller.patch |

| crates/api/src/{client,lib,sim}.rs | Publish managed stop intent before inline completion; add model lifetime controls and correct simulator observation. | AC-T7.4.1: signed219f804e follows an actual74481869 inline-stop failure and the retained5d8 compilation failure. | 0012-publish-managed-stop-intent-before-synchronous-native.patch |

| crates/api/{Cargo.toml,src/lib.rs,src/client.rs} | Pass the canonical fallibly allocated native receipt through managed launch/attach without a wrapper; use the stable checked atomic operation name. | AC-T7.4.1: signed eee80c12 API delta plus ROOT-owned exact triomphe unsize / unsize1.1.0 dependency overlay; upstream peer Cargo/main untouched. | 0013-transfer-the-same-fallibly-allocated-native-receipt-to-the.patch |

| crates/abi/src/gpu/probe.rs | Use fixed two-byte chunks and the equivalent single native PCI predicate. | AC-T7.4.1: exact9e6243da follows actual L4 ABI strict-Clippy101, preserving checked width and physical identity refusal semantics. | 0014-use-fixed-probe-chunks-and-the-equivalent-native-pci-guard.patch |

| crates/api/src/{client,lib}.rs | Carry configured node runtime roots, prepared host-managed containment marker, query-only GPU arguments and typed scope mismatch. | AC-T7.4.1: reviewed source aa7e028508383e58b55d622a299b263bfac064b0; probe/native roots stay explicit and borrowed/standalone argument bytes are unchanged. Parent/VMM candidate and native behavior still require joined validation. | 0015-carry-typed-node-runtime-paths-and-native-scope-observations.patch |

| crates/abi/src/gpu{,/probe}.rs and crates/api/src/{lib,physical,client}.rs | Carry actual selected renderer deviceName and DEVICE_LOCAL heap bytes with a versioned bounded probe. | AC-T7.4.1: reviewed API-only eff3d055; None/known zero stay distinct, older v2 properties remain unknown, and borrowed/standalone argument bodies stay exact. Matching renderer/VMM metadata and native effects remain separately validated. | 0016-preserve-actual-renderer-name-and-device-local-memory-in-probe.patch |

| crates/abi/src/gpu{,/probe}.rs and crates/api/src/physical.rs | Add bounded native-scope report frames and render final transferred report/lifeline descriptors. | AC-T7.4.1: reviewed API-only 218ef6df over e980; scope text alone grants no native authority, complete client/lib files remain unchanged, and the matching cancellable observer/VMM/native proof remains separate. | 0017-carry-bounded-private-gpu-reports-and-final-native.patch |

| crates/abi/src/gpu/probe.rs and crates/api/src/physical.rs | Import the no-std formatting macro in tests and compare final native arguments using OsString. | AC-T7.4.1: exact signed 9ef468c522f65129ec8bbb927e6b5cfe1505a7e6 / e6ccf56990488abfade07bd9123ce0ad09328cfc correct reproduced test compilation failures; production API/ABI unchanged. | 0018-correct-bounded-gpu-report-test-imports-and-native-argument.patch |

| crates/api/{Cargo.toml,src/lib.rs,src/client.rs,src/physical{,/report,/report/unix,/report/windows}.rs} | Carry a private bounded GPU report request/writer through the existing managed native owner; retain pending Windows IO storage and subscribe to actual completion after cleanup errors. | AC-T7.4.1: reviewed fff4548b/5ba9ca50/2d1f77f4 plus fixture-only b3510027; existing borrowed, supported-backend and shared-Venus patches remain intact. Windows native error/late-completion proof and coherent final launcher/VMM validation remain separate. Only the existing windows-sys Pipes feature is added; no dependency version changes. | 0019-observe-scoped-gpu-report-completion-through-the-native-owner.patch |

| crates/api/src/{client.rs,physical/report/unix.rs} | Order the native late-completion regression before last API drop and use the equivalent checked poll rounding expression. | AC-T7.4.1: source39489068 corrects the old control ordering without production observer changes; e4411330 fixes actual macOS API Clippy101 after3 private-pipe passes. Windows original-function replay/native proof remains pending. | 0020-order-late-completion-control-and-preserve-native-poll-rounding.patch |

| crates/api/src/client.rs | Preserve test-model timeout/identity conditions as typed Observer refusals. | AC-T7.4.1: exact89936ab9 fixes two actual panic_in_result_fn diagnostics atROOT5de strict API test Clippy; production code and the original-window ordered native control remain unchanged. | 0021-return-typed-test-model-refusals-for-inline-wait-and-identity.patch |

| crates/api/src/client.rs | Retain the original finite owned-retry expiry regression before any remedy. | AC-T7.4.1: signed2ee4410f adds only a controlled socket/expiry test; production prefix stays byte-identical and actual result remains pending. | 0022-retain-finite-owned-retry-deadline-regression.patch |

| crates/api/src/{lib,client,sim}.rs | Add absolute owning connections and preserve the original finite retry deadline. | AC-T7.4.1: reviewed b93/b287/1b93 uses the existing native owner and fixes actual db481f0e late dial; borrowed and owning zero/MAX compatibility remain separate. | 0023-carry-absolute-owning-guest-connections-and-preserve-finite.patch |

| crates/api/src/{lib,client,said,native,native/tests}.rs | Retain typed native startup inputs and verify complete declared images from the same original cost evidence. | AC-T7.4.1: reviewed source7e306/4b0e/68941; existing borrowed, absolute deadline, runtime owner and ARM patches preserved. Checked open Files are content witnesses, not executed-image or native-scope authority. New SDK3/Hex2 source controls initially unrun; native/worker/TOCTOU/resource proof remains separate. | 0024-bind-native-startup-images-to-existing-cost-evidence.patch |

| docs/research/measurements/managed-proc-main/reader-tests.rs | Add gpu:false and empty gpu_devices to the original CPU-only process reader GuestSpec. | AC-T7.4.1: ROOT786 standalone SDK compilation reproduced E0063 before any selected tests; the prior fixture predates0002/0008. Only required field defaults, original assertions/process/refusal behavior unchanged. | 0025-keep-original-process-reader-fixture-cpu-only.patch |

| crates/api/src/physical/report/windows.rs | Express the existing nanosecond-to-millisecond nonzero remainder using is_multiple_of. | AC-T7.4.1: exact source511f927 after actual786 Windows standalone API Clippy101; same checked ceiling, original budget, bounds and observer semantics. No new allowance/tolerance/unsafe or native claim. | 0026-use-equivalent-native-poll-ceiling-predicate.patch |

| crates/api/src/physical/report.rs | Make one deadline recheck of take() observe expiry through a test-only clock, and test the frame and parse rechecks each alone. | AC-T7.4.1: 0019/0020's rechecks had no direct test. Each mutation that removes one recheck fails only its own test; production still reads Instant::now(). | 0027-test-each-deadline-recheck-of-the-private-gpu-report-take.patch |

| crates/api/src/{lib,client,sim,conformance}.rs | Add Guest::connect_once_before: one dial under the caller's absolute deadline, refused at once as closed unanswered or in the dial when nothing listens, and by the deadline when nothing answers. The client, the simulator and a new conformance case agree. | AC-T7.4.1: connect_before retries until its deadline, so a later dial to a gone relay waited out the learned boot bound. A mutation that lets the single attempt retry dials 563 times and fails its test; the retrying form is unchanged. | 0028-add-one-bounded-connect-attempt-for-a-port-the-guest-has.patch |

| crates/api/src/{lib,taken}.rs, crates/api/src/taken/reader_tests.rs (moved from docs/research/measurements/managed-proc-main/reader-tests.rs) | Run the three /proc/self/fd-counting reader tests each alone in a child test process, and restore upstream's Refusal::Process source() (its failure's own source). | AC-T7.4.1: four taken::linux tests failed on main. Three compared whole-process descriptor counts while tests on other threads opened theirs (all pass with --test-threads=1). The fourth caught 0011 returning Process's failure as source(), which a printed chain repeats after Display. | 0029-run-descriptor-count-reader-tests-alone-and-keep-process.patch |

| crates/api/src/client.rs | The two take-back tests also accept Os { OpenStat, ENOENT } after a stop: the guest reaped between Held::find's pidfd open and its /proc stat open. | AC-T7.4.1: attach_takes_back_a_guest_whose_client_crashed (and _handle_was_lost) failed intermittently on main with exactly that typed refusal; every other phase of the same race was already accepted. Production's refusal is unchanged. | 0030-accept-the-reap-between-pidfd-and-stat-in-take-back-tests.patch |
