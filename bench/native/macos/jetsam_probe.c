// What a rootless macOS box can cap a spawned member with, and how its death is told from an ordinary
// SIGKILL (native plan step 1, the `_ext` backstop). `posix_spawnattr_setjetsam_ext` (libSystem, declared
// below as spawn_private.h declares it) arms a per-process physical-footprint limit, in MiB, fatal: the
// kernel SIGKILLs the process at the limit, no privilege needed. This probe confirms, uid != 0:
//   (i)   the signature and the flag values (POSIX_SPAWN_JETSAM_SET | _MEMLIMIT_ACTIVE_FATAL |
//         _MEMLIMIT_INACTIVE_FATAL = 0x8000 | 0x04 | 0x08, bsd/sys/spawn_internal.h) and the priority arg;
//   (ii)  a child capped at N MiB is SIGKILLed at ~N MiB (overshoot read from the zombie's rusage peak);
//   (iii) three ways to tell the jetsam fatal-memlimit kill from an ordinary SIGKILL, before reap:
//           a) the parent's kqueue EVFILT_PROC exit event carries NOTE_EXIT_MEMORY (sys/event.h,
//              0x00020000) in its fflags when NOTE_EXIT_DETAIL is asked for -- race-free, in the event;
//           b) proc_pidinfo(PROC_PIDEXITREASONINFO) reports namespace OS_REASON_JETSAM (== 1);
//           c) the zombie's proc_pid_rusage peak (ri_lifetime_max_phys_footprint) >= the cap.
//
// Build and run under ~/.cache/hex-t7 on adas-mac-studio-3 (never /tmp); it spawns itself as the hog.
//
//   jetsam_probe            parent: for each (flags, cap) spawn a capped hog and report (i)-(iii)
//   jetsam_probe hog MIB    child: allocate and touch past MIB until the kernel kills it

#include <errno.h>
#include <libproc.h>
#include <mach-o/dyld.h>
#include <signal.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/event.h>
#include <sys/proc_info.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <unistd.h>

// posix_spawnattr_setjetsam_ext: spawn_private.h declares it (private; "All memory limits are in MiB."),
// the symbol is exported from libsystem_kernel.dylib. Flag values: bsd/sys/spawn_internal.h.
extern int posix_spawnattr_setjetsam_ext(posix_spawnattr_t *attr, short flags, int priority,
                                         int memlimit_active, int memlimit_inactive);

#define POSIX_SPAWN_JETSAM_SET 0x8000
#define POSIX_SPAWN_JETSAM_MEMLIMIT_ACTIVE_FATAL 0x04
#define POSIX_SPAWN_JETSAM_MEMLIMIT_INACTIVE_FATAL 0x08

// OS_REASON_JETSAM: bsd/sys/reason.h. PROC_PIDEXITREASONINFO: bsd/sys/proc_info.h (absent from the
// CommandLineTools SDK, so named here). NOTE_EXIT_MEMORY comes from sys/event.h.
#define OS_REASON_JETSAM 1
#ifndef PROC_PIDEXITREASONINFO
#define PROC_PIDEXITREASONINFO 20
#endif

#define MIB (1024ULL * 1024ULL)

// A sink the hog's writes escape into, so -O2 cannot elide them (the pages must really fault).
static volatile uint64_t g_sink;

static uint64_t self_footprint(uint64_t *peak_out) {
    struct rusage_info_v4 ri;
    memset(&ri, 0, sizeof(ri));
    if (proc_pid_rusage(getpid(), RUSAGE_INFO_V4, (rusage_info_t *)&ri) != 0) return 0;
    if (peak_out) *peak_out = ri.ri_lifetime_max_phys_footprint;
    return ri.ri_phys_footprint;
}

static int hog(unsigned long long cap_mib) {
    // Allocate and touch far past the cap, fast, so a fatal per-process limit kills us before this returns.
    // volatile + a global sink defeat dead-store elimination; an xorshift fill is incompressible, so the
    // compressor can't shrink the pages below the cap.
    unsigned long long limit = (cap_mib + 64) * MIB;
    unsigned long long done = 0;
    const size_t chunk = 4 * MIB;
    uint64_t x = 0x9e3779b97f4a7c15ULL ^ (uint64_t)getpid();
    uint64_t acc = 0;
    while (done < limit) {
        volatile uint64_t *p = (volatile uint64_t *)malloc(chunk);
        if (!p) {
            fprintf(stderr, "hog: malloc failed at %llu MiB\n", done / MIB);
            return 2;
        }
        for (size_t i = 0; i < chunk / sizeof(uint64_t); i++) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            p[i] = x;
            acc ^= p[i];
        }
        done += chunk;
    }
    g_sink = acc;
    uint64_t peak = 0, fp = self_footprint(&peak);
    // Reached the safety ceiling without being killed: the cap did not fire.
    printf("hog: NOT KILLED, allocated %llu MiB (cap %llu MiB) self_footprint=%.1f MiB self_peak=%.1f MiB "
           "sink=%llx\n",
           done / MIB, cap_mib, (double)fp / (double)MIB, (double)peak / (double)MIB,
           (unsigned long long)g_sink);
    return 0;
}

