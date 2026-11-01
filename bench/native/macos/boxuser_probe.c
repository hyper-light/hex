// What a privileged helper can bound a native macOS box with: the per-task physical-footprint ledger limit,
// its EXC_RESOURCE, and the box user (native plan step 2, docs/research/runtime/macos-box-user-and-ledger.md).
// Most subcommands need root (the ledger limit is PRIV_VM_FOOTPRINT_LIMIT, task.c:7593); they print what they
// needed if run without it. Root on the Mac Studio needs Ada's admin password (plan step 2); never store it.
//
//   priv MB            root sets task_set_phys_footprint_limit on a spawned sleeper and reads it back
//   cap fatal MB       a hog spawned with a FATAL per-task limit (posix_spawnattr_setjetsam_ext): overshoot and
//                      the kill signal — the hard kernel cap from the first instruction (also the spawn-arm path)
//   cap soft MB        a hog armed with a non-fatal task_set_phys_footprint_limit, watched on an EXC_MASK_RESOURCE
//                      port: time to the notification, that it fires once, and whether a root re-arm re-fires it
//   uid UID N          N members run as pool user UID, one daemonized (setsid): proc_listpids(PROC_UID_ONLY)
//                      finds all, RLIMIT_NPROC caps the forks, a kill-all empties the uid
//   box B N kind       a box of bound B MiB across N members each allocating at full speed, per-task FATAL limits
//                      apportioned to sum to B; kind=steady spawns them, kind=burst forks them at once: the box's
//                      total overshoot past B, and the watcher/helper CPU
// Build on the Mac: clang -O2 -Wall -Wextra -o boxuser_probe boxuser_probe.c
#include <errno.h>
#include <libproc.h>
#include <mach/mach.h>
#include <mach/mach_time.h>
#include <mach/task.h>
#include <pthread.h>
#include <signal.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/event.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

extern char **environ;

// task_set_phys_footprint_limit is the public MIG routine (mach/task.h); forward-declared here so the probe
// builds against any SDK that ships the trap but not the prototype. (task_get_phys_footprint_limit is not
// exported in user space on macOS 26, so the limit is read back through the `old` out-parameter of a second
// set, not a get.)
extern kern_return_t task_set_phys_footprint_limit(task_t task, int new_limit_mb, int *old_limit_mb);

// posix_spawnattr_setjetsam_ext (spawn_private.h, private): arms a per-process memory limit, in MiB, at spawn,
// so the leader is bounded from its first instruction. The FATAL flag makes the kernel kill the task at the
// limit (native-platforms research; signature per the step-1 agent's notes, xnu-6153.61.1 spawn_private.h).
#define POSIX_SPAWN_JETSAM_SET 0x8000
#define POSIX_SPAWN_JETSAM_MEMLIMIT_ACTIVE_FATAL 0x04
extern int posix_spawnattr_setjetsam_ext(posix_spawnattr_t *attr, short flags, int priority,
                                         int memlimit_active, int memlimit_inactive);

#define MIB (1024ULL * 1024ULL)
#define MAX_PIDS 8192

// This binary's own path, resolved once in main, so a subcommand can re-exec it as `__hog`.
static char g_self[4096];
static const char *getprogname_self(void) { return g_self; }

static uint64_t now_ns(void) { return clock_gettime_nsec_np(CLOCK_UPTIME_RAW); }

// A process's physical footprint now, in bytes, 0 if it has no such pid (ESRCH) or has ended.
static uint64_t footprint(pid_t pid) {
    struct rusage_info_v4 ri;
    if (proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)&ri) != 0) return 0;
    if (ri.ri_proc_exit_abstime != 0) return 0;
    return ri.ri_phys_footprint;
}
static uint64_t peak_footprint(pid_t pid) {
    struct rusage_info_v4 ri;
    if (proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)&ri) != 0) return 0;
    return ri.ri_lifetime_max_phys_footprint;
}

