# Optional owned delegate for box_run_cli — unapplied proposal

ROOT requested one test-helper proposal, not a new production interface or installed binary. Fixed source is `e990ff1ee953a12466551b678e7c7a9e60c552dc`; patch target is only `crates/hex-cli/tests/box_run_cli.rs`. L1 writes only the docs beside this file. No patch application, rustfmt, compile, test, native execution, dry run or wrapper creation occurred.

`box-run-owned-delegate.patch` contains ONE unified hunk. It adds `HEX_BOX_RUN_DELEGATE_COMMAND` (optional absolute executable path), a shared command constructor, and uses it at BOTH ordinary box_run and the prlimit test. The actual CLI argv suffixes, output capture, exit/status assertions, all numerical budgets, shell programs and descriptor-limit oracle stay unchanged. The existing systemd version probe and fallback argv remain exactly the same when the variable is absent.

- **Absent:** existing systemd behavior/skips remain. This is compatibility, not permission to dispatch a manager-created scope outside ROOT's envelope.
- **Present:** delegate is chosen without probing/invoking systemd. Empty/relative path fails its test assertion; missing/nonexecutable path fails spawn. Nonzero/malformed output reaches the existing status/output oracles. Never fallback or skip after a supplied delegate fails.
- **Arguments:** `DELEGATE HEX box run <unchanged limits> -- <unchanged program argv>` and `DELEGATE prlimit --nofile=<unchanged soft:hard> -- HEX box run -- /bin/sh -c <unchanged program>`. Every argument is separate, not shell-expanded or reinterpreted. The delegate receives no systemd flags or synthetic success option.
- **ROOT custody:** path is an explicitly supplied, immutable/hash-pinned owned executable within the run lab; not PATH discovery, an installed helper, socket, provider or manager. ROOT separately pins its interpreter if it is a script. It may not proxy commands to another service.
- **Before exec:** using existing a128 self-placement semantics, validate exact scope/driver identity, original deadline, resource limits and fresh child; move ONLY the delegate's OWN PID into that empty exclusive child under the SAME capped Scope; positively verify membership, then exec exactly the supplied argv. Do not migrate a child postspawn, foreign processes, Cargo, guard or an ancestor. A distinct fresh child is required for each of the four box_run calls and the prlimit call.
- **Lifetime:** preserve stdin/stdout/stderr, argv/environment, signal and exit status; no extra uncharged helper/background scope. Original outer deadline includes preparation, exec and cleanup; failure/timeout retains typed evidence and uncertainty. Scope emptiness/descendant cleanup is ROOT proof, not inferred from wrapper success. This proposal supplies no wrapper implementation or new budgets.

## Immutable bytes and ROOT intake

Base target: 4,886 bytes, SHA256 `60861acc079188dece0ea8568aca70134ec62018668fa17369d35f079f2fa473`.
Proposed target (in-memory only): 5,417 bytes, SHA256 `6a1dd9b107fb08e02a1cdfc6a3178d9a79356bc6e4f34a95bfdbaef3d885ce76`.
Patch: 6,584 bytes, SHA256 `110c1cc6b66f55e3c13d49cd43dbd3a35ad1c06838d2b9cb2bb7970c468b69bf`.

For ROOT's separate private candidate only (these commands were NOT executed by L1):

```sh
git apply --check /ABSOLUTE/PINNED/box-run-owned-delegate.patch
git apply /ABSOLUTE/PINNED/box-run-owned-delegate.patch
# Read back target SHA256 == 6a1dd9b107fb08e02a1cdfc6a3178d9a79356bc6e4f34a95bfdbaef3d885ce76.
# Restore only this exact proposal if ROOT elects to do so, never discard other work:
git apply --reverse --check /ABSOLUTE/PINNED/box-run-owned-delegate.patch
git apply --reverse /ABSOLUTE/PINNED/box-run-owned-delegate.patch
# Read back target SHA256 == 60861acc079188dece0ea8568aca70134ec62018668fa17369d35f079f2fa473.
```

Future ROOT validation must demonstrate both original test oracles actually execute through the owned delegate, all five invocations self-place before exec, expected status7/status137/status125 and descriptor soft:hard remain, and a failing/missing delegate cannot become a skip/systemd fallback. The whole-scope observer must include prlimit and all foreign descendants. Those results are owed, not executed here. Existing baseline/source/raw, including all earlier native-gate and Sim evidence, stay untouched.
