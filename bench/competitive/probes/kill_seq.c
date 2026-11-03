// Goal 5 extended, step 6: does a child cloned with CLONE_INTO_CGROUP into a cgroup that has had cgroup.kill
// written to it survive? Linux v6.19's cgroup_css_set_fork samples the forking task's own cgroup's kill_seq for
// such a clone (kernel/cgroup/cgroup.c:6702-6705, kargs->cgrp set only at :6768), and cgroup_post_fork kills the
// child when the target's differs (:6902, :6941). Usage: kill_seq <cgroup dir to make the two cgroups under> (bench/competitive/probes/kill_seq.md).
#define _GNU_SOURCE
#include <fcntl.h>
#include <linux/sched.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <unistd.h>

static int child_survives(const char *dir) {
    int fd = open(dir, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (fd < 0) { perror("open cgroup"); exit(2); }
    struct clone_args args;
    memset(&args, 0, sizeof args);
    args.flags = CLONE_INTO_CGROUP;
    args.exit_signal = SIGCHLD;
    args.cgroup = (unsigned long)fd;
    long pid = syscall(SYS_clone3, &args, sizeof args);
    if (pid < 0) { perror("clone3"); exit(2); }
    if (pid == 0) { usleep(100000); _exit(7); }
    int status = 0;
    if (waitpid((pid_t)pid, &status, 0) < 0) { perror("waitpid"); exit(2); }
    close(fd);
    if (WIFEXITED(status) && WEXITSTATUS(status) == 7) return 1;
    if (WIFSIGNALED(status)) { printf("  child killed by signal %d\n", WTERMSIG(status)); return 0; }
    printf("  child ended with status %d\n", status); return 0;
}

int main(int argc, char **argv) {
    if (argc != 2) { fprintf(stderr, "usage: kill_seq <parent cgroup dir>\n"); return 2; }
    char fresh[4096], killed[4096], kill_file[4200];
    snprintf(fresh, sizeof fresh, "%s/killseq-fresh", argv[1]);
    snprintf(killed, sizeof killed, "%s/killseq-killed", argv[1]);
    snprintf(kill_file, sizeof kill_file, "%s/cgroup.kill", killed);
    if (mkdir(fresh, 0755) || mkdir(killed, 0755)) { perror("mkdir"); return 2; }
    int k = open(kill_file, O_WRONLY | O_CLOEXEC);
    if (k < 0 || write(k, "1", 1) != 1) { perror("cgroup.kill"); return 2; }
    close(k);
    int a = child_survives(fresh);
    printf("fresh cgroup: child %s\n", a ? "survived" : "KILLED");
    int b = child_survives(killed);
    printf("cgroup after cgroup.kill: child %s\n", b ? "survived" : "KILLED");
    int c = child_survives(killed);
    printf("cgroup after cgroup.kill, a second clone: child %s\n", c ? "survived" : "KILLED");
    rmdir(fresh); rmdir(killed);
    return 0;
}
