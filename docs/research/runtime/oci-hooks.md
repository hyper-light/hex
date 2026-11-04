# The OCI create- and start-time hooks: where the spec puts them, where runc and crun run them

The sources behind `docs/design/runtime.md`, "Decision: createContainer and startContainer hooks run in
the container, at the spec's points". Accessed 2026-11-04 from raw.githubusercontent.com at the tag named,
with their lines; sha256 of each file as fetched in brackets. Every fact is `DESIGN` (from the text or the
code), none measured.

- [H-1] OCI Runtime Specification v1.3.0, `config.md`
  (`raw.githubusercontent.com/opencontainers/runtime-spec/v1.3.0/config.md`) [7a6488aa5d220ce086da3c8c911b2faaeebc2ce55cc5a16048abfa8523b441e3].
- [H-2] OCI Runtime Specification v1.3.0, `runtime.md` (same tree) [6236bfd0c5762cfcf1e86be084c07e767cb6dff35692959e6811cecf2d334d91].
- [H-3] runc v1.5.2, `libcontainer/rootfs_linux.go` [27dd1899339109017a9c52fb4818822c6f3504cc2c4ff8a2802d9a3ff3c43c71], `libcontainer/standard_init_linux.go`
  [ca458ed85eee8e68e2b13f4b79cdc947e487bba949f88e477389610c5b14f8ee], `libcontainer/process_linux.go` [bcdcf882ef67377fa74b6cfe3965709bfedc5c54b167d7b492b2b35416689c89].
- [H-4] crun 1.30.1, `src/libcrun/container.c` [4bcfc09879873f31f14de93c1a57277ed20bdefb334112644dece04595260cf8].

## 1. What the spec requires

- **Order.** "Hooks MUST be called in the listed order" [H-1] `config.md:591`: prestart, createRuntime,
  createContainer, startContainer, poststart, poststop (`:552-588`). "The `prestart` hooks MUST be called
  before the `createRuntime` hooks" (`:598`).
- **createRuntime**: "after the runtime environment has been created … but before the `pivot_root` or any
  equivalent operation has been executed" (`:607`); path resolves in, and runs in, the runtime namespace
  (`:609-610`).
- **createContainer**: the same point, "before the `pivot_root` or any equivalent operation" (`:618`), and
  "MUST be called after the `createRuntime` hooks" (`:619`); its path "MUST resolve in the runtime
  namespace" (`:621`) and it "MUST be executed in the container namespace" (`:622`). Hook authors "should
  only expect … that the mount namespace and different mounts will be setup" (`:626`).
- **startContainer**: "MUST be called before the user-specified process is executed as part of the
  `start` operation" (`:630`); its path resolves in, and it runs in, the container namespace (`:633-634`).
  The summary table says the same (`:659-661`).
- **State on stdin.** "The state of the container MUST be passed to hooks over stdin" (`:592`). The state's
  `pid`: "For hooks executed in the runtime namespace, it is the pid as seen by the runtime. For hooks
  executed in the container namespace, it is the pid as seen by the container" [H-2] `runtime.md:26-27`.
- **Failure.** For each of prestart, createRuntime, createContainer and startContainer, "If any … hook
  fails, the runtime MUST generate an error, stop the container, and continue the lifecycle at step 12"
  [H-2] `runtime.md:62-70`. Step 8 runs the program, step 9 the poststart hooks (`:71-73`), so the order
  is startContainer, then the program, then poststart.
- **Timeout**: "the number of seconds before aborting the hook. If set, `timeout` MUST be greater than
  zero" [H-1] `config.md:559-560` (prestart's entry, which createRuntime's repeats at `:569-570`; the other lists share its schema, `:574-586`).
- **Path**: "This specification extends the IEEE standard in that **`path`** MUST be absolute" (`:556`).

## 2. runc v1.5.2

- The init builds the mounts and `/dev`, then asks the parent to run the create-time runtime hooks
  (`syncParentHooks`, [H-3] `rootfs_linux.go:205-212`): "The hooks are run after the mounts are setup, but
  before we switch to the new root". The parent sets the cgroup, then runs prestart and createRuntime with
  `Status = creating` and the init's host pid, and replies `procHooksDone` (`process_linux.go:1017-1046`).
- The init then `fchdir`s into the root and runs createContainer with its own `getpid()` and `creating`
  (`rootfs_linux.go:222-232`), then pivots (`:234-241`). Read-only and masked paths come after the pivot
  (`standard_init_linux.go:139`, `:144`).
- The state the init's hooks see is sent to it only when it has createContainer or startContainer hooks
  (`process_linux.go:897-902`).
- startContainer runs after the init has written the exec FIFO's byte (`standard_init_linux.go:269`) with
  its own pid and `created` (`:283-289`). `runc start` returns, and runs poststart, once it has read the
  byte. So runc's poststart may run before startContainer ends, and a startContainer failure ends the init
  after start has already succeeded.

## 3. crun 1.30.1

- createContainer runs in the init "after the mounts are set up but before the pivot_root"; when it is
  present crun takes the deferred-pivot path ([H-4] `container.c:1063-1079`). Like runc's, its createRuntime
  hooks are the parent's.
- The init reads the exec FIFO (`:1309-1329`) and then runs startContainer (`:1369-1384`), so start has
  already been answered, as in runc.
- crun turns off its `OPEN_TREE_NAMESPACE` path when create-time hooks exist (`:2250-2252` gathers the
  same five lists for the hooks' output; the open-tree check is cited in
  `oci-runtimes-and-fast-start.md` §2.5).

## 4. What follows for Hex

- createContainer must run from the container's init, in its namespaces, after its mounts and before its
  switch of root. createRuntime and prestart must come before it, and so before the switch as well. Hex
  ran them after the switch, with the init already waiting at the FIFO, which `config.md:607` doesn't
  allow.
- A container-namespace hook is given the pid the container sees (`runtime.md:27`), which only the init
  knows, so the init fills it in.
- startContainer can run before the FIFO byte: once the init's write-open of the FIFO returns, start has
  been called. If the byte comes only after the hooks, the spec's order (startContainer, program,
  poststart) holds, and a hook failure can be reported by start itself. Neither runc nor crun does this.
