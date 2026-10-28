/* bench/competitive/probes: the floor under an exec into a running container (exec_breakdown.md, goal 3
 * plan step 1). A prepared, single-threaded process runs /usr/bin/true in a container's cgroup and
 * namespaces and waits for it, as goal 3's seed would (plan step 2): each cycle points its children's pid
 * namespace at the container's (setns), clones into the container's cgroup with a pidfd (clone3 with
 * CLONE_INTO_CGROUP and CLONE_PIDFD), and the child joins the container's other namespaces, the mount
 * namespace last, takes the root as its working directory and execs; the parent waits on the pidfd. Nothing
 * else: no state read, no config parsed, no user or capabilities set, so no exec of `true` into that
 * container can cost less.
 *
 *   exec_floor <out.json> <n> <warmup> <container init pid> <container cgroup dir>
 *
 * Beside the floor, in the same process and interleaved cycle by cycle (the order rotated each cycle), the
 * lesser floors it is made of:
 *   spawn   posix_spawn and wait of /usr/bin/true in the probe's own context, as oci_bench.py spawns
 *           every call (oci_breakdown.md section 2 timed it from Python);
 *   clone   clone3 with a pidfd, exec and wait: no cgroup, no namespaces;
 *   cgroup  the same, born in the container's cgroup: no namespaces;
 *   floor   the same, in the container's namespaces too;
 * and a gauge of the CPU's speed at those moments, nothing to do with exec:
 *   spin    SPIN_STEPS steps of a dependent multiply-add chain, with no system call, so its time is the
 *           clock the host gives a burst of work right then (a hybrid laptop part's, under its governor, swings
 *           with what else the host is doing).
 * Each cycle is timed alone on CLOCK_MONOTONIC, from before its first system call to after its wait
 * returns. The JSON holds every timed cycle's nanoseconds by mode; exec_breakdown.py takes percentiles.
 * Any failure ends the probe with why on stderr and exit status 1, and no JSON. */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <signal.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/utsname.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#ifndef P_PIDFD
#define P_PIDFD 3
#endif
#ifndef CLONE_PIDFD
#define CLONE_PIDFD 0x00001000
#endif
#ifndef CLONE_INTO_CGROUP
#define CLONE_INTO_CGROUP 0x200000000ULL
#endif

/* clone3's argument (include/uapi/linux/sched.h, struct clone_args, its size from Linux 5.7), written out
 * here rather than included beside glibc's <sched.h>, as Hex's CloneArgs is (crates/hex-runtime/src/linux/sys.rs). */
struct clone3_args {
  uint64_t flags, pidfd, child_tid, parent_tid, exit_signal, stack, stack_size, tls, set_tid, set_tid_size, cgroup;
};

enum { SPAWN, CLONE, CGROUP, FLOOR, SPIN, MODES };
static const char *const MODE_NAME[MODES] = { "spawn", "clone", "cgroup", "floor", "spin" };

/* spin's steps: about 0.1 ms at 4 GHz, a multiply-add's latency each (a dependent chain), so a burst as
 * long as the parts of an exec it gauges. Its result is stored where the compiler must keep every step. */
enum { SPIN_STEPS = 100000 };
volatile uint64_t spin_sink;

/* The container's namespaces the child joins, in this order: the mount namespace last, since it changes
 * what every path names. pid is the parent's to join, for its children (setns(2): a process's own pid
 * namespace never changes). cgroup joins from inside the cgroup the child is born in, as Hex's exec does
 * (crates/hex-runtime/src/linux/oci/exec.rs, NAMESPACES). */
static const struct { const char *name; int flag; } NS[] = {
  { "ipc", CLONE_NEWIPC }, { "uts", CLONE_NEWUTS }, { "net", CLONE_NEWNET },
  { "cgroup", CLONE_NEWCGROUP }, { "mnt", CLONE_NEWNS },
};
enum { NS_COUNT = sizeof (NS) / sizeof (NS[0]) };

static char *const ARGV[] = { "true", NULL };
static char *const ENVP[] = { "PATH=/usr/bin:/usr/sbin", NULL };

