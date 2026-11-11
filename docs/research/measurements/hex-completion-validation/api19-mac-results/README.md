# API19 private-report Mac results

ROOT actual macOS arm64 checks; OS-private pipes only, no GPU/backend/VM invocation. Each cohort used fresh private source/target/cache/temp,1CPU/4GiB/12GiB,nice10/jobs1/threads1,Rust1.99.1.

Original3d31cb88 v1 exited101 before Rust compilation/tests: `cargo test -p shards-api` cannot test a dev-dependency package excluded from the ROOT workspace. Executed tests0, not an API defect. Driver36353/Cargo36493 reaped,temp0/source0diff.

Changed harness at ba5fa7f2 v2 invokes the actual nested manifest. Exact ROOT Cargo.lock seeds only PRIVATEvendor/shards/Cargo.lock; Cargo pruned it to15registry packages, every name/version/checksum remains a subset of exact ROOT seed18a1c5dd48ba3ee855841e67b59fa38512a15eb769df0b18835c8c828173f082. Pruned lockdcbe9479bcbd6b91c7f1cbe46a59a482e78d8d097bd49a0af91c352880ee9c84 is the sole source difference. No dependency/manifest version change.

- physical::report::tests::3passed/0failed/0ignored, log1e72e4645a630bdf05c5eab8e4a9e39ecebcf9d823c897968f1481e4765a596c.
- APIalltargetClippy101, only report/unix.rs85 `manual_is_multiple_of` for the checked millisecond round-up expression; original loga29974db059aea6396282c634f5e54dc915ed2dcc3dadc09523fcbfcc19cad62 retained.
- ROOT hex-node+hex-cli alltarget cargo check0, logf2442259735794fc196faad7d52cff556d92de1c15df10c9d4181e6714fdee0b.

Whole driver36955 exit1; Cargo37743/37792/37823 reaped,temp0, nativebackend0. LocalCLI38076/61184 returned1; collector66803 returned0/reaped. Raw469402bytes SHAa5dfbeb07ab00ef95ad23ed56cf931188580f9ef5e60487e9f89aa2587ccd27f; allmembers/sourcearchive/before/post hashes independently verified. Executed binary digests are in compressed collected-binaries.json. Inputs/source/targets/caches/original failures remain retained.

L2 owns the one-line equivalent Clippy correction. These three pipe tests do not validate the Windows cancellation/last-owner/later-kernel-completion trace; the b351 fixture order source concern is retained separately and its native run is held for a corrected deterministic original-function control. No prior Slot or seven-test batch was repeated.
