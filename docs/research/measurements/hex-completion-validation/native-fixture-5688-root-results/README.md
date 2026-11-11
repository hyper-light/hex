# Fixed native fixture preparation: actual ROOT result

Coordinator policy7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506, rules SHA256 df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3. Tested signed source5688fd7516d260732796fbc53b45a8e66a2c46a5, Rust1.99.1, Linux x86-64 glibc2.44/kernel7.3.3. No native process/VM/GPU workload was invoked.

The two new shared preparation controls passed (2 selected, 2 passed, 0 failed, 0 ignored). The affected Runtime/Node/CLI all-target strict Clippy check passed, including compile-only coverage of the new parent-death observation source. No older producer5 or native baseline case was repeated. Exact commands, counts, log/source/toolchain hashes and child PIDs are in proof/cohort-5688fd75-linux-native-fixture-5688/result.json and proof/collection.json. Both raw logs were read in full and all collected members independently hash-verified.

Driver852640 and Cargo855618/920579 completed and were reaped. Source-before and source-after manifests are identical; temporary entries0. The raw archive is373012B, SHA256620f019980efb8717224700f000491498e235b36fa63ef6ee4ad01686d435d59. The earlier driver recorded Cargo PID/reap but not per-Cargo birth snapshots; no complete native-incarnation audit is claimed. The collection helper's generic post_source_only_vendor_lock field does not assert a lock change: this exact result has source_changes[].

This result validates pure construction and compilation. Actual trusted-entry execution, full image/TOCTOU and pre-main facts, whole-worker release, GPU execution, resource profiling and wider platform/native acceptance remain unverified. The separate inherited-FD baseline/bypass/restored comparison is a new source-specific job and is not a result of this bundle.
