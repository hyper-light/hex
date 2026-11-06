# The waits census: each learned wait's answer distribution

Decision 9 of `docs/design/runtime-architecture.md` and the lead's failure-bound rule (2026-11-06): before a node has
an answer of its own, each wait's bound is twice the slowest answer in that wait's recorded distribution for the host
class. This probe records those distributions. Each wait is a named `hex_core::patience::Patience`; with
`HEX_WAITS_CENSUS` naming a file (and `HEX_WAITS_CENSUS_MAX` its most lines), the node and its client append every
answer each wait records, `name nanos`, from the wait's own timing point (`hex_node::census`), so nothing here
re-implements a wait.

Run on fedora under hex-slot, its timed part under `fedora-timed.lock`:
`[N=10000] [CPUSET=4-15] [LIST_BOXES=N] [LISTS=100] [RESTART_BOXES=N] bash bench/competitive/probes/waits_census.sh <hex> <seam_oci>`.
It runs seam_oci over the node's socket (`HEXBENCH_DRIVER=socket`) in a container with host.sh's OCI-pass flags, in
three arms, each with a census file of its own; `waits_census.py` reads each into p50, p99, p99.9, max and n by wait:

| arm | the node's state | owner | phases |
| --- | --- | --- | --- |
| `tmpfs` | the container's tmpfs `/run` | none (the bar's no-owner conditions) | `lifecycle sustained rtt list restart` |
| `disk` | the host's own filesystem, fedora's btrfs (`/opt/disk`) | none | the same |
| `owner` | tmpfs | `HEXBENCH_OWNER=bench`, as host.sh's reuse-on pass | `lifecycle sustained` |

seam_oci's `list` phase makes `LIST_BOXES` boxes and lists them `LISTS` times, so a list's parts wait for their
reader; its `restart` phase makes `RESTART_BOXES` boxes, ends the agent with SIGTERM and starts it again over the
same state, so recovery lists their parts, then deletes them. Each is bounded by the node's own: a box past its
budget is refused, and the arm's error says so.

## What each row seeds, from which arm, and where

A row seeds its wait only with **n of at least 1,000**: the seed is the row's max, and with fewer samples than that the
nearest-rank p99.9 is the max itself, so the tail the seed must cover hasn't been seen. `waits_census.py` writes no
seed for a row under it, and names the row's n.

| census row | the wait | arm that seeds it | seeds | where |
| --- | --- | --- | --- | --- |
| `store.turns` | a record change's wait for the same box's: a turn's hold, its write and fsyncs | `tmpfs` and `disk`: the slower arm's | `WRITE_PATIENCE` | `crates/hex-node/src/store.rs`, `Store::open` |
| `node.turns` | a create or delete's wait for the same box's | `tmpfs` | `TURN_PATIENCE` | `crates/hex-node/src/node.rs`, the node's `Turns` |
| `node.emptying` | a killed box's emptying, from its kill to `populated 0` | `tmpfs` | `EMPTY_PATIENCE`, serve's `STOP_PATIENCE` | `node.rs`, `EMPTYING` |
| `node.listing` | a recovery's listing, per entry | `tmpfs` and `disk` (`restart`): the slower | `LIST_PATIENCE` | `node.rs`, `LISTING` |
| `serve.reads` | a list's worker's wait for its client to read a part | `tmpfs` (`list`) | `READ_PATIENCE` | `crates/hex-node/src/serve.rs`, `READS` |
| `client.answer.<kind>` | a client's call's wait for its answer, by kind | `tmpfs` | each kind's | `crates/hex-client/src/linux.rs`, `ANSWERS` |
| `netcheck.settles`, `netcheck.answers` | a fresh namespace's IPv6 settle; a dump's read | `owner` | the network check's | `crates/hex-runtime/src/linux/oci/netcheck.rs` |
| `netns.make` | the pool's refill thread's make of a network namespace (no node wait; the census's timing point) | `tmpfs` | the netns tests' patience | `crates/hex-node/src/netns/tests.rs`, `MAKE_RECORDED` |
| `lifecycle.exec` (from `seam-tmpfs.json`) | a container's exec of `true`, to the program's end | `tmpfs` | the conformance suites' patience, and every test that takes it | `crates/hex-api/src/conformance.rs`, `PROGRAM_END` |
| `client.answer.inspect` | the server's answer to a call over its socket | `tmpfs` (`rtt`) | the socket's tests' patience | `crates/hex-node/src/serve/tests.rs`, `ANSWER_RECORDED` |
| `seeds.hang`, `seeds.wait`, `exec_seeds.answers` | the seeds' waits, seeded from `seed_path.md` run 3 already | `tmpfs` | a check of that record | `crates/hex-core/src/seeds.rs`, `crates/hex-node/src/seed.rs` |

Until a run is recorded here each of those waits stays MEASURING in the code, naming this file's row. Where a row's n
is short of 1,000 (`client.answer.hello` is one a connection; `node.listing` a restart's entries; `serve.reads` only
the parts that waited), the run says so and the arm's count is what to raise.

## Runs

None yet: fedora hasn't answered kiln since about 14:40Z on 2026-11-06. The lead runs it, or the waits agent once
fedora answers.
