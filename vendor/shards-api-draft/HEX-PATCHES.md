# Hex patches to the provisional Shards API

Base: hyper-light/shards `p/integrate-1107` commit
`c55d85db9b8c88656b4fe4bd582f5ebe6500e959` (SAID `da8bcd272`, vmm-round2 `7ddceb974`).
The guest kernel pin and minimum binary revision supporting `--console-said` remain as recorded in Cargo.toml.

HEX-RT-17 adds `shards_api::connect_borrowed(&Identity, port, patience)`. It verifies the VM process before
and after one existing CONNECT handshake, returning the existing stream type. It acquires only a query/exit
observation handle, with no Guest lifecycle, process control, watcher thread, retry or stream copier.
Positive clock-representable patience bounds the whole handshake; zero retains the existing one-attempt
semantics bounded by the VM's own vsock request handling. The patch is prepared for upstream review.

Modified source files:

- `crates/api/src/lib.rs`: export the borrowed connector.
- `crates/api/src/borrowed.rs`: connector and identity checks; Windows query/synchronize process handle RAII.
- `crates/api/src/borrowed/tests.rs`: fake AF_UNIX muxer and owned helper-process tests.
- `crates/api/src/taken.rs`: safe nonblocking exit observation on the existing Unix held process.
- `crates/api/src/client.rs`: make the existing handshake crate-visible and use one deadline across its
  connect, partial request writes and answer reads. The owning Guest client uses this same handshake.

The other source files, including ABI, afunix and testdir, remain byte-for-byte at the base revision.

Windows process APIs are documented by Microsoft:
[OpenProcess](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-openprocess),
[process access rights](https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights),
[GetProcessTimes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-getprocesstimes),
[WaitForSingleObject](https://learn.microsoft.com/en-us/windows/win32/api/synchapi/nf-synchapi-waitforsingleobject).
