// What a native macOS box can bound with public interfaces alone (docs/research/runtime/native-platforms.md §3, §7).
//   read   K   reading a box of K processes: proc_listpids of its process group, then proc_pid_rusage of each
//   over  T   a member allocating as fast as it can, ended once its footprint passes 256 MiB, polled every T µs:
//              how far past the bound it got, and how fast it allocated
//   adapt T    the same allocator, polled every (bound - footprint) / rate, at least T µs and at most 100 ms
//   adaptk K T the same, with K members allocating side by side, the box's footprint summed over its process group
//   hold  T    a member that sits at 95% of the bound for 2 s, polled so: how many reads that costs
//   HEX_PROBE_QOS=ui runs the watcher at QOS_CLASS_USER_INTERACTIVE; every run prints the QoS it ran at, since a
//   timer's coalescing, and so a poll's real interval, follows it
//   HEX_PROBE_BOX_QOS=utility runs the box's members at QOS_CLASS_UTILITY, below the watcher, as a node would
//   HEX_PROBE_TIMER=kq waits on a kqueue timer marked NOTE_CRITICAL (minimal coalescing) instead of usleep
//   check      whether proc_pid_rusage reads this process and a child, at each flavor
//   escape     a member that forks, setsid()s and leaves its parent: which listing still finds it
// Build: clang -O2 -Wall -Wextra -o box_probe box_probe.c
#include <errno.h>
#include <libproc.h>
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
#define RUNS 20
#define READS 2000
#define MAX_PIDS 4096

static uint64_t now_ns(void) { return clock_gettime_nsec_np(CLOCK_UPTIME_RAW); }

static int cmp_u64(const void *a, const void *b) {
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return (x > y) - (x < y);
}

static uint64_t pct(uint64_t *v, int n, int p) {
    qsort(v, (size_t)n, sizeof *v, cmp_u64);
    int i = (n * p) / 100;
    return v[i >= n ? n - 1 : i];
}

// Allocates n bytes and writes every page through a volatile pointer, kept in a global: an unused malloc and memset
// are otherwise removed at -O2, and the child then allocates nothing.
static char *volatile kept;
static int touch(uint64_t n) {
    volatile char *p = malloc((size_t)n);
    if (!p) return -1;
    for (uint64_t i = 0; i < n; i += 4096) p[i] = 1;
    kept = (char *)p;
    return 0;
}

// Waits us microseconds: usleep, or a one-shot kqueue timer marked NOTE_CRITICAL when HEX_PROBE_TIMER=kq.
static int kq = -1;
static void wait_us(long us) {
    if (kq < 0) {
        const char *t = getenv("HEX_PROBE_TIMER");
        kq = (t && strcmp(t, "kq") == 0) ? kqueue() : 0;
    }
    if (kq <= 0) {
        usleep((useconds_t)us);
        return;
    }
    struct kevent ev, out;
    EV_SET(&ev, 1, EVFILT_TIMER, EV_ADD | EV_ONESHOT, NOTE_USECONDS | NOTE_CRITICAL, us, NULL);
    kevent(kq, &ev, 1, &out, 1, NULL);
}

// In a box's member: drop to the box's QoS when HEX_PROBE_BOX_QOS=utility.
static void member_qos(void) {
    const char *e = getenv("HEX_PROBE_BOX_QOS");
    if (e && strcmp(e, "utility") == 0) pthread_set_qos_class_self_np(QOS_CLASS_UTILITY, 0);
}

static uint64_t footprint(pid_t pid) {
    struct rusage_info_v4 ri;
    if (proc_pid_rusage(pid, RUSAGE_INFO_V4, (rusage_info_t *)&ri) != 0) return 0;
    return ri.ri_phys_footprint;
}

// The pids of one kind of listing (PROC_PGRP_ONLY, PROC_UID_ONLY, PROC_PPID_ONLY), into out; returns the count.
static int list(uint32_t type, uint32_t info, pid_t *out) {
    int bytes = proc_listpids(type, info, out, (int)(MAX_PIDS * sizeof(pid_t)));
    return bytes <= 0 ? 0 : bytes / (int)sizeof(pid_t);
}

static int has(const pid_t *v, int n, pid_t p) {
    for (int i = 0; i < n; i++)
        if (v[i] == p) return 1;
    return 0;
}