static void
die (const char *what)
{
  fprintf (stderr, "exec_floor: %s: %s\n", what, strerror (errno));
  exit (1);
}

static uint64_t
now_ns (void)
{
  struct timespec ts;
  if (clock_gettime (CLOCK_MONOTONIC, &ts) < 0)
    die ("clock_gettime");
  return (uint64_t) ts.tv_sec * 1000000000u + (uint64_t) ts.tv_nsec;
}

/* The container init's namespace `name`, opened, when it isn't this process's own; -1 when it is. */
static int
theirs_if_other (long pid, const char *name)
{
  char path[64];
  struct stat a, b;
  int fd;

  snprintf (path, sizeof (path), "/proc/%ld/ns/%s", pid, name);
  fd = open (path, O_RDONLY | O_CLOEXEC);
  if (fd < 0)
    die (path);
  snprintf (path, sizeof (path), "/proc/self/ns/%s", name);
  if (fstat (fd, &a) < 0 || stat (path, &b) < 0)
    die ("stat of a namespace");
  if (a.st_dev == b.st_dev && a.st_ino == b.st_ino)
    {
      close (fd);
      return -1;
    }
  return fd;
}

struct target {
  int cgroup;            /* the container's cgroup directory, for CLONE_INTO_CGROUP */
  int pid_ns, own_pid_ns;
  int ns[NS_COUNT];      /* -1 where the container's is this process's own */
};

/* The child of a clone: into the container's namespaces when `join`, then /usr/bin/true. It never returns. */
static void
child (const struct target *t, int join)
{
  if (join)
    {
      for (int i = 0; i < NS_COUNT; i++)
        if (t->ns[i] >= 0 && setns (t->ns[i], NS[i].flag) < 0)
          _exit (126);
      if (chdir ("/") < 0)
        _exit (126);
    }
  execve ("/usr/bin/true", ARGV, ENVP);
  _exit (127);
}

/* One cycle of `mode`: its nanoseconds, from before its first system call to after its wait returns. */
static uint64_t
cycle (const struct target *t, int mode)
{
  uint64_t t0 = now_ns ();
  siginfo_t info;

  if (mode == SPIN)
    {
      uint64_t x = t0;
      for (int i = 0; i < SPIN_STEPS; i++)
        x = x * 6364136223846793005u + 1442695040888963407u;
      spin_sink = x;
      return now_ns () - t0;
    }
  if (mode == SPAWN)
    {
      pid_t pid;
      int status, e = posix_spawn (&pid, "/usr/bin/true", NULL, NULL, ARGV, ENVP);
      if (e != 0)
        {
          errno = e;
          die ("posix_spawn");
        }
      if (waitpid (pid, &status, 0) < 0)
        die ("waitpid");
      if (! WIFEXITED (status) || WEXITSTATUS (status) != 0)
        {
          fprintf (stderr, "exec_floor: spawned true ended with wait status %d\n", status);
          exit (1);
        }
      return now_ns () - t0;
    }

  if (mode == FLOOR && setns (t->pid_ns, CLONE_NEWPID) < 0)
    die ("setns into the container's pid namespace");
  int pidfd = -1;
  struct clone3_args args;
  memset (&args, 0, sizeof (args));
  args.flags = CLONE_PIDFD | (mode == CLONE ? 0 : CLONE_INTO_CGROUP);
  args.pidfd = (uint64_t) (uintptr_t) &pidfd;
  args.exit_signal = SIGCHLD;
  args.cgroup = mode == CLONE ? 0 : (uint64_t) t->cgroup;
  /* No CLONE_VM and no stack: the child goes on in a copy of this single-threaded process, as after
   * fork(2), so it may call anything before its exec. */
  long pid = syscall (SYS_clone3, &args, sizeof (args));
  if (pid < 0)
    die ("clone3");
  if (pid == 0)
    child (t, mode == FLOOR);
  memset (&info, 0, sizeof (info));
  if (waitid ((idtype_t) P_PIDFD, (id_t) pidfd, &info, WEXITED) < 0)
    die ("waitid");
  uint64_t took = now_ns () - t0;
  close (pidfd);
  if (info.si_code != CLD_EXITED || info.si_status != 0)
    {
      fprintf (stderr, "exec_floor: %s's true ended with code %d status %d\n", MODE_NAME[mode], info.si_code,
               info.si_status);
      exit (1);
    }
  /* Back to this process's own pid namespace for the next cycle's children, untimed. */
  if (mode == FLOOR && setns (t->own_pid_ns, CLONE_NEWPID) < 0)
    die ("setns back to the probe's pid namespace");
  return took;
}

