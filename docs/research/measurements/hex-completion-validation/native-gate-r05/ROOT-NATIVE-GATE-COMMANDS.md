# ROOT-only native gate inputs and mutation

Source/command handoff only; none of these commands has been executed by L4. Role implementer; full current policy 7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506 / rules df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3 reread. Fixed ROOT source inspected: c6a64f4b29606fe65ffc0cf11968178d8350c9f7. Compose consumer 82393cd7e0fc925ee42b6d2889dea0ccb8d5f708 -> b0392150c089f16ba0e9816f156b5b808c1f2920 with the already integrated 70fd producer, its 52dd fixture correction and dev-only Linux feature edge. Use ROOT's final signed matched composite for all source/binary/config manifests; c6a alone is not the b039 native result.

The b039 test leaf SHA256 is da4ba641b6122a402007aa8939cddb6154dbe114ac437ce47073afdae7431241. No leaf, gate oracle, shared source or metadata changes accompany this note. The reviewed 92cf Mac measurement is independent; these are Linux stand-in controls, not VM/GPU execution.

## Current config, no defaults

The exact file decoder is `Config` in the test leaf: all twelve fields below are required, unknown fields refuse. `spec` and `controller` use the current actual API serialization, not copied replacement DTOs. ROOT materializes and hashes the JSON using its existing admitted inputs. Do not substitute these descriptions as JSON values.

| Field | Exact value type and current requirement |
| --- | --- |
| `delegated_parent` | Path string; actual existing private delegated cgroup parent, canonical descendant of `/sys/fs/cgroup`, not the root. |
| `lab` | Path string; actual existing private durable filesystem directory, retained for raw evidence. |
| `spec` | Valid current `BoxSpec`; `devices=[]`; explicit finite `bounds.cpu_max_milli`, `memory_max`, `pids_max`; original class Firecracker/Shards, or OCI with its actual allocation. Preserve any existing route/allocation and all API validation. Use ROOT's checked serialized spec, no invented routing receipt. |
| `controller` | Current `Program`: `args` string array, `env` array of `[name,value]`, `cwd` string or null, `user` actual API user or null. Positive valid program, absolute immutable pinned `args[0]`. See marker contract below. |
| `controller_sha256` | Lowercase SHA256 string of the exact configured executable bytes used for `args[0]`. |
| `test_binary_sha256` | Lowercase SHA256 string of the exact matched hex-node libtest executable. |
| `pidfd_budget` | Caller-supplied `usize`, at least two; no implicit budget. |
| `guard_ms` | Positive caller-supplied `u64`; same existing observation/cleanup bound. |
| `gate_budget_ms` | Caller-supplied `u64`, strictly greater than `guard_ms`; both checked for Instant overflow. |
| `observation_ms` | Existing positive `u64`, at most `guard_ms`; retained config provenance, not a polling interval in b039. |
| `gate_wait_syscall` | Existing pinned config's `u64` ABI value; retained provenance only. It is not sampled and grants no phase authority. |
| `byte_limit` | Positive caller-supplied `u64`; same config/record/output/ACK byte bound. Must equal the environment byte limit. |

Do not point `delegated_parent` at a runner/service ancestor. The existing `BoxRoot::open` needs all four existing controllers `cpu`, `io`, `memory`, `pids`, and hands them down only inside this approved private hierarchy. A CPU-only parent does not meet the contract. Missing delegation/controllers remains UNVERIFIED; no zero/noop/ignored-controller fallback or shared parent change. ROOT supplies its admitted resource envelope, original finite outer job/whole-owned-process guard, existing private hierarchy and durable lab. No new resource allowance is selected here.

`Lab::reserve` substitutes `@MARKER@`, `@TOKEN@`, optional `@SCOPE@` in controller arguments. The stand-in must write exactly the token bytes to the marker path, with **no newline**, then exit zero. No marker may exist before release. One concrete Program shape for ROOT's already pinned existing shell is below; replace only the absolute executable placeholder and hash its actual immutable bytes. This installs no helper and is not a default executable or a claimed native result.

```json
{
  "args": ["/ABSOLUTE/ROOT/PINNED/SHELL", "-c", "printf '%s' \"$2\" > \"$1\"", "hex-gate-standin", "@MARKER@", "@TOKEN@"],
  "env": [],
  "cwd": null,
  "user": null
}
```

## Exactly three ignored native selections

The shell names `NODE_TEST_BINARY`, `GATE_CONFIG` and `GATE_BYTES` below refer to ROOT's existing absolute immutable artifacts and the JSON's exact `byte_limit`; they add no fixture input. Execute each command only within ROOT's existing admitted Linux job and finite owned guard. Each must actually execute its one named test; zero selected cases, another selector or a skipped/unavailable path is not native proof. The parent-death case internally starts the same exact binary/selector with its owned role; the outer commands must not inherit that role.

```sh
env -u HEX_COMPLETION_GATE_ROLE -u HEX_COMPLETION_GATE_SCOPE \
  HEX_COMPLETION_ROOT_AUTHORIZED=1 HEX_COMPLETION_GATE_CONFIG="$GATE_CONFIG" HEX_COMPLETION_GATE_BYTES="$GATE_BYTES" \
  "$NODE_TEST_BINARY" --ignored --exact supervise::completion_native_gate_tests::completion_publication_refusal_never_executes_marker --nocapture --test-threads=1

env -u HEX_COMPLETION_GATE_ROLE -u HEX_COMPLETION_GATE_SCOPE \
  HEX_COMPLETION_ROOT_AUTHORIZED=1 HEX_COMPLETION_GATE_CONFIG="$GATE_CONFIG" HEX_COMPLETION_GATE_BYTES="$GATE_BYTES" \
  "$NODE_TEST_BINARY" --ignored --exact supervise::completion_native_gate_tests::completion_durable_publication_releases_and_reaps --nocapture --test-threads=1

env -u HEX_COMPLETION_GATE_ROLE -u HEX_COMPLETION_GATE_SCOPE \
  HEX_COMPLETION_ROOT_AUTHORIZED=1 HEX_COMPLETION_GATE_CONFIG="$GATE_CONFIG" HEX_COMPLETION_GATE_BYTES="$GATE_BYTES" \
  "$NODE_TEST_BINARY" --ignored --exact supervise::completion_native_gate_tests::completion_controller_parent_death_cancels_gate --nocapture --test-threads=1
```

