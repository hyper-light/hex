// Why the macOS box watcher's Mach time-constraint thread needs a computation budget above the floor's
// 50 µs (crates/hex-runtime/src/macos/process.rs). A box of 28 members (twice adas-mac-studio-3's 14 cores)
// allocating at full speed overshot its 256 MiB bound by hundreds of MiB in the class bench, not because
// the poll interval was too long but because the watcher read was descheduled: under the members'
// memory-bandwidth contention a read (one proc_listpids + a proc_pid_rusage each) runs slower than the
// 50 µs budget, so the kernel demotes the thread mid-period and it then waits behind the members. This
// probe is the watcher's inner loop alone (no class bookkeeping), sweeping the computation budget, and
// reports the overshoot and the largest gap between two reads.
//
//   rtbudget K COMP_US FLOOR_US RATE_GIB   (defaults 28 50 250 256)
//
// Measured on adas-mac-studio-3 (M4 Max, 14 cores), 2026-11-01, load 1.75:
//   k=28 comp=50  -> overshoot_max 41.8 MiB, max_read_gap 1007 us
//   k=28 comp=150 -> overshoot_max 18.3 MiB, max_read_gap  950 us
//   k=28 comp=230 -> overshoot_max 19.9 MiB
//   k=14 comp=150 -> overshoot_max 60.2 MiB
// So 150 µs (per 250 µs period) holds 28 members well within the 64 MiB bar; the class uses it.
// Build: clang -O2 -Wall -Wno-deprecated-declarations -o rtbudget rtbudget.c
#include <libproc.h>
#include <mach/mach.h>
#include <mach/mach_time.h>
#include <mach/thread_policy.h>
#include <pthread.h>
#include <pthread/qos.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/event.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#define MIB (1024ULL * 1024ULL)
#define BOUND (256ULL * MIB)
#define CEIL (4096ULL * MIB)
static uint64_t now_ns(void) { return clock_gettime_nsec_np(CLOCK_UPTIME_RAW); }
static char *volatile kept;
static int touch(uint64_t n) {
    volatile char *p = malloc((size_t)n);
    if (!p) return -1;
    for (uint64_t i = 0; i < n; i += 4096) p[i] = 1;
    kept = (char *)p;
    return 0;
}
static uint64_t footprint(pid_t pid) {
    struct rusage_info_v4 ri;
    if (proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)&ri) != 0) return 0;
    return ri.ri_phys_footprint;
}
static int lst(uint32_t t, uint32_t i, pid_t *o) {
    int b = proc_listpids(t, i, o, 4096 * (int)sizeof(pid_t));
    return b <= 0 ? 0 : b / (int)sizeof(pid_t);
}
static uint64_t box_fp(pid_t leader) {
    static pid_t v[4096];
    uint64_t s = 0;
    int n = lst(PROC_PGRP_ONLY, (uint32_t)leader, v);
    for (int i = 0; i < n; i++) s += footprint(v[i]);
    return s;
}
static void set_rt(long comp_us) {
    mach_timebase_info_data_t tb;
    mach_timebase_info(&tb);
    double per = 1000.0 * tb.denom / tb.numer;
    thread_time_constraint_policy_data_t p = {(uint32_t)(250 * per), (uint32_t)(comp_us * per),
                                              (uint32_t)(250 * per), 1};
    kern_return_t kr = thread_policy_set(pthread_mach_thread_np(pthread_self()),
                                         THREAD_TIME_CONSTRAINT_POLICY, (thread_policy_t)&p,
                                         THREAD_TIME_CONSTRAINT_POLICY_COUNT);
    if (kr != KERN_SUCCESS) fprintf(stderr, "rt refused %d\n", kr);
}
static long nxt(uint64_t fp, long floor, uint64_t rate) {
    uint64_t room = fp < BOUND ? BOUND - fp : 0;
    long us = (long)((double)room * 1e6 / (double)rate);
    return us < floor ? floor : us > 100000 ? 100000 : us;
}
int main(int argc, char **argv) {
    int k = argc > 1 ? atoi(argv[1]) : 28;
    long comp = argc > 2 ? atol(argv[2]) : 50;
    long floor = argc > 3 ? atol(argv[3]) : 250;
    uint64_t rate = (argc > 4 ? (uint64_t)atoll(argv[4]) : 256) * 1024ULL * MIB;
    int RUNS = 20;
    uint64_t over[64], gap[64], ro[64];
    for (int run = 0; run < RUNS && run < 64; run++) {
        int go[2];
        if (pipe(go)) return 1;
        pid_t leader = fork();
        if (leader == 0) {
            setpgid(0, 0);
            for (int i = 0; i < k; i++)
                if (fork() == 0) {
                    pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0);
                    char b;
                    if (read(go[0], &b, 1) != 1) _exit(3);
                    for (uint64_t g = 0; g < CEIL / (uint64_t)k; g += MIB)
                        if (touch(MIB)) _exit(2);
                    pause();
                    _exit(0);
                }
            pause();
            _exit(0);
        }
        setpgid(leader, leader);
        static pid_t v[4096];
        while (lst(PROC_PGRP_ONLY, (uint32_t)leader, v) < k + 1) usleep(1000);
        set_rt(comp);
        char bb[64];
        memset(bb, 1, sizeof bb);
        if (write(go[1], bb, (size_t)k) != (ssize_t)k) return 1;
        uint64_t fp = 0, t0 = now_ns(), last = now_ns(), maxgap = 0, fp0 = 0, tf = 0;
        int kq = kqueue();
        struct kevent ev, out;
        for (;;) {
            fp = box_fp(leader);
            uint64_t t = now_ns(), g = t - last;
            if (g > maxgap) maxgap = g;
            last = t;
            if (!tf && fp > 64 * MIB) { tf = t; fp0 = fp; }
            if (fp > BOUND) break;
            if (now_ns() - t0 > 30ULL * 1000000000ULL) break;
            long w = nxt(fp, floor, rate);
            EV_SET(&ev, 1, EVFILT_TIMER, EV_ADD | EV_ONESHOT, NOTE_USECONDS | NOTE_CRITICAL, w, NULL);
            kevent(kq, &ev, 1, &out, 1, NULL);
        }
        uint64_t thit = now_ns();
        killpg(leader, SIGKILL);
        while (waitpid(-1, NULL, 0) > 0) {}
        close(go[0]);
        close(go[1]);
        over[run] = fp > BOUND ? fp - BOUND : 0;
        gap[run] = maxgap;
        double sec = tf ? (double)(thit - tf) / 1e9 : 0;
        ro[run] = sec > 0 ? (uint64_t)((double)(fp - fp0) / sec) : 0;
    }
    uint64_t mo = 0, mg = 0;
    double sr = 0;
    for (int i = 0; i < RUNS; i++) {
        if (over[i] > mo) mo = over[i];
        if (gap[i] > mg) mg = gap[i];
        sr += (double)ro[i];
    }
    printf("k=%d comp_us=%ld floor_us=%ld rate_gib=%ld: overshoot_max=%.1f MiB  max_read_gap_us=%.0f  "
           "mean_obs_rate_gib=%.0f\n",
           k, comp, floor, (long)(rate / 1024 / MIB), (double)mo / MIB, (double)mg / 1e3,
           sr / RUNS / 1073741824.0);
    return 0;
}
