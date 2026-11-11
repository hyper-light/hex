// TEST/MEASUREMENT ONLY. ROOT builds/runs on admitted macOS arm64; no production adapter.
// Read macos-setexec-preseal-control.md before execution. All child-trampoline storage,
// attributes/actions/argv/env are prepared before COW fork; no child heap/format/lock/destructor.
#if !defined(__APPLE__) || !defined(__aarch64__)
#error This measurement is for the supported macOS arm64 target only
#endif
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <libproc.h>
#include <limits.h>
#include <mach-o/dyld.h>
#include <poll.h>
#include <pthread.h>
#include <pthread/qos.h>
#include <pthread/spawn.h>
#include <signal.h>
#include <spawn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/event.h>
#include <sys/proc.h>
#include <sys/proc_info.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

// Same ABI/constants as hex-runtime/macos/sys.rs and pinned XNU libsyscall wrapper.
// SAFETY: initialized attr object, signed 16-bit flags, checked native int limits.
extern int posix_spawnattr_setjetsam_ext(posix_spawnattr_t *, short, int, int, int);
#define GUARD_NS UINT64_C(39507000000) // unchanged existing macOS test hang guard, not latency allowance
#define MIB UINT64_C(1048576)
#define FRAME_MAGIC UINT32_C(0x48585350) // private measurement frame, not a product protocol
#define FRAME_VERSION UINT32_C(1)

enum kind { READY = 1, PREEXEC_ERROR = 2, CANARY = 3, IMAGE_RESULT = 4, MEMORY_START = 5, UNEXPECTED_CHILD = 6 };
enum scenario { NORMAL = 0, MISSING = 1, BAD_SESSION = 2, MEMORY = 3 };
struct frame {
    uint32_t magic, version, kind, failure;
    int32_t pid, operation_errno, fork_errno, spawn_errno, raise_errno, denied_errno;
    int32_t thread_create, thread_join, qos_self;
    uint64_t birth, nproc_soft, nproc_hard;
};
_Static_assert(sizeof(struct frame) <= PIPE_BUF, "private typed frame must be one atomic pipe write");
struct prepared {
    posix_spawnattr_t attr;
    posix_spawn_file_actions_t actions;
    bool attr_ready, actions_ready;
    int ready[2], report[2], gate[2], denied, nullfd, kq;
    uint64_t deadline;
    char path[PATH_MAX], missing_path[PATH_MAX], fd_text[32], denied_text[32], deadline_text[32], memory_text[32];
    char *argv[8], *env[3];
};

