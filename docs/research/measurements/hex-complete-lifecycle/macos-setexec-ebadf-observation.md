# SETEXEC EBADF: source chain and fixed FD observations

ROOT13d334ee ran the unchanged combined6b control on admitted remote macOS26.4
arm64 with explicit installed SDK26.5. Compile0, binary
46c1fb0e3611ffc45dba7aca07f5b9a591629462f64db29cbec57c9b5d6c2908.
NORMAL failed before NOTE_EXEC: a complete PREEXEC_ERROR carried native errno9
(EBADF), controller2916/birth14443012091384 ended127 and was positively reaped;
main2915 was reaped, parent limits unchanged, source0diff/temp0. No SPAWN_RETURNED
frame/extra child was observed. Later three cuts were UNEXECUTED. Full581B native
log SHA65d7e37920d23c892c14e62afe15b44fa7a58cfa483aaa2efbbbe82e49ccd5a6 was
independently verified from the retained complete CLI receipt's exact raw framing.
This is no mechanism success, no all-descendant assertion and no unsupported-SETEXEC
finding. Original source/raw/guard remain retained.

## Pinned source inference, not installed-kernel attribution

Official Apple XNU pin f6217f891ac0bb64f3d375211650a4c1ff8ca1ea provides this chain:

1. kern_exec.c SETEXEC path4190 calls cloneproc(CLONEPROC_EXEC). Its normal spawn
   path instead calls fork1(PROC_CREATE_SPAWN).
2. kern_fork.c derives in_exec from that flag; its process-copy path1100 calls
   fdt_fork(...,in_exec).
3. kern_descrip.c fdt_fork846–1010 copies descriptors. Its skip predicate around969
   excludes FP_CLOEXEC when in_exec is true, before the new process is returned.
4. kern_exec.c4235–4260 applies file actions after that copy. Actions run in supplied
   order. DUP2 at3042 calls dup2, whose source lookup can refuse a missing fd;
   kern_descrip.c2456 validates that source and destination. INHERIT at3099–3122
   explicitly returns EBADF if its fd does not exist. It clears the descriptor's
   close-on-exec only after finding it. CLOEXEC_DEFAULT marks the mentioned action
   destinations after actions; final fdt_exec closure is later.
5. The libsyscall posix_spawn wrapper3014–3112 forwards the already-prepared file
   action object and attribute pointers to the public native entry (or existing
   optional filter). adddup2/addinherit constructors range-check and append actions;
   they do not establish that the eventual source fd is still open.

This fixture currently prepares nullfd with O_CLOEXEC, and the report writer with
FD_CLOEXEC. Its ordered file actions are nullfd->0/1/2, then report-writer inherit.
On the pinned source path those close-on-exec sources would be omitted before
the action lookup. That is a concrete explanation consistent with EBADF, not proof
that d132's installed kernel took that exact branch. Its actual pre-spawn descriptor
numbers/flags have not yet been observed. A fixture lifetime error or another kernel
path remains possible until the next causal data exists.

Official source byte identities:

- [kern_exec.c](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_exec.c),
  251651B, SHA89531d33618b8361064236189212c3c96be1b32e098690b17802b6b15d932d1a.
- [kern_fork.c](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_fork.c),
  47686B, SHA73b578159deebc71434185d3638ab1468f995a19c882e1902d001994bb0391c3.
- [kern_descrip.c](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/bsd/kern/kern_descrip.c),
  147011B, SHA480cfed4e987be874bd71fb6933c254adf9fb1f36de8496dee8f351b18da13b1.
- [libsyscall posix_spawn.c](https://github.com/apple-oss-distributions/xnu/blob/f6217f891ac0bb64f3d375211650a4c1ff8ca1ea/libsyscall/wrappers/spawn/posix_spawn.c),
  80627B, SHA15392bdbd233e899de34b238afdb2f991a71a24583dec92ae503cb42bc5f9fad.

## Accepted observation-only successor

ROOT accepted F_GETFD on exactly two existing private sources: the nullfd used by
all three DUP2 actions, and the report writer used by INHERIT. The parent records
both after preparation and before fork. The trampoline records the same two
sources immediately before posix_spawn, after its gate closes. Role/fd/getfd result/
errno are typed private frames, prepared in the existing COW storage. No directory,
process or descriptor enumeration occurs; no new descriptor is opened or duplicated.
The observation helper saves/restores errno and changes no descriptor flag.

The parent validates only frame identity/role/fd correspondence and prints raw
observations; it does not assert a particular F_GETFD value or reinterpret it as
launch success. Existing PREEXEC_ERROR, returned-child cleanup, NOTE_EXEC, stopped/
birth/canary/resume, limits/threads/FD/QoS/jetsam/memory and terminal oracles stay
unchanged. There are the same four fixed cuts, same stop-first rule and original
39.507s total absolute guard. The parent report-writer flag comparison remains
intact. No own C compilation/native run, production or SDK change is dispatched.

These observations distinguish concrete alternatives. An invalid pre-spawn source
would favor a fixture lifetime cause. Valid CLOEXEC sources plus repeated EBADF
would support the pinned exec-copy explanation; flags already clear would falsify
that particular explanation. Even then an internal-kernel attribution or a child-only
flag remedy requires ROOT review and actual control results. No flag is cleared,
parent state widened, default fallback added, timeout extended or extra native cut
introduced by this successor. Production release and full Mac helper2ef remain held.

R01–R16 scope is measurement-only implementer: source research/read-only F_GETFD,
bounded two-role frames, existing finite transport/owner/cleanup, no new unsafe Rust,
resource manager, cache, service, credential or security setting. Policy7d7e/df2 full
read receipt persists. No performance/allocation-free/cleanup/native pass is claimed;
ROOT owns new execution, actual effects/profile/fault evidence, integration and main.
