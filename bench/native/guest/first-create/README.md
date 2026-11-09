# HEX-GUEST-39: an upgrade's first create, before and after the node boots its guest when made

An upgrade's first boot costs 1.5-5.3 s of the host's first-run checks of its new code before the guest's kernel
runs (bench/native/guest/boot-ready/RECORD.md, the under-load record). A node that boots its guest at the first
create puts that on a user's create. One that boots it when made (HEX-GUEST-39, `WithGuest::new`) pays it before
it accepts a box. This measures what a user waits for, the first create after an upgrade, in both.

- `crates/hex-cli/tests/guest_first_create.rs`: one process makes the node (`WithGuest` over a real Shards guest)
  and creates and deletes one `oci` box (alpine, by its blobs), timing the making and the create. The same
  file builds against both trees.
- `pair.sh`: each turn is one upgrade (a fresh copy of Shards and of its VM process, each re-signed under a
  new identifier, the VM process with Shards' `vm.entitlements` and Hardened Runtime, and a fresh
  `SHARDS_CACHE`), then one run of an arm's binary. Turns go ABBA, under the host's own co-tenant load,
  written before each turn. No turn is dropped.
- `summary.py`: per arm, the making and the first create, p50 / p99 / max, and the median of the pairs'
  differences in the first create.

A = the node booting at the first create (e18b720), B = booting when made (6589fc3 on). N = 299 pairs (a p99
claim at 95%: 0.99ⁿ ≤ 0.05).
