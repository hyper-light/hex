/* bench/competitive/libcrun: libcrun in process, for oci_bench.py's "libcrun" runtime (goal 0 step 2).
 *
 *   libcrun-driver <state root>     requests on fd 3, replies on fd 4
 *
 * One request a line, each answered by one reply line:
 *   create <bundle> <id> | start <id> | exec <id> | delete <id>
 *   -> "ok <wall ns> <cpu ns>" or "err <why>"
 *
 * Each call makes the libcrun calls crun's command line makes for the same command (src/run_create.c,
 * start.c, exec.c and delete.c at 1.30.1), in this process: no program is loaded per call. Only exec
 * forks, and only because libcrun's exec waits for its caller to have no child left (do_exec).
 * oci_bench.py runs one driver per concurrency slot and its phases unchanged, so the phases, their
 * JSON and the CLI runtimes' are the same code. The wall time is the libcrun call's alone; the CPU is
 * this process's and its reaped children's over it (getrusage), as wait4 gives the CLI call's.
 *
 * libcrun's create clones the container's init from the calling process, so here every init is this
 * process's child, as it is the CLI's until the CLI exits. delete reaps it after the call, untimed,
 * waiting on its pidfd for at most CALL_TIMEOUT_S.
 */
#define _GNU_SOURCE
#include <config.h>

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/resource.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#include "git-version.h"
#include "libcrun/container.h"
#include "libcrun/custom-handler.h"
#include "libcrun/status.h"

#define CALL_TIMEOUT_S 60
#define REQUEST_FD 3
#define REPLY_FD 4

static const char *state_root;
static struct custom_handler_manager_s *handlers;
static FILE *replies;
/* The logging handler, set once (as crun's CLI sets it once per process), reused by every call: a
 * long-lived process must not re-init it per request. */
static crun_output_handler driver_output_handler;
static void *driver_output_handler_arg;

