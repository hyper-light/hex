# Shards API seam

Repository: https://github.com/hyper-light/shards
Revision: `66113379cc78b8464c778658e44ace5276ba5590` (main).

The API, ABI, AF_UNIX transport and test-directory crates, license, formatting,
lint configuration and binding policy are copied exactly from this revision, with
the measurement fixtures the API's tests include by path. Cargo.toml is Hex's
four-member API-only workspace scaffold (listed as own in vendor/MANIFEST.toml),
retaining upstream package/dependency/lint settings. No VMM/kernel binary or pin is
changed here: the `shards` binary Hex boots GPU guests with is vendor/shards-gpu's.

This revision holds what Hex's node speaks to: the direct boot (D215), launches
through a client's native owner (D233), GPU selection by exact identity (D232), and
an image booted under shards-init for an orchestrator (D231, `shards_api::image`),
whose channel (version 4) adds each published port's receiving members (D231's
`Bound::to`), destinations kept by name (D236), the nesting grant (D234) and plugs
(D235).

## Recorded changes

Every change Hex carried at 33df95b0 (patches 0001-0031) is upstream: Shards'
hex-upstream-v1 maps each to its commit (docs/research/measurements/hex-upstream-v1/
README.md at this revision), but 0004, 0005, 0007 and 0029, Hex's own campaign over
the borrowed connector, which Shards classed as Hex-only. That campaign is now a test
of Hex's on its dependency, its body unchanged: crates/hex-client/tests/
shards_borrowed_contract.rs. Since this revision Hex carries these, each offered to
Shards' ROOT for upstreaming:

| Files | Change | Why | Patch |
| --- | --- | --- | --- |
| `crates/api/src/image/wire.rs` | `Narrowing::members`: each member's reach of the destinations kept (`MemberReach`), appended to the request as channel version 5, a request naming none written as version 4. Its run host's half, which holds every member of the image to it before the VM starts, is vendor/shards-gpu's change 0002, built into Hex's `shards`. | Joint plan step 15: a harness reaches exactly the providers and the host its grants name. D236 keeps a destination image-wide, and each member reaches those of its own destinations that are kept, so a destination kept for one member is reached as well by every other member whose image names it; Hex names each member's reach, and the run host refuses a boot in which any member would reach other than that (Hex's lead, 2026-11-12; docs/design/network-grants.md N4). | `0002-image-each-member-s-reach-of-the-destinations-kept-which-its.patch` |
