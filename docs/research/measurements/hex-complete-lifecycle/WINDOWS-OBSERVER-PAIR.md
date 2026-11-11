# Native readiness for the same bounded observer

Source-only helper requested by L2 for its existing guest observer. No additional
worker, waiter, pending OVERLAPPED, Port, Arc, socket/handle duplication, timeout
option or accounting owner is introduced.

```rust
pub fn readable_pair(
    sockets: [Option<std::os::windows::io::BorrowedSocket<'_>>; 2],
    patience: std::time::Duration,
) -> std::io::Result<[bool; 2]>;
```

The caller keeps both exact socket owners alive and is their sole reader. The
helper compacts only active entries into WSAPOLLFD[2] on its stack. None omits an
EOF-complete endpoint, so its HUP cannot spin while the other stream is pending.
Both None and active duplicate socket values refuse. A finite caller budget becomes one checked
absolute deadline. Zero asks immediate readiness; MAX refuses. Positive fractions
round up to Windows' millisecond unit and long finite waits use i32-max native
chunks against that same deadline, with no repeated state probe or new cadence.
The caller rechecks its original deadline before processing results. OS scheduling
and one millisecond rounding are not universal response-time guarantees.

Read-ready identifies the actual endpoint without consuming bytes. HUP/ERR also
returns read-ready so the sole caller reads its actual EOF/error. Invalid entries,
duplicate raw values and impossible returns refuse. WSAPoll failures capture the
raw WSA error immediately; an interrupted wait resumes against the same deadline.
Readiness, pipe/socket EOF and controller exit never imply whole-scope release.

The primary contract is Microsoft's [WSAPoll reference](https://github.com/MicrosoftDocs/sdk-api/blob/b359122fcb6af0938ae83d10194186a3e586ed77/sdk-api-src/content/winsock2/nf-winsock2-wsapoll.md),
revision `b359122fcb6af0938ae83d10194186a3e586ed77`, SHA-256
`8adb4c8a865aaba7158e8dff7ad27830ea79781d36ff4bfd415601c79e29ae6d`,
10,705 bytes. Signed timeout units, finite/zero/indefinite cases, POLLRDNORM,
HUP/ERR/NVAL and WSA error capture come from that interface. The earlier considered
SO_RCVTIMEO approach changes socket state and does not replace this pair event
observation; a separate reader worker/IOCP owner would duplicate ownership.

Two new source controls are UNRUN: finite native timeout boundaries, and private
loopback sockets with immediate idle/data/EOF observations, nonconsumption and
duplicate/MAX refusal. They create no VM, GPU, child process or native job. The
loopback case does not prove AF_UNIX or another actual Given socket provider;
ROOT must verify the real observer endpoints and original deadline behavior.

ROOT integration is the new imports, pair_timeout/readable_pair functions and
observer_socket_tests module in existing windows/sys.rs ONLY. The functions do
not depend on SDK5ba/report carriage elsewhere in L3's parent file; preserve
ROOT's current native producer and use this commit's narrow delta. Existing
windows-sys WinSock capability is needed; ROOT owns features/CLAUDE rationale.
The justified unsafe calls remain isolated in the already-owned Windows sys
adapter with immediately adjacent SAFETY comments. L3 dispatched no test/native
run; ROOT owns matched compilation and real endpoint acceptance.
