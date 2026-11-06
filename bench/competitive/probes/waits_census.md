# The waits census: each learned wait's answer distribution

Decision 9 of `docs/design/runtime-architecture.md` and the lead's failure-bound rule (2026-11-06): before a node has
an answer of its own, each wait's bound is twice the slowest answer in that wait's recorded distribution for the host
class. This probe records those distributions. Each wait is a named `hex_core::patience::Patience`; with
`HEX_WAITS_CENSUS` naming a file (and `HEX_WAITS_CENSUS_MAX` its most lines), the node and its client append every
answer each wait records, `name nanos`, from the wait's own timing point (`hex_node::census`), so nothing here
re-implements a wait.

Run on fedora under hex-slot, its timed part under `fedora-timed.lock`:
`[N=10000] [CPUSET=4-15] bash bench/competitive/probes/waits_census.sh <hex> <seam_oci>`. It runs seam_oci over the
node's socket (`HEXBENCH_DRIVER=socket`; phases `lifecycle sustained rtt`, N each) in a container with host.sh's
OCI-pass flags and the bar's no-owner conditions, twice: the node's state on the container's tmpfs `/run`, and on the
host's own filesystem (fedora's btrfs, `/opt/disk`). `waits_census.py` reads each census into p50, p99, p99.9, max and
n by wait.

## What each row seeds, and where

| census row | the wait | seeds | where |
| --- | --- | --- | --- |
| `store.turns` (both places) | a record change's wait for the same box's: a turn's hold, its write and fsyncs | `WRITE_PATIENCE`'s seed, the slower place's max | `crates/hex-node/src/store.rs`, `Store::open` |
| `node.turns` | a create or delete's wait for the same box's | `TURN_PATIENCE`'s seed | `crates/hex-node/src/node.rs`, the node's `Turns` |
| `node.emptying` | a killed box's emptying, from its kill to `populated 0` | `EMPTY_PATIENCE`'s and serve's `STOP_PATIENCE`'s seed | `node.rs`, `EMPTYING` |
| `node.listing` | a recovery's listing, per entry | `LIST_PATIENCE`'s seed | `node.rs`, `LISTING` |
| `serve.reads` | a list's worker's wait for its client to read a part | `READ_PATIENCE`'s seed | `crates/hex-node/src/serve.rs`, `READS` |
| `client.answer.<kind>` | a client's call's wait for its answer, by kind | each kind's seed | `crates/hex-client/src/linux.rs`, `ANSWERS` |
| `netcheck.settles`, `netcheck.answers` | a fresh namespace's IPv6 settle; a dump's read | the network check's seeds | `crates/hex-runtime/src/linux/oci/netcheck.rs` |
| `seeds.hang`, `seeds.wait`, `exec_seeds.answers` | the seeds' waits, seeded from `seed_path.md` run 3 already | a check of that record against the census's | `crates/hex-core/src/seeds.rs`, `crates/hex-node/src/seed.rs` |

Until a run is recorded here each of those waits stays MEASURING in the code, naming this file's row. What this probe
can't drive yet, it says so here when a run lands: `serve.reads` needs a list over enough boxes that a part waits for
its reader (seam_oci has no list phase), `node.listing` sees only the listings of a node starting over an empty state
(a restart over N boxes is a phase to add), and the netcheck rows need an owner's boxes (`HEXBENCH_OWNER`), which the
no-owner run leaves out; each such row's n is 0 or small, and its seed waits for the phase that drives it.

## Runs

None yet: fedora hasn't answered kiln since about 14:40Z on 2026-11-06. The lead runs it, or the waits agent once
fedora answers.
