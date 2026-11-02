// Point-4 probe (lead's delete-fix review): does a PROCESS-DIRECTED signal reach the live worker of a
// leaderless thread group (leader pthread_exit'd, a non-leader alive) that cgroup.kill leaves behind?
// If yes, the fix needs no per-thread (PIDFD_THREAD/tgkill) enumeration. Two routes, each in its own
// leaf of a delegated cgroup v2 subtree, after cgroup.kill has missed:
//   R1. kill(tgid, SIGKILL)         -- process-directed via the pid read from cgroup.procs
//   R2. pidfd_open(tgid)+pidfd_send_signal(SIGKILL, NULL)  -- process-directed via a process pidfd
// Reports, for each, whether pidfd_open succeeds on the (zombie-leader) tgid and whether the leaf goes
// to populated 0. Usage: point4 <cgroup-root-of-a-delegated-subtree>
#define _GNU_SOURCE
#include <fcntl.h>
#include <sys/stat.h>
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include <errno.h>
#include <signal.h>
#include <sys/syscall.h>

static void wr(const char *path, const char *val) {
    int fd = open(path, O_WRONLY);
    if (fd < 0) { fprintf(stderr, "open %s: %m\n", path); return; }
    if (write(fd, val, strlen(val)) < 0) fprintf(stderr, "write %s: %m\n", path);
    close(fd);
}
static int read_first_int(const char *path) {
    int fd = open(path, O_RDONLY); if (fd < 0) return -1;
    char b[256]; int n = read(fd, b, sizeof b - 1); close(fd); if (n <= 0) return -1; b[n] = 0;
    return atoi(b);
}
static const char *events(const char *leaf) {
    static char b[256]; char p[4096]; snprintf(p, sizeof p, "%s/cgroup.events", leaf);
    int fd = open(p, O_RDONLY); int n = fd >= 0 ? read(fd, b, sizeof b - 1) : 0; if (fd >= 0) close(fd);
    if (n < 0) n = 0; b[n] = 0; for (char *q = b; *q; q++) if (*q == '\n') *q = ' '; return b;
}

static volatile int leader_gone = 0;
static void *thread_fn(void *arg) {
    (void)arg;
    while (!leader_gone) usleep(1000);
    for (int i = 0; i < 600; i++) usleep(100000); // live up to 60 s after the leader exits
    return NULL;
}

// Make a leaderless group in `leaf`; return the group's tgid (the forked child's pid).
static pid_t make_leaderless(const char *leaf) {
    mkdir(leaf, 0755);
    pid_t pid = fork();
    if (pid == 0) {
        char procs[4096]; snprintf(procs, sizeof procs, "%s/cgroup.procs", leaf);
        wr(procs, "0");
        pthread_t t; pthread_create(&t, NULL, thread_fn, NULL);
        usleep(200000);
        leader_gone = 1;
        pthread_exit(NULL); // leader exits; non-leader thread lives on, holding the cgroup populated
    }
    sleep(1);
    return pid;
}

// After cgroup.kill, poll events for up to ~3 s; return 1 if it reaches populated 0.
static int emptied(const char *leaf) {
    for (int i = 0; i < 30; i++) {
        usleep(100000);
        char p[4096]; snprintf(p, sizeof p, "%s/cgroup.events", leaf);
        // events has "populated N"; read the populated value
        int fd = open(p, O_RDONLY); if (fd < 0) return 0; char b[256];
        int n = read(fd, b, sizeof b - 1); close(fd); if (n <= 0) continue; b[n] = 0;
        char *pp = strstr(b, "populated"); if (pp && atoi(pp + 9) == 0) return 1;
    }
    return 0;
}

static int run_route(const char *root, const char *name, int use_pidfd) {
    char leaf[4096]; snprintf(leaf, sizeof leaf, "%s/%s", root, name);
    pid_t tgid = make_leaderless(leaf);
    char killp[4096]; snprintf(killp, sizeof killp, "%s/cgroup.kill", leaf);
    wr(killp, "1");
    int after_kill = emptied(leaf);
    printf("[%s] after cgroup.kill: emptied=%d events[%s] (expect 0 = the known miss)\n",
           name, after_kill, events(leaf));

    // The process-directed route under test:
    int reached = -1, pidfd = -1, sig_errno = 0;
    if (use_pidfd) {
        pidfd = syscall(SYS_pidfd_open, tgid, 0u); // 0 = a PROCESS pidfd (no PIDFD_THREAD)
        if (pidfd < 0) {
            printf("[%s] pidfd_open(tgid=%d) FAILED: %m (so a process pidfd can't be acquired on a "
                   "zombie-leader group)\n", name, tgid);
        } else {
            long r = syscall(SYS_pidfd_send_signal, pidfd, SIGKILL, (void *)0, 0u); // NULL info = process-directed
            sig_errno = r ? errno : 0;
            printf("[%s] pidfd_open(tgid=%d)=ok, pidfd_send_signal(SIGKILL, NULL)=%ld (%s)\n",
                   name, tgid, r, r ? strerror(sig_errno) : "ok");
            close(pidfd);
        }
    } else {
        long r = syscall(SYS_kill, tgid, SIGKILL); // process-directed via the pid
        sig_errno = r ? errno : 0;
        printf("[%s] kill(tgid=%d, SIGKILL)=%ld (%s)\n", name, tgid, r, r ? strerror(sig_errno) : "ok");
    }
    reached = emptied(leaf);
    printf("[%s] after process-directed signal: emptied=%d events[%s]  => %s\n",
           name, reached, events(leaf), reached ? "REACHED the leaderless worker" : "did NOT reach it");
    int st; waitpid(tgid, &st, 0);
    rmdir(leaf);
    return reached;
}

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: point4 <delegated cgroup dir>\n"); return 2; }
    printf("kernel: "); fflush(stdout); system("uname -r");
    int r1 = run_route(argv[1], "R1-kill-tgid", 0);
    int r2 = run_route(argv[1], "R2-process-pidfd", 1);
    printf("\nVERDICT: kill(tgid)=%s  process-pidfd=%s\n",
           r1 ? "reaches" : "MISSES", r2 ? "reaches" : "MISSES");
    return (r1 && r2) ? 0 : 1;
}