static int do_read(int k) {
    pid_t leader = fork();
    if (leader == 0) {  // the box's leader: its own process group, then k-1 idle members
        setpgid(0, 0);
        for (int i = 1; i < k; i++)
            if (fork() == 0) { pause(); _exit(0); }
        pause();
        _exit(0);
    }
    setpgid(leader, leader);
    static pid_t pids[MAX_PIDS];
    while (list(PROC_PGRP_ONLY, (uint32_t)leader, pids) < k) usleep(1000);
    static uint64_t t[READS];
    uint64_t sum = 0;
    for (int r = 0; r < READS; r++) {
        uint64_t a = now_ns();
        int n = list(PROC_PGRP_ONLY, (uint32_t)leader, pids);
        for (int i = 0; i < n; i++) sum += footprint(pids[i]);
        t[r] = now_ns() - a;
    }
    killpg(leader, SIGKILL);
    while (waitpid(-1, NULL, 0) > 0) {}
    printf("read k=%d reads=%d p50_us=%.1f p99_us=%.1f max_us=%.1f per_pid_p50_us=%.2f (sum %llu)\n", k, READS,
           pct(t, READS, 50) / 1e3, pct(t, READS, 99) / 1e3, pct(t, READS, 100) / 1e3,
           pct(t, READS, 50) / 1e3 / k, (unsigned long long)(sum & 1));
    return 0;
}

static int do_over(long interval_us) {
    static uint64_t over[RUNS], rate[RUNS], lag[RUNS];
    for (int run = 0; run < RUNS; run++) {
        pid_t c = fork();
        if (c == 0) {  // allocates and touches 1 MiB at a time, as fast as it can, up to CEIL
            for (uint64_t got = 0; got < CEIL; got += MIB)
                if (touch(MIB) != 0) _exit(2);
            _exit(0);
        }
        uint64_t t0 = now_ns(), fp = 0, fp0 = 0, t_first = 0;
        int reaped = 0;
        for (;;) {
            if (waitpid(c, NULL, WNOHANG) == c) { reaped = 1; break; }  // ended before its bound: no overshoot to read
            fp = footprint(c);
            if (fp > 16 * MIB && !t_first) { t_first = now_ns(); fp0 = fp; }
            if (fp > BOUND) break;
            if (interval_us > 0) wait_us(interval_us);
            if (now_ns() - t0 > 30ULL * 1000000000ULL) break;
        }
        uint64_t t_detect = now_ns();
        if (!reaped) {
            kill(c, SIGKILL);
            waitpid(c, NULL, 0);
        }
        uint64_t t_dead = now_ns();
        over[run] = fp > BOUND ? fp - BOUND : 0;
        double secs = (double)(t_detect - (t_first ? t_first : t0)) / 1e9;
        rate[run] = secs > 0 ? (uint64_t)((double)(fp - fp0) / secs) : 0;
        lag[run] = t_dead - t_detect;
    }
    printf("over interval_us=%ld runs=%d overshoot_mib p50=%.1f p99=%.1f max=%.1f alloc_mib_per_s p50=%.0f "
           "kill_to_reaped_us p50=%.0f max=%.0f\n",
           interval_us, RUNS, pct(over, RUNS, 50) / (double)MIB, pct(over, RUNS, 99) / (double)MIB,
           pct(over, RUNS, 100) / (double)MIB, pct(rate, RUNS, 50) / (double)MIB, pct(lag, RUNS, 50) / 1e3,
           pct(lag, RUNS, 100) / 1e3);
    return 0;
}

static int do_escape(void) {
    int fd[2];
    if (pipe(fd) != 0) return 1;
    pid_t leader = fork();
    if (leader == 0) {
        setpgid(0, 0);
        pid_t mid = fork();
        if (mid == 0) {
            pid_t g = fork();
            if (g == 0) {  // the escaper: a new session, its parent gone
                setsid();
                pid_t me = getpid();
                if (write(fd[1], &me, sizeof me) != (ssize_t)sizeof me) _exit(3);
                sleep(30);
                _exit(0);
            }
            _exit(0);
        }
        waitpid(mid, NULL, 0);
        pause();
        _exit(0);
    }
    setpgid(leader, leader);
    pid_t esc = 0;
    if (read(fd[0], &esc, sizeof esc) != (ssize_t)sizeof esc) return 1;
    usleep(100000);
    static pid_t v[MAX_PIDS];
    struct proc_bsdinfo bi;
    int ppid = proc_pidinfo(esc, PROC_PIDTBSDINFO, 0, &bi, sizeof bi) == (int)sizeof bi ? (int)bi.pbi_ppid : -1;
    int in_pgrp = has(v, list(PROC_PGRP_ONLY, (uint32_t)leader, v), esc);
    int in_kids = has(v, list(PROC_PPID_ONLY, (uint32_t)leader, v), esc);
    int in_uid = has(v, list(PROC_UID_ONLY, (uint32_t)getuid(), v), esc);
    printf("escape pid=%d ppid=%d in_box_pgrp=%d child_of_leader=%d same_uid_listed=%d\n", esc, ppid, in_pgrp,
           in_kids, in_uid);
    kill(esc, SIGKILL);
    killpg(leader, SIGKILL);
    while (waitpid(-1, NULL, 0) > 0) {}
    return 0;
}

#define POLL_MAX_US 100000