// SAFETY: fixed initialized timespec owned for the call; validate the returned native representation.
static int raw_now(uint64_t *out) {
    struct timespec ts = {0};
    if (clock_gettime(CLOCK_UPTIME_RAW, &ts) != 0) return errno;
    if (ts.tv_sec < 0 || ts.tv_nsec < 0 || ts.tv_nsec >= 1000000000L) return EINVAL;
    uint64_t sec = (uint64_t)ts.tv_sec;
    if (sec > (UINT64_MAX - (uint64_t)ts.tv_nsec) / UINT64_C(1000000000)) return EOVERFLOW;
    *out = sec * UINT64_C(1000000000) + (uint64_t)ts.tv_nsec;
    return 0;
}
static int remaining(uint64_t deadline, uint64_t *left) {
    uint64_t now = 0;
    int error = raw_now(&now);
    if (error) return error;
    if (now >= deadline) return ETIMEDOUT;
    *left = deadline - now;
    return 0;
}
static int number(const char *text, uint64_t *out) {
    if (!text || !*text || *text == '-') return EINVAL;
    errno = 0;
    char *end = NULL;
    unsigned long long n = strtoull(text, &end, 10);
    if (errno || !end || *end) return EINVAL;
    *out = (uint64_t)n;
    return 0;
}
static int native_usage(pid_t pid, struct rusage_info_v4 *out) {
    memset(out, 0, sizeof(*out));
    // SAFETY: exact initialized native V4 buffer remains alive, no native handle ownership changes.
    return proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)out) == 0 ? 0 : errno;
}
static int native_stop(pid_t pid, bool *stopped) {
    struct proc_bsdinfo info = {0};
    // SAFETY: exact sizeof buffer and actual unreaped owned PID; reject partial output.
    int n = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, (int)sizeof(info));
    if (n != (int)sizeof(info)) return n <= 0 ? (errno ? errno : EIO) : EIO;
    *stopped = info.pbi_status == SSTOP;
    return 0;
}
// Used in the trusted trampoline: only bounded pipe writes, native clock and errno; no allocation.
static int send_frame(int fd, const struct frame *frame, uint64_t deadline) {
    const unsigned char *data = (const unsigned char *)frame;
    size_t at = 0;
    while (at < sizeof(*frame)) {
        uint64_t left;
        int error = remaining(deadline, &left);
        if (error) return error;
        ssize_t n = write(fd, data + at, sizeof(*frame) - at);
        if (n > 0 && (size_t)n <= sizeof(*frame) - at) at += (size_t)n;
        else if (n < 0 && errno == EINTR) continue;
        else return n < 0 ? errno : EIO;
    }
    return 0;
}
static int read_frame(int fd, struct frame *frame, uint64_t deadline) {
    unsigned char *data = (unsigned char *)frame;
    size_t at = 0;
    memset(frame, 0, sizeof(*frame));
    while (at < sizeof(*frame)) {
        uint64_t left;
        int error = remaining(deadline, &left);
        if (error) return error;
        struct pollfd pfd = { .fd = fd, .events = POLLIN };
        int ms = (int)(left / UINT64_C(1000000) + (left % UINT64_C(1000000) != 0));
        int ready = poll(&pfd, 1, ms);
        if (ready < 0 && errno == EINTR) continue;
        if (ready <= 0) return ready == 0 ? ETIMEDOUT : errno;
        if ((error = remaining(deadline, &left))) return error;
        ssize_t n = read(fd, data + at, sizeof(*frame) - at);
        if (n > 0 && (size_t)n <= sizeof(*frame) - at) at += (size_t)n;
        else if (n < 0 && (errno == EINTR || errno == EAGAIN)) continue;
        else return n == 0 ? EPIPE : errno;
    }
    if (frame->magic != FRAME_MAGIC || frame->version != FRAME_VERSION) return EPROTO;
    fprintf(stderr, "FRAME kind=%u pid=%d birth=%" PRIu64 " failure=%u errno=%d\n", frame->kind, frame->pid, frame->birth, frame->failure, frame->operation_errno);
    return 0;
}
static int private_pipe(int pair[2], bool nonblocking_reader) {
    if (pipe(pair) != 0) return errno;
    // SAFETY: only these newly created private descriptors; no later writer CLOEXEC mutation.
    for (int i = 0; i < 2; ++i) {
        if (fcntl(pair[i], F_SETFD, FD_CLOEXEC) != 0) return errno;
    }
    if (nonblocking_reader) {
        int flags = fcntl(pair[0], F_GETFL);
        if (flags < 0 || fcntl(pair[0], F_SETFL, flags | O_NONBLOCK) != 0) return errno;
    }
    return 0;
}
static void dispose(struct prepared *p) {
    if (p->actions_ready) (void)posix_spawn_file_actions_destroy(&p->actions);
    if (p->attr_ready) (void)posix_spawnattr_destroy(&p->attr);
    int fds[] = {p->ready[0], p->ready[1], p->report[0], p->report[1], p->gate[0], p->gate[1], p->denied, p->nullfd, p->kq};
    for (size_t i = 0; i < sizeof(fds)/sizeof(fds[0]); ++i) if (fds[i] >= 0) (void)close(fds[i]);
}
static int prepare(struct prepared *p, enum scenario which, int memory_mib, uint64_t deadline, const char *self) {
    memset(p, 0, sizeof(*p));
    p->ready[0] = p->ready[1] = p->report[0] = p->report[1] = p->gate[0] = p->gate[1] = -1;
    p->denied = p->nullfd = p->kq = -1;
    p->deadline = deadline;
    if (strlen(self) >= sizeof(p->path)) return ENAMETOOLONG;
    memcpy(p->path, self, strlen(self) + 1);
    int path_count = snprintf(p->missing_path, sizeof(p->missing_path), "%s.no-such-owned-image", self);
    if (path_count < 0 || (size_t)path_count >= sizeof(p->missing_path)) return ENAMETOOLONG;
    if (which == MISSING) {
        struct stat absent;
        if (lstat(p->missing_path, &absent) == 0 || errno != ENOENT) return EEXIST;
    }
    int error = private_pipe(p->ready, true);
    if (error) return error;
    if ((error = private_pipe(p->report, true)) || (error = private_pipe(p->gate, false))) return error;
    p->denied = open("/dev/null", O_RDONLY | O_CLOEXEC);
    p->nullfd = open("/dev/null", O_RDWR | O_CLOEXEC);
    p->kq = kqueue();
    if (p->denied < 0 || p->nullfd < 0 || p->kq < 0) return errno;
    if (fcntl(p->kq, F_SETFD, FD_CLOEXEC) != 0) return errno;
    // Passive child-completion notification, same existing native primitive: no handler/mask change.
    struct kevent signal_event;
    EV_SET(&signal_event, SIGCHLD, EVFILT_SIGNAL, EV_ADD | EV_ENABLE | EV_CLEAR, 0, 0, NULL);
    if (kevent(p->kq, &signal_event, 1, NULL, 0, NULL) < 0) return errno;
    if ((error = posix_spawnattr_init(&p->attr))) return error;
    p->attr_ready = true;
    short flags = POSIX_SPAWN_SETEXEC | POSIX_SPAWN_START_SUSPENDED | POSIX_SPAWN_SETPGROUP |
        POSIX_SPAWN_SETSIGMASK | POSIX_SPAWN_SETSIGDEF | POSIX_SPAWN_CLOEXEC_DEFAULT;
    if (which == BAD_SESSION) flags |= POSIX_SPAWN_SETSID;
    sigset_t empty, all;
    if (sigemptyset(&empty) || sigfillset(&all)) return errno;
    if ((error = posix_spawnattr_setflags(&p->attr, flags)) ||
        (error = posix_spawnattr_setpgroup(&p->attr, 0)) ||
        (error = posix_spawnattr_setsigmask(&p->attr, &empty)) ||
        (error = posix_spawnattr_setsigdefault(&p->attr, &all)) ||
        (error = posix_spawnattr_set_qos_class_np(&p->attr, QOS_CLASS_BACKGROUND))) return error;
    uint16_t bits = UINT16_C(0x800c); // SAME Hex SET + active/inactive fatal, same priority40
    short jetsam_flags;
    memcpy(&jetsam_flags, &bits, sizeof(jetsam_flags));
    if ((error = posix_spawnattr_setjetsam_ext(&p->attr, jetsam_flags, 40, memory_mib, memory_mib))) return error;
    if ((error = posix_spawn_file_actions_init(&p->actions))) return error;
    p->actions_ready = true;
    for (int fd = 0; fd < 3; ++fd) if ((error = posix_spawn_file_actions_adddup2(&p->actions, p->nullfd, fd))) return error;
    // Final native child mapping: the actual writer is this explicit inherited Given FD.
    // SAFETY: action affects only child inheritance; parent writer remains CLOEXEC throughout.
    if ((error = posix_spawn_file_actions_addinherit_np(&p->actions, p->report[1]))) return error;
    if (snprintf(p->fd_text, sizeof(p->fd_text), "%d", p->report[1]) < 0 ||
        snprintf(p->denied_text, sizeof(p->denied_text), "%d", p->denied) < 0 ||
        snprintf(p->deadline_text, sizeof(p->deadline_text), "%" PRIu64, deadline) < 0 ||
        snprintf(p->memory_text, sizeof(p->memory_text), "%d", memory_mib) < 0) return EIO;
    p->argv[0] = p->path;
    p->argv[1] = which == MEMORY ? "--image-memory" : "--image";
    p->argv[2] = p->fd_text; p->argv[3] = p->denied_text; p->argv[4] = p->deadline_text;
    p->argv[5] = p->memory_text; p->argv[6] = NULL;
    p->env[0] = "PATH=/usr/bin:/bin"; p->env[1] = "LANG=C"; p->env[2] = NULL;
    return 0;
}

