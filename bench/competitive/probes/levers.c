// Goal 5's added levers, measured: clone3's cost by namespace flag (the parent's call, then the child's
// exit and reap), a child's setns into an ipc and a uts namespace made ahead, and a record write (temp
// file, write, close, rename) by filesystem.
//   levers clone <n> [cgroup dir]     levers setns <n>     levers write <n> <dir> [bytes]
#define _GNU_SOURCE
#include <fcntl.h>
#include <linux/sched.h>
#include <sched.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static long now_ns(void) { struct timespec t; clock_gettime(CLOCK_MONOTONIC, &t); return t.tv_sec * 1000000000L + t.tv_nsec; }
static int cmp(const void *a, const void *b) { long x = *(const long *)a, y = *(const long *)b; return (x > y) - (x < y); }
static void report(const char *what, long *v, int n) {
    qsort(v, n, sizeof(long), cmp);
    printf("%-22s n=%d p50 %8.1f us  p90 %8.1f  p99 %8.1f\n", what, n, v[n / 2] / 1e3, v[n * 9 / 10] / 1e3, v[n * 99 / 100] / 1e3);
}

static int do_clone(int n, const char *cg) {
    int cgfd = -1;
    if (cg && (cgfd = open(cg, O_RDONLY | O_DIRECTORY | O_CLOEXEC)) < 0) { perror("open cgroup"); return 1; }
    struct { const char *name; unsigned long long flags; int intocg; } sets[] = {
        {"base", 0, 0}, {"newns", CLONE_NEWNS, 0}, {"newpid", CLONE_NEWPID, 0}, {"newipc", CLONE_NEWIPC, 0},
        {"newuts", CLONE_NEWUTS, 0}, {"ns+pid+ipc+uts", CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWIPC | CLONE_NEWUTS, 0},
        {"intocgroup", 0, 1}, {"all+intocgroup", CLONE_NEWNS | CLONE_NEWPID | CLONE_NEWIPC | CLONE_NEWUTS, 1}};
    long *c = malloc(sizeof(long) * n), *r = malloc(sizeof(long) * n);
    for (unsigned s = 0; s < sizeof sets / sizeof sets[0]; s++) {
        if (sets[s].intocg && cgfd < 0) continue;
        for (int i = 0; i < n; i++) {
            int pidfd = -1;
            struct clone_args ca;
            memset(&ca, 0, sizeof ca);
            ca.flags = CLONE_PIDFD | sets[s].flags | (sets[s].intocg ? CLONE_INTO_CGROUP : 0);
            ca.pidfd = (unsigned long long)(unsigned long)&pidfd;
            ca.exit_signal = SIGCHLD;
            if (sets[s].intocg) ca.cgroup = (unsigned long long)cgfd;
            long t0 = now_ns();
            long pid = syscall(SYS_clone3, &ca, sizeof ca);
            if (pid == 0) _exit(0);
            long t1 = now_ns();
            if (pid < 0) { perror(sets[s].name); return 1; }
            waitpid((pid_t)pid, NULL, 0);
            long t2 = now_ns();
            close(pidfd);
            c[i] = t1 - t0;
            r[i] = t2 - t1;
        }
        char name[64];
        snprintf(name, sizeof name, "clone %s", sets[s].name); report(name, c, n);
        snprintf(name, sizeof name, " exit+reap %s", sets[s].name); report(name, r, n);
    }
    return 0;
}

// A child that unshares an ipc and a uts namespace and waits holds them; each timed child joins them by
// setns, as an init would join namespaces made ahead, timing its own two calls into a shared slot.
static int do_setns(int n) {
    int ready[2];
    if (pipe(ready) != 0) { perror("pipe"); return 1; }
    pid_t holder = fork();
    if (holder == 0) {
        if (unshare(CLONE_NEWIPC | CLONE_NEWUTS) != 0) _exit(1);
        if (write(ready[1], "r", 1) != 1) _exit(1);
        pause();
        _exit(0);
    }
    char r;
    if (read(ready[0], &r, 1) != 1) { fprintf(stderr, "the holder failed\n"); return 1; }
    char path[64];
    snprintf(path, sizeof path, "/proc/%d/ns/ipc", holder);
    int ipc = open(path, O_RDONLY | O_CLOEXEC);
    snprintf(path, sizeof path, "/proc/%d/ns/uts", holder);
    int uts = open(path, O_RDONLY | O_CLOEXEC);
    if (ipc < 0 || uts < 0) { perror("open the made-ahead namespaces"); return 1; }
    long *slot = mmap(NULL, sizeof(long) * 2, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, -1, 0);
    long *a = malloc(sizeof(long) * n), *b = malloc(sizeof(long) * n);
    for (int i = 0; i < n; i++) {
        pid_t pid = fork();
        if (pid == 0) {
            long t0 = now_ns();
            if (setns(ipc, CLONE_NEWIPC) != 0) _exit(1);
            long t1 = now_ns();
            if (setns(uts, CLONE_NEWUTS) != 0) _exit(1);
            slot[0] = t1 - t0;
            slot[1] = now_ns() - t1;
            _exit(0);
        }
        int status;
        waitpid(pid, &status, 0);
        if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) { fprintf(stderr, "setns failed\n"); return 1; }
        a[i] = slot[0];
        b[i] = slot[1];
    }
    kill(holder, SIGKILL);
    waitpid(holder, NULL, 0);
    report("setns ipc", a, n);
    report("setns uts", b, n);
    return 0;
}

static int do_write(int n, const char *dir, int bytes) {
    char tmp[4096], fin[4096], *buf = malloc(bytes);
    memset(buf, 'r', bytes);
    long *v = malloc(sizeof(long) * n);
    snprintf(tmp, sizeof tmp, "%s/.rec.tmp", dir);
    snprintf(fin, sizeof fin, "%s/rec.json", dir);
    for (int i = 0; i < n; i++) {
        long t0 = now_ns();
        int fd = open(tmp, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
        if (fd < 0 || write(fd, buf, bytes) != bytes || close(fd) != 0 || rename(tmp, fin) != 0) { perror(dir); return 1; }
        v[i] = now_ns() - t0;
    }
    unlink(fin);
    char name[64];
    snprintf(name, sizeof name, "write %d B", bytes); report(name, v, n);
    return 0;
}

int main(int argc, char **argv) {
    if (argc >= 3 && !strcmp(argv[1], "clone")) return do_clone(atoi(argv[2]), argc > 3 ? argv[3] : NULL);
    if (argc >= 3 && !strcmp(argv[1], "setns")) return do_setns(atoi(argv[2]));
    if (argc >= 4 && !strcmp(argv[1], "write")) return do_write(atoi(argv[2]), argv[3], argc > 4 ? atoi(argv[4]) : 400);
    fprintf(stderr, "usage: levers clone <n> [cgroup dir] | levers setns <n> | levers write <n> <dir> [bytes]\n");
    return 2;
}
