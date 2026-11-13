# Shards API seam

Repository: https://github.com/hyper-light/shards
Revision: `e82b9403b863718930fe9f224186d3ae862bfd61` (main).

The API, ABI, AF_UNIX transport and test-directory crates, license, formatting,
lint configuration and binding policy are copied exactly from this revision, with
the measurement fixtures the API's and its transport's tests include by path. Cargo.toml is Hex's
four-member API-only workspace scaffold (listed as own in vendor/MANIFEST.toml),
retaining upstream package/dependency/lint settings. No VMM/kernel binary or pin is
changed here: the `shards` binary Hex boots GPU guests with is vendor/shards-gpu's.

This revision holds what Hex's node speaks to: the direct boot (D215), launches
through a client's native owner (D233), GPU selection by exact identity (D232), and
an image booted under shards-init for an orchestrator (D231, `shards_api::image`),
with its nesting grant (D234), plugs (D235), destinations by name and the receivers a
published port reaches (D236), and a member's stdin (channel version 6); and an AF_UNIX
read on Windows that waits in WSAPoll, which sees a peer's end that a waiting recv can
sleep through (shards-afunix, 7ea3ce27, D140, PM M536). The take list adds
`docs/research/measurements/api-flakes/spawns.rs` and `half-close.rs`, which the API's and
shards-afunix's tests include by path at this revision (`crates/api/src/client.rs`,
`crates/afunix/src/lib.rs`).

## Recorded changes

Every change Hex carried at 33df95b0 (patches 0001-0031) is upstream: Shards'
hex-upstream-v1 maps each to its commit (docs/research/measurements/hex-upstream-v1/
README.md at this revision), but 0004, 0005, 0007 and 0029, Hex's own campaign over
the borrowed connector, which Shards classed as Hex-only. That campaign is now a test
of Hex's on its dependency, its body unchanged: crates/hex-client/tests/
shards_borrowed_contract.rs. The changes below are Hex's since, each offered upstream
(`vendor/patches/shards/*.patch`, applied in name order).

| Files | Change | Why | Patch |
| --- | --- | --- | --- |
| `crates/api/src/image/spawn.rs` (new), `crates/api/src/image/session.rs`, `crates/api/src/image.rs`, `crates/api/Cargo.toml` | D231's run host (`shards api-run`) is started by `image::spawn`, not std's `Command`: on Unix by posix_spawn(3), file actions putting its stdin, stdout and stderr on 0, 1 and 2, its signal mask emptied and SIGPIPE at its default, with `POSIX_SPAWN_CLOEXEC_DEFAULT` on Apple's; on Windows by CreateProcessW with a `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` of inheritable copies of those three alone (windows-sys `Win32_Security` for it). Its wait, kill and reap move with it. `a_run_host_holds_no_end_it_was_not_given` (macOS, Windows; in a test process of its own) holds it to that. | std's spawn hands the run host what another thread of its orchestrator holds inheritable at that moment, for as long as its VM runs (CVE-2024-21626's class): macOS's pipe, socket, socketpair and accept take no close-on-exec flag, and std's CreateProcessW inherits with no handle list. Hex starts every program holding its stdio alone, so it booted no image VM on macOS or Windows; Ada (2026-11-12): the image class works there too (Hex joint plan step 12, AC-T7.4.1). Red then green on macOS (HVF Mac) and Windows (WHP, jupiter). | `0001-image-the-run-host-started-holding-its-three-standard.patch` |
| `crates/api/src/image/wire.rs` | `Narrowing::members`: each member's reach of the destinations kept (`MemberReach`), appended to the request as channel version 9 (the next after Shards main's 7, D241, and 8, D242), a request naming none written as version 6. Its run host's half, which holds every member of the image to it before the VM starts, is vendor/shards-gpu's change 0002, built into Hex's `shards`. | Joint plan step 15: a harness reaches exactly the providers and the host its grants name. D236 keeps a destination image-wide, and each member reaches those of its own destinations that are kept, so a destination kept for one member is reached as well by every other member whose image names it; Hex names each member's reach, and the run host refuses a boot in which any member would reach other than that (Hex's lead, 2026-11-12; docs/design/network-grants.md N4). | `0002-image-each-member-s-reach-of-the-destinations-kept-which-its.patch` |