// The fastest a box is taken to allocate, in bytes/s: HEX_PROBE_RATE_GIB GiB/s, 32 by default (one allocator reached
// 31.5 here; a box's members together reach far more, which `adaptk` measures).
static uint64_t rate_max(void) {
    static uint64_t r;
    if (!r) {
        const char *e = getenv("HEX_PROBE_RATE_GIB");
        r = (e ? (uint64_t)atoll(e) : 32ULL) * 1024ULL * MIB;
    }
    return r;
}

// The next poll's wait: the time the fastest allocation would take to use the headroom left, clamped.
static long next_wait_us(uint64_t fp, long floor_us) {
    uint64_t room = fp < BOUND ? BOUND - fp : 0;
    long us = (long)((double)room * 1e6 / (double)rate_max());
    return us < floor_us ? floor_us : us > POLL_MAX_US ? POLL_MAX_US : us;
}

static int do_adapt(long floor_us) {
    static uint64_t over[RUNS], reads[RUNS];
    for (int run = 0; run < RUNS; run++) {
        pid_t c = fork();
        if (c == 0) {
            for (uint64_t got = 0; got < CEIL; got += MIB)
                if (touch(MIB) != 0) _exit(2);
            _exit(0);
        }
        uint64_t fp = 0, n = 0;
        int reaped = 0;
        for (;;) {
            if (waitpid(c, NULL, WNOHANG) == c) { reaped = 1; break; }
            fp = footprint(c);
            n++;
            if (fp > BOUND) break;
            wait_us(next_wait_us(fp, floor_us));
        }
        if (!reaped) {
            kill(c, SIGKILL);
            waitpid(c, NULL, 0);
        }
        over[run] = fp > BOUND ? fp - BOUND : 0;
        reads[run] = n;
    }
    printf("adapt floor_us=%ld runs=%d overshoot_mib p50=%.1f p99=%.1f max=%.1f reads_per_run p50=%llu\n", floor_us,
           RUNS, pct(over, RUNS, 50) / (double)MIB, pct(over, RUNS, 99) / (double)MIB, pct(over, RUNS, 100) / (double)MIB,
           (unsigned long long)pct(reads, RUNS, 50));
    return 0;
}

static uint64_t box_footprint(pid_t leader) {
    static pid_t v[MAX_PIDS];
    uint64_t sum = 0;
    int n = list(PROC_PGRP_ONLY, (uint32_t)leader, v);
    for (int i = 0; i < n; i++) sum += footprint(v[i]);
    return sum;
}

static int do_adaptk(int k, long floor_us) {
    static uint64_t over[RUNS], reads[RUNS], rate[RUNS];
    for (int run = 0; run < RUNS; run++) {
        int go[2];
        if (pipe(go) != 0) return 1;
        pid_t leader = fork();
        if (leader == 0) {  // the box: k allocators in its process group, started together
            setpgid(0, 0);
            for (int i = 0; i < k; i++)
                if (fork() == 0) {
                    member_qos();
                    char b;
                    if (read(go[0], &b, 1) != 1) _exit(3);
                    for (uint64_t got = 0; got < CEIL / (uint64_t)k; got += MIB)
                        if (touch(MIB) != 0) _exit(2);
                    pause();
                    _exit(0);
                }
            pause();
            _exit(0);
        }
        setpgid(leader, leader);
        static pid_t v[MAX_PIDS];
        while (list(PROC_PGRP_ONLY, (uint32_t)leader, v) < k + 1) usleep(1000);
        char b[64];
        memset(b, 1, sizeof b);
        if (write(go[1], b, (size_t)k) != (ssize_t)k) return 1;
        uint64_t fp = 0, n = 0, t0 = now_ns(), fp_first = 0, t_first = 0;
        for (;;) {
            fp = box_footprint(leader);
            n++;
            if (!t_first && fp > 64 * MIB) { t_first = now_ns(); fp_first = fp; }
            if (fp > BOUND) break;
            if (now_ns() - t0 > 30ULL * 1000000000ULL) break;
            wait_us(next_wait_us(fp, floor_us));
        }
        uint64_t t_hit = now_ns();
        killpg(leader, SIGKILL);
        while (waitpid(-1, NULL, 0) > 0) {}
        close(go[0]);
        close(go[1]);
        over[run] = fp > BOUND ? fp - BOUND : 0;
        reads[run] = n;
        double secs = t_first ? (double)(t_hit - t_first) / 1e9 : 0;
        rate[run] = secs > 0 ? (uint64_t)((double)(fp - fp_first) / secs) : 0;
    }
    printf("adaptk members=%d floor_us=%ld runs=%d overshoot_mib p50=%.1f p99=%.1f max=%.1f reads_per_run p50=%llu "
           "box_alloc_mib_per_s p50=%.0f\n", k, floor_us, RUNS, pct(over, RUNS, 50) / (double)MIB,
           pct(over, RUNS, 99) / (double)MIB, pct(over, RUNS, 100) / (double)MIB, (unsigned long long)pct(reads, RUNS, 50),
           pct(rate, RUNS, 50) / (double)MIB);
    return 0;
}

