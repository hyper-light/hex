# AC-T7.4.1: original allocation-schema integration result

ROOT ran signed `516a370bf7dccc7caea6683b0576f4c0880f9057` in a fresh private
d132 arm64 macOS source/target/cache, admitted 1 CPU/4 GiB/12 GiB, nice10 requested,
one Cargo job and one test thread. Wire tests executed 35 cases: **34 passed,
1 failed, 0 ignored, 315 filtered**, Cargo27372 exit101. Driver27352 and local
CLI48567 ended; Cargo was reaped, temp entries were zero and all source hashes
before/after matched the immutable archive input.

The new retained BoxInfo allocation inspect/list/truncation/invalid-envelope test
passed. The sole failure was the old adopted-byte hostile fixture: its offset
counted backward from the answer's end without accounting for the new None
allocation tag. It mutated the following byte and then expected an adopted error.
This is a fixture error, not an observed production decoder defect. Successor
`9ba81af8da7375b3c0e6c2e907e50c1dcb911c07` changes only that offset and comment.

The driver stopped after wire failure. **Index cost/recovery, peer cost/codec,
client-link and Clippy phases did not run.** They are not counted as passed or
executed here. A separate follow-up targets only the corrected hostile case and
those previously unrun phases, retaining all34 unchanged passes at516.

The compressed original raw proof is 186,507 bytes, SHA-256
`b7f27b62896cd2fabe8a05e60d1c8ae6156553737886f255d91190c1b56f1404`.
The data-only collector's first CLI11567 received no response within15s; that
failure is preserved. Its readonly retry CLI64713 completed/reaped0 and verified
every archive member, original input and post-source manifest. It did not rerun
tests. Exact commands/log and binary hashes/receipts are in the adjacent files and
unmodified raw bundle. Remote targets, sources, binaries and caches remain intact.

No native VM/GPU/HCS/attachment was invoked. New managed runtime/native adapters,
full supported target checks, native effects, audits/profiles and final gates/main
remain outstanding.
