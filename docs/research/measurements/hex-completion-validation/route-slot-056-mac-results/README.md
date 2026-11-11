# Route-slot changed-source Mac results

ROOT actual macOS arm64 light check at source056005d0fe745c936e0c34dcdfdcf9a7075447da, not a native backend/GPU/VM execution. New private source/target/cache/temp, admitted1CPU/4GiB/12GiB, nice10/jobs1/threads1.

- New selected_slot_keeps_one_value_across_failed_reservation:0passed/1failed/0ignored. It stopped in model_connection at Client::connect: the node-agent send reported none in590.582microseconds, twice its slowest learned answer. The owned fixture peer reported UnexpectedEof. This is a retained actual failure, not a diagnosed Slot production defect.
- Affected selected_route_callback_cannot_block_independent_siblings:1passed/0failed/0ignored.
- hex-client+hex-runtime native all-target Clippy:exit0.
- hex-node+hex-cli native all-target cargo check:exit0; tests not executed in this phase.

Whole driver exit1. Cargo30928/31132/31134/31214 waited/reaped; driver30873. Source before/archive/after hashes match; temp0; native invocations0. ROOT local CLI18029 returned1; data collector85254 returned0/reaped. The compressed raw archive/member hashes are independently verified in collection.json; archive235331bytes SHA2563529e6a761658a60c6c056bd037f08f4ab9836ac33475ca8fb18390d110accfd. The collector omitted the hex_runtime binary glob, so no executed-binary digest is claimed by this collection. Original remote source/target/cache and local input/archive/log remain preserved.

L2 received the exact new control failure for diagnosis before any remedy; no timeout increase, retry, old seven-test repeat or platform-readiness conclusion follows.
