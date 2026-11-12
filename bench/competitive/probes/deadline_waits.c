// deadline_waits.c: how far past its deadline each of the OS's timed waits returns, and how soon a wake reaches
// a waiter (AC-T7.4.1; docs/design/deadline-waits.md).
//   macOS: cc -O2 -Wall -o deadline_waits deadline_waits.c
//   Linux: cc -O2 -Wall -o deadline_waits deadline_waits.c -lpthread
//   ./deadline_waits [n]   n waits per mechanism, timeout and arm (default 100)
// For each timeout, two arms: n waits nobody wakes (overshoot: return - deadline), and n waits woken halfway to
// their deadline by another thread (latency: return - the waker's call). Percentiles are nearest-rank, in
// microseconds. The mechanisms are interleaved wait by wait, so the host's load falls on each alike. Each wait
// loops on its deadline as Hex's sites do (a spurious or early return waits again for what is left).
#define _GNU_SOURCE
#include <errno.h>
#include <math.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/utsname.h>
#include <time.h>
#include <unistd.h>
#ifdef __APPLE__
#include <dispatch/dispatch.h>
#include <os/os_sync_wait_on_address.h>
#include <pthread/qos.h>
#include <sys/event.h>
#include <sys/sysctl.h>
extern int pthread_cond_timedwait_relative_np(pthread_cond_t *, pthread_mutex_t *, const struct timespec *);
#else
#include <linux/futex.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#endif

#define NS 1000000000ull

// Rust's Instant: CLOCK_UPTIME_RAW on Apple, CLOCK_MONOTONIC on Linux.
static uint64_t now_ns(void) {
#ifdef __APPLE__
  return clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
#else
  struct timespec t;
  clock_gettime(CLOCK_MONOTONIC, &t);
  return (uint64_t)t.tv_sec * NS + (uint64_t)t.tv_nsec;
#endif
}

static struct timespec span(uint64_t ns) {
  struct timespec t = {(time_t)(ns / NS), (long)(ns % NS)};
  return t;
}

struct mech {
  const char *name;
  void (*init)(void);
  void (*arm)(void);
  int (*wait)(uint64_t deadline); // 1: woken, 0: its deadline passed
  void (*wake)(void);
  void (*disarm)(void);
};

static void none(void) {}

// A condition variable: std's Condvar on Apple (pthread_cond_timedwait_relative_np); glibc's on CLOCK_MONOTONIC on
// Linux (std's own Linux Condvar is the futex below).
static pthread_mutex_t cm = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cc;
static int cflag;
static void cond_init(void) {
#ifdef __APPLE__
  pthread_cond_init(&cc, NULL);
#else
  pthread_condattr_t a;
  pthread_condattr_init(&a);
  pthread_condattr_setclock(&a, CLOCK_MONOTONIC);
  pthread_cond_init(&cc, &a);
#endif
}
static void cond_arm(void) { cflag = 0; }
static int cond_wait(uint64_t deadline) {
  pthread_mutex_lock(&cm);
  while (!cflag) {
    uint64_t t = now_ns();
    if (t >= deadline) break;
#ifdef __APPLE__
    struct timespec rel = span(deadline - t);
    pthread_cond_timedwait_relative_np(&cc, &cm, &rel);
#else
    struct timespec abs = span(deadline);
    pthread_cond_timedwait(&cc, &cm, &abs);
#endif
  }
  int woke = cflag;
  pthread_mutex_unlock(&cm);
  return woke;
}
static void cond_wake(void) {
  pthread_mutex_lock(&cm);
  cflag = 1;
  pthread_cond_signal(&cc);
  pthread_mutex_unlock(&cm);
}

// poll(2) on a pipe, written to wake (millisecond timeouts, rounded up).
static int pfd[2];
static void poll_arm(void) {
  if (pipe(pfd) != 0) abort();
}
static int poll_wait(uint64_t deadline) {
  for (;;) {
    uint64_t t = now_ns();
    if (t >= deadline) return 0;
    struct pollfd p = {pfd[0], POLLIN, 0};
    int ms = (int)((deadline - t + 999999) / 1000000);
    if (poll(&p, 1, ms) > 0) return 1;
  }
}
static void poll_wake(void) {
  if (write(pfd[1], "x", 1) != 1) abort();
}
static void poll_disarm(void) {
  close(pfd[0]);
  close(pfd[1]);
}

