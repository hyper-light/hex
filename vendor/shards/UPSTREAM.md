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

None. Every change Hex carried at 33df95b0 (patches 0001-0031) is upstream: Shards'
hex-upstream-v1 maps each to its commit (docs/research/measurements/hex-upstream-v1/
README.md at this revision), but 0004, 0005, 0007 and 0029, Hex's own campaign over
the borrowed connector, which Shards classed as Hex-only. That campaign is now a test
of Hex's on its dependency, its body unchanged: crates/hex-client/tests/
shards_borrowed_contract.rs.