// A hog child: allocates `mb` MiB (or until killed) and touches every page, then sleeps so its footprint
// stays up to be read. Re-exec of this probe as `__hog MB`, so posix_spawn starts a fresh program whose
// jetsam limit (set at spawn) is active from its first instruction.
static int do_hog(uint64_t mb) {
    uint64_t n = mb * MIB;
    volatile char *p = malloc((size_t)n);
    if (!p) return 1;
    for (uint64_t i = 0; i < n; i += 4096) p[i] = 1;
    for (;;) sleep(1);
    return 0;
}

// Spawns `self __hog MB`, optionally with a fatal per-process memory limit `limit_mb` set at spawn. Returns
// the pid, or -1.
static pid_t spawn_hog(const char *self, uint64_t alloc_mb, int limit_mb, int fatal) {
    char ab[32];
    snprintf(ab, sizeof ab, "%llu", (unsigned long long)alloc_mb);
    char *argv[] = {(char *)self, (char *)"__hog", ab, NULL};
    posix_spawnattr_t attr;
    posix_spawnattr_init(&attr);
    if (limit_mb > 0) {
        short flags = POSIX_SPAWN_JETSAM_SET;
        if (fatal) flags |= POSIX_SPAWN_JETSAM_MEMLIMIT_ACTIVE_FATAL;
        if (posix_spawnattr_setjetsam_ext(&attr, flags, 40 /* a middling jetsam priority */, limit_mb,
                                          limit_mb) != 0)
            fprintf(stderr, "posix_spawnattr_setjetsam_ext: %s\n", strerror(errno));
    }
    pid_t pid = -1;
    int r = posix_spawn(&pid, self, NULL, &attr, argv, environ);
    posix_spawnattr_destroy(&attr);
    if (r != 0) {
        fprintf(stderr, "posix_spawn: %s\n", strerror(r));
        return -1;
    }
    return pid;
}

// The task port of a pid: task_for_pid, which needs privilege for another process (root here).
static int task_of(pid_t pid, task_t *out) {
    kern_return_t kr = task_for_pid(mach_task_self(), pid, out);
    if (kr != KERN_SUCCESS) {
        fprintf(stderr, "task_for_pid(%d): %s%s\n", pid, mach_error_string(kr),
                geteuid() == 0 ? "" : " (needs root)");
        return -1;
    }
    return 0;
}

// A forkhog child: forks a grandchild that allocates `alloc_mb` and sits, then waits. When this process was
// spawned with a fatal cap, the grandchild shows whether the cap survives a fork — it should not (fork
// resets the kernel's memory limit, kern_fork.c:1207-1209, and re-arming a running task needs root).
static int do_forkhog(uint64_t alloc_mb) {
    pid_t c = fork();
    if (c == 0) return do_hog(alloc_mb);
    for (;;) sleep(1);
}