// nanosleep(2): woken only by a signal (SIGUSR1, no SA_RESTART), there being no other way to wake it.
static pthread_t sleeper;
static void on_usr1(int s) { (void)s; }
static void sleep_init(void) {
  struct sigaction a;
  memset(&a, 0, sizeof a);
  a.sa_handler = on_usr1;
  sigaction(SIGUSR1, &a, NULL);
  sleeper = pthread_self();
}
static int sleep_wait(uint64_t deadline) {
  for (;;) {
    uint64_t t = now_ns();
    if (t >= deadline) return 0;
    struct timespec rel = span(deadline - t);
    if (nanosleep(&rel, NULL) == -1 && errno == EINTR) return 1;
  }
}
static void sleep_wake(void) { pthread_kill(sleeper, SIGUSR1); }

#ifdef __APPLE__
// dispatch_semaphore_wait: std's thread::park_timeout on Apple (library/std/src/sys/sync/thread_parking/darwin.rs).
static dispatch_semaphore_t ds;
static void dsema_init(void) { ds = dispatch_semaphore_create(0); }
static int dsema_wait(uint64_t deadline) {
  for (;;) {
    uint64_t t = now_ns();
    if (t >= deadline) return 0;
    if (dispatch_semaphore_wait(ds, dispatch_time(DISPATCH_TIME_NOW, (int64_t)(deadline - t))) == 0) return 1;
  }
}
static void dsema_wake(void) { dispatch_semaphore_signal(ds); }
static void dsema_disarm(void) {
  while (dispatch_semaphore_wait(ds, DISPATCH_TIME_NOW) == 0) {}
}

// kevent(2) with its timeout argument, on a kqueue holding an EVFILT_USER event triggered to wake.
static int kq;
static void kq_arm(void) {
  kq = kqueue();
  struct kevent ev;
  EV_SET(&ev, 1, EVFILT_USER, EV_ADD | EV_CLEAR, 0, 0, NULL);
  if (kq < 0 || kevent(kq, &ev, 1, NULL, 0, NULL) != 0) abort();
}
static int kq_wait(uint64_t deadline) {
  for (;;) {
    uint64_t t = now_ns();
    if (t >= deadline) return 0;
    struct timespec rel = span(deadline - t);
    struct kevent out;
    if (kevent(kq, NULL, 0, &out, 1, &rel) > 0) return 1;
  }
}
static void kq_wake(void) {
  struct kevent ev;
  EV_SET(&ev, 1, EVFILT_USER, 0, NOTE_TRIGGER, 0, NULL);
  kevent(kq, &ev, 1, NULL, 0, NULL);
}
static void kq_disarm(void) { close(kq); }

// kevent(2) with no timeout, on a one-shot EVFILT_TIMER (NOTE_CRITICAL: "system does minimal timer coalescing")
// registered in the same call, beside the EVFILT_USER event.
static int ktimer_wait(uint64_t deadline) {
  for (;;) {
    uint64_t t = now_ns();
    if (t >= deadline) return 0;
    struct kevent ev, out;
    EV_SET(&ev, 2, EVFILT_TIMER, EV_ADD | EV_ONESHOT, NOTE_NSECONDS | NOTE_CRITICAL, (intptr_t)(deadline - t), NULL);
    if (kevent(kq, &ev, 1, &out, 1, NULL) > 0 && out.filter == EVFILT_USER) return 1;
  }
}

// os_sync_wait_on_address_with_timeout (macOS 14.4+), the public futex.
static _Atomic uint32_t ow;
static void os_arm(void) { atomic_store(&ow, 0); }
static int os_wait(uint64_t deadline) {
  for (;;) {
    if (atomic_load(&ow)) return 1;
    uint64_t t = now_ns();
    if (t >= deadline) return 0;
    os_sync_wait_on_address_with_timeout((void *)&ow, 0, 4, OS_SYNC_WAIT_ON_ADDRESS_NONE, OS_CLOCK_MACH_ABSOLUTE_TIME,
                                         deadline - t);
  }
}
static void os_wake(void) {
  atomic_store(&ow, 1);
  os_sync_wake_by_address_any((void *)&ow, 4, OS_SYNC_WAKE_BY_ADDRESS_NONE);
}

