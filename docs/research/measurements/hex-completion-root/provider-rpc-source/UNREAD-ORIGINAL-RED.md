# Original unread-counter control and old-launcher fence

AC-T7.4.1; implementer L1. Actual pure source control, not native GPU evidence.
No production load-quality fix preceded this selected failure.

Frozen source: ROOT696b94eff8575c0f046b2725cf9411b66859bcbc, exact02d796ac
facts/control, c59 declaration, explicit index:usize compile annotation and the
documented pure fixture. Private source/target/cache/temp `red696-pure-v6` on
d1323898b255a3ed435eb83fa0f7b0cf; admitted 1CPU/4GiB/12GiB, precise no-sync,
nice10/jobs1/testthreads1, installed rustc1.99.1, auto-install disabled,
offline/locked. Registry lock SHA
`0bee4e19d932ff2c1255e1b7ad90cd2b16877a2e389a881a29f8712d763af7d9`.
Overlay e13c17ad03ff5cf48d910da22499e511fd1f124beb8f5ee712226dc38b1745ed verified.

Actual invocation selected ONLY
`device_offer_tests::unread_native_counters_never_become_a_fresh_idle_free_offer`
in the path-included pure fixture. It calls actual gpu_facts -> Facts::node ->
attachment offers -> the existing bind, with a failed busy counter and a separate
failed memory counter. Cargo42899 was recorded before wait, exited101 and was
reaped; temp was empty. Result **0 passed, 1 failed, 0 ignored, 12 filtered out**.
The exact observed symptom was `selected=Some([0])`, then the intended assertion
that Unread cannot grant fresh known idle/free failed. This is the relevant
failure, not the earlier compile blockers. No hardware effect was invoked.

Log SHA `9ecf2ee4fcea622a2ae7a7b5046309a2b1259304098ae24f8fcfb9793ed219a3`;
test binary SHA `5ca12040a9b12eb89debd931d98569a591081fedd4926a532a8a29c4eb4ba513`.
The exact unchanged control body is retained in `unread-original-control.rs`
(SHA c68a848bb5beb718b7c6b437c2d03f111bb7651739b256f1b2adbf6ec4c059c4).
This archival source is intentionally not registered in a current crate.

Large retained raw artifacts, not committed or deleted:
`/Users/adalundhe/.cache/hex-complete-bindings/red696-pure-v6/` contains
`raw-and-fence.tar.gz`, `original-source.tar.gz`, the exact dispatch script and
the remote scope retains the binary/target. Raw-and-fence archive SHA
`7f5655b6015fb1e8327b53fa2b3c5bbe4af5919726c0f45d466ea272ec067ae8`.
The source archive includes original facts/capacity/offers/control and fixture
inputs, preserving the body before its future type-only assertion adaptation.

## Exact delayed v5 entrypoint fence

Absence of old logs was NOT treated as resolution. The full original v5 command
is retained in `unread-original-v5-driver.sh`, SHA
`6c5d591966644ec53f7f9265a9f74f08360315fc35995f1e804edbef34c201aa`.
Its first line is `set -eu`; the first mutation is the straight-line command
`cat > "$check_scope/overlay-v5.tar.gz"`, followed by shasum, tar, control
preparation and finally Cargo. It has no earlier exclusive directory mkdir.

An admitted d132 metadata-only command atomically created a DIRECTORY at that
EXACT first-write filename in the OLD private scope:
`red696-pure-v3/overlay-v5.tar.gz`. mkdir succeeded on
2026-11-11T01:00:16Z, inode32862950, mode drwxr-xr-x. Because the original first
redirection now fails EISDIR, cat returns nonzero; the original straight-line
`set -e` shell exits before shasum/tar/control/Cargo. This proof uses the actual
driver control flow, not just an assumed interpretation of absent logs. A prior
successful archive write would have occupied that pathname and prevented this
mkdir. No existing data was replaced or foreign job/PID acted on.

Retained `L1-FENCED` marker SHA
`cc9ab76bbf35ea5ab286a70a93e63bdede8242cebf839bc61f7c68d843bc7f2d`;
the successful mkdir's empty stderr SHA
`e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
Both are in the raw-and-fence archive. The old v5 cannot enter its control;
no result is attributed to it. v6 uses separate private paths and is not repeated.

The now-authorized narrow remedy must carry unknown counters independently,
refuse fresh grants while retaining exact claims, and preserve CPU-only progress.
Green will be this same intended seam control with only the required Option
assertion syntax adaptation, plus bounded retention/CPU controls. Native/fault,
mutation, portability and resource/profile proof remain ROOT-owned and unrun.