// fork: spawn a leader with a FATAL cap of cap_mb that forks a grandchild allocating alloc_mb; report
// whether the grandchild is bounded (killed near cap_mb) or escapes (allocates past it and lives). This is
// the rootless gap the privileged helper closes: rootless a spawned process is capped, but its forks aren't.
static int do_fork(uint64_t cap_mb, uint64_t alloc_mb) {
    char ab[32];
    snprintf(ab, sizeof ab, "%llu", (unsigned long long)alloc_mb);
    char *argv[] = {(char *)getprogname_self(), (char *)"__forkhog", ab, NULL};
    posix_spawnattr_t attr;
    posix_spawnattr_init(&attr);
    posix_spawnattr_setjetsam_ext(&attr,
                                  (short)(POSIX_SPAWN_JETSAM_SET | POSIX_SPAWN_JETSAM_MEMLIMIT_ACTIVE_FATAL),
                                  40, (int)cap_mb, (int)cap_mb);
    pid_t leader = -1;
    int r = posix_spawn(&leader, getprogname_self(), NULL, &attr, argv, environ);
    posix_spawnattr_destroy(&attr);
    if (r != 0) {
        fprintf(stderr, "posix_spawn: %s\n", strerror(r));
        return 1;
    }
    usleep(200000);
    pid_t kids[MAX_PIDS];
    int bytes = proc_listpids(PROC_PPID_ONLY, (uint32_t)leader, kids, (int)sizeof kids);
    int found = bytes > 0 ? bytes / (int)sizeof(pid_t) : 0;
    pid_t gc = 0;
    for (int i = 0; i < found; i++)
        if (kids[i] != 0) gc = kids[i];
    if (gc == 0) {
        printf("fork: no grandchild found\n");
        kill(leader, SIGKILL);
        return 1;
    }
    uint64_t peak = 0;
    int alive = 0;
    for (int i = 0; i < 3000; i++) {
        uint64_t f = footprint(gc);
        if (f > peak) peak = f;
        alive = (kill(gc, 0) == 0);
        if (!alive) break;
        usleep(1000);
    }
    printf("fork: leader FATAL-capped at %llu MB; grandchild (forked) tried %llu MiB -> peak %llu MiB, "
           "alive=%d => the spawn cap %s the fork\n",
           (unsigned long long)cap_mb, (unsigned long long)alloc_mb, (unsigned long long)(peak / MIB), alive,
           (alive && peak / MIB > cap_mb) ? "does NOT bound (escaped, needs the helper)" : "bounds");
    kill(gc, SIGKILL);
    kill(leader, SIGKILL);
    while (waitpid(-1, NULL, WNOHANG) > 0) {}
    return 0;
}

// priv: root arms a ledger limit on a spawned sleeper and reads it back.
static int do_priv(uint64_t mb) {
    pid_t pid = spawn_hog(getprogname_self(), 0, 0, 0);
    if (pid < 0) return 1;
    usleep(100000);
    task_t task;
    if (task_of(pid, &task) != 0) { kill(pid, SIGKILL); return 1; }
    int old = -2;
    kern_return_t kr = task_set_phys_footprint_limit(task, (int)mb, &old);
    printf("task_set_phys_footprint_limit(%llu MB) -> %s (old %d MB)\n", (unsigned long long)mb,
           mach_error_string(kr), old);
    // Read back by setting the same limit again: its returned `old` is what the first set wrote.
    int readback = -2;
    kern_return_t kr2 = task_set_phys_footprint_limit(task, (int)mb, &readback);
    printf("read back -> %s (limit now %d MB, expected %llu)\n", mach_error_string(kr2), readback,
           (unsigned long long)mb);
    kill(pid, SIGKILL);
    waitpid(pid, NULL, 0);
    return kr == KERN_SUCCESS ? 0 : 1;
}

// cap fatal: spawn a hog with a FATAL limit of MB, let it allocate 4*MB; report overshoot and the kill.
static int do_cap_fatal(uint64_t mb) {
    pid_t pid = spawn_hog(getprogname_self(), mb * 4, (int)mb, 1);
    if (pid < 0) return 1;
    int status = 0;
    uint64_t peak = 0;
    for (int i = 0; i < 2000; i++) {
        uint64_t f = peak_footprint(pid);
        if (f > peak) peak = f;
        pid_t w = waitpid(pid, &status, WNOHANG);
        if (w == pid) break;
        usleep(1000);
    }
    if (WIFSIGNALED(status))
        printf("fatal limit %llu MB: killed by signal %d, peak footprint %llu MiB (overshoot %lld MiB)\n",
               (unsigned long long)mb, WTERMSIG(status), (unsigned long long)(peak / MIB),
               (long long)(peak / MIB) - (long long)mb);
    else
        printf("fatal limit %llu MB: NOT killed (status %d), peak %llu MiB\n", (unsigned long long)mb, status,
               (unsigned long long)(peak / MIB));
    kill(pid, SIGKILL);
    waitpid(pid, NULL, 0);
    return 0;
}