// EXACT trusted child trampoline. SAFETY: all p storage/actions/strings are COW copies prepared
// before fork. Only own private FD closes, setrlimit/setsid/getpid/read/write/clock and the prepared
// public posix_spawn wrapper execute here; no allocator/format/locks/destructors. No UID/filter change.
static void trampoline(const struct prepared *p, enum scenario which) {
    (void)close(p->ready[0]); (void)close(p->report[0]); (void)close(p->gate[1]); (void)close(p->kq);
    struct frame f = { .magic = FRAME_MAGIC, .version = FRAME_VERSION, .kind = READY, .pid = getpid() };
    struct rlimit zero = { .rlim_cur = 0, .rlim_max = 0 };
    if (setrlimit(RLIMIT_NPROC, &zero) != 0 || (which == BAD_SESSION && setsid() < 0)) {
        f.kind = PREEXEC_ERROR; f.operation_errno = errno;
        (void)send_frame(p->ready[1], &f, p->deadline); _exit(126);
    }
    if (send_frame(p->ready[1], &f, p->deadline) != 0) _exit(126);
    unsigned char permit = 0;
    ssize_t n;
    do {
        uint64_t left;
        if (remaining(p->deadline, &left)) _exit(126);
        n = read(p->gate[0], &permit, 1);
    } while (n < 0 && errno == EINTR);
    if (n != 1 || permit != 'R') _exit(126);
    (void)close(p->gate[0]);
    const char *path = which == MISSING ? p->missing_path : p->path;
    int error = posix_spawn(NULL, path, &p->actions, &p->attr, p->argv, p->env);
    f.kind = PREEXEC_ERROR; f.operation_errno = error;
    (void)send_frame(p->ready[1], &f, p->deadline);
    _exit(127); // success cannot return to this image; no atexit/Rust/native-owner destructor
}

