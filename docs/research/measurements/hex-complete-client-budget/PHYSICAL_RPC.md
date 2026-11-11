# Narrow native physical report client integration

Authorized ROOT contract: API2dd7357d741c723aa588e6139d5ba64d9d1297ea,
wire/export/dispatcher5e5a0f5047bb1ef452e9a0dd6c3b70c0198df8ca.
The wire variants are Call::PhysicalReport0x13 and the unboxed
Answer::PhysicalReport(PhysicalReport)0x800e.

Core Runtime uses plain(Call::PhysicalReport), validates report.check() before
returning it, propagates actual typed Refused, and rejects an unexpected answer.
There is no new provider, inventory, cache, holder or embedded Node. The native
Node remains the sole Prober/Gpus owner; this method owns only the existing
client RPC. It acquires no native pool/selection lock around the remote call.

The new measuring wait appends at index24, after unchanged Capacity index23.
KINDS becomes25. The guest table appends
Patience::measuring("client.guest.answer.physical_report"). Existing calibration
values, the immutable ReplyDeadline implementation and holder authority do not
change. Unknown native counters remain unknown values in the typed report.

ROOT integration outside this lane:

- Append one measuring physical_report entry to linux.rs, macos.rs and windows.rs
  answer arrays, keeping all current entries/indexes fixed.
- In ROOT-owned calls/tests.rs, update KINDS's expected count24->25 and append
  (Request::Call(Call::PhysicalReport),24) to the operation-index oracle.
- Compose the exact API/wire exports and this client source, then publish a
  coherent fixed pin before dependent client checks. The supplied5e5 pin alone
  precedes ROOT's native answer-table append; knowingly missing arrays are not a
  reason to run a redundant compile.

Source only at this checkpoint. The preceding immutable client budget source
has a lib/tests Clippy0 result at08a; that result is not compilation of this new
RPC. The deadline fixture correction is separately signed at ea438d01. No new
batch, API/core duplicate job, backend/native/VM/GPU call or full gate is run.
