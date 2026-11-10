# Windows AF_UNIX ownership probes, 2026-11-09

These are capability probes, not a completed forwarding protocol. The reproduction patch and full test source are retained under the lead's `~/.cache/hex-lead/probes/windows-socket-transfer{.patch,-full.rs}`; they add test-only WSADuplicateSocket/WSASocket helpers and tempfile as a test dependency. No unsafe cross-process export API is exposed in production.

On Jupiter with CARGO_TARGET_DIR=$HOME/.cache/hex-win-guest-streams/target, CARGO_BUILD_JOBS=2, below-normal priority:

- r9: `cargo test -p hex-afunix --lib -- --exact tests::a_socket_can_be_shared_with_another_process --nocapture`: 1/1 passed in 0.02s. The child imports the actual AF_UNIX socket, then continues exchanging P/C after the original descriptor closes.
- r10: `cargo test -p hex-afunix --lib -- --exact tests::an_unimported_socket_offer_releases_its_recipient_reservation --nocapture`: failed in 0.02s. Recipient process handles: 99 before export, 100 after export, still 100 after the source socket and opaque protocol-info bytes drop. The recipient remained alive, blocked before import. The probe then killed and waited for only its recorded child. This is an OS lifetime counterexample: raw export is not cancellable by dropping its bytes. It is not an implemented/correct forwarding path.

Primary sources:
- https://learn.microsoft.com/en-us/windows/win32/api/winsock2/nf-winsock2-wsaduplicatesocketw : protocol info is targeted to one process and may be imported once; descriptors retain shared socket state.
- https://learn.microsoft.com/en-us/windows/win32/api/winsock2/ns-winsock2-wsaprotocol_infow : dwProviderReserved is reserved for the service provider. It is not a documented revocation handle.
- https://learn.microsoft.com/en-us/windows/win32/api/handleapi/nf-handleapi-closehandle : socket ownership must close through closesocket, not CloseHandle.

These probes motivate caller-initiated dialing through the existing relay/control core, with a dedicated control per offered program. The positive transfer probe establishes a capability; the cancellation counterexample rules out using an unconsumed target reservation as the forwarding protocol. Current forwarding verification is separate from these historical probe results.
