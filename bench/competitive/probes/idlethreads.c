// bench/competitive/probes/idlethreads.c (goal 5 plan step 2, AC-c): holds `n` idle threads on the host
// (default 24000), each on a 64 KiB stack blocked in pause(), so `pivot_root`'s walk of every thread on
// the host (chroot_fs_refs's for_each_process_thread, v6.19 fs/fs_struct.c) has that many to visit. The
// create breakdown spawns it on the host (outside the bench container) to show the pruned-seed root is
// flat in the host's thread count where the classic pivot_root grows with it. Prints "ready" once the
// threads are up, then blocks until killed. Built on the host: gcc -O2 -pthread.
#define _GNU_SOURCE
#include <pthread.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>

static void *idle(void *arg) {
    (void)arg;
    pause();
    return NULL;
}

int main(int argc, char **argv) {
    long n = argc > 1 ? strtol(argv[1], NULL, 10) : 24000;
    pthread_attr_t attr;
    if (pthread_attr_init(&attr) != 0) {
        return 1;
    }
    // Small stacks and detached, so 24,000 threads cost little memory and need no join.
    pthread_attr_setstacksize(&attr, 64 * 1024);
    pthread_attr_setdetachstate(&attr, PTHREAD_CREATE_DETACHED);
    long up = 0;
    for (long i = 0; i < n; i++) {
        pthread_t t;
        if (pthread_create(&t, &attr, idle, NULL) == 0) {
            up++;
        }
    }
    pthread_attr_destroy(&attr);
    fprintf(stderr, "idlethreads: %ld of %ld up\n", up, n);
    printf("ready\n");
    fflush(stdout);
    pause();
    return 0;
}
