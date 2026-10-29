/* bench/competitive/probes: what a pool of fresh network namespaces costs (netns_pool.md, goal 4 plan step 1). A
 * single-threaded process makes network namespaces the way the node agent's pool would: each is a fresh namespace
 * of its own (unshare(CLONE_NEWNET) on this thread), kept alive by a descriptor (/proc/thread-self/ns/net, open
 * close-on-exec), with loopback brought up inside it (SIOCSIFFLAGS on a socket made there), and no process left in
 * it. It reads commands on stdin, one a line, and answers each on stdout with one line:
 *
 *   make <n>   makes n more and holds them: {"made":n,"held":total,"ns":[each make's nanoseconds]}
 *   exit       closes every descriptor and exits 0
 *
 * so a driver can read the held memory of the cgroup it runs in before and after a make (netns_pool.py). Each make
 * is timed alone on CLOCK_MONOTONIC, from before its unshare to after its loopback is up. Any failure ends the probe
 * with why on stderr and exit status 1. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <net/if.h>
#include <sched.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

/* The most namespaces one probe holds: a bound on its descriptors and on what it asks of the kernel. */
enum { MAX_HELD = 4096 };

static int held_fd[MAX_HELD];
static int held;

static void die(const char *what) {
  fprintf(stderr, "netns_pool: %s: %s\n", what, strerror(errno));
  exit(1);
}

static uint64_t now_ns(void) {
  struct timespec ts;
  clock_gettime(CLOCK_MONOTONIC, &ts);
  return (uint64_t)ts.tv_sec * 1000000000u + (uint64_t)ts.tv_nsec;
}

/* One fresh namespace: this thread moves into it, its descriptor is kept, and its loopback comes up. */
static void make_one(void) {
  if (unshare(CLONE_NEWNET) < 0) die("unshare(CLONE_NEWNET)");
  int fd = open("/proc/thread-self/ns/net", O_RDONLY | O_CLOEXEC);
  if (fd < 0) die("opening /proc/thread-self/ns/net");
  int s = socket(AF_INET, SOCK_DGRAM | SOCK_CLOEXEC, 0);
  if (s < 0) die("a socket in the new namespace");
  struct ifreq ifr;
  memset(&ifr, 0, sizeof ifr);
  strncpy(ifr.ifr_name, "lo", IFNAMSIZ - 1);
  if (ioctl(s, SIOCGIFFLAGS, &ifr) < 0) die("SIOCGIFFLAGS lo");
  ifr.ifr_flags |= IFF_UP;
  if (ioctl(s, SIOCSIFFLAGS, &ifr) < 0) die("SIOCSIFFLAGS lo up");
  close(s);
  held_fd[held++] = fd;
}

int main(void) {
  char line[64];
  setvbuf(stdout, NULL, _IOLBF, 0);
  while (fgets(line, sizeof line, stdin)) {
    if (strncmp(line, "exit", 4) == 0) break;
    long n;
    if (sscanf(line, "make %ld", &n) != 1 || n < 1 || n > MAX_HELD - held) {
      fprintf(stderr, "netns_pool: a command is `make <n>` (1 to %d more) or `exit`, not %s", MAX_HELD - held, line);
      return 1;
    }
    uint64_t *ns = calloc((size_t)n, sizeof *ns);
    if (!ns) die("allocating the timings");
    for (long i = 0; i < n; i++) {
      uint64_t t0 = now_ns();
      make_one();
      ns[i] = now_ns() - t0;
    }
    printf("{\"made\":%ld,\"held\":%d,\"ns\":[", n, held);
    for (long i = 0; i < n; i++) printf("%s%llu", i ? "," : "", (unsigned long long)ns[i]);
    printf("]}\n");
    free(ns);
  }
  for (int i = 0; i < held; i++) close(held_fd[i]);
  return 0;
}