static long long
now_ns (void)
{
  struct timespec ts;
  clock_gettime (CLOCK_MONOTONIC, &ts);
  return (long long) ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

static long long
tv_us (struct timeval tv)
{
  return (long long) tv.tv_sec * 1000000LL + tv.tv_usec;
}

static long long
cpu_ns (void)
{
  struct rusage self, kids;
  getrusage (RUSAGE_SELF, &self);
  getrusage (RUSAGE_CHILDREN, &kids);
  return 1000 * (tv_us (self.ru_utime) + tv_us (self.ru_stime) + tv_us (kids.ru_utime) + tv_us (kids.ru_stime));
}

/* The context crun's init_libcrun_context (src/crun.c) makes for every command, with no global options. */
static int
context_for (libcrun_context_t *con, const char *id, libcrun_error_t *err)
{
  memset (con, 0, sizeof (*con));
  con->id = id;
  con->state_root = state_root;
  con->fifo_exec_wait_fd = -1;
  con->bundle = ".";
  con->handler_manager = handlers;
  con->output_handler = driver_output_handler;
  con->output_handler_arg = driver_output_handler_arg;
  (void) id;
  (void) err;
  return 0;
}

/* `crun create --bundle <bundle> <id>`: chdir to the bundle, load its config.json, create. */
static int
do_create (const char *bundle, const char *id, libcrun_error_t *err)
{
  libcrun_context_t con;
  libcrun_container_t *container;
  int ret;

  ret = context_for (&con, id, err);
  if (ret < 0)
    return ret;
  if (chdir (bundle) < 0)
    return libcrun_make_error (err, errno, "chdir `%s`", bundle);
  container = libcrun_container_load_from_file ("config.json", err);
  if (container == NULL)
    return -1;
  con.bundle = bundle;
  ret = libcrun_container_create (&con, container, 0, err);
  libcrun_container_free (container);
  /* create opens the container's exec fifo (con.fifo_exec_wait_fd) to gate `start`; crun's command line
   * closes it by exiting after the command, so a long-lived driver must close it itself or it leaks one
   * fd per container (start re-opens the fifo from the state). */
  if (con.fifo_exec_wait_fd >= 0)
    close (con.fifo_exec_wait_fd);
  return ret;
}

static int
do_start (const char *id, libcrun_error_t *err)
{
  libcrun_context_t con;
  int ret = context_for (&con, id, err);
  if (ret < 0)
    return ret;
  return libcrun_container_start (&con, id, err);
}

/* Waits on `pid`'s pidfd until it ends, at most CALL_TIMEOUT_S: 1 when it has (a pid that's gone has),
 * 0 when the bound passed first. */
static int
ended_within_bound (pid_t pid, libcrun_error_t *err)
{
  struct pollfd p;
  int fd, r;

  fd = (int) syscall (SYS_pidfd_open, pid, 0);
  if (fd < 0)
    return errno == ESRCH ? 1 : libcrun_make_error (err, errno, "pidfd_open of %d", pid);
  p.fd = fd;
  p.events = POLLIN;
  r = poll (&p, 1, CALL_TIMEOUT_S * 1000);
  close (fd);
  if (r < 0)
    return libcrun_make_error (err, errno, "waiting for %d", pid);
  return r;
}

/* `crun exec <id> /usr/bin/true`: the process crun's exec builds from its arguments, merged with the
 * container's environment. libcrun's exec makes its caller a child subreaper and waits until the caller
 * has no child left (reap_subprocesses: waitpid(-1) until ECHILD). That holds for crun's command line,
 * whose only child is the exec'd process, but here every container's init is the driver's child too, so
 * the wait would never end. So the call runs in a fork of the driver, whose only child is the process it
 * execs, and which exits with that process's status. The fork is part of the call's time. */
static int
do_exec (const char *id, libcrun_error_t *err)
{
  libcrun_context_t con;
  struct libcrun_container_exec_options_s opts;
  runtime_spec_schema_config_schema_process *process;
  char why[512];
  ssize_t got;
  int ret, status, msg[2];
  pid_t helper;

  ret = context_for (&con, id, err);
  if (ret < 0)
    return ret;
  process = calloc (1, sizeof (*process));
  if (process == NULL)
    return libcrun_make_error (err, ENOMEM, "allocating the exec process");
  process->args = calloc (2, sizeof (char *));
  if (process->args == NULL || (process->args[0] = strdup ("/usr/bin/true")) == NULL)
    {
      free_runtime_spec_schema_config_schema_process (process);
      return libcrun_make_error (err, ENOMEM, "allocating the exec process");
    }
  process->args_len = 1;
  memset (&opts, 0, sizeof (opts));
  opts.struct_size = sizeof (opts);
  opts.process = process;
  opts.merge_env = true;
  if (pipe2 (msg, O_CLOEXEC) < 0)
    {
      free_runtime_spec_schema_config_schema_process (process);
      return libcrun_make_error (err, errno, "a pipe for exec's error");
    }
  helper = fork ();
  if (helper < 0)
    {
      free_runtime_spec_schema_config_schema_process (process);
      close (msg[0]);
      close (msg[1]);
      return libcrun_make_error (err, errno, "fork for exec");
    }
  if (helper == 0)
    {
      close (msg[0]);
      ret = libcrun_container_exec_with_options (&con, id, &opts, err);
      if (ret < 0)
        {
          const char *m = (*err && (*err)->msg) ? (*err)->msg : "exec failed";
          _exit (write (msg[1], m, strlen (m)) < 0 ? 254 : 255);
        }
      _exit (ret & 0xff);
    }
  close (msg[1]);
  free_runtime_spec_schema_config_schema_process (process);
  ret = ended_within_bound (helper, err);
  if (ret <= 0)
    {
      kill (helper, SIGKILL);
      waitpid (helper, NULL, 0);
      close (msg[0]);
      return ret < 0 ? ret : libcrun_make_error (err, 0, "exec still running %d s after it began", CALL_TIMEOUT_S);
    }
  if (waitpid (helper, &status, 0) < 0)
    {
      close (msg[0]);
      return libcrun_make_error (err, errno, "reaping exec's fork");
    }
  got = read (msg[0], why, sizeof (why) - 1);
  close (msg[0]);
  if (got > 0)
    {
      why[got] = '\0';
      return libcrun_make_error (err, 0, "%s", why);
    }
  if (! WIFEXITED (status) || WEXITSTATUS (status) != 0)
    return libcrun_make_error (err, 0, "exec of /usr/bin/true ended with wait status %d", status);
  return 0;
}

/* `crun delete --force <id>`: libcrun reads the container's config from its state. */
static int
do_delete (const char *id, libcrun_error_t *err)
{
  libcrun_context_t con;
  int ret = context_for (&con, id, err);
  if (ret < 0)
    return ret;
  return libcrun_container_delete (&con, NULL, id, true, err);
}

/* The container's init, read from its state before delete removes it; 0 when there is none. */
static pid_t
init_pid (const char *id)
{
  libcrun_container_status_t status;
  libcrun_error_t err = NULL;
  pid_t pid;

  memset (&status, 0, sizeof (status));
  if (libcrun_read_container_status (&status, state_root, id, &err) < 0)
    {
      libcrun_error_release (&err);
      return 0;
    }
  pid = status.pid;
  libcrun_free_container_status (&status);
  return pid;
}

/* Reaps a deleted container's init, once it has ended within the bound. One that isn't this process's
 * child (never was, or already reaped) has nothing to reap. */
static int
reap (pid_t pid, libcrun_error_t *err)
{
  int r = ended_within_bound (pid, err);
  if (r < 0)
    return r;
  if (r == 0)
    return libcrun_make_error (err, 0, "init %d still running %d s after its delete", pid, CALL_TIMEOUT_S);
  if (waitpid (pid, NULL, 0) < 0 && errno != ECHILD)
    return libcrun_make_error (err, errno, "reaping init %d", pid);
  return 0;
}

/* One reply line; the message's newlines become spaces, so a reply is always one line. */
static void
reply_err (libcrun_error_t *err)
{
  const char *msg = (*err && (*err)->msg) ? (*err)->msg : "unknown error";
  int status = *err ? (*err)->status : 0;

  fputs ("err ", replies);
  for (const char *c = msg; *c; c++)
    fputc (*c == '\n' ? ' ' : *c, replies);
  if (status)
    fprintf (replies, ": %s", strerror (status));
  fputc ('\n', replies);
  fflush (replies);
  libcrun_error_release (err);
}

int
main (int argc, char **argv)
{
  char line[4096];
  libcrun_error_t err = NULL;
  FILE *requests;

  if (argc == 2 && strcmp (argv[1], "--version") == 0)
    {
      printf ("libcrun-driver: libcrun %s (commit %s), in process\n", PACKAGE_VERSION, GIT_VERSION);
      return 0;
    }
  if (argc != 2)
    {
      fprintf (stderr, "usage: libcrun-driver <state root> (requests on fd 3, replies on fd 4)\n");
      return 2;
    }
  state_root = argv[1];
  /* Neither pipe may reach a container: its init would hold them past this process. */
  if (fcntl (REQUEST_FD, F_SETFD, FD_CLOEXEC) < 0 || fcntl (REPLY_FD, F_SETFD, FD_CLOEXEC) < 0)
    {
      perror ("libcrun-driver: fds 3 and 4");
      return 2;
    }
  requests = fdopen (REQUEST_FD, "r");
  replies = fdopen (REPLY_FD, "w");
  if (requests == NULL || replies == NULL)
    {
      perror ("libcrun-driver: fdopen");
      return 2;
    }
  handlers = libcrun_handler_manager_create (&err);
  if (handlers == NULL)
    {
      reply_err (&err);
      return 1;
    }
  if (libcrun_init_logging (&driver_output_handler, &driver_output_handler_arg, "libcrun-driver", NULL, &err) < 0)
    {
      reply_err (&err);
      return 1;
    }
  while (fgets (line, sizeof (line), requests) != NULL)
    {
      char *save = NULL, *op, *a, *b;
      long long t0, t1, c0, c1;
      pid_t pid = 0;
      int ret;

      line[strcspn (line, "\n")] = '\0';
      op = strtok_r (line, " ", &save);
      a = strtok_r (NULL, " ", &save);
      b = strtok_r (NULL, " ", &save);
      if (op == NULL || a == NULL || (strcmp (op, "create") == 0) != (b != NULL))
        {
          fputs ("err a request is `create <bundle> <id>`, `start <id>`, `exec <id>` or `delete <id>`\n", replies);
          fflush (replies);
          continue;
        }
      if (strcmp (op, "delete") == 0)
        pid = init_pid (a);
      c0 = cpu_ns ();
      t0 = now_ns ();
      if (strcmp (op, "create") == 0)
        ret = do_create (a, b, &err);
      else if (strcmp (op, "start") == 0)
        ret = do_start (a, &err);
      else if (strcmp (op, "exec") == 0)
        ret = do_exec (a, &err);
      else if (strcmp (op, "delete") == 0)
        ret = do_delete (a, &err);
      else
        ret = libcrun_make_error (&err, 0, "unknown request `%s`", op);
      t1 = now_ns ();
      c1 = cpu_ns ();
      if (ret >= 0 && pid > 0)
        ret = reap (pid, &err);
      if (ret < 0)
        reply_err (&err);
      else
        {
          fprintf (replies, "ok %lld %lld\n", t1 - t0, c1 - c0);
          fflush (replies);
        }
    }
  return 0;
}
