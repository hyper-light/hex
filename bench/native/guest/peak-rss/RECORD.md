# The guest VM process's peak resident set, per test workload (native plan step 5, constants)

What sizes a real guest's memory where a test isn't told (docs/research/runtime/shards-guest-constants.md):
Shards' least guest (`shards_api::min_memory_mib`) plus twice the most of three runs below.

- machine: adas-mac-studio.local-2 (Mac15,14, macOS 26.4, aarch64, Hypervisor.framework), launchd one-shots
  hex.ns5.dpeak2.20261106T163155Z and hex.ns5.dver.20261106T163737Z, load average 6-13 (noisy, as run)
- tree: Hex cp/t7-native-s5-consts 749823c (host and guest); guest image from scripts/guest/image.sh at that
  tree, initramfs-aarch64.cpio sha256 0200a7741035f631…; Shards hex/shards-api a0cced87 debug build; kernel
  Shards' released Image-6.18.48-aarch64 (87fbe8c0…)
- method: SHARDS_BIN is a stand-in that runs the real shards with the same arguments, stdio and signals, and
  when it ends appends getrusage(RUSAGE_CHILDREN).ru_maxrss (bytes on macOS): the VM process's peak, guest
  memory it touched included
- 2 vCPUs

| workload | given | runs (bytes) | most, MiB |
|---|---|---|---|
| HEX-GUEST-19 (guest_real, the conformance suite) | 1024 MiB | 266,256,384; 253,968,384; 297,402,368 | 284 |
| HEX-GUEST-20 (guest_validation, the OCI validation) | 4096 MiB | 1,022,279,680; 914,407,424; 962,641,920 | 975 |

Checked at the derived sizes (memory unset, run hex.ns5.dver): HEX-GUEST-20 passed (28 pass, 30 fail of 58, as
RECORD-mac.txt's), peak 837,369,856 bytes in 2014 MiB; HEX-GUEST-19 at 632 MiB peaked at 287,801,344 bytes. Its
one failing case, HEX-RT-1's second create ("staged files … Broken pipe"), fails the same at 2660759, before
any of this branch's changes (hex.ns5.dbase.20261106T163546Z).

Not yet measured: x86_64 under KVM (fedora) and WHP (jupiter), both unreachable for this pass. The same rule
applies with their arch's least (128 MiB); their peaks are this record's until measured.
