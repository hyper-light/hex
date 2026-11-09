// What bounds a macOS process's memory in the kernel, rootless — the evidence behind the native plan's
// split: step 1's rootless box bounds memory by a polling rule (an eviction by a stated rule, CLAUDE.md
// §2; box_probe.c measures it), because macOS gives an unprivileged process no kernel-enforced or
// event-driven per-task memory cap; the real cap (a per-task physical-footprint ledger limit that fires a
// Mach EXC_RESOURCE, event-driven and near-overshoot-free) needs privilege, so it is step 2's, through the
// installer's helper. Each check prints its rootless result.
//
//   mem_bound_probe ledger   task_set_phys_footprint_limit on self + EXC_MASK_RESOURCE: does the kernel arm
//                            and fire it rootless? (measured KERN_NO_ACCESS on adas-mac-studio-3, 2026-11-01)
//   mem_bound_probe jetsam   memorystatus_control(SET_MEMLIMIT_PROPERTIES, fatal) on self and a child
//                            (measured EPERM)
//   mem_bound_probe rlimit   setrlimit(RLIMIT_AS) and RLIMIT_RSS, then allocate past them
//                            (measured EINVAL to set, not enforced: allocated 515 MB past a 192 MB limit)
//   (no argument runs all three)
//
// Build: clang -O2 -Wall -Wno-deprecated-declarations -o mem_bound_probe mem_bound_probe.c
#include <errno.h>
#include <libproc.h>
#include <mach/mach.h>
#include <mach/task.h>
#include <mach/exception_types.h>
#include <pthread.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

// mach/task.h declares this routine; it sets the task's physical-footprint ledger limit, in MB.
extern kern_return_t task_set_phys_footprint_limit(task_t, int, int *);

// From bsd/sys/kern_memorystatus.h (private); the numbers are the stable ABI launchd/iOS use.
#define MEMORYSTATUS_CMD_SET_MEMLIMIT_PROPERTIES 8
#define MEMORYSTATUS_MEMLIMIT_ATTR_FATAL 1
typedef struct {
    int32_t active;
    uint32_t active_attr;
    int32_t inactive;
    uint32_t inactive_attr;
} memlimit_properties_t;

static uint64_t footprint(pid_t pid) {
    struct rusage_info_v4 ri;
    if (proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)&ri) != 0) return 0;
    return ri.ri_phys_footprint;
}

static mach_port_t exc_port;
static volatile int got_exception;

static void *catcher(void *arg) {
    (void)arg;
    struct {
        mach_msg_header_t head;
        char body[512];
    } msg;
    for (;;) {
        if (mach_msg(&msg.head, MACH_RCV_MSG, 0, sizeof msg, exc_port, MACH_MSG_TIMEOUT_NONE,
                     MACH_PORT_NULL) != KERN_SUCCESS)
            return NULL;
        got_exception++;
    }
}

static int probe_ledger(void) {
    int old = -1;
    kern_return_t kr = task_set_phys_footprint_limit(mach_task_self(), 128, &old);
    printf("ledger: task_set_phys_footprint_limit(128MB) kr=%d (0=SUCCESS, 8=KERN_NO_ACCESS) old=%d\n", kr, old);
    if (kr != KERN_SUCCESS) {
        printf("ledger: not armable rootless -> step 2 (privileged helper)\n");
        return 1;
    }
    mach_port_allocate(mach_task_self(), MACH_PORT_RIGHT_RECEIVE, &exc_port);
    mach_port_insert_right(mach_task_self(), exc_port, exc_port, MACH_MSG_TYPE_MAKE_SEND);
    kr = task_set_exception_ports(mach_task_self(), EXC_MASK_RESOURCE, exc_port, EXCEPTION_DEFAULT, 0);
    printf("ledger: task_set_exception_ports(EXC_MASK_RESOURCE) kr=%d\n", kr);
    pthread_t th;
    pthread_create(&th, NULL, catcher, NULL);
    char *volatile keep = NULL;
    for (int mb = 0; mb < 512; mb++) {
        char *b = malloc(1 << 20);
        if (!b) break;
        for (int j = 0; j < (1 << 20); j += 4096) b[j] = 1;
        keep = b;
        struct timespec ts = {0, 2000000};
        nanosleep(&ts, NULL);
    }
    (void)keep;
    printf("ledger: after 512 MB, exceptions=%d footprint=%llu MB\n", got_exception, footprint(getpid()) >> 20);
    return 0;
}