// Parent-only native event wait, one fixed event and the original absolute budget.
static int event(int kq, uint64_t deadline, struct kevent *out) {
    for (;;) {
        uint64_t left;
        int error = remaining(deadline, &left);
        if (error) return error;
        struct timespec wait = { .tv_sec = (time_t)(left / UINT64_C(1000000000)), .tv_nsec = (long)(left % UINT64_C(1000000000)) };
        int n = kevent(kq, NULL, 0, out, 1, &wait);
        if (n < 0 && errno == EINTR) continue;
        if (n != 1) return n == 0 ? ETIMEDOUT : errno;
        if (out->flags & EV_ERROR) return out->data ? (int)out->data : EIO;
        if ((error = remaining(deadline, &left))) return error;
        return 0;
    }
}
// SAME parent is the sole terminal waiter. Peek/reap are distinct, never signal after reaping.
static int terminal(pid_t pid, int kq, uint64_t deadline, int *status, uint64_t *peak, uint32_t *exit_flags) {
    for (;;) {
        siginfo_t info = {0};
        if (waitid(P_PID, (id_t)pid, &info, WEXITED | WNOHANG | WNOWAIT) != 0) return errno;
        if (info.si_pid == pid) {
            // Drain the two registered filter kinds after the actual waitable fact. No polling.
            struct kevent queued[2];
            struct timespec zero_wait = {0};
            int count = kevent(kq, NULL, 0, queued, 2, &zero_wait);
            if (count < 0 && errno != EINTR) return errno;
            for (int at = 0; at < count; ++at) {
                if (queued[at].filter == EVFILT_PROC && queued[at].ident == (uintptr_t)pid)
                    *exit_flags |= queued[at].fflags;
            }
            struct rusage_info_v4 usage;
            int error = native_usage(pid, &usage);
            if (error || !usage.ri_proc_exit_abstime) return error ? error : EIO;
            *peak = usage.ri_lifetime_max_phys_footprint;
            pid_t got;
            do {
                got = waitpid(pid, status, WNOHANG);
                if (got < 0 && errno == EINTR) {
                    uint64_t left;
                    if ((error = remaining(deadline, &left))) return error;
                }
            } while (got < 0 && errno == EINTR);
            return got == pid ? 0 : (got < 0 ? errno : ECHILD);
        }
        struct kevent ended;
        int error = event(kq, deadline, &ended);
        if (error) return error;
        if (ended.filter == EVFILT_PROC && ended.ident == (uintptr_t)pid) *exit_flags |= ended.fflags;
    }
}
static int cleanup(pid_t pid, uint64_t birth, int kq, uint64_t deadline, bool *reaped) {
    siginfo_t info = {0};
    if (waitid(P_PID, (id_t)pid, &info, WEXITED | WNOHANG | WNOWAIT) != 0) return errno;
    if (info.si_pid != pid) {
        struct rusage_info_v4 usage;
        int error = native_usage(pid, &usage);
        if (error || !usage.ri_proc_start_abstime || (birth && usage.ri_proc_start_abstime != birth)) return error ? error : ESTALE;
        birth = usage.ri_proc_start_abstime;
        struct kevent change;
        EV_SET(&change, (uintptr_t)pid, EVFILT_PROC, EV_ADD | EV_ENABLE | EV_CLEAR, NOTE_EXIT | NOTE_EXITSTATUS, 0, NULL);
        if (kevent(kq, &change, 1, NULL, 0, NULL) < 0 && errno != ESRCH) return errno;
        // SAFETY: still this parent's exact unreaped direct child, joined to native birth.
        if (kill(pid, SIGKILL) != 0 && errno != ESRCH) return errno;
    }
    int status = 0; uint64_t peak = 0; uint32_t exit_flags = 0;
    int error = terminal(pid, kq, deadline, &status, &peak, &exit_flags);
    if (!error) *reaped = true;
    fprintf(stderr, "CLEANUP pid=%d birth=%" PRIu64 " error=%d status=%d peak=%" PRIu64 " reaped=%d\n", pid, birth, error, status, peak, *reaped);
    return error;
}
static void *thread_body(void *arg) { return arg; }
static int unexpected_child(pid_t pid, int kq, int report, uint64_t deadline) {
    if (pid <= 0) return EINVAL;
    struct rusage_info_v4 usage;
    int error = native_usage(pid, &usage);
    struct frame observed = { .magic = FRAME_MAGIC, .version = FRAME_VERSION,
        .kind = UNEXPECTED_CHILD, .pid = pid, .operation_errno = error,
        .birth = error ? 0 : usage.ri_proc_start_abstime };
    // Publish the exact returned child ID/birth before any assertion or cleanup can fail.
    int published = send_frame(report, &observed, deadline);
    // The image's preprepared private kqueue already watches SIGCHLD before this effect.
    bool reaped = false;
    int cleaned = cleanup(pid, error ? 0 : usage.ri_proc_start_abstime, kq, deadline, &reaped);
    return cleaned || !reaped || published ? (cleaned ? cleaned : (published ? published : ECHILD)) : 0;
}
static int image(int argc, char **argv, bool memory) {
    uint64_t fd, denied, deadline, memory_mib;
    if (argc != 6 || number(argv[2], &fd) || number(argv[3], &denied) || number(argv[4], &deadline) || number(argv[5], &memory_mib) ||
        fd > INT_MAX || denied > INT_MAX || memory_mib == 0 || memory_mib > INT_MAX) return 31;
    struct frame f = { .magic = FRAME_MAGIC, .version = FRAME_VERSION, .kind = CANARY, .pid = getpid() };
    struct rusage_info_v4 usage;
    if (native_usage(getpid(), &usage)) return 32;
    f.birth = usage.ri_proc_start_abstime;
    if (send_frame((int)fd, &f, deadline)) return 33;
    struct rlimit limit;
    if (getrlimit(RLIMIT_NPROC, &limit) != 0) return 34;
    f.nproc_soft = limit.rlim_cur; f.nproc_hard = limit.rlim_max;
    f.qos_self = (int32_t)qos_class_self();
    errno = 0;
    if (fcntl((int)denied, F_GETFD) >= 0) f.failure = 1;
    f.denied_errno = errno;
    if (f.denied_errno != EBADF || limit.rlim_cur != 0 || limit.rlim_max != 0 || f.qos_self != (int32_t)QOS_CLASS_BACKGROUND) f.failure = 1;
    if (memory) {
        f.kind = MEMORY_START;
        if (send_frame((int)fd, &f, deadline)) return 35;
        if (memory_mib > UINT64_MAX / MIB - 1) return 36;
        size_t bytes = (size_t)((memory_mib + 1) * MIB);
        volatile uint64_t *data = malloc(bytes);
        if (!data) return 37;
        uint64_t value = UINT64_C(0x9e3779b97f4a7c15);
        for (size_t at = 0; at < bytes / sizeof(*data); ++at) {
            if (at % (MIB / sizeof(*data)) == 0) {
                uint64_t left;
                if (remaining(deadline, &left)) { free((void *)data); return 43; }
            }
            value ^= value << 13; value ^= value >> 7; value ^= value << 17;
            data[at] = value;
        }
        free((void *)data);
        return 38; // reached cap+one native MiB without the requested fatal memory kill
    }
    int observer = kqueue();
    if (observer < 0) return 39; // prepare the sole unexpected-child observer before any attempt
    struct kevent signal_event;
    EV_SET(&signal_event, SIGCHLD, EVFILT_SIGNAL, EV_ADD | EV_ENABLE | EV_CLEAR, 0, 0, NULL);
    if (kevent(observer, &signal_event, 1, NULL, 0, NULL) < 0) { (void)close(observer); return 39; }
    errno = 0;
    pid_t attempted = fork();
    f.fork_errno = attempted < 0 ? errno : 0;
    if (attempted == 0) _exit(0);
    if (attempted > 0) { f.failure = 1; if (unexpected_child(attempted, observer, (int)fd, deadline)) return 39; }
    char *noop[] = {argv[0], "--noop", NULL};
    char *env[] = {"PATH=/usr/bin:/bin", "LANG=C", NULL};
    pid_t spawned = -1;
    f.spawn_errno = posix_spawn(&spawned, argv[0], NULL, NULL, noop, env);
    if (f.spawn_errno == 0) { f.failure = 1; if (unexpected_child(spawned, observer, (int)fd, deadline)) return 40; }
    struct rlimit raised = { .rlim_cur = 1, .rlim_max = 1 };
    errno = 0;
    f.raise_errno = setrlimit(RLIMIT_NPROC, &raised) == 0 ? 0 : errno;
    pthread_t thread;
    f.thread_create = pthread_create(&thread, NULL, thread_body, &f);
    void *returned = NULL;
    f.thread_join = f.thread_create ? f.thread_create : pthread_join(thread, &returned);
    if (f.fork_errno != EAGAIN || f.spawn_errno != EAGAIN || f.raise_errno != EPERM ||
        f.thread_create || f.thread_join || returned != &f) f.failure = 1;
    (void)close(observer);
    f.kind = IMAGE_RESULT;
    if (send_frame((int)fd, &f, deadline)) return 41;
    return f.failure ? 42 : 0;
}
static int run_case(enum scenario which, int memory_mib, uint64_t deadline, const char *self) {
    struct prepared p;
    int error = prepare(&p, which, memory_mib, deadline, self);
    if (error) { fprintf(stderr, "SETUP case=%d error=%d\n", which, error); dispose(&p); return 1; }
    int before_flags = fcntl(p.report[1], F_GETFD);
    pid_t pid = fork();
    if (pid == 0) trampoline(&p, which);
    if (pid < 0) { error = errno; dispose(&p); return error; }
    bool reaped = false, relevant = false;
    uint64_t birth = 0;
    fprintf(stderr, "OWNED case=%d child_id=%d\n", which, pid);
    (void)close(p.ready[1]); p.ready[1] = -1;
    // Keep original report writer for the explicit parent-flag check, then close before reading EOF.
    (void)close(p.gate[0]); p.gate[0] = -1;
    struct rusage_info_v4 before;
    error = native_usage(pid, &before);
    fprintf(stderr, "BIRTH case=%d pid=%d query=%d start=%" PRIu64 " ended=%" PRIu64 "\n", which, pid, error, before.ri_proc_start_abstime, before.ri_proc_exit_abstime);
    if (error || !before.ri_proc_start_abstime || before.ri_proc_exit_abstime) {
        if (!error) error = ESTALE;
        goto done;
    }
    birth = before.ri_proc_start_abstime;
    struct kevent change;
    EV_SET(&change, (uintptr_t)pid, EVFILT_PROC, EV_ADD | EV_ENABLE | EV_CLEAR, NOTE_EXEC | NOTE_EXIT | NOTE_EXITSTATUS | NOTE_EXIT_DETAIL, 0, NULL);
    if (kevent(p.kq, &change, 1, NULL, 0, NULL) < 0) { error = errno; goto done; }
    struct frame ready;
    if ((error = read_frame(p.ready[0], &ready, deadline))) goto done;
    if (ready.kind != READY || ready.pid != pid) { error = EPROTO; goto done; }
    if (write(p.gate[1], "R", 1) != 1) { error = errno ? errno : EIO; goto done; }
    (void)close(p.gate[1]); p.gate[1] = -1;
    struct kevent observed;
    do {
        if ((error = event(p.kq, deadline, &observed))) goto done;
    } while (observed.filter != EVFILT_PROC || observed.ident != (uintptr_t)pid);
    fprintf(stderr, "EVENT case=%d pid=%d fflags=%u data=%" PRIdPTR "\n", which, pid, observed.fflags, observed.data);
    if (which == NORMAL || which == MEMORY) {
        if (!(observed.fflags & NOTE_EXEC)) { error = EPROTO; goto done; }
        struct rusage_info_v4 after;
        bool stopped = false;
        if ((error = native_usage(pid, &after)) || (error = native_stop(pid, &stopped))) goto done;
        unsigned char byte;
        errno = 0;
        ssize_t canary = read(p.report[0], &byte, 1);
        int canary_errno = errno;
        int after_flags = fcntl(p.report[1], F_GETFD);
        fprintf(stderr, "HELD case=%d pid=%d start=%" PRIu64 " stopped=%d canary=%zd errno=%d parent_flags=%d/%d\n", which, pid, after.ri_proc_start_abstime, stopped, canary, canary_errno, before_flags, after_flags);
        if (after.ri_proc_start_abstime != birth || after.ri_proc_exit_abstime || !stopped || getpgid(pid) != pid ||
            canary != -1 || canary_errno != EAGAIN || before_flags != after_flags || !(after_flags & FD_CLOEXEC)) { error = EPROTO; goto done; }
        (void)close(p.report[1]); p.report[1] = -1;
        uint64_t resume_budget;
        if ((error = remaining(deadline, &resume_budget))) goto done;
        // SAFETY: positive exec+stop+same native birth, exact still-unreaped direct child only.
        if (kill(pid, SIGCONT) != 0) { error = errno; goto done; }
        struct frame first, result;
        if ((error = read_frame(p.report[0], &first, deadline)) || (error = read_frame(p.report[0], &result, deadline))) goto done;
        // Exactly two forbidden creation attempts exist in this fixed image. Their receipts are
        // diagnostic failure facts, never scope-release authority or a generic actor inventory.
        for (int attempt = 0; attempt < 2 && result.kind == UNEXPECTED_CHILD; ++attempt) {
            if ((error = read_frame(p.report[0], &result, deadline))) goto done;
        }
        if (first.kind != CANARY || first.pid != pid || first.birth != birth || result.pid != pid || result.birth != birth || result.failure) { error = EPROTO; goto done; }
        if ((which == NORMAL && result.kind != IMAGE_RESULT) || (which == MEMORY && result.kind != MEMORY_START)) { error = EPROTO; goto done; }
        fprintf(stderr, "IMAGE case=%d samepid=%d birth=%" PRIu64 " kind=%u nproc=%" PRIu64 "/%" PRIu64 " fork=%d spawn=%d raise=%d thread=%d/%d deny=%d qos=%d\n", which, result.pid, result.birth, result.kind, result.nproc_soft, result.nproc_hard, result.fork_errno, result.spawn_errno, result.raise_errno, result.thread_create, result.thread_join, result.denied_errno, result.qos_self);
    } else if (which == MISSING) {
        struct frame failure;
        if ((error = read_frame(p.ready[0], &failure, deadline))) goto done;
        fprintf(stderr, "NEGATIVE native_errno=%d kind=%u\n", failure.operation_errno, failure.kind);
        if (failure.kind != PREEXEC_ERROR || failure.operation_errno != ENOENT) { error = EPROTO; goto done; }
    }
    int status = 0; uint64_t peak = 0; uint32_t exit_flags = observed.fflags;
    if ((error = terminal(pid, p.kq, deadline, &status, &peak, &exit_flags))) goto done;
    reaped = true;
    relevant = which == NORMAL ? WIFEXITED(status) && WEXITSTATUS(status) == 0 :
        which == MISSING ? WIFEXITED(status) && WEXITSTATUS(status) == 127 :
        WIFSIGNALED(status) && WTERMSIG(status) == SIGKILL;
    if (which == MEMORY && (peak < (uint64_t)memory_mib * MIB || !(exit_flags & NOTE_EXIT_MEMORY))) relevant = false;
    fprintf(stderr, "EXIT_FLAGS case=%d fflags=%u memory_detail=%d\n", which, exit_flags, (exit_flags & NOTE_EXIT_MEMORY) != 0);
    fprintf(stderr, "TERMINAL case=%d pid=%d status=%d peak=%" PRIu64 " memory_mib=%d relevant=%d reaped=1\n", which, pid, status, peak, memory_mib, relevant);
done:
    if (!reaped) {
        int cleaned = cleanup(pid, birth, p.kq, deadline, &reaped);
        fprintf(stderr, "FAILED case=%d original_error=%d cleanup_error=%d reaped=%d\n", which, error, cleaned, reaped);
    }
    dispose(&p);
    fprintf(stderr, "RESULT case=%d error=%d relevant=%d main_cleanup=%s\n", which, error, relevant, reaped ? "REAPED" : "UNKNOWN");
    return error || !relevant || !reaped;
}
int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "--noop") == 0) return 0;
    if (argc > 1 && (strcmp(argv[1], "--image") == 0 || strcmp(argv[1], "--image-memory") == 0)) return image(argc, argv, strcmp(argv[1], "--image-memory") == 0);
    uint64_t memory_mib;
    if (argc != 3 || strcmp(argv[1], "--memory-mib") || number(argv[2], &memory_mib) || memory_mib == 0 || memory_mib > INT_MAX) {
        fprintf(stderr, "usage: ROOT-admitted-control --memory-mib EXPLICIT_NATIVE_BOUND\n"); return 2;
    }
    if (getuid() == 0 || geteuid() == 0 || getuid() != geteuid()) { fprintf(stderr, "REFUSE root/credential transition\n"); return 2; }
    struct sigaction child_action;
    if (sigaction(SIGCHLD, NULL, &child_action) || child_action.sa_handler != SIG_DFL || (child_action.sa_flags & SA_NOCLDWAIT)) { fprintf(stderr, "REFUSE nonretained child ownership\n"); return 2; }
    struct rlimit parent_before = {0}, parent_after = {0};
    if (getrlimit(RLIMIT_NPROC, &parent_before)) return 2;
    char executable[PATH_MAX], self[PATH_MAX]; uint32_t length = sizeof(executable);
    if (_NSGetExecutablePath(executable, &length) || !realpath(executable, self)) return 2;
    struct stat self_stat;
    if (stat(self, &self_stat) || (self_stat.st_mode & (S_ISUID | S_ISGID))) return 2;
    uint64_t start = 0;
    if (raw_now(&start) || start > UINT64_MAX - GUARD_NS) return 2;
    uint64_t deadline = start + GUARD_NS;
    int failed = 0;
    for (int which = NORMAL; which <= MEMORY; ++which) {
        failed |= run_case((enum scenario)which, (int)memory_mib, deadline, self);
        if (failed) break; // preserve failed setup/cleanup; never retry or extend the guard
    }
    if (getrlimit(RLIMIT_NPROC, &parent_after) || parent_after.rlim_cur != parent_before.rlim_cur || parent_after.rlim_max != parent_before.rlim_max) failed = 1;
    fprintf(stderr, "PARENT limits=%" PRIu64 "/%" PRIu64 " after=%" PRIu64 "/%" PRIu64 " failed=%d\n", (uint64_t)parent_before.rlim_cur, (uint64_t)parent_before.rlim_max, (uint64_t)parent_after.rlim_cur, (uint64_t)parent_after.rlim_max, failed);
    return failed ? 1 : 0;
}
