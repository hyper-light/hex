/* hex-vulkan-ready-calibrate: PID 1 of a Shards VM that measures what hex-vulkan-guest-ready uses,
 * the census the GPU observer's bounds come from (AC-T7.4.1). scripts/guest/vulkan-ready-manifest.py's
 * `calibrate` boots it with `shards run vm --init`, one VM per GPU and round, given the GPU, the
 * closure vulkan-ready.sh built (virtio-fs tag shards0, read-only) and the round's folder (shards1).
 *
 * The round's folder holds `args`, one `key value` a line: runs, expected, queues, allocation, output,
 * wait-ns, period-us, and cpu-max-milli, memory-max and pids-max, each a number or `-` (unarmed: the
 * measuring rounds). Each run: caches dropped, so the run pays the closure's first touch as the pool
 * VM's one observer run does; a fresh cgroup under the root, bounded as Hex bounds the observer's box
 * where armed (hex_core::cgroup::writes: cpu.max, memory.max with memory.swap.max 0, pids.max); the
 * Program launched into it (clone3's CLONE_INTO_CGROUP) through the closure's dynamic linker, with the
 * Program's arguments and environment as hex-runtime's observer gives them; its stdout and stderr
 * copied to run-N.out and run-N.err; cpu.stat's usage sampled each period; then, once it is reaped,
 * the cgroup's own files written verbatim to run-N.cgroup, and the cgroup removed. A run that exits
 * other than 0 ends the round there. First, guest.txt names the guest the runs had: its kernel's
 * release, its CPUs, its memory (/proc/meminfo's MemTotal) and its most processes (pid_max). At the
 * end, `done` names the runs made, and the VM powers off.
 * Nothing is decided here: the census and its bounds are vulkan-ready-manifest.py's, from these files.
 */
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <limits.h>
#include <linux/sched.h>
#include <poll.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/reboot.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <sys/utsname.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define CLOSURE "/closure"
#define ROUND "/round"
#define CGROUP "/sys/fs/cgroup"

/* The Program's environment, as hex-runtime's observer config gives it. */
#define ICD CLOSURE "/icd/virtio_icd.json"

struct args {
    uint64_t runs, expected, queues, allocation, output, wait_ns, period_us;
    /* Zero where unarmed. */
    uint64_t cpu_max_milli, memory_max, pids_max;
};

static void say(const char *what)
{
    /* The console is the VM's; a lost line loses nothing the round's folder holds. */
    ssize_t ignored = write(STDOUT_FILENO, what, strlen(what));
    (void)ignored;
}

static _Noreturn void stop(const char *why)
{
    say("hex-vulkan-ready-calibrate: ");
    say(why);
    say("\n");
    sync();
    reboot(RB_POWER_OFF);
    _exit(1);
}

static bool write_all(int fd, const char *bytes, size_t count)
{
    while (count != 0) {
        ssize_t wrote = write(fd, bytes, count);
        if (wrote < 0 && errno == EINTR)
            continue;
        if (wrote <= 0 || (size_t)wrote > count)
            return false;
        bytes += (size_t)wrote;
        count -= (size_t)wrote;
    }
    return true;
}

static bool put(const char *path, const char *text)
{
    int fd = open(path, O_WRONLY | O_CLOEXEC);
    if (fd < 0)
        return false;
    bool wrote = write_all(fd, text, strlen(text));
    return close(fd) == 0 && wrote;
}

static uint64_t now_us(void)
{
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0)
        stop("the monotonic clock is unreadable");
    return (uint64_t)now.tv_sec * UINT64_C(1000000) + (uint64_t)now.tv_nsec / UINT64_C(1000);
}

static bool number(const char *text, uint64_t *value)
{
    if (*text < '0' || *text > '9')
        return false;
    uint64_t n = 0;
    for (const char *p = text; *p >= '0' && *p <= '9'; ++p) {
        if (n > (UINT64_MAX - (uint64_t)(*p - '0')) / 10)
            return false;
        n = n * 10 + (uint64_t)(*p - '0');
    }
    *value = n;
    return true;
}

