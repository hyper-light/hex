# How big a program's arguments and environment may be, on each OS

Hex's `Program` carries its arguments and its whole environment. CLAUDE.md §2 asks every input a bound and §3 a
typed refusal, so the bound each OS's exec enforces is read here, at its source, and Hex refuses past it by name
before any spawn, instead of meeting the kernel's `E2BIG` (or, on Windows, no limit at all) afterwards.

## Linux (v6.19)

`execve` checks two things.

- **The whole block.** `bprm_stack_limits`
  ([`fs/exec.c:398-441`](https://raw.githubusercontent.com/torvalds/linux/v6.19/fs/exec.c)): the strings may take
  a quarter of the stack's soft limit, capped at three quarters of `_STK_LIM` and never under `ARG_MAX`:
  `limit = _STK_LIM / 4 * 3; limit = min(limit, bprm->rlim_stack.rlim_cur / 4); limit = max_t(unsigned long, limit,
  ARG_MAX);` (:410-416). The argv and envp pointers come out of it first: `ptr_size = (max(argc, 1) + envc) *
  sizeof(void *)`, refused if `limit <= ptr_size`, then `limit -= ptr_size` (:433-438). `_STK_LIM` is 8 MiB
  ([`include/uapi/linux/resource.h:66`](https://raw.githubusercontent.com/torvalds/linux/v6.19/include/uapi/linux/resource.h));
  `ARG_MAX` is 131,072 bytes
  ([`include/uapi/linux/limits.h:8`](https://raw.githubusercontent.com/torvalds/linux/v6.19/include/uapi/linux/limits.h)).
  The filename is copied onto the same stack before the strings (`copy_string_kernel(bprm->filename, bprm)`).
- **Each string.** `valid_arg_len`: `len <= MAX_ARG_STRLEN` (fs/exec.c:191-194, with an MMU), and `MAX_ARG_STRLEN`
  is `PAGE_SIZE * 32`
  ([`include/uapi/linux/binfmts.h:15`](https://raw.githubusercontent.com/torvalds/linux/v6.19/include/uapi/linux/binfmts.h)).

So Hex reads both at run time, kind (a): the stack's soft limit (`getrlimit(RLIMIT_STACK)`, which rustix wraps
safely) through the kernel's own formula, and the page size. glibc's `sysconf(_SC_ARG_MAX)` returns a quarter of the
stack limit without the kernel's 6 MiB cap, so it would let through what the kernel refuses; the formula above is the
kernel's.

## macOS (XNU, xnu-12377.61.12)

`exec_add_user_string` charges each argv and envv string to `ip_argspace`, refusing with `E2BIG` past it
([`bsd/kern/kern_exec.c:448-468`](https://raw.githubusercontent.com/apple-oss-distributions/xnu/xnu-12377.61.12/bsd/kern/kern_exec.c)),
and `exec_extract_strings` charges each argv/envv pointer slot to the same budget (:6546-6549, :6629-6633,
:6664-6665). The budget is `NCARGS` (:578), which is `ARG_MAX`
([`bsd/sys/param.h:99`](https://raw.githubusercontent.com/apple-oss-distributions/xnu/xnu-12377.61.12/bsd/sys/param.h)),
1 MiB on macOS
([`bsd/sys/syslimits.h:75`](https://raw.githubusercontent.com/apple-oss-distributions/xnu/xnu-12377.61.12/bsd/sys/syslimits.h)).
The running kernel reports it as the `kern.argmax` sysctl, which Hex reads at run time, kind (a).

## Windows

Microsoft's documentation ([Environment Variables](https://learn.microsoft.com/en-us/windows/win32/procthread/environment-variables))
says "The maximum size of a user-defined environment variable is 32,767 characters" and "There is no technical
limitation on the size of the environment block". So each variable has a platform bound, kind (c), and the block
takes one from data: the largest environment in a census of real container images, with a stated margin, kind (b).
Native step 4 measures and applies both.

## Counting

Hex counts a program as Linux does, which is at least what XNU counts: each argument and each `KEY=VALUE` string with
its NUL, the first argument once more for the filename, and one pointer per string (at least one argument). A count
at or under the bound runs; one past it is refused with `Refusal::Invalid` naming both sizes and the bound's source.
