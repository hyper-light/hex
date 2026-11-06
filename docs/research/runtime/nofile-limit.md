# The node's descriptor limit: systemd, the kernel, Docker, runc and crun

Source notes for the native plan's step 6 (the Linux installer). The installer runs the node agent as a
systemd **user** service, `hex-node.service`, under the user's manager (`user@UID.service`). At its bounds
the agent needs a soft `RLIMIT_NOFILE` of at least **594,304**: its own descriptors (330,112) plus four
held `/proc/net` files for each of up to 66,048 kept network namespaces (264,192;
`bench/install/linux_lab_inside.sh:87-90`). This note establishes what limit such a unit can get, who
can raise it, and what an `oci` box whose spec names no `RLIMIT_NOFILE` then inherits, compared with
what Docker, containerd, runc and crun give.

Sources were read at fixed revisions, accessed 2026-11-04. Each quote is one line of its source, copied
exactly (a fragment of a line where marked …); `path:line` is at that revision.

| Source | Revision |
|---|---|
| systemd (`systemd/systemd`) | tag v258 = `781d9d0789379d1ea1f2ecefb804d41e9c8b6c38`; v259 = `9ca433482f2281d71718718705ca8cd3bf562ad6` |
| Linux (`torvalds/linux`) | `05441d697f7f9a927264f4058cbbc6a03dfcac9d` |
| man-pages (`git.kernel.org/pub/scm/docs/man-pages/man-pages.git`) | tag man-pages-6.20 = `1bffe43b9ff4a4d8f73d33f6bd24743f4c1254d6` |
| moby (`moby/moby`) | master `b56dc7e027c6ff860465fe20f4646b7632365d83`; commit `c8930105bc9fc3c1a8a90886c23535cc6c41e130` (PR #45534), its parent `b1c112d35e4f1921ca82c9f76d86e5c92c2a9280` |
| `docker/docs` | `d40e4d2c029e471aa18744e28bebe179d96abb6f` |
| containerd (`containerd/containerd`) | main `8a9c72699b5a29daa94cbf1bfe270f1bd1b52d12`; commits `3ca39ef01608fdd44245c0173bf071682b3bfe3c` (PR #8924), `c691c36614622b205a859ae7e656badb4a553076`; tags v1.7.29 `442cb34b`, v2.0.0 `207ad711` |
| runc (`opencontainers/runc`) | tag v1.4.3 = `bb14dabeb7185bb72c8c86735d090dcb20f36587` |
| Go (`golang/go`) | tag go1.24.0 = `3901409b5d0fb7c85a3e6730a59943cc93b2835c` (runc v1.4.3's `go.mod:3` "go 1.24.0"); go1.26.9 = `2ae494ef9fb90e0da0073c31800ba38870249994` |
| crun (`containers/crun`) | tag 1.30.1 = `079ff6a7a16d029460af882f9ea44674e6a510b6` |
| OCI runtime-spec (`opencontainers/runtime-spec`) | tag v1.3.0 = `92249139eea7161e13745abd4cb6d0ea02a3227a` |
| Meson (`mesonbuild/meson`) | tag 1.9.0 = `75ef08140346d9a058546493036a31431774eb7a` |

URLs are `https://raw.githubusercontent.com/<owner>/<repo>/<revision>/<path>`; man-pages are
`https://git.kernel.org/pub/scm/docs/man-pages/man-pages.git/plain/<path>?h=man-pages-6.20`; commit
messages are `https://github.com/<owner>/<repo>/commit/<sha>` (message line numbers counted from the
subject as 1). Facts are `DESIGN` unless marked `MEASURED-OWN`.

## 1. systemd

### 1.1 `LimitNOFILE=` syntax

- `man/systemd.exec.xml:1013-1015` (v258): "either as single value to set a specific soft and hard limit
  to the same value, or as colon-separated" / "pair <option>soft:hard</option> to set both limits
  individually" / "(e.g. <literal>LimitAS=4G:16G</literal>).  Use the string <option>infinity</option>
  to configure no". So `LimitNOFILE=N` sets both, `LimitNOFILE=S:H` each.
- The table's note for `LimitNOFILE=` (`:1122`, one line): "… Be careful when raising the soft limit
  above 1024, since <citerefentry project='man-pages'><refentrytitle>select</refentrytitle>…" and "…
  Nowadays, the hard limit defaults to 524288, a very high value compared to historical defaults.
  Typically applications should increase their soft limit to the hard limit on their own, …". Same
  lines in v259.

### 1.2 `DefaultLimitNOFILE=` is 1024:524288, and why

- `man/systemd-system.conf.xml:269` (v258): "<listitem><para><varname>DefaultLimitNOFILE=</varname>
  defaults to 1024:&HIGH_RLIMIT_NOFILE;." The entity is set in `meson.build:115`
  "conf.set('HIGH_RLIMIT_NOFILE',          512*1024)" (v259 `:107`), i.e. 524,288.
- PID 1 itself runs higher (`:279-281`): "Note that the service manager internally in PID 1 bumps
  <varname>RLIMIT_NOFILE</varname> and" … "defaults for all child processes forked off.</para>".
- Where it is set: `src/core/main.c` `fallback_rlimit_nofile` (v258). The select(2) reasoning, `:2652-2653`:
  "* not the soft limit. That's because select() works the way it works, and chokes on fds" / "* >=
  1024. If we'd bump the soft limit globally, it might accidentally happen to". Only the **system**
  manager raises the hard limit, `:2660-2661`: "if (arg_runtime_scope == RUNTIME_SCOPE_SYSTEM)" /
  "rl->rlim_max = MIN((rlim_t) read_nr_open(), MAX(rl->rlim_max, (rlim_t) HIGH_RLIMIT_NOFILE));". Every
  manager, user ones included, caps the soft limit it passes on, `:2666`: "rl->rlim_cur =
  MIN(rl->rlim_cur, (rlim_t) FD_SETSIZE);". So **a user manager's units default to 1024 soft and the
  user manager's own hard limit**, which it inherited from `user@UID.service`.
- systemd may also raise `fs.nr_open`: `meson_options.txt:76-77` "option('bump-proc-sys-fs-nr-open',
  type : 'boolean'," / "description : 'bump /proc/sys/fs/nr_open to INT_MAX')", with no `value :`; a
  Meson boolean with no default is true (Meson `docs/markdown/Build-options.md:43-44` "Booleans may have
  values of either `true` or `false`. If no default" / "value is supplied then `true` will be used as
  the default."). `main.c:1259` "/* Let's bump fs.file-max and fs.nr_open to their respective maximums. …"
  `MEASURED-OWN` on adacachy (CachyOS, Linux 7.2.9), 2026-11-04: `/proc/sys/fs/nr_open` reads
  2147483584. A distro can build with the option off; **gap:** Fedora's, Debian's and Ubuntu's build
  settings were not read.

### 1.3 A user manager can't raise a unit above its own hard limit, and clamps silently

- `man/systemd.exec.xml:1054-1056` (v258): "in a user service (i.e. a service run by the per-user
  instance of the service manager) they cannot be" / "used to raise the limits above those set for the
  user manager itself when it was first invoked, as" / "the user's service manager generally lacks the
  privileges to do so." The way up, `:1060-1063`: "privileges. In most cases it is possible to configure
  higher per-user resource limits via PAM or by" / "setting limits on the system service encapsulating
  the user's service manager, i.e. the user's" / "instance of <filename>user@.service</filename>. After
  making such changes, make sure to restart the" / "user's service manager.</para>".
- A unit asking for more is **clamped, not refused**: `src/core/exec-invoke.c:5705`
  "r = setrlimit_closest_all((const struct rlimit* const *) context->rlimit, &which_failed);", and
  `src/basic/rlimit-util.c:28-29` "/* So we failed to set the desired setrlimit, then let's try" / "* to
  get as close as we can */", `:39-40` ".rlim_cur = MIN(rlim->rlim_cur, highest.rlim_max)," /
  ".rlim_max = MIN(rlim->rlim_max, highest.rlim_max),", reported only at debug level (`:48`
  "log_debug(\"Failed at setting rlimit \" …").
- PAM can override the drop-in: `user@.service.in:19` "PAMName=systemd-user", and
  `exec-invoke.c:5702-5703` "/* Let's set the resource limits before we call into PAM, so that
  pam_limits wins over what" / "* is set here. (See below.) */". A `nofile` line in
  `/etc/security/limits.conf` for the user therefore wins over the installer's drop-in.

## 2. Kernel and man-pages: the ceilings

- `Documentation/admin-guide/sysctl/fs.rst:136-139`: "This denotes the maximum number of file-handles a
  process can" / "allocate. Default value is 1024*1024 (1048576) which should be" / "enough for most
  machines. Actual limit depends on ``RLIMIT_NOFILE``" / "resource limit." In code, `fs/file.c:96`
  "unsigned int sysctl_nr_open __read_mostly = 1024*1024;".
- The hard limit can't pass it, even for root, `kernel/sys.c:1516-1518` "if (resource == RLIMIT_NOFILE
  &&" / "new_rlim->rlim_max > sysctl_nr_open)" / "return -EPERM;"; raising the hard limit at all needs
  `CAP_SYS_RESOURCE` in the **initial** user namespace, `:1526` "* Keep the capable check against
  init_user_ns until cgroups can", `:1529-1530` "if (new_rlim->rlim_max > rlim->rlim_max &&" /
  "!capable(CAP_SYS_RESOURCE))". The kernel's own initial limit is 1024:4096
  (`include/asm-generic/resource.h:20` "[RLIMIT_NOFILE]		= {   INR_OPEN_CUR,   INR_OPEN_MAX },	\";
  `include/uapi/linux/fs.h:37-38` "#define INR_OPEN_CUR 1024	/* Initial setting for nfile rlimits */" /
  "#define INR_OPEN_MAX 4096	/* Hard limit for nfile rlimits */").
- `man/man2/getrlimit.2:59-60` (man-pages-6.20): "an unprivileged process may set only its soft limit
  to a value in the" / "range from 0 up to the hard limit, and (irreversibly) lower its hard limit.";
  `:61`, `:63` "A privileged process (under Linux: one with the" … "capability in the initial user
  namespace)". Errors, `:491` "An unprivileged process tried to raise the hard limit;", `:497`, `:499-500`
  "The caller tried to increase the hard" … "limit above the maximum defined by" / "/proc/sys/fs/nr_open".
  Inheritance, `:569`, `:571-572`: "A child process created via" … "inherits its parent's resource
  limits." / "Resource limits are preserved across" (execve(2)).
- `man/man5/proc_sys_fs.5:234`, `:238-239`: "This file imposes a ceiling on the value to which the" …
  "This ceiling is enforced for both unprivileged and privileged process." / "The default value in this
  file is 1048576."

So 1,048,576 is the highest hard limit settable on **every** kernel without touching `fs.nr_open`, and
only a privileged process (here: PID 1 starting `user@UID.service`) can set it.

## 3. Docker and containerd

- moby's `docker.service` had `LimitNOFILE=infinity` (parent `b1c112d3`, `contrib/init/systemd/docker.service:31`
  "LimitNOFILE=infinity"; added in `80039b4699e3`, 2018) until `c8930105` removed it (PR #45534, 2023).
  Its message, line 3: "During review, it was decided to remove `LimitNOFILE` from `docker.service` to
  rely on the systemd v240 implicit default of `1024:524288`. …"; line 6: "- `524288` hard limit is an
  implicit default since systemd v240 and is adequate for most processes …"; line 8: "- When set to
  `infinity` (usually as the soft limit) software may experience significantly increased resource
  usage, resulting in a performance regression or runtime failures that are difficult to
  troubleshoot." At master `b56dc7e0` the unit names only `LimitNPROC=infinity` and `LimitCORE=infinity`
  (`:23-24`), no `LimitNOFILE`.
- containerd's `containerd.service`: `1048576` → `infinity` in `c691c366` (2020, "systemd: use
  LimitNOFILE=infinity instead of hard-coded max value"), removed in `3ca39ef0` (PR #8924), message line
  5: "Remove `LimitNOFILE` from `containerd.service` to rely on the systemd v240 implicit default of
  `1024:524288`. …". v1.7.29 still has it (`containerd.service:35` "LimitNOFILE=infinity"); v2.0.0 and
  v2.1.5 do not.
- What a container gets with no `--ulimit`: moby sets the spec's rlimits from the user's and the
  daemon's `default-ulimits` only, `daemon/oci_linux.go:57` "s.Process.Rlimits = rlimits" (built at
  `:46-52` from `hostConfig.Ulimits`), over a default spec that names none (`daemon/pkg/oci/defaults.go`:
  a search for `rlimit` finds nothing). With neither set, the list is empty and the container inherits.
  containerd's own default spec names 1024:1024 (`pkg/oci/spec.go:188-190` "Type: \"RLIMIT_NOFILE\","
  / "Hard: uint64(1024)," / "Soft: uint64(1024),"), but its CRI path clears it,
  `internal/cri/opts/spec_opts.go:192-193` "// Remove default rlimits (See
  https://github.com/containerd/cri/issues/515)" / "s.Process.Rlimits = nil".