static void read_args(struct args *a)
{
    FILE *file = fopen(ROUND "/args", "re");
    if (file == NULL)
        stop("the round's args are unreadable");
    char key[32], value[32];
    unsigned seen = 0;
    while (fscanf(file, "%31s %31s", key, value) == 2) {
        static const char *const keys[] = {"runs", "expected", "queues", "allocation", "output",
                                           "wait-ns", "period-us", "cpu-max-milli", "memory-max",
                                           "pids-max"};
        uint64_t *slots[] = {&a->runs, &a->expected, &a->queues, &a->allocation, &a->output,
                             &a->wait_ns, &a->period_us, &a->cpu_max_milli, &a->memory_max,
                             &a->pids_max};
        size_t k = 0;
        while (k < sizeof(keys) / sizeof(*keys) && strcmp(key, keys[k]) != 0)
            ++k;
        if (k == sizeof(keys) / sizeof(*keys) || (seen & (1u << k)) != 0)
            stop("the round's args name an unknown or repeated key");
        seen |= 1u << k;
        if (k >= 7 && strcmp(value, "-") == 0)
            *slots[k] = 0;
        else if (!number(value, slots[k]) || *slots[k] == 0)
            stop("the round's args hold a value that is not a positive number");
    }
    fclose(file);
    if (seen != (1u << 10) - 1)
        stop("the round's args lack a key");
}

/* The closure's dynamic linker: lib/ld-linux*, as vulkan-ready.sh copied it. */
static void linker(char *path, size_t room)
{
    DIR *dir = opendir(CLOSURE "/lib");
    if (dir == NULL)
        stop("the closure's lib is unreadable");
    const struct dirent *entry;
    bool found = false;
    while ((entry = readdir(dir)) != NULL) {
        if (strncmp(entry->d_name, "ld-linux", strlen("ld-linux")) == 0) {
            if (found || snprintf(path, room, CLOSURE "/lib/%s", entry->d_name) >= (int)room)
                stop("the closure has no single dynamic linker");
            found = true;
        }
    }
    closedir(dir);
    if (!found)
        stop("the closure has no dynamic linker");
}

/* The first line of `path` that starts with `key`, or its whole first line where `key` is "". */
static bool line_of(const char *path, const char *key, char *line, size_t room)
{
    FILE *file = fopen(path, "re");
    if (file == NULL)
        return false;
    bool found = false;
    while (!found && fgets(line, (int)room, file) != NULL)
        found = strncmp(line, key, strlen(key)) == 0;
    fclose(file);
    return found;
}