// An EXC_RESOURCE message as delivered with EXCEPTION_DEFAULT | MACH_EXCEPTION_CODES (id 2405): a complex
// message carrying the thread and task send rights, then the exception type and 64-bit codes.
#pragma pack(4)
typedef struct {
    mach_msg_header_t Head;
    mach_msg_body_t msgh_body;
    mach_msg_port_descriptor_t thread;
    mach_msg_port_descriptor_t task;
    NDR_record_t NDR;
    exception_type_t exception;
    mach_msg_type_number_t codeCnt;
    int64_t code[2];
    char trailer[64];
} exc_req_t;
typedef struct {
    mach_msg_header_t Head;
    NDR_record_t NDR;
    kern_return_t RetCode;
} exc_reply_t;
#pragma pack()

// Waits up to `timeout_ms` for one EXC_RESOURCE on `port`; returns 1 and fills *code0 on receipt, 0 on
// timeout. Replies KERN_SUCCESS so a synchronous (fatal/traced) delivery lets the task proceed.
static int recv_exc(mach_port_t port, int timeout_ms, int64_t *code0) {
    exc_req_t req;
    memset(&req, 0, sizeof req);
    mach_msg_return_t r = mach_msg(&req.Head, MACH_RCV_MSG | MACH_RCV_TIMEOUT, 0, sizeof req, port,
                                   (mach_msg_timeout_t)timeout_ms, MACH_PORT_NULL);
    if (r != MACH_MSG_SUCCESS) return 0;
    if (code0) *code0 = req.code[0];
    exc_reply_t reply;
    memset(&reply, 0, sizeof reply);
    reply.Head.msgh_bits = MACH_MSGH_BITS(MACH_MSGH_BITS_REMOTE(req.Head.msgh_bits), 0);
    reply.Head.msgh_size = sizeof reply;
    reply.Head.msgh_remote_port = req.Head.msgh_remote_port;
    reply.Head.msgh_local_port = MACH_PORT_NULL;
    reply.Head.msgh_id = req.Head.msgh_id + 100;
    reply.NDR = req.NDR;
    reply.RetCode = KERN_SUCCESS;
    mach_msg(&reply.Head, MACH_SEND_MSG, sizeof reply, 0, MACH_PORT_NULL, MACH_MSG_TIMEOUT_NONE,
             MACH_PORT_NULL);
    return 1;
}

// cap soft: arm a non-fatal ledger limit on a hog, catch its EXC_RESOURCE on a registered port, and see
// whether it fires once, whether the hog survives, and whether a re-arm re-fires it.
static int do_cap_soft(uint64_t mb) {
    pid_t pid = spawn_hog(getprogname_self(), mb * 4, 0, 0);
    if (pid < 0) return 1;
    usleep(100000);
    task_t task;
    if (task_of(pid, &task) != 0) { kill(pid, SIGKILL); return 1; }
    mach_port_t port;
    mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &port);
    mach_port_insert_right(mach_task_self(), port, port, MACH_MSG_TYPE_MAKE_SEND);
    kern_return_t kr = task_set_exception_ports(task, EXC_MASK_RESOURCE, port,
                                                EXCEPTION_DEFAULT | MACH_EXCEPTION_CODES, THREAD_STATE_NONE);
    if (kr != KERN_SUCCESS) fprintf(stderr, "task_set_exception_ports: %s\n", mach_error_string(kr));
    int old = -2;
    uint64_t t0 = now_ns();
    kr = task_set_phys_footprint_limit(task, (int)mb, &old);
    printf("soft limit %llu MB armed -> %s\n", (unsigned long long)mb, mach_error_string(kr));
    int64_t code0 = 0;
    int got = recv_exc(port, 5000, &code0);
    if (got)
        printf("EXC_RESOURCE after %llu us, code0=0x%llx, hog alive=%d, footprint %llu MiB\n",
               (unsigned long long)((now_ns() - t0) / 1000), (unsigned long long)code0,
               footprint(pid) > 0, (unsigned long long)(footprint(pid) / MIB));
    else
        printf("no EXC_RESOURCE within 5 s (hog alive=%d, footprint %llu MiB)\n", footprint(pid) > 0,
               (unsigned long long)(footprint(pid) / MIB));
    // Does it fire a second time without a re-arm? (It should not: once per task per limit, task.c:7529.)
    int again = recv_exc(port, 1000, &code0);
    printf("second notification without re-arm: %s\n", again ? "yes" : "no (as the source says)");
    kill(pid, SIGKILL);
    waitpid(pid, NULL, 0);
    return got ? 0 : 1;
}