Retain each actual exit/count, stdout/stderr and `gate-result.json`; the kept `gate-*` lab scope; first/sibling Creating record bytes/hash and immutable scope/spec commitment; ACK and tail files; `parent.stdout`/`parent.stderr`; exact PID/birth/pidfd, STOP, terminal/reap/cleanup and private cgroup/resource observations. Keep partial/error facts even on failure. A main controller reap or a boolean cleanup result does not manufacture independent whole-VM/helper proof. Preserve configuration, source/binary and all raw manifests, including no-op/unavailable/missed-phase failures. No raw field absent from this producer may be invented by the collector.

## Whole inherited-FD guard mutation and restoration

At fixed c6a, `crates/hex-runtime/src/linux/sys.rs` SHA256 is f90284ac37e877201342f3c170f4ae091a82269f460fbc47815706dd73f055e4, 38626 bytes. The unique call is `held_child` line 464: `&& held_close_unexpected(&keep).is_ok()`. It is after the actual cgroup join, before `held_ready(byte2)` and `held_wait(expected1)`. The existing guard closes ranges or walks `/proc/self/fd`. Neutralize this one call in ROOT's **private mutation source copy only**, retaining its function item to keep the original helper type/body referenced:

```diff
     if joined
-        && held_close_unexpected(&keep).is_ok()
+        && {
+            let _retained_guard_body = held_close_unexpected;
+            true
+        }
         && held_ready(start.reader).is_ok()
         && held_wait(start.reader, start.deadline, 1).is_ok()
```

No new FD export, selector, owner, knob or producer mechanism is needed. The deliberately skipped guard can retain own **and** foreign gate/notification writers. A phase-witnessed causal failure is meaningful evidence for that whole guard, not uniquely foreign-writer attribution; no foreign-only mutant holds this test. EOF/Open remains separate raw evidence, never an ACK prerequisite. Keep real ACK2, held publication, actual parent terminal `P<D<G` with t0 before spawn, kernel STOP, first127 before G, no authorization/canary and owned cleanup. A missed phase/window, compile failure or unrelated setup refusal is not semantic mutation rejection.

The following snippets are source preparation/restoration only, not a dispatch. `ROOT_PRIVATE_SOURCE` is ROOT's isolated exact composite source copy, never the shared checkout. `ROOT_RAW` is its existing private evidence directory. Capture the file once and record both hashes before any build:

```python
from pathlib import Path
import hashlib, json

source = Path(ROOT_PRIVATE_SOURCE) / "crates/hex-runtime/src/linux/sys.rs"
saved = Path(ROOT_RAW) / "gate-isolation-original.sys.rs"
receipt = Path(ROOT_RAW) / "gate-isolation-mutation.json"
original = source.read_bytes()
assert hashlib.sha256(original).hexdigest() == "f90284ac37e877201342f3c170f4ae091a82269f460fbc47815706dd73f055e4"
old = b"        && held_close_unexpected(&keep).is_ok()\n"
new = b"        && {\n            let _retained_guard_body = held_close_unexpected;\n            true\n        }\n"
assert original.count(old) == 1
mutant = original.replace(old, new, 1)
with saved.open("xb") as file:
    file.write(original)
facts = {"source": str(source), "original_sha256": hashlib.sha256(original).hexdigest(),
         "mutant_sha256": hashlib.sha256(mutant).hexdigest(), "changed_path": str(source.relative_to(ROOT_PRIVATE_SOURCE))}
with receipt.open("x") as file:
    json.dump(facts, file, indent=2)
source.write_bytes(mutant)
```

After ROOT's bounded mutation/cleanup/evidence collection, restore only that same owned path, refusing an unexpected intervening edit:

```python
from pathlib import Path
import hashlib, json

source = Path(ROOT_PRIVATE_SOURCE) / "crates/hex-runtime/src/linux/sys.rs"
facts = json.loads((Path(ROOT_RAW) / "gate-isolation-mutation.json").read_text())
original = (Path(ROOT_RAW) / "gate-isolation-original.sys.rs").read_bytes()
assert str(source) == facts["source"]
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["mutant_sha256"]
assert hashlib.sha256(original).hexdigest() == facts["original_sha256"]
source.write_bytes(original)
assert hashlib.sha256(source.read_bytes()).hexdigest() == facts["original_sha256"]
```

The exact source hash assertion intentionally refuses if ROOT's final composite changed this file after c6a; review and record that new original before preparing a mutation, do not silently float or overwrite it. Build baseline, mutant and restored binaries in distinct initially EMPTY target directories with their exact source/overlay manifests and compiler/binary hashes. No timestamp-preserving target-cache reuse across cohorts: prior actual Cargo `fresh:true` stale-body evidence remains authoritative. ROOT reruns only the parent-death selector for the mutation/restoration comparison, preserving phase establishment and unchanged bounds. A restored green and expected phase-witnessed mutant failure are different evidence from any of the three baseline results.

All three native cases, compiler results, mutation outcome, owned cleanup/resource restoration and gate acceptance remain ROOT-owned and unrun here. The general attribution clarification in 8e7ed8b9 remains unchanged.