static void guest_facts(void)
{
    struct utsname name;
    char pid_max[64], memory[128], text[512];
    long cpus = sysconf(_SC_NPROCESSORS_ONLN);
    if (uname(&name) != 0 || cpus <= 0 || !line_of("/proc/sys/kernel/pid_max", "", pid_max, sizeof(pid_max)) ||
        !line_of("/proc/meminfo", "MemTotal:", memory, sizeof(memory)))
        stop("the guest's own facts are unreadable");
    int n = snprintf(text, sizeof(text), "release %s\ncpus %ld\npid_max %s%s", name.release, cpus, pid_max, memory);
    int fd = open(ROUND "/guest.txt", O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
    if (n < 0 || n >= (int)sizeof(text) || fd < 0 || !write_all(fd, text, (size_t)n) || close(fd) != 0)
        stop("the guest's own facts can't be written");
}

static void mounts(void)
{
    const struct {
        const char *source, *target, *type;
        unsigned long flags;
    } each[] = {
        {"proc", "/proc", "proc", 0},
        {"sysfs", "/sys", "sysfs", 0},
        {"devtmpfs", "/dev", "devtmpfs", 0},
        {"cgroup2", CGROUP, "cgroup2", 0},
        {"shards0", CLOSURE, "virtiofs", MS_RDONLY},
        {"shards1", ROUND, "virtiofs", 0},
    };
    for (size_t i = 0; i < sizeof(each) / sizeof(*each); ++i) {
        if (mkdir(each[i].target, 0755) != 0 && errno != EEXIST)
            stop("a mount point can't be made");
        if (mount(each[i].source, each[i].target, each[i].type, each[i].flags, NULL) != 0 &&
            errno != EBUSY)
            stop("a file system can't be mounted");
    }
    if (!put(CGROUP "/cgroup.subtree_control", "+cpu +memory +pids"))
        stop("the root cgroup can't enable cpu, memory and pids for its children");
}

/* One file of the run's cgroup, verbatim, after its name. A file the kernel lacks says so. */
static bool receipt(int out, const char *dir, const char *name)
{
    char path[PATH_MAX], bytes[4096];
    if (snprintf(path, sizeof(path), "%s/%s", dir, name) >= (int)sizeof(path))
        return false;
    if (!write_all(out, "== ", 3) || !write_all(out, name, strlen(name)) || !write_all(out, "\n", 1))
        return false;
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0)
        return errno == ENOENT && write_all(out, "absent\n", strlen("absent\n"));
    ssize_t got;
    while ((got = read(fd, bytes, sizeof(bytes))) > 0)
        if (!write_all(out, bytes, (size_t)got))
            break;
    return close(fd) == 0 && got == 0;
}

static bool usage_usec(const char *dir, uint64_t *usage)
{
    char path[PATH_MAX], text[4096];
    if (snprintf(path, sizeof(path), "%s/cpu.stat", dir) >= (int)sizeof(path))
        return false;
    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0)
        return false;
    ssize_t got = read(fd, text, sizeof(text) - 1);
    close(fd);
    if (got <= 0)
        return false;
    text[got] = '\0';
    const char *field = strstr(text, "usage_usec ");
    return field == text && number(field + strlen("usage_usec "), usage);
}

static void decimal(char *buffer, size_t room, uint64_t value)
{
    if (snprintf(buffer, room, "%" PRIu64, value) >= (int)room)
        stop("a number outgrew its text");
}

