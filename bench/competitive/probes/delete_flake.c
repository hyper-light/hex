// Probe: on this kernel, does a cgroup hold `populated=1` while cgroup.procs and cgroup.threads read
// empty? Two scenarios in a delegated cgroup v2 subtree, each in its own leaf:
//   A. leaderless thread group: a process whose thread-group leader exits (pthread_exit) while a
//      non-leader thread lives; then read events/procs/threads, then cgroup.kill, then poll.
//   B. a zombie child in a nested pid namespace whose ns-init is dying (the container-teardown shape):
//      not done here (needs CLONE_NEWPID + root or userns); scenario A is the listing-gap probe.
// Usage: probe <cgroup-root-of-a-delegated-subtree>
#define _GNU_SOURCE
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <pthread.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>
#include <errno.h>
#include <sys/syscall.h>

static void wr(const char *path, const char *val) {
    int fd = open(path, O_WRONLY);
    if (fd < 0) { fprintf(stderr, "open %s: %m\n", path); return; }
    if (write(fd, val, strlen(val)) < 0) fprintf(stderr, "write %s: %m\n", path);
    close(fd);
}
static void show(const char *dir, const char *tag) {
    char p[4096]; char buf[4096];
    const char *files[] = {"cgroup.events", "cgroup.procs", "cgroup.threads"};
    printf("--- %s (%s)\n", tag, dir);
    for (int i = 0; i < 3; i++) {
        snprintf(p, sizeof p, "%s/%s", dir, files[i]);
        int fd = open(p, O_RDONLY); if (fd < 0) { printf("  %s: %m\n", files[i]); continue; }
        int n = read(fd, buf, sizeof buf - 1); close(fd); if (n < 0) n = 0; buf[n] = 0;
        for (char *q = buf; *q; q++) if (*q == '\n') *q = ' ';
        printf("  %-14s [%s]\n", files[i], buf);
    }
    fflush(stdout);
}

static volatile int leader_gone = 0;
static void *thread_fn(void *arg) {
    (void)arg;
    while (!leader_gone) usleep(1000);
    // leader has exited; this non-leader thread lives on, in the cgroup, for a while
    for (int i = 0; i < 600; i++) usleep(100000); // up to 60 s
    return NULL;
}

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: probe <delegated cgroup dir>\n"); return 2; }
    char leaf[4096]; snprintf(leaf, sizeof leaf, "%s/flakeprobeA", argv[1]);
    if (mkdir(leaf, 0755) < 0 && access(leaf, F_OK) < 0) { fprintf(stderr, "mkdir %s: %m\n", leaf); return 1; }
    pid_t pid = fork();
    if (pid == 0) {
        char procs[4096]; snprintf(procs, sizeof procs, "%s/cgroup.procs", leaf);
        wr(procs, "0");                 // move this child into the leaf cgroup
        pthread_t t; pthread_create(&t, NULL, thread_fn, NULL);
        usleep(200000);
        leader_gone = 1;
        pthread_exit(NULL);             // leader exits; the process stays alive via thread t
    }
    sleep(1);
    show(leaf, "A: leader exited, non-leader thread alive");
    // Now kill the leaf and watch populated for up to ~5 s
    char killp[4096]; snprintf(killp, sizeof killp, "%s/cgroup.kill", leaf);
    wr(killp, "1");
    for (int i = 0; i < 10; i++) {
        usleep(500000);
        char ep[4096]; snprintf(ep, sizeof ep, "%s/cgroup.events", leaf);
        int fd = open(ep, O_RDONLY); char b[256]; int n = fd >= 0 ? read(fd, b, sizeof b - 1) : 0;
        if (fd >= 0) close(fd); if (n < 0) n = 0; b[n] = 0; for (char *q = b; *q; q++) if (*q=='\n') *q=' ';
        printf("  after kill +%d.%ds events [%s]\n", i/2, (i%2)*5, b); fflush(stdout);
    }
    show(leaf, "A: after cgroup.kill");
    // THE FIX under test: SIGKILL each live THREAD by tid (from cgroup.threads), which reaches a
    // non-leader thread cgroup.kill's leader-signal missed. Any thread's SIGKILL is fatal to the group.
    char tp[4096]; snprintf(tp, sizeof tp, "%s/cgroup.threads", leaf);
    int fd = open(tp, O_RDONLY); char tb[4096]; int n = fd>=0?read(fd,tb,sizeof tb-1):0; if(fd>=0)close(fd);
    if(n<0)n=0; tb[n]=0;
    for (char *line = strtok(tb, "\n"); line; line = strtok(NULL, "\n")) {
        int tid = atoi(line); if (tid <= 0) continue;
        long r = syscall(SYS_tkill, tid, SIGKILL);   // tkill the thread directly (no tgid; reaches a non-leader thread)
        printf("  fix: tgkill(%d) = %ld (%s)\n", tid, r, r?strerror(errno):"ok");
    }
    for (int i = 0; i < 6; i++) {
        usleep(300000);
        char ep[4096]; snprintf(ep, sizeof ep, "%s/cgroup.events", leaf);
        int f2 = open(ep, O_RDONLY); char b[256]; int m = f2>=0?read(f2,b,sizeof b-1):0; if(f2>=0)close(f2);
        if(m<0)m=0; b[m]=0; for(char *q=b;*q;q++) if(*q=='\n')*q=' ';
        printf("  after fix +%dms events [%s]\n", i*300, b); fflush(stdout);
    }
    show(leaf, "A: after per-thread SIGKILL (the fix)");
    int st; waitpid(pid, &st, 0);
    rmdir(leaf);

    // CONTROL: a normal single-thread process in its own leaf; cgroup.kill must empty it at once.
    char cleaf[4096]; snprintf(cleaf, sizeof cleaf, "%s/flakeprobeC", argv[1]);
    mkdir(cleaf, 0755);
    pid_t cp = fork();
    if (cp == 0) { char pr[4096]; snprintf(pr,sizeof pr,"%s/cgroup.procs",cleaf); wr(pr,"0"); pause(); _exit(0); }
    sleep(1); show(cleaf, "C: normal process before kill");
    char ck[4096]; snprintf(ck,sizeof ck,"%s/cgroup.kill",cleaf); wr(ck,"1");
    usleep(300000); show(cleaf, "C: normal process after cgroup.kill");
    int cs; waitpid(cp,&cs,0); rmdir(cleaf);
    return 0;
}