static struct mech mechs[] = {
    {"cond_rel", cond_init, cond_arm, cond_wait, cond_wake, none},
    {"dsema", dsema_init, none, dsema_wait, dsema_wake, dsema_disarm},
    {"kevent", none, kq_arm, kq_wait, kq_wake, kq_disarm},
    {"ktimer", none, kq_arm, ktimer_wait, kq_wake, kq_disarm},
    {"poll", none, poll_arm, poll_wait, poll_wake, poll_disarm},
    {"os_sync", none, os_arm, os_wait, os_wake, none},
    {"nanosleep", sleep_init, none, sleep_wait, sleep_wake, none},
};
#else
// The futex wait std's Linux Condvar makes (FUTEX_WAIT_BITSET, an absolute CLOCK_MONOTONIC deadline).
static _Atomic uint32_t fw;
static void futex_arm(void) { atomic_store(&fw, 0); }
static int futex_wait(uint64_t deadline) {
  for (;;) {
    if (atomic_load(&fw)) return 1;
    if (now_ns() >= deadline) return 0;
    struct timespec abs = span(deadline);
    syscall(SYS_futex, &fw, FUTEX_WAIT_BITSET_PRIVATE, 0, &abs, NULL, FUTEX_BITSET_MATCH_ANY);
  }
}
static void futex_wake(void) {
  atomic_store(&fw, 1);
  syscall(SYS_futex, &fw, FUTEX_WAKE_PRIVATE, 1, NULL, NULL, 0);
}

static struct mech mechs[] = {
    {"futex", none, futex_arm, futex_wait, futex_wake, none},
    {"cond_mono", cond_init, cond_arm, cond_wait, cond_wake, none},
    {"poll", none, poll_arm, poll_wait, poll_wake, poll_disarm},
    {"nanosleep", sleep_init, none, sleep_wait, sleep_wake, none},
};
#endif
#define MECHS (sizeof mechs / sizeof mechs[0])

struct waker {
  struct mech *m;
  _Atomic uint64_t at;
  _Atomic uint64_t sent;
};

// Spins to its moment rather than sleeping, so its own timer's precision doesn't move the wake.
static void *waker_main(void *p) {
  struct waker *w = p;
  uint64_t at;
  while ((at = atomic_load(&w->at)) == 0) {}
  while (now_ns() < at) {}
  atomic_store(&w->sent, now_ns());
  w->m->wake();
  return NULL;
}

static int cmp(const void *a, const void *b) {
  double x = *(const double *)a, y = *(const double *)b;
  return x < y ? -1 : x > y;
}
static double rank(const double *v, int n, double q) {
  int i = (int)ceil(q * n) - 1;
  return v[i < 0 ? 0 : i];
}

static void loadavg(const char *when) {
  double l[3] = {0, 0, 0};
  getloadavg(l, 3);
  printf("load %s: %.2f %.2f %.2f\n", when, l[0], l[1], l[2]);
}

