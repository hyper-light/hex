# The fsyncs a seam create makes (goal 5 plan step 2)

A box's record is fsynced only when it must outlive the host: when it names a directory on a filesystem a
host crash leaves behind (decision 1, `crates/hex-node/src/store.rs`). A node keeps its containers' state under
its runtime directory, `/run/hex-node` as root, so on a host, where `/run` is tmpfs, an `oci` box made from a
given bundle names nothing else and its records aren't fsynced. `record_syncs.sh` counts what a seam create
makes: seam_oci's lifecycle phase (create, start, exec of `true`, delete of a `sleep` container), 5 lifecycles
by each driver, under `strace -f -c`, as root in the OCI pass's container (host.sh's flags), once with `/run` a
tmpfs and once with it on the container's overlay, as host.sh's container has it unless it mounts one. The
renames are counted beside the fsyncs: each record write is one, and each container state write.

## Run of record

`record-syncs-20261030T002934Z` (`~/.cache/hex-bench/out` on fedora): fedora-2, Linux 6.19.10-300.fc44,
2026-10-30T00:29:34Z, load 3.05 at start. hex 0.1.0 (cff3699), sha256 4f1812bd76b1e003; seam_oci sha256
334402959ca5a176, built at cff3699; strace 7.3 (Fedora's package, unpacked without installing it). The harness
is cff3699's; kiln's synced copy had dropped the two scripts' exec bits, the only difference env.txt notes.

| `/run` | driver | lifecycles | fsync | fdatasync | syncfs, sync, sync_file_range, msync | renames |
|---|---|---|---|---|---|---|
| tmpfs | in process | 5 | 0 | 0 | 0 | 21 |
| tmpfs | socket | 5 | 0 | 0 | 0 | 21 |
| overlay | in process | 5 | 30 | 0 | 0 | 21 |
| overlay | socket | 5 | 30 | 0 | 0 | 21 |

With `/run` on tmpfs no lifecycle makes an fsync, by either driver. On the overlay each makes 6: its three record
writes (creating, ready, deleting), each the file and the record directory, as the store makes a durable one.
The renames are the same in both: the writes are made, only their fsyncs differ.

Before this step, at ef64dbc, the same create in the same container wrote two durable records, four fsyncs,
12.187 ms of create's 18.297 ms p50, against libcrun's 5.015 ms (goal 5 plan step 1's breakdown,
`create-breakdown-20261030T000822Z`).

## Reproducing

    record_syncs.sh <hex binary> <seam_oci binary> <strace binary>

strace without root: `dnf download strace` and `rpm2cpio strace-*.rpm | cpio -idm` in a directory of its own;
its libraries are the host's, which the container sees through its read-only `/usr`.
