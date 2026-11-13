# Shards API seam

Repository: https://github.com/hyper-light/shards
Revision: `2365d7844f7b4ecdfbba9aa94579b8fc007f3e63` (main).

The API, ABI, AF_UNIX transport and test-directory crates, license, formatting,
lint configuration and binding policy are copied exactly from this revision, with
the measurement fixtures the API's tests include by path. Cargo.toml is Hex's
four-member API-only workspace scaffold (listed as own in vendor/MANIFEST.toml),
retaining upstream package/dependency/lint settings. No VMM/kernel binary or pin is
changed here: the `shards` binary Hex boots GPU guests with is vendor/shards-gpu's.

This revision holds what Hex's node speaks to: the direct boot (D215), launches
through a client's native owner (D233), GPU selection by exact identity (D232), and
an image booted under shards-init for an orchestrator (D231, `shards_api::image`),
with its nesting grant (D234), plugs (D235), destinations by name and the receivers a
published port reaches (D236), and a member's stdin (channel version 6). The take list
adds `docs/research/measurements/api-flakes/spawns.rs`, which the API's tests include by
path at this revision (`crates/api/src/client.rs`).

## Recorded changes

None. Every change Hex carried at 33df95b0 (patches 0001-0031) is upstream: Shards'
hex-upstream-v1 maps each to its commit (docs/research/measurements/hex-upstream-v1/
README.md at this revision), but 0004, 0005, 0007 and 0029, Hex's own campaign over
the borrowed connector, which Shards classed as Hex-only. That campaign is now a test
of Hex's on its dependency, its body unchanged: crates/hex-client/tests/
shards_borrowed_contract.rs.