static int do_hold(long floor_us) {
    pid_t c = fork();
    if (c == 0) {
        if (touch(BOUND / 100 * 95) != 0) _exit(2);
        pause();
        _exit(0);
    }
    while (footprint(c) < BOUND / 100 * 90) usleep(1000);
    uint64_t t0 = now_ns(), n = 0, busy = 0;
    while (now_ns() - t0 < 2000000000ULL) {
        uint64_t a = now_ns();
        uint64_t fp = footprint(c);
        busy += now_ns() - a;
        n++;
        wait_us(next_wait_us(fp, floor_us));
    }
    kill(c, SIGKILL);
    waitpid(c, NULL, 0);
    printf("hold floor_us=%ld seconds=2 reads_per_s=%.0f read_cpu_us_per_s=%.1f\n", floor_us, n / 2.0, busy / 2e3);
    return 0;
}

// Whether proc_pid_rusage reads this process and a child of the same user, at each flavor, and what it reads.
static int do_check(void) {
    pid_t c = fork();
    if (c == 0) {
        if (touch(64 * MIB) != 0) _exit(2);
        pause();
        _exit(0);
    }
    usleep(300000);
    pid_t who[2] = {getpid(), c};
    for (int w = 0; w < 2; w++)
        for (int flavor = RUSAGE_INFO_V0; flavor <= RUSAGE_INFO_CURRENT; flavor++) {
            struct rusage_info_v6 ri;
            memset(&ri, 0, sizeof ri);
            int r = proc_pid_rusage(who[w], flavor, (rusage_info_t *)&ri);
            int e = r == 0 ? 0 : errno;
            printf("check %s pid=%d flavor=%d ret=%d errno=%d (%s) phys_footprint_mib=%.1f resident_mib=%.1f\n",
                   w == 0 ? "self" : "child", who[w], flavor, r, e, r == 0 ? "ok" : strerror(e),
                   flavor >= RUSAGE_INFO_V1 ? ri.ri_phys_footprint / (double)MIB : -1.0,
                   ri.ri_resident_size / (double)MIB);
        }
    kill(c, SIGKILL);
    waitpid(c, NULL, 0);
    return 0;
}

static const char *qos_name(qos_class_t q) {
    switch (q) {
    case QOS_CLASS_USER_INTERACTIVE: return "user-interactive";
    case QOS_CLASS_USER_INITIATED: return "user-initiated";
    case QOS_CLASS_DEFAULT: return "default";
    case QOS_CLASS_UTILITY: return "utility";
    case QOS_CLASS_BACKGROUND: return "background";
    default: return "unspecified";
    }
}

int main(int argc, char **argv) {
    const char *want = getenv("HEX_PROBE_QOS");
    if (want && strcmp(want, "ui") == 0 && pthread_set_qos_class_self_np(QOS_CLASS_USER_INTERACTIVE, 0) != 0)
        fprintf(stderr, "could not set the QoS\n");
    qos_class_t q = QOS_CLASS_UNSPECIFIED;
    int rel = 0;
    pthread_get_qos_class_np(pthread_self(), &q, &rel);
    int bg = getpriority(PRIO_DARWIN_PROCESS, 0);
    if (want && strcmp(want, "ui") == 0 && bg != 0 && setpriority(PRIO_DARWIN_PROCESS, 0, 0) != 0)
        fprintf(stderr, "could not clear background: %s\n", strerror(errno));
    fprintf(stderr, "qos %s darwin_bg %d -> %d\n", qos_name(q), bg, getpriority(PRIO_DARWIN_PROCESS, 0));
    if (argc >= 2 && strcmp(argv[1], "check") == 0) return do_check();
    if (argc >= 3 && strcmp(argv[1], "adapt") == 0) return do_adapt(atol(argv[2]));
    if (argc >= 3 && strcmp(argv[1], "hold") == 0) return do_hold(atol(argv[2]));
    if (argc >= 4 && strcmp(argv[1], "adaptk") == 0) return do_adaptk(atoi(argv[2]), atol(argv[3]));
    if (argc >= 3 && strcmp(argv[1], "read") == 0) return do_read(atoi(argv[2]));
    if (argc >= 3 && strcmp(argv[1], "over") == 0) return do_over(atol(argv[2]));
    if (argc >= 2 && strcmp(argv[1], "escape") == 0) return do_escape();
    fprintf(stderr, "usage: box_probe read K | over US | adapt FLOOR_US | adaptk K FLOOR_US | hold FLOOR_US | check | escape\n");
    return 2;
}
