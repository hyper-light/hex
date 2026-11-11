# Exact client refusal check (AC-T7.4.1)

ROOT coordinator, current0342/rulesdf2 policy. Source732fcd9fe18a973f37593e2c54e1f12674b5c4b7
ran on admitted d132/macOS26.4/arm64 with Rust1.99.1,1CPU/4GiB/12GiB,
nice10/jobs1/testthreads1 and private no-sync source/target/Cargo/temp.

Exact guest::tests::physical_gpu_refusals_preserve_retry_category:1pass/0fail/
0ignored/55filtered, exit0. hex-client all-target native Clippy exit0. No guest,
VM, GPU or native attachment ran. This verifies the public invalid-versus-
exhausted category, not allocator-fault injection or backend lifecycle behavior.

Unit log SHA3e2e74104df3d638552e06116ad41d07637d3407169baac1454db6a57c71ef25;
Clippy SHA f33ad4efc937d3f7bf0c12df0756dc18025e3dd19ece554f3afe6b546405cac2.
All source files before/after match the exact fixed-Git archive. Cargo2418/2970
and localCLI56722 were reaped; temp0. CollectionCLI8814 exited0/reaped.
The lossless original raw archive is178090 bytes/SHA256
e7ed69ea1a6debdc099c8c68659174c77cf6d72a0939edbfeaaa73d6b9898ede.
Every member byte/hash and input manifest was independently verified locally;
collection.json contains the index. Original private source/targets/caches/logs
remain retained. Later caller-budget implementation is a different source scope
owned by L4, with no passing result inferred from this check.