static int run_cap(const char *self, short flags, const char *flabel, unsigned long long cap_mib) {
    int priority = 40; // a middling jetsam band; the fatal per-process limit kills regardless of band.
    posix_spawnattr_t attr;
    int r = posix_spawnattr_init(&attr);
    if (r != 0) {
        fprintf(stderr, "posix_spawnattr_init: %s\n", strerror(r));
        return 1;
    }
    r = posix_spawnattr_setjetsam_ext(&attr, flags, priority, (int)cap_mib, (int)cap_mib);
    if (r != 0) {
        fprintf(stderr, "posix_spawnattr_setjetsam_ext: %s\n", strerror(r));
        return 1;
    }
    char mib_arg[32];
    snprintf(mib_arg, sizeof(mib_arg), "%llu", cap_mib);
    char *argv[] = {(char *)self, (char *)"hog", mib_arg, NULL};
    extern char **environ;
    pid_t pid = 0;
    r = posix_spawn(&pid, self, NULL, &attr, argv, environ);
    posix_spawnattr_destroy(&attr);
    if (r != 0) {
        fprintf(stderr, "posix_spawn: %s\n", strerror(r));
        return 1;
    }
    printf("[%s=0x%04x cap=%llu] spawned pid=%d priority=%d\n", flabel, (unsigned)(flags & 0xffff), cap_mib,
           pid, priority);

    // Watch the child for its exit WITH detail, as the node's watcher would (the node is its parent).
    int kq = kqueue();
    struct kevent ev;
    EV_SET(&ev, pid, EVFILT_PROC, EV_ADD | EV_RECEIPT, NOTE_EXIT | NOTE_EXITSTATUS | NOTE_EXIT_DETAIL, 0,
           NULL);
    struct kevent receipt;
    (void)kevent(kq, &ev, 1, &receipt, 1, NULL);
    struct kevent out;
    int n = kevent(kq, NULL, 0, &out, 1, NULL);
    if (n == 1) {
        unsigned int ff = (unsigned int)out.fflags;
        printf("[%s cap=%llu] kevent fflags=0x%08x data=0x%llx NOTE_EXIT=%d NOTE_EXIT_DETAIL=%d "
               "NOTE_EXIT_MEMORY=%d\n",
               flabel, cap_mib, ff, (unsigned long long)out.data, (ff & NOTE_EXIT) != 0,
               (ff & NOTE_EXIT_DETAIL) != 0, (ff & NOTE_EXIT_MEMORY) != 0);
    } else {
        printf("[%s cap=%llu] kevent returned %d (errno %s)\n", flabel, cap_mib, n, strerror(errno));
    }
    close(kq);

    // Before reaping: the zombie's peak footprint, and its exit reason namespace.
    struct rusage_info_v4 ri;
    memset(&ri, 0, sizeof(ri));
    if (proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)&ri) == 0) {
        unsigned long long peak = ri.ri_lifetime_max_phys_footprint;
        long long overshoot = (long long)peak - (long long)(cap_mib * MIB);
        printf("[%s cap=%llu] zombie rusage peak=%llu bytes (%.1f MiB) overshoot=%.1f MiB\n", flabel,
               cap_mib, peak, (double)peak / (double)MIB, (double)overshoot / (double)MIB);
    } else {
        printf("[%s cap=%llu] proc_pid_rusage on zombie: %s\n", flabel, cap_mib, strerror(errno));
    }
    struct proc_exitreasonbasicinfo beri;
    memset(&beri, 0, sizeof(beri));
    int got = proc_pidinfo(pid, PROC_PIDEXITREASONINFO, 0, &beri, sizeof(beri));
    if (got >= (int)sizeof(beri)) {
        printf("[%s cap=%llu] exitreason namespace=%u code=%llu jetsam=%d\n", flabel, cap_mib,
               beri.beri_namespace, (unsigned long long)beri.beri_code,
               beri.beri_namespace == OS_REASON_JETSAM);
    } else {
        printf("[%s cap=%llu] proc_pidinfo(PROC_PIDEXITREASONINFO)=%d (errno %s)\n", flabel, cap_mib, got,
               strerror(errno));
    }

    int status = 0;
    if (waitpid(pid, &status, 0) != pid) {
        printf("[%s cap=%llu] waitpid: %s\n", flabel, cap_mib, strerror(errno));
        return 1;
    }
    if (WIFSIGNALED(status)) {
        printf("[%s cap=%llu] waitpid: signaled signal=%d SIGKILL=%d\n", flabel, cap_mib, WTERMSIG(status),
               WTERMSIG(status) == SIGKILL);
    } else if (WIFEXITED(status)) {
        printf("[%s cap=%llu] waitpid: exited code=%d (NOT killed -> cap did not fire)\n", flabel, cap_mib,
               WEXITSTATUS(status));
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc >= 3 && strcmp(argv[1], "hog") == 0) {
        return hog(strtoull(argv[2], NULL, 10));
    }
    char path[4096];
    uint32_t sz = sizeof(path);
    if (_NSGetExecutablePath(path, &sz) != 0) {
        fprintf(stderr, "_NSGetExecutablePath: buffer too small\n");
        return 1;
    }
    printf("jetsam_probe uid=%d euid=%d\n", getuid(), geteuid());
    short active = (short)(POSIX_SPAWN_JETSAM_SET | POSIX_SPAWN_JETSAM_MEMLIMIT_ACTIVE_FATAL);
    short both = (short)(POSIX_SPAWN_JETSAM_SET | POSIX_SPAWN_JETSAM_MEMLIMIT_ACTIVE_FATAL |
                         POSIX_SPAWN_JETSAM_MEMLIMIT_INACTIVE_FATAL);
    int rc = 0;
    rc |= run_cap(path, active, "SET|ACTIVE", 256);
    rc |= run_cap(path, both, "SET|ACTIVE|INACTIVE", 256);
    rc |= run_cap(path, both, "SET|ACTIVE|INACTIVE", 64);
    return rc;
}