- Docker's release notes, `content/manuals/engine/release-notes/25.0.md:208` "- The daemon now uses
  systemd's default `LimitNOFILE`. In earlier versions of", `:214-215` "The new setting makes containers
  behave the same way as programs running on" / "the host, but may cause programs that make incorrect
  assumptions based on the", `:219-221` "This change currently only affects build containers created
  with `docker" / "build` when using BuildKit with the `docker` driver. Starting with Docker" / "Engine
  v29.0 (containerd v2.1.5), this limit applies to all containers, not". So from Docker 29 a container
  with no ulimit runs at **1024:524288**; before, it inherited containerd's `infinity` (i.e. nr_open).

## 4. runc, crun and the OCI spec when the spec names no `RLIMIT_NOFILE`

- OCI runtime-spec v1.3.0 `config.md:261` "* **`rlimits`** (array of objects, OPTIONAL) allows setting
  resource limits for the process."; `:274`, `:276` "* **`hard`** (uint64, REQUIRED) the ceiling for the soft
  limit that could be set by an unprivileged process." … "Only a privileged process (e.g. one with the
  `CAP_SYS_RESOURCE` capability) can raise a hard limit." **Gap:** the spec says nothing of a type the
  list omits; that the process keeps the runtime's limit follows from getrlimit(2)'s inheritance (§2).
