# HEX-GUEST-20's runs: the OCI validation in a real guest, whole test (native plan step 5, constants)

What bounds the test's wait for its lab (crates/hex-cli/tests/guest_validation.rs `SUITE`): twice (the
margin) the slowest run here (the lead's rule for a timeout that can't wait on a fact alone). Each time is the
test's whole run, as cargo prints it (boot, staging, the lab, the comparison), so the lab's own part is less.

| host, class | when, run | tree | seconds |
|---|---|---|---|
| adas-mac-studio.local-2, HVF | 2026-11-06 11:42Z, launchd hex.ns5.real.20261106T110909Z (bench/oci-validation/RECORD-mac.txt) | 61bb22a | 48.18 |
| jupiter, WHP | 2026-11-06 11:00Z, the Windows agent (RECORD-mac.txt "beside it"; cp/t7-native-windows 44e8b02) | 73e4d3b | 75.5 |
| adas-mac-studio.local-2, HVF | 2026-11-06 16:32Z, launchd hex.ns5.dpeak2.20261106T163155Z, 4096 MiB, load 7.6-13.3 | 749823c | 59.05, 62.85, 62.54 |
| adas-mac-studio.local-2, HVF | 2026-11-06 16:37Z, launchd hex.ns5.dver.20261106T163737Z, 2014 MiB | 749823c + sizes | 73.75 |
| adas-mac-studio.local-2, HVF | 2026-11-06 16:58Z, launchd hex.ns5.dver2.20261106T165820Z, 2014 MiB, load 9.2-10.5 | de00973 | 67.13 |

The slowest: **75.5 s** (jupiter), so `SUITE` is 151 s. KVM (fedora) isn't recorded in a guest yet.