/* Opens `name` in the round's folder for one run's output. */
static int output(uint64_t run, const char *suffix)
{
    char path[PATH_MAX];
    if (snprintf(path, sizeof(path), ROUND "/run-%" PRIu64 ".%s", run, suffix) >= (int)sizeof(path))
        stop("a run's file name outgrew its room");
    int fd = open(path, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
    if (fd < 0)
        stop("a run's file can't be made");
    return fd;
}

static bool armed(const char *dir, const struct args *a)
{
    char path[PATH_MAX], value[64];
    if (a->cpu_max_milli != 0) {
        /* hex_core::cgroup::writes: a thousandth of a CPU is a thousandth of the period's quota. */
        if (a->cpu_max_milli > UINT64_MAX / a->period_us ||
            snprintf(value, sizeof(value), "%" PRIu64 " %" PRIu64,
                     a->cpu_max_milli * a->period_us / 1000, a->period_us) >= (int)sizeof(value) ||
            snprintf(path, sizeof(path), "%s/cpu.max", dir) >= (int)sizeof(path) || !put(path, value))
            return false;
    }
    if (a->memory_max != 0) {
        decimal(value, sizeof(value), a->memory_max);
        if (snprintf(path, sizeof(path), "%s/memory.max", dir) >= (int)sizeof(path) || !put(path, value))
            return false;
        /* A guest without swap has no memory.swap.max: then there is nothing to hold. */
        if (snprintf(path, sizeof(path), "%s/memory.swap.max", dir) >= (int)sizeof(path) ||
            (!put(path, "0") && errno != ENOENT))
            return false;
    }
    if (a->pids_max != 0) {
        decimal(value, sizeof(value), a->pids_max);
        if (snprintf(path, sizeof(path), "%s/pids.max", dir) >= (int)sizeof(path) || !put(path, value))
            return false;
    }
    return true;
}

/* Copies what one pipe holds now, a pipe's atomic write unit (PIPE_BUF) a read: false at its end. */
static bool copy(int from, int to)
{
    char bytes[PIPE_BUF];
    for (;;) {
        ssize_t got = read(from, bytes, sizeof(bytes));
        if (got > 0) {
            if (!write_all(to, bytes, (size_t)got))
                stop("a run's output can't be written");
            continue;
        }
        if (got == 0)
            return false;
        if (errno == EINTR)
            continue;
        if (errno == EAGAIN)
            return true;
        stop("a run's output can't be read");
    }
}

/* One run: returns its wait status. */
static int run(uint64_t index, const struct args *a, char *const argv[])
{
    char dir[PATH_MAX], line[256];
    if (snprintf(dir, sizeof(dir), CGROUP "/run-%" PRIu64, index) >= (int)sizeof(dir))
        stop("a run's cgroup name outgrew its room");
    sync();
    if (!put("/proc/sys/vm/drop_caches", "3"))
        stop("the page cache can't be dropped");
    if (mkdir(dir, 0755) != 0)
        stop("a run's cgroup can't be made");
    if (!armed(dir, a))
        stop("a run's cgroup can't be bounded");
    int cgroup = open(dir, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    int out[2], err[2];
    if (cgroup < 0 || pipe2(out, O_CLOEXEC | O_NONBLOCK) != 0 || pipe2(err, O_CLOEXEC | O_NONBLOCK) != 0)
        stop("a run's cgroup or pipes can't be opened");
    int saved_out = output(index, "out"), saved_err = output(index, "err");
    char *const envp[] = {"VK_DRIVER_FILES=" ICD, "VK_LOADER_LAYERS_DISABLE=~all~", NULL};
    int pidfd = -1;
    struct clone_args launch = {
        .flags = CLONE_INTO_CGROUP | CLONE_PIDFD,
        .pidfd = (__u64)(uintptr_t)&pidfd,
        .exit_signal = SIGCHLD,
        /* Open above, so not negative. */
        .cgroup = (__u64)(unsigned int)cgroup,
    };
    uint64_t start = now_us();
    long pid = syscall(SYS_clone3, &launch, sizeof(launch));
    if (pid == 0) {
        int null = open("/dev/null", O_RDONLY);
        if (null < 0 || dup2(null, STDIN_FILENO) < 0 || dup2(out[1], STDOUT_FILENO) < 0 ||
            dup2(err[1], STDERR_FILENO) < 0)
            _exit(127);
        execve(argv[0], argv, envp);
        _exit(127);
    }
    if (pid < 0)
        stop("the Program can't be launched into its cgroup");
    close(out[1]);
    close(err[1]);
    close(cgroup);
    /* cpu.stat's usage at each period's end: the run's receipt keeps every sample. */
    int receipts = output(index, "cgroup");
    uint64_t next = start + a->period_us;
    bool out_open = true, err_open = true, exited = false;
    while (out_open || err_open || !exited) {
        uint64_t now = now_us();
        if (now >= next) {
            uint64_t usage;
            if (!usage_usec(dir, &usage))
                stop("a run's cpu.stat is unreadable");
            int n = snprintf(line, sizeof(line), "sample %" PRIu64 " %" PRIu64 "\n", now - start, usage);
            if (n < 0 || n >= (int)sizeof(line) || !write_all(receipts, line, (size_t)n))
                stop("a run's sample can't be written");
            next += a->period_us;
            continue;
        }
        struct pollfd fds[] = {
            {.fd = out_open ? out[0] : -1, .events = POLLIN},
            {.fd = err_open ? err[0] : -1, .events = POLLIN},
            {.fd = exited ? -1 : pidfd, .events = POLLIN},
        };
        uint64_t wait_ms = (next - now + 999) / 1000;
        int ready = poll(fds, 3, wait_ms > INT_MAX ? INT_MAX : (int)wait_ms);
        if (ready < 0 && errno != EINTR)
            stop("waiting on a run failed");
        if (ready <= 0)
            continue;
        if (out_open && (fds[0].revents & (POLLIN | POLLHUP)) != 0)
            out_open = copy(out[0], saved_out);
        if (err_open && (fds[1].revents & (POLLIN | POLLHUP)) != 0)
            err_open = copy(err[0], saved_err);
        if (!exited && (fds[2].revents & POLLIN) != 0)
            exited = true;
    }
    siginfo_t info = {0};
    if (waitid(P_PIDFD, (id_t)pidfd, &info, WEXITED) != 0)
        stop("a run can't be reaped");
    uint64_t end = now_us(), usage;
    if (!usage_usec(dir, &usage))
        stop("a run's cpu.stat is unreadable");
    int status = info.si_code == CLD_EXITED ? info.si_status : 128 + info.si_status;
    int n = snprintf(line, sizeof(line),
                     "sample %" PRIu64 " %" PRIu64 "\nexit %d\nwall_usec %" PRIu64 "\n",
                     end - start, usage, status, end - start);
    if (n < 0 || n >= (int)sizeof(line) || !write_all(receipts, line, (size_t)n))
        stop("a run's end can't be written");
    static const char *const files[] = {"cpu.stat", "cpu.max", "memory.peak", "memory.max",
                                        "memory.swap.max", "memory.events", "pids.peak", "pids.max",
                                        "pids.events"};
    for (size_t i = 0; i < sizeof(files) / sizeof(*files); ++i)
        if (!receipt(receipts, dir, files[i]))
            stop("a run's cgroup files can't be kept");
    if (close(receipts) != 0 || close(saved_out) != 0 || close(saved_err) != 0)
        stop("a run's files can't be closed");
    close(out[0]);
    close(err[0]);
    close(pidfd);
    if (rmdir(dir) != 0)
        stop("a run's cgroup can't be removed");
    return status;
}

int main(void)
{
    if (getpid() != 1)
        stop("not the VM's init");
    mounts();
    guest_facts();
    struct args a = {0};
    read_args(&a);
    char path[PATH_MAX];
    linker(path, sizeof(path));
    char expected[24], queues[24], allocation[24], output_bytes[24], wait_ns[24];
    decimal(expected, sizeof(expected), a.expected);
    decimal(queues, sizeof(queues), a.queues);
    decimal(allocation, sizeof(allocation), a.allocation);
    decimal(output_bytes, sizeof(output_bytes), a.output);
    decimal(wait_ns, sizeof(wait_ns), a.wait_ns);
    /* hex-runtime's observer: its launch prefix, then the Program's six flags in its order. */
    char *const argv[] = {path, "--library-path", CLOSURE "/lib", CLOSURE "/bin/hex-vulkan-guest-ready",
                          "--expected-devices", expected, "--max-queue-families", queues,
                          "--max-allocation-bytes", allocation, "--max-output-bytes", output_bytes,
                          "--icd", ICD, "--wait-ns", wait_ns, NULL};
    uint64_t made = 0;
    while (made < a.runs) {
        int status = run(made + 1, &a, argv);
        ++made;
        if (status != 0)
            break;
    }
    char done[64];
    int n = snprintf(done, sizeof(done), "runs %" PRIu64 "\n", made);
    int fd = open(ROUND "/done", O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0644);
    if (n < 0 || n >= (int)sizeof(done) || fd < 0 || !write_all(fd, done, (size_t)n) || close(fd) != 0)
        stop("the round's end can't be written");
    stop("round complete");
}
