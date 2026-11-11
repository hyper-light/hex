# Canonical physical owner at the existing catalog seam

AC-T7.4.1; L1 implementer, ca3/df2 policy. ROOT and L2/L3/L4 acknowledged the exact shape before editing. SOURCE ONLY; focused source controls are unrun, no native claim.

`DeviceHolder::Box(BoxId) | Backing(BackingKey)` replaces only `DeviceInfo.holder` ownership, as `Option<DeviceHolder>`. Untagged serde retains the old box string exactly; the backing case is the canonical key object with fixed pool/generation arrays. Existing wire None0/Box1 is retained, Backing2 is appended by L4. ROOT reexports DeviceHolder and ports outside callers. No fake BoxId, None-free pool grant or extra registry exists.

New holder/device/physical-device/report ownership has no implicit Clone. Each exposes fallible try_clone; a backing key copies fixed bytes, Box identity/model/authority/vector ownership reserve explicitly. Capacity delegates these copies. PhysicalReport remains telemetry only and refuses any Box or Backing holder. A route may consume its own exact observer grant; another backing generation is busy. Existing catalog Box scans now return an owned typed holder and Entry::report moves it, avoiding a second unchecked owner copy.

This checkpoint does NOT claim the new Pool Store join is complete. L3 signed d83a1eb0 supplies canonical PoolRecord/NativeRecord; the next same-owner Books/Transaction/native increment joins admitted Granted/Live/Draining pool claims at this SAME catalog seam. Until that real join and boot/provider consumer are present, no pool capability is initialized or advertised from this source alone. Prepared configuration references are not measured offers or grants. Existing Box behavior and focused fixture ownership are preserved.

New pure control physical_holder_preserves_legacy_box_and_exact_backing_generation covers old JSON bytes, exact pool generation, malformed fixed arrays, fallible copy and telemetry refusal. Source fmt/diff only; no compiler/test result claimed. ROOT owns native/VM/fault/mutation/profiling/full gates. RouteReceipt::charged adds the common host-charge derivation used by the upcoming model candidate/admit/recovery consumer; no DTO field/tag changes.
