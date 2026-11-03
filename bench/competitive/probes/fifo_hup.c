// Goal 5 extended, step 6: does a poll on a FIFO's read end report a hang-up while the FIFO's writer is opening
// it, so that the read after the poll finds the writer there and no byte yet (EAGAIN)? A start does what this
// reader does (crates/hex-runtime/src/linux/oci/run.rs, start_with: open O_RDONLY|O_NONBLOCK, poll, read), and a
// container's init what this writer does (init.rs: open O_WRONLY, write one byte, close). Linux v6.19's pipe_poll
// reads the writers and the writers' counter without the pipe's lock (fs/pipe.c:670, :691-692), and fifo_open
// raises the counter before the writers (:1192-1193): a poll whose two loads straddle those two stores sees no
// writer and a counter past the one its open recorded (:1172-1174), a hang-up. pipe_read then finds a writer and
// no byte and returns EAGAIN to a non-blocking reader (:370-376).
//
// Each iteration: the reader opens the FIFO (no writer is open, so its open records the counter), tells the
// writer to go, waits a random 0 to 2047 spins, polls and reads; the writer waits a random 0 to 2047 spins after
// the go, opens, writes its byte and closes. Every poll that reports a hang-up without input is the race: the
// writer always writes before it closes. Counted by what the read after it got; a read that got EAGAIN then polls
// and reads again until it has the byte, so every iteration ends with its byte. Usage (bench/competitive/probes/
// fifo_hup.md):
//   fifo_hup <dir to make the FIFO in> <iterations> <reader cpu> <writer cpu>
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <sched.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

static char path[4096];
static long iterations;
static atomic_long go, done;

static void pin(int cpu) {
    cpu_set_t set;
    CPU_ZERO(&set);
    CPU_SET(cpu, &set);
    if (sched_setaffinity(0, sizeof set, &set) != 0) { perror("sched_setaffinity"); exit(2); }
}

static void spin(unsigned n) {
    for (volatile unsigned i = 0; i < n; i++) {
    }
}

static unsigned next(unsigned *seed) {
    *seed = *seed * 1103515245u + 12345u;
    return *seed >> 8;
}

static void *writer(void *arg) {
    pin(*(int *)arg);
    unsigned seed = 7;
    for (long i = 1; i <= iterations; i++) {
        while (atomic_load_explicit(&go, memory_order_acquire) < i) {
        }
        spin(next(&seed) % 2048);
        int w = open(path, O_WRONLY | O_CLOEXEC);
        if (w < 0) { perror("writer: open"); exit(3); }
        if (write(w, "0", 1) != 1) { perror("writer: write"); exit(3); }
        close(w);
        atomic_store_explicit(&done, i, memory_order_release);
    }
    return NULL;
}

int main(int argc, char **argv) {
    if (argc != 5) {
        fprintf(stderr, "usage: fifo_hup <dir> <iterations> <reader cpu> <writer cpu>\n");
        return 2;
    }
    snprintf(path, sizeof path, "%s/fifo_hup.%d", argv[1], (int)getpid());
    iterations = atol(argv[2]);
    int reader_cpu = atoi(argv[3]), writer_cpu = atoi(argv[4]);
    unlink(path);
    if (mkfifo(path, 0600) != 0) { perror("mkfifo"); return 2; }
    pin(reader_cpu);
    pthread_t w;
    if (pthread_create(&w, NULL, writer, &writer_cpu) != 0) { perror("pthread_create"); return 2; }
    long in = 0, in_hup = 0, hup_eagain = 0, hup_byte = 0, hup_eof = 0, other = 0, timeouts = 0, repolls = 0;
    unsigned seed = 11;
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    for (long i = 1; i <= iterations; i++) {
        int r = open(path, O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        if (r < 0) { perror("reader: open"); return 3; }
        atomic_store_explicit(&go, i, memory_order_release);
        spin(next(&seed) % 2048);
        struct pollfd p = {.fd = r, .events = POLLIN};
        int n = poll(&p, 1, 10000);
        char byte;
        ssize_t got = read(r, &byte, 1);
        int err = got < 0 ? errno : 0;
        if (n == 0) {
            timeouts++;
        } else if (p.revents == POLLHUP) {
            if (got < 0 && err == EAGAIN) hup_eagain++;
            else if (got == 1) hup_byte++;
            else if (got == 0) hup_eof++;
            else other++;
        } else if ((p.revents & POLLIN) && (p.revents & POLLHUP)) {
            in_hup++;
        } else if (p.revents & POLLIN) {
            in++;
        } else {
            other++;
        }
        // Finish the iteration with its byte, as a start that waits again would.
        while (got < 0 && err == EAGAIN) {
            repolls++;
            if (poll(&p, 1, 10000) <= 0) { fprintf(stderr, "iteration %ld: no byte after EAGAIN\n", i); return 4; }
            got = read(r, &byte, 1);
            err = got < 0 ? errno : 0;
        }
        if (got != 1) { fprintf(stderr, "iteration %ld: read %zd (%s)\n", i, got, strerror(err)); return 4; }
        while (atomic_load_explicit(&done, memory_order_acquire) < i) {
        }
        close(r);
    }
    clock_gettime(CLOCK_MONOTONIC, &t1);
    pthread_join(w, NULL);
    unlink(path);
    double s = (double)(t1.tv_sec - t0.tv_sec) + (double)(t1.tv_nsec - t0.tv_nsec) / 1e9;
    printf("iterations %ld in %.2f s (reader cpu %d, writer cpu %d): poll IN %ld, IN|HUP %ld; HUP alone %ld: then "
           "EAGAIN %ld, the byte %ld, end-of-file %ld; other %ld, timeouts %ld; polls again after EAGAIN %ld\n",
           iterations, s, reader_cpu, writer_cpu, in, in_hup, hup_eagain + hup_byte + hup_eof, hup_eagain, hup_byte,
           hup_eof, other, timeouts, repolls);
    return 0;
}
