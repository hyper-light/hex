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


The exact native-Mac API bridge regression at533dc2d1 ran3selected tests:
2passed/1failed/0ignored, exit101, on the Windows/aarch64 API recognition
assertion. Log4d73110c813d9d20852899c8fad3162dc3c977228beac4d846904d27bc6745bb;
Cargo39646 and CLI49599 reaped,temp0. All dependencies compiled using the exact
recorded locked triomphe entry; the failure was the backend assertion, not build.
An unused old guest-capacity fixture socket also printed an unjoined helper EOF
panic despite that test passing. The native-capacity control now uses the
existing no-boot counter fixture instead: it requires zero guest boots, with no
unused socket thread. Original logs/source are retained; no claim that that
old helper emitted clean output. The new recognition patch is recorded by the
unchanged vendor.py record command, as0001 in vendor/patches/shards; native ARM64
WHP remains unverified while the user's VM pause instruction persists.

Preparation error: local systemPython3.9 lacked tarfile.extractall(filter=).
The initial shell incorrectly continued and signed an incomplete vendor-removal
checkpoint6ee323d2; nothing was pushed. Exact previous vendor was restored and
byte-verified, import rerun with validated regular relative member paths, and
all36 imported/scaffold hashes verified against Git. The old copy is preserved
at ~/.cache/hex-lead/transfers/vendor-c55-before-main33df-20261110 as well as Git.
Correct current import76a26d3d contains the full tree and locked dependency entry.
Subsequent commands check every subprocess exit code. The failed preparation and
corrected source are preserved, not rewritten or reported as passing validation.
