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

No source patches at this checkpoint.
