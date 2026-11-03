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

/* With the argument `ipcuts`, each make is what the node's ipc and uts pool's thread does (goal 5 extended step 4,
 * hex-runtime's oci/ipcuts.rs), going back to these after each. */
static int ipcuts, home_ipc, home_uts;

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

/* One fresh ipc and uts pair: this thread moves into them, their descriptors are kept, and it goes back home. */
static void make_pair(void) {
  if (unshare(CLONE_NEWIPC | CLONE_NEWUTS) < 0) die("unshare(CLONE_NEWIPC | CLONE_NEWUTS)");
  int ipc = open("/proc/thread-self/ns/ipc", O_RDONLY | O_CLOEXEC);
  int uts = open("/proc/thread-self/ns/uts", O_RDONLY | O_CLOEXEC);
  if (ipc < 0 || uts < 0) die("opening /proc/thread-self/ns/ipc and uts");
  if (setns(home_ipc, CLONE_NEWIPC) < 0 || setns(home_uts, CLONE_NEWUTS) < 0) die("setns back home");
  held_fd[held++] = ipc;
  held_fd[held++] = uts;
}

int main(int argc, char **argv) {
  char line[64];
  ipcuts = argc > 1 && strcmp(argv[1], "ipcuts") == 0;
  if (ipcuts) {
    home_ipc = open("/proc/thread-self/ns/ipc", O_RDONLY | O_CLOEXEC);
    home_uts = open("/proc/thread-self/ns/uts", O_RDONLY | O_CLOEXEC);
    if (home_ipc < 0 || home_uts < 0) die("opening this thread's ipc and uts namespaces");
  }
  setvbuf(stdout, NULL, _IOLBF, 0);
  while (fgets(line, sizeof line, stdin)) {
    if (strncmp(line, "exit", 4) == 0) break;
    long n;
    if (sscanf(line, "make %ld", &n) != 1 || n < 1 || n > (MAX_HELD - held) / (ipcuts ? 2 : 1)) {
      fprintf(stderr, "netns_pool: a command is `make <n>` (1 to %d more) or `exit`, not %s", MAX_HELD - held, line);
      return 1;
    }
    uint64_t *ns = calloc((size_t)n, sizeof *ns);
    if (!ns) die("allocating the timings");
    for (long i = 0; i < n; i++) {
      uint64_t t0 = now_ns();
      if (ipcuts) make_pair(); else make_one();
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