int main(int argc, char **argv) {
  int n = argc > 1 ? atoi(argv[1]) : 100;
  if (n < 1) return 2;
  setvbuf(stdout, NULL, _IOLBF, 0);
  struct utsname u;
  uname(&u);
  time_t wall = time(NULL);
  char date[32];
  strftime(date, sizeof date, "%Y-%m-%dT%H:%M:%SZ", gmtime(&wall));
  printf("deadline_waits n=%d date=%s host=%s %s %s %s\n", n, date, u.nodename, u.sysname, u.release, u.machine);
#ifdef __APPLE__
  char model[64] = "", os[32] = "";
  size_t ml = sizeof model, ol = sizeof os;
  sysctlbyname("hw.model", model, &ml, NULL, 0);
  sysctlbyname("kern.osproductversion", os, &ol, NULL, 0);
  qos_class_t q;
  int rel;
  pthread_get_qos_class_np(pthread_self(), &q, &rel);
  printf("model=%s macOS=%s thread_qos=0x%x\n", model, os, (unsigned)q);
#else
  printf("timer_slack_ns=%d\n", prctl(PR_GET_TIMERSLACK, 0, 0, 0, 0));
#endif
  loadavg("before");
  for (size_t m = 0; m < MECHS; m++) mechs[m].init();
  // 0.1 ms: Hex waits on sub-millisecond patiences too (hex-testkit's node.listing entry, 137 us).
  static const uint64_t timeouts_us[] = {100, 1000, 5000, 25000, 100000};
  double *over = calloc((size_t)n * MECHS, sizeof *over), *lat = calloc((size_t)n * MECHS, sizeof *lat);
  int *woken = calloc(MECHS, sizeof *woken), *late = calloc(MECHS, sizeof *late);
  if (!over || !lat || !woken || !late) return 1;
  for (size_t ti = 0; ti < sizeof timeouts_us / sizeof timeouts_us[0]; ti++) {
    uint64_t T = timeouts_us[ti] * 1000ull;
    memset(woken, 0, MECHS * sizeof *woken);
    memset(late, 0, MECHS * sizeof *late);
    for (int i = 0; i < n; i++) {
      for (size_t m = 0; m < MECHS; m++) {
        mechs[m].arm();
        uint64_t t0 = now_ns();
        int woke = mechs[m].wait(t0 + T);
        uint64_t t1 = now_ns();
        mechs[m].disarm();
        if (woke) return 3;
        over[m * n + i] = ((double)t1 - (double)(t0 + T)) / 1e3;
      }
    }
    for (int i = 0; i < n; i++) {
      for (size_t m = 0; m < MECHS; m++) {
        struct waker w = {&mechs[m], 0, 0};
        pthread_t th;
        mechs[m].arm();
        if (pthread_create(&th, NULL, waker_main, &w) != 0) return 1;
        uint64_t t0 = now_ns();
        atomic_store(&w.at, t0 + T / 2);
        int woke = mechs[m].wait(t0 + T);
        uint64_t t1 = now_ns();
        pthread_join(th, NULL);
        mechs[m].disarm();
        if (woke) {
          lat[m * n + woken[m]] = ((double)t1 - (double)atomic_load(&w.sent)) / 1e3;
          woken[m]++;
        } else {
          late[m]++;
        }
      }
    }
    for (size_t m = 0; m < MECHS; m++) {
      double *o = over + m * n, *l = lat + m * n;
      qsort(o, (size_t)n, sizeof *o, cmp);
      printf("%-9s T=%5.1fms timeout n=%d over_us min %.0f p50 %.0f p90 %.0f p99 %.0f max %.0f", mechs[m].name,
             (double)timeouts_us[ti] / 1e3, n, o[0], rank(o, n, .5), rank(o, n, .9), rank(o, n, .99), o[n - 1]);
      if (woken[m] > 0) {
        qsort(l, (size_t)woken[m], sizeof *l, cmp);
        printf(" | wake n=%d woken=%d timedout=%d lat_us p50 %.1f p90 %.1f p99 %.1f max %.1f\n", n, woken[m], late[m],
               rank(l, woken[m], .5), rank(l, woken[m], .9), rank(l, woken[m], .99), l[woken[m] - 1]);
      } else {
        printf(" | wake n=%d woken=0 timedout=%d\n", n, late[m]);
      }
    }
  }
  // What a waiter's own wake object costs to make and close: a kqueue with its user event (macOS), a pipe.
  enum { MAKES = 1000 };
  static double made[MAKES];
#ifdef __APPLE__
  for (int i = 0; i < MAKES; i++) {
    uint64_t t0 = now_ns();
    kq_arm();
    kq_disarm();
    made[i] = (double)(now_ns() - t0) / 1e3;
  }
  qsort(made, MAKES, sizeof *made, cmp);
  printf("make kqueue+user+close n=%d us p50 %.2f p90 %.2f p99 %.2f max %.2f\n", MAKES, rank(made, MAKES, .5),
         rank(made, MAKES, .9), rank(made, MAKES, .99), made[MAKES - 1]);
#endif
  for (int i = 0; i < MAKES; i++) {
    uint64_t t0 = now_ns();
    poll_arm();
    poll_disarm();
    made[i] = (double)(now_ns() - t0) / 1e3;
  }
  qsort(made, MAKES, sizeof *made, cmp);
  printf("make pipe+close n=%d us p50 %.2f p90 %.2f p99 %.2f max %.2f\n", MAKES, rank(made, MAKES, .5),
         rank(made, MAKES, .9), rank(made, MAKES, .99), made[MAKES - 1]);
  loadavg("after");
  return 0;
}
