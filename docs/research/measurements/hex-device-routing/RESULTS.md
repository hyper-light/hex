# Shared device routing: actual regression and correction

ROOT Hex01a1aa77778471339bea3beff90d6bbd reproduced the shared WithGuest
wrapper refusing a valid physical claim before invoking its configured route.
Exact source0aa59e26eb606a43278cc2f8145c5555d5f54d59: one selected test,
0passed/1failed/0ignored; exit101. Actual refusal was Platform, devices, macos.
Original log463db9365cc78cc9084a12f93836d14714a5124d4958d9d1855f0f0e9e4fe5e5.
The fixture observes dispatch only, with no native GPU/VM operation or claim.

Fixed source8fd2572b22751b63aa433564af8506f4e5275d82 removes only the common
wrapper's early check_no_devices for create/run. It preserves spec validation,
routing, cross-backend BoxId collision checks and the selected backend's own
admission and attachment authority. Exactly the unchanged regression then passed:
1passed/0failed/0ignored, followed by runtime all-target native Clippy0.
Test loge26a1f12e3cc56a1a8c3bd6dc3dc855cbd3638afcada7e173f1dda44058535a7;
Clippy4b56abbc0b3d1a51a8f6fdc02f6765b5a7619c587a6310c64c81f4f05e31a625.
This does not establish that each destination has completed its device adapter.
Ada's requirement for real support on every supported OS remains open.

The same independent collection retains exact40d5c644 node all-target
Clippy0 on aarch64-linux-musl, including native block and GPU identity checks,
and3selected Driver model passes/0fail/0ignored on native macOS. Model behavior
and crosscompilation are not physical device effects. Original40d5, red0aa and
green8fd snapshots/targets/binaries/archives/logs remain preserved on d132.

Each job had admitted1CPU/4GiB/12GiB, nice10 requested, private sources/targets/
Cargo homes/temp, jobs1/threads1. Every recorded Cargo child was reaped, all temp
directories empty. The collector verifies actual expected failure/success and
nonzero counts, every raw log hash, post-run complete source manifests and exact
signed Git archives. No test was repeated for collection. Full commands/results/
PIDs and safe archive-member hashes are in receipt.json and proof.tar.gz.
Collected archive SHA256e8d6e4e3d18856a0b77647d4dbda5dc5f466565039c1602476a14383d8420202.
Local frozenproof ~/.cache/hex-lead/device-routing-independent-proof-v1.

A separate current source regression now covers WithGuest.capacity hiding the
native Windows OCI pool behind its Linux guest; its red/fix evidence will be
recorded separately. The current capacity, Apple telemetry, native HCS cleanup,
actual hardware, guest GPU exposure, full matrix and benchmark/profile work are
not completed by this routing control. ROOT owns all those combined/native/heavy
checks, integration review and publication. No platform-refusal/model-only result
is an accepted replacement for the required supported-OS device behavior.
