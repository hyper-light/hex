# AC-T7.4.1: first physical report check

ROOT coordinator `01a1aa77778471339bea3beff90d6bbd` checked signed source
`5e5a0f5047bb1ef452e9a0dd6c3b70c0198df8ca` in a new private d132 arm64
macOS source/target/cache with 1 CPU, 4 GiB memory, 12 GiB disk, requested
nice 10, one Cargo job and one test thread. No VM, GPU or attachment was invoked.
Policy revision and file hashes are in `policy-read-ca3.json`.

The exact API physical-report test passed (1 passed, 0 failed, 0 ignored,
39 filtered). The wire suite executed 34 tests: **29 passed, 5 failed, 0
ignored, 315 filtered**, Cargo exit 101. The two new physical-report codec tests
passed. The five failures came from inserting new examples before positional
legacy examples and keeping an unknown-call fixture at the newly allocated
`0x0013`. They are fixture errors, not a reproduced production defect.

`result.json` inside the original archive reports an empty wire summary and
`nonzero=false`: the first harness regex parsed only `test result: ok`, so it
missed the `FAILED` summary. That raw file is preserved. The counts above come
from the original wire log. Clippy did not run after the wire failure.

Fixture-only successor `4bbcde33804b1d298f8ecf275f6d8e4eb97af108` appends examples
after the stable tables, moves the unknown header to `0x0014`, and checks the new
call/answer headers explicitly. Production codec bytes were unchanged by that
correction. The subsequent client RPC integration has a separate source and
result; it is not attributed to this run.

Driver 20510, Cargo 20515 and 21213 and local CLI 20101 ended; both Cargo children
were reaped, private temp entries were zero, and all source hashes before/after
matched. The later data-only collector exited zero and verified every archive
member, original Git input and post-source manifest. `raw.tar.gz` is 183,762
bytes, SHA-256 `68ed136adb2d794a95ec90dc73a08fbc6d42357c73253a1d74f59fea533bb83f`.
The original remote source, target, binaries, logs and cache remain preserved.

Commands, exact source/archive identities, log hashes, binary identities and
collection verification are retained in the adjacent scripts, submission and
collection receipts and the unmodified compressed raw bundle. This result does
not establish native behavior, integration readiness, platform gates or scheduler
completion.