// uid: run N members as pool user UID (one daemonized), list them, cap with RLIMIT_NPROC, then kill-all.
static int do_uid(uid_t uid, int n) {
    if (geteuid() != 0) { fprintf(stderr, "uid: needs root to start members as another user\n"); return 1; }
    // A leader that drops to UID, sets RLIMIT_NPROC, and forks N-1 members, one of which setsid()s.
    pid_t leader = fork();
    if (leader == 0) {
        struct rlimit rl = {.rlim_cur = (rlim_t)(n + 4), .rlim_max = (rlim_t)(n + 4)};
        setrlimit(RLIMIT_NPROC, &rl);
        if (setgid(uid) != 0 || setuid(uid) != 0) _exit(3);
        for (int i = 0; i < n - 1; i++) {
            pid_t c = fork();
            if (c == 0) {
                if (i == 0) setsid(); // one daemonizes: leaves the group/session, keeps the uid
                for (;;) sleep(1);
            }
        }
        for (;;) sleep(1);
    }
    usleep(300000);
    pid_t pids[MAX_PIDS];
    int bytes = proc_listpids(PROC_UID_ONLY, uid, pids, (int)sizeof pids);
    int found = bytes > 0 ? bytes / (int)sizeof(pid_t) : 0;
    int alive = 0;
    for (int i = 0; i < found; i++)
        if (pids[i] != 0 && kill(pids[i], 0) == 0) alive++;
    printf("uid %u: started %d members, proc_listpids(PROC_UID_ONLY) found %d\n", uid, n, alive);
    // Delete: kill every process of the uid, then confirm none is left.
    for (int i = 0; i < found; i++)
        if (pids[i] != 0) kill(pids[i], SIGKILL);
    usleep(300000);
    while (waitpid(-1, NULL, WNOHANG) > 0) {}
    int left = 0;
    bytes = proc_listpids(PROC_UID_ONLY, uid, pids, (int)sizeof pids);
    int after = bytes > 0 ? bytes / (int)sizeof(pid_t) : 0;
    for (int i = 0; i < after; i++)
        if (pids[i] != 0 && kill(pids[i], 0) == 0) left++;
    printf("uid %u: after delete, %d processes left\n", uid, left);
    return left == 0 ? 0 : 1;
}