static int set_memlimit(pid_t pid, int mb) {
    memlimit_properties_t p = {mb, MEMORYSTATUS_MEMLIMIT_ATTR_FATAL, mb, MEMORYSTATUS_MEMLIMIT_ATTR_FATAL};
    return (int)syscall(SYS_memorystatus_control, MEMORYSTATUS_CMD_SET_MEMLIMIT_PROPERTIES, pid, 0, &p, sizeof p);
}

static int probe_jetsam(void) {
    int r = set_memlimit(getpid(), 128);
    printf("jetsam: memorystatus_control SET_MEMLIMIT(self,128MB) ret=%d errno=%d (%s)\n", r, r ? errno : 0,
           r ? strerror(errno) : "ok");
    fflush(stdout); // so the child doesn't inherit and re-print our buffered output
    pid_t c = fork();
    if (c == 0) {
        usleep(100000);
        char *volatile keep = NULL;
        for (int mb = 0; mb < 512; mb++) {
            char *b = malloc(1 << 20);
            if (!b) _exit(7);
            for (int j = 0; j < (1 << 20); j += 4096) b[j] = 1;
            keep = b;
        }
        (void)keep;
        _exit(0);
    }
    int rc = set_memlimit(c, 128);
    printf("jetsam: SET_MEMLIMIT(child,128MB) ret=%d errno=%d (%s)\n", rc, rc ? errno : 0, rc ? strerror(errno) : "ok");
    int st;
    waitpid(c, &st, 0);
    printf("jetsam: child %s\n", WIFSIGNALED(st) ? "killed by the kernel (a cap)" : "exited uncapped");
    return 0;
}

static int rlimit_child(int which, const char *name, rlim_t mb) {
    struct rlimit rl = {mb << 20, mb << 20};
    int sr = setrlimit(which, &rl);
    printf("rlimit: setrlimit(%s,%llu MB) ret=%d errno=%d\n", name, (unsigned long long)mb, sr, sr ? errno : 0);
    char *volatile keep = NULL;
    for (int m = 0; m < 512; m++) {
        char *b = malloc(1 << 20);
        if (!b) {
            printf("rlimit: %s refused at %d MB (enforced)\n", name, m);
            fflush(stdout); // the caller _exit()s this child, which would skip the flush
            return 0;
        }
        for (int j = 0; j < (1 << 20); j += 4096) b[j] = 1;
        keep = b;
    }
    (void)keep;
    printf("rlimit: %s reached 512 MB, footprint=%llu MB (NOT enforced)\n", name, footprint(getpid()) >> 20);
    fflush(stdout);
    return 1;
}

static int probe_rlimit(void) {
    fflush(stdout); // so each child doesn't inherit and re-print our buffered output
    pid_t c = fork();
    if (c == 0) _exit(rlimit_child(RLIMIT_AS, "RLIMIT_AS", 192));
    int st;
    waitpid(c, &st, 0);
#ifdef RLIMIT_RSS
    fflush(stdout);
    c = fork();
    if (c == 0) _exit(rlimit_child(RLIMIT_RSS, "RLIMIT_RSS", 128));
    waitpid(c, &st, 0);
#endif
    return 0;
}

int main(int argc, char **argv) {
    printf("uid=%d\n", getuid());
    const char *what = argc >= 2 ? argv[1] : "all";
    int all = strcmp(what, "all") == 0;
    if (all || strcmp(what, "ledger") == 0) probe_ledger();
    if (all || strcmp(what, "jetsam") == 0) probe_jetsam();
    if (all || strcmp(what, "rlimit") == 0) probe_rlimit();
    return 0;
}