static void
loadavg (char *out, size_t size)
{
  FILE *f = fopen ("/proc/loadavg", "r");
  if (f == NULL || fgets (out, (int) size, f) == NULL)
    die ("/proc/loadavg");
  fclose (f);
  out[strcspn (out, "\n")] = '\0';
}

int
main (int argc, char **argv)
{
  if (argc != 6)
    {
      fprintf (stderr, "usage: exec_floor <out.json> <n> <warmup> <container init pid> <container cgroup dir>\n");
      return 2;
    }
  long n = strtol (argv[2], NULL, 10), warmup = strtol (argv[3], NULL, 10), pid = strtol (argv[4], NULL, 10);
  if (n <= 0 || n > 1000000 || warmup < 0 || warmup > 1000000 || pid <= 0)
    {
      fprintf (stderr, "exec_floor: n (1 to 1,000,000), warmup (0 to 1,000,000) and the pid must be given\n");
      return 2;
    }

  struct target t;
  t.cgroup = open (argv[5], O_RDONLY | O_DIRECTORY | O_CLOEXEC);
  if (t.cgroup < 0)
    die (argv[5]);
  t.pid_ns = theirs_if_other (pid, "pid");
  if (t.pid_ns < 0)
    {
      fprintf (stderr, "exec_floor: the container shares this process's pid namespace\n");
      return 1;
    }
  t.own_pid_ns = open ("/proc/self/ns/pid", O_RDONLY | O_CLOEXEC);
  if (t.own_pid_ns < 0)
    die ("/proc/self/ns/pid");
  for (int i = 0; i < NS_COUNT; i++)
    t.ns[i] = theirs_if_other (pid, NS[i].name);

  uint64_t *ns[MODES];
  for (int m = 0; m < MODES; m++)
    if ((ns[m] = calloc ((size_t) n, sizeof (uint64_t))) == NULL)
      die ("allocating the samples");

  char load_start[128], load_end[128];
  loadavg (load_start, sizeof (load_start));
  for (long i = 0; i < warmup + n; i++)
    for (int k = 0; k < MODES; k++)
      {
        int m = (int) ((i + k) % MODES);
        uint64_t took = cycle (&t, m);
        if (i >= warmup)
          ns[m][i - warmup] = took;
      }
  loadavg (load_end, sizeof (load_end));

  struct utsname u;
  if (uname (&u) < 0)
    die ("uname");
  FILE *out = fopen (argv[1], "w");
  if (out == NULL)
    die (argv[1]);
  fprintf (out, "{\"probe\":\"exec_floor\",\"n\":%ld,\"warmup\":%ld,\"host\":\"%s\",\"kernel\":\"%s\",", n, warmup,
           u.nodename, u.release);
  fprintf (out, "\"load_start\":\"%s\",\"load_end\":\"%s\",\"init_pid\":%ld,\"cgroup\":\"%s\",\"joined\":[\"pid\"",
           load_start, load_end, pid, argv[5]);
  for (int i = 0; i < NS_COUNT; i++)
    if (t.ns[i] >= 0)
      fprintf (out, ",\"%s\"", NS[i].name);
  fprintf (out, "],\"ns\":{");
  for (int m = 0; m < MODES; m++)
    {
      fprintf (out, "%s\"%s\":[", m ? "," : "", MODE_NAME[m]);
      for (long i = 0; i < n; i++)
        fprintf (out, "%s%llu", i ? "," : "", (unsigned long long) ns[m][i]);
      fprintf (out, "]");
    }
  fprintf (out, "}}\n");
  if (fclose (out) != 0)
    die (argv[1]);
  return 0;
}