// box: N members each allocating at full speed under per-task FATAL limits summing to B MiB; the box total's
// overshoot past B, and the watcher's CPU. steady spawns each member; burst forks them from one leader.
static int do_box(uint64_t b_mib, int n, const char *kind) {
    if (n < 1) n = 1;
    // Apportion B equally as per-task fatal limits (the shell's hex_core::boxuser does this with a reserve;
    // the probe measures the raw kernel behaviour, equal shares, no reserve).
    int share = (int)(b_mib / (uint64_t)n);
    pid_t pids[256];
    int np = 0;
    uint64_t t0 = now_ns();
    if (strcmp(kind, "burst") == 0) {
        // One leader forks N hogs at once; the probe arms each via task_for_pid after the fork (the window).
        pid_t leader = fork();
        if (leader == 0) {
            for (int i = 0; i < n; i++)
                if (fork() == 0) do_hog(b_mib); // child allocates the whole bound, so the share must bite
            for (;;) sleep(1);
        }
        usleep(50000);
        pid_t all[MAX_PIDS];
        int bytes = proc_listpids(PROC_PPID_ONLY, (uint32_t)leader, all, (int)sizeof all);
        int found = bytes > 0 ? bytes / (int)sizeof(pid_t) : 0;
        for (int i = 0; i < found && np < 256; i++) {
            if (all[i] == 0) continue;
            task_t task;
            if (task_for_pid(mach_task_self(), all[i], &task) == KERN_SUCCESS) {
                int old = 0;
                task_set_phys_footprint_limit(task, share, &old);
            }
            pids[np++] = all[i];
        }
    } else {
        for (int i = 0; i < n && np < 256; i++) {
            pid_t p = spawn_hog(getprogname_self(), b_mib, share, 1);
            if (p > 0) pids[np++] = p;
        }
    }
    // Watch the box total for 2 s; the members are fatally capped at their share, so the total should peak
    // near B. Each member's lifetime peak footprint is summed (it survives on a zombie that dies at its
    // cap), so the peak total holds even as fatally-capped members are killed.
    uint64_t peak_total = 0;
    struct rusage ru0, ru1;
    getrusage(RUSAGE_SELF, &ru0);
    for (int t = 0; t < 2000; t++) {
        uint64_t total = 0;
        for (int i = 0; i < np; i++) total += peak_footprint(pids[i]);
        if (total > peak_total) peak_total = total;
        usleep(1000);
    }
    getrusage(RUSAGE_SELF, &ru1);
    double cpu_ms = (double)(ru1.ru_utime.tv_sec - ru0.ru_utime.tv_sec) * 1000.0 +
                    (double)(ru1.ru_utime.tv_usec - ru0.ru_utime.tv_usec) / 1000.0 +
                    (double)(ru1.ru_stime.tv_sec - ru0.ru_stime.tv_sec) * 1000.0 +
                    (double)(ru1.ru_stime.tv_usec - ru0.ru_stime.tv_usec) / 1000.0;
    printf("box %llu MiB, %d members (%s): peak total %llu MiB (overshoot %lld MiB), probe CPU %.1f%% of a "
           "core over %llu ms\n",
           (unsigned long long)b_mib, n, kind, (unsigned long long)(peak_total / MIB),
           (long long)(peak_total / MIB) - (long long)b_mib, cpu_ms / 2000.0 * 100.0,
           (unsigned long long)((now_ns() - t0) / 1000000));
    for (int i = 0; i < np; i++) kill(pids[i], SIGKILL);
    while (waitpid(-1, NULL, WNOHANG) > 0) {}
    return 0;
}

int main(int argc, char **argv) {
    // Resolve this binary's path for re-exec as __hog.
    uint32_t sz = sizeof g_self;
    if (proc_pidpath(getpid(), g_self, sz) <= 0) snprintf(g_self, sz, "%s", argv[0]);

    if (argc >= 3 && strcmp(argv[1], "__hog") == 0) return do_hog(strtoull(argv[2], NULL, 10));
    if (argc >= 3 && strcmp(argv[1], "__forkhog") == 0) return do_forkhog(strtoull(argv[2], NULL, 10));
    if (argc >= 3 && strcmp(argv[1], "priv") == 0) return do_priv(strtoull(argv[2], NULL, 10));
    if (argc >= 4 && strcmp(argv[1], "fork") == 0)
        return do_fork(strtoull(argv[2], NULL, 10), strtoull(argv[3], NULL, 10));
    if (argc >= 4 && strcmp(argv[1], "cap") == 0 && strcmp(argv[2], "fatal") == 0)
        return do_cap_fatal(strtoull(argv[3], NULL, 10));
    if (argc >= 4 && strcmp(argv[1], "cap") == 0 && strcmp(argv[2], "soft") == 0)
        return do_cap_soft(strtoull(argv[3], NULL, 10));
    if (argc >= 4 && strcmp(argv[1], "uid") == 0)
        return do_uid((uid_t)strtoul(argv[2], NULL, 10), atoi(argv[3]));
    if (argc >= 5 && strcmp(argv[1], "box") == 0)
        return do_box(strtoull(argv[2], NULL, 10), atoi(argv[3]), argv[4]);
    fprintf(stderr,
            "usage: boxuser_probe priv MB | cap fatal MB | cap soft MB | uid UID N | box B N steady|burst\n");
    return 2;
}