- **runc** passes on its caller's limit as the caller had it, not Go's raised one. Go raises its soft
  limit at start (go1.24.0 `src/syscall/rlimit.go:20` "// Go does not use select, so it should not be
  subject to these limits.", `:33` "origRlimitNofile.Store(&lim)", `:42` "nlim.Cur = nlim.Max - 1") and
  restores the original before `execve` (`src/syscall/exec_unix.go:288-290` "rlim :=
  origRlimitNofile.Load()" / "if rlim != nil {" / "Setrlimit(RLIMIT_NOFILE, rlim)") and in a forked
  child (go1.26.9 `src/syscall/exec_linux.go:653` "RawSyscall6(SYS_PRLIMIT64, 0, RLIMIT_NOFILE,
  uintptr(unsafe.Pointer(rlim)), 0, 0, 0)"). runc v1.4.3's init ends in that `syscall.Exec`
  (`libcontainer/standard_init_linux.go:305` "return linux.Exec(name, l.config.Args, l.config.Env)",
  `internal/linux/linux.go:21` "return unix.Exec(cmd, args, env)", vendored
  `golang.org/x/sys/unix/syscall_unix.go:578` "return syscall.Exec(argv0, argv, envv)"). When the spec
  does name `RLIMIT_NOFILE`, runc sets it on the init by `prlimit` (`libcontainer/init_linux.go:616`)
  and drops Go's saved value so the restore doesn't undo it (`:241-243` "// Clean the RLIMIT_NOFILE
  cache in go runtime." / "// Issue: https://github.com/opencontainers/runc/issues/4195" /
  "maybeClearRlimitNofileCache(config.Rlimits)"; `libcontainer/system/rlimit_linux.go:8` "// is process
  RLIMIT_NOFILE values. Relies on go.dev/cl/588076."). **Gap:** runc issue #4195 and Go CL 588076 were
  not opened; they are cited only as the code cites them. **Gap:** the Go release that built a given
  runc binary was not checked (go1.24.0 and go1.26.9 read alike here).
- **crun** touches rlimits only from the spec: `src/libcrun/linux.c:4863` "for (i = 0; i < len; i++)",
  `:4875` "if (UNLIKELY (setrlimit (resource, &limit) < 0))", called with the spec's list
  (`src/libcrun/container.c:3705-3706` "/* This must be done before we enter a user namespace.  */" /
  "ret = libcrun_set_rlimits (process->rlimits, process->rlimits_len, err);"; `linux.c:6496`). A search
  of `src/` for `setrlimit`/`prlimit` finds only that line and `ebpf.c`'s `RLIMIT_MEMLOCK`. So a crun
  container with no entry inherits crun's caller's limits exactly, raised soft included.

## 5. What Hex should do

1. **`user@.service` drop-in** (`/etc/systemd/system/user@.service.d/50-hex-delegate.conf`, `[Service]`):
   `LimitNOFILE=1024:1048576`. The hard limit is the kernel's default `fs.nr_open` (§2), the most a
   privileged PID 1 can set on any kernel without a sysctl change, and covers 594,304 with 454,272
   spare; the soft 1024 keeps every other user service where systemd puts it for select(2) (§1.2), and
   the user manager caps its children's soft at `FD_SETSIZE` regardless (`main.c:2666`). Not `infinity`:
   it resolves to `nr_open` (up to 2^31 on a systemd-bumped host, §1.2), the condition moby and
   containerd removed (§3). The user's manager must **restart** to see it (§1.3, "make sure to restart
   the"): the installer's `systemctl reload user@UID.service` (installer-per-os.md §1.4) is a re-exec of
   the same process, and limits are preserved across execve(2) (§2), so a reload alone should leave the
   old hard limit — **inference, not measured** (`LimitNOFILE=` is applied in the spawn path,
   `exec-invoke.c:5705`; no path re-applying it to a running unit was looked for). The installer should
   restart the user manager (or the lab must show a reload suffices), and the lab should read
   `/proc/<user manager>/limits` after install.
2. **`hex-node.service`**: `LimitNOFILE=1024:1048576`. The unit can't go above the user manager's hard
   limit (§1.3); naming it documents the need, and the node keeps raising its own soft limit to the hard
   one at start (`crates/hex-cli/src/node.rs:98`), as systemd advises (§1.1).
3. **Check, don't trust**: systemd clamps a too-high request silently and `pam_limits` overrides the
   drop-in (§1.3), so the node must read its soft limit after raising it and refuse to start, with a
   typed refusal naming the limit it has and the 594,304 it needs (CLAUDE.md §3), rather than meet
   `EMFILE` at load. The lab's HEX-INST-5 check (`bench/install/linux_lab_inside.sh:91-99`) is the
   falsifier for the install path.
4. **Boxes whose spec names no `RLIMIT_NOFILE`**: a child inherits the node's limits (§2) and Hex is not
   Go, so without action an `oci` (or `process`) box gets the node's **raised** soft limit, 1,048,576,
   which is crun's behaviour (§4) and the pre-Docker-25 `infinity` condition. Docker 29 with runc gives
   1024:524288 (§3). Hex should do as Go and runc do: keep the limit the node inherited before raising it
   and set it on every box whose spec names none, giving 1024:1048576 under the recommended units (soft
   as Docker's, hard 2x). A spec that names `RLIMIT_NOFILE` gets what it names; a hard limit above the
   node's is refused by the kernel with `EPERM` unless the node holds `CAP_SYS_RESOURCE` in the initial
   namespace (§2), which a rootless node does not, so the oci class should refuse it by name. What a box
   gets today is recorded by `crates/hex-cli/tests/node_socket.rs:98-107`; **not measured in this note.**

Falsifiers: `/proc/<hex-node>/limits` after a fresh install shows a soft limit under 594,304; a
user-manager unit gets a hard limit above its manager's; or `setrlimit` of `RLIMIT_NOFILE` hard
1,048,576 fails with `EPERM` on a kernel whose `fs.nr_open` is the default.

## 6. macOS: which boxes get the default, and why plain ones don't

Ada's call (2026-11-06, native plan step 6): a box whose spec names no `RLIMIT_NOFILE` gets Docker 29's
1024:524288, capped at the node's hard limit (`hex_core::limits`).

- **Box-user boxes get it.** The privileged helper starts each program from a forked child that calls
  only async-signal-safe libc before `execve`; it sets `RLIMIT_NOFILE` there beside `RLIMIT_NPROC`, capped
  at the helper's own hard limit (`crates/hex-runtime/src/macos/helper_sys.rs`, `fork_exec_as`). The root
  lab's `box_user_boxes_hold_the_contract` checks it (HEX-INST-6).
- **Plain (process-group) boxes keep the embedding process's limit.** There is no macOS `hex node`
  (`crates/hex-cli/src/lib.rs:23-24`: Linux only), and the installer installs only the helper, so a
  process box's members inherit whatever process embeds `MacRuntime`. Hex raises no limit on macOS. Measured
  on adas-mac-studio-2 (macOS 26.2, 2026-11-06), a shell started by kiln's runner: soft 10,240, hard
  unlimited, with `kern.maxfilesperproc` 138,240 bounding any process's descriptors below 524,288. Not
  measured: a launchd-started process's (launchd's `maxfiles`).
- **Why the default isn't applied there.** XNU's `posix_spawn`, which the class uses for its group,
  signals, QoS clamp and fatal memory cap, has no rlimit attribute (`bsd/kern/kern_exec.c`, `posix_spawn()`;
  `posix_spawnattr_set_filedesclimit_ext` is a separate descriptor count that notifies, not
  `RLIMIT_NOFILE`, `bsd/kern/kern_descrip.c` `fd_check_limit_exceeded`; XNU `main` f6217f891ac0). Setting it
  in a forked child before the spawn means calling `posix_spawn` after a fork of the multi-threaded node,
  which isn't async-signal-safe (it may take a malloc or libc lock another thread held at the fork): a
  deadlock hazard (CLAUDE.md §3), so not done (the lead, 2026-11-06). Lowering the node's own limit
  around a spawn would give its other threads `EMFILE`.
- **Measured, the fork path failed outright.** Built once (c205566, reverted at 1c8309f), its merge into
  cp/t7-native (e048975, `crates/hex-runtime/src/macos` byte for byte d14bc2a's) failed every spawn on
  adas-mac-studio-2 with `Bad file descriptor (os error 9)` at each test's first exec. That was
  `macos_process_box` 0 passed, 8 failed, twice, in a launchd one-shot outside kiln's run tree
  (`hex-integrate-mac-e048975-20261106T092002Z`). With the revert (6ef0169) the same suite passed 7/7 three
  times on adas-mac-studio.local-2.
- **The safe route, if plain boxes must have it:** move their spawn into a single-threaded helper process,
  as box-user mode does, whose child can call `setrlimit` before `execve`. Not built; the lead's call.

Falsifiers: a box-user program shows other than `min(1024, hard)`:`min(524288, helper hard)`; a macOS
`hex node` appears that raises its own limit (then plain boxes would inherit the raised one).
