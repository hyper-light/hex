# Current Shards API seam, source checkpoint

ROOT coordinator Hex01a1aa77778471339bea3beff90d6bbd, policy33df95b08 /
rulesdf2acf47 read in full. Re-vendor exact main33df95b08ad8f95bddeaa1c48acf0c64386e0eac
API+ABI+AF_UNIX+testdir, replacing provisional c55 + the old Hex borrowed patch.
No VMM/kernel binary or kernel pin changed. All old source/proofs remain in Git
and frozen caches. The only local workspace scaffold is explicitly own; source
hashes identify every copied file. triomphe0.1.17 with std only comes from the
upstream API manifest; exact no-dependency lock entry/checksum copied from the
upstream Cargo.lock. No Cargo command/build/test ran on the laptop.

This checkpoint is SOURCE ONLY. Existing upstream validation is attributed to
the peer, not ROOT's current combined source. Current root compile/native/matrix,
vendor check and full gates must run. Upstream public backend_of still rejects
Windows ARM64 despite its existing WHP ARM64 VMM; that adapter mismatch needs a
regression and patch. No supported-platform unsupported response counts as done.
GPU/VFIO GuestSpec exposure and exact physical selection remain required work.
