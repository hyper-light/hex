// devfs_clone.c: what a box's /dev costs made fresh, as step 3's pool makes it, against handed on within one owner,
// cloned from a kept original and checked after (goal 5 extended, step 6; the lead's 2026-11-03T17:47Z).
//
// One thread, pinned to the CPU given, as root (fsopen and open_tree's clone need CAP_SYS_ADMIN over the caller's
// mount namespace). Each iteration times, in nanoseconds:
//   fresh:  fsopen("tmpfs"), fsconfig of runc's /dev options (mode=755, size=65536k), FSCONFIG_CMD_CREATE, fsmount
//           (nosuid, strictatime), the six default nodes (mknodat, then fchmodat to their mode), the five default
//           links (symlinkat): init.rs's prepare_dev; then the box's three mountpoints (mkdirat pts, shm, mqueue);
//           then close, the mount's last reference, which tears its superblock down.
//   handon: open_tree(original, OPEN_TREE_CLONE), the box's three mountpoints in it, close of the clone; then the
//           check, through the original: getdents64 of the root, statx and listxattrat of every entry and of the
//           root, readlinkat of each link, and the mountpoints removed (unlinkat AT_REMOVEDIR).
// Prints each step's p50, p99 and mean, and the whole of each path.
//   devfs_clone <cpu> <iterations>
#define _GNU_SOURCE
#include <dirent.h>
#include <errno.h>
#include <fcntl.h>
#include <sched.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/sysmacros.h>
#include <sys/xattr.h>
#ifndef SYS_listxattrat
#define SYS_listxattrat 465
#endif
#include <time.h>
#include <unistd.h>

static const struct { const char *name; unsigned major, minor; mode_t mode; } DEVICES[] = {
    {"null", 1, 3, 0666}, {"zero", 1, 5, 0666}, {"full", 1, 7, 0666},
    {"random", 1, 8, 0666}, {"urandom", 1, 9, 0666}, {"tty", 5, 0, 0666},
};
static const struct { const char *target, *name; } LINKS[] = {
    {"/proc/self/fd", "fd"}, {"/proc/self/fd/0", "stdin"}, {"/proc/self/fd/1", "stdout"},
    {"/proc/self/fd/2", "stderr"}, {"pts/ptmx", "ptmx"},
};
static const char *MOUNTPOINTS[] = {"pts", "shm", "mqueue"};
#define N_DEV (sizeof DEVICES / sizeof DEVICES[0])
#define N_LINK (sizeof LINKS / sizeof LINKS[0])
#define N_MP (sizeof MOUNTPOINTS / sizeof MOUNTPOINTS[0])

static uint64_t now(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (uint64_t)t.tv_sec * 1000000000u + (uint64_t)t.tv_nsec;
}

static void die(const char *what) {
    fprintf(stderr, "devfs_clone: %s: %s\n", what, strerror(errno));
    exit(1);
}

enum { MAKE, NODES, LINKS_, POINTS, CLOSE, FRESH, CLONE, CPOINTS, CCLOSE, SCAN, RMDIRS, HANDON, STEPS };
static const char *STEP[STEPS] = {
    "fresh: fsopen..fsmount", "fresh: six nodes", "fresh: five links", "fresh: three mountpoints",
    "fresh: close (teardown)", "fresh, whole", "handon: open_tree clone", "handon: three mountpoints",
    "handon: close of the clone", "handon: check (dents, statx, xattrs, links)", "handon: mountpoints removed",
    "handon, whole",
};

static int make_fs(void) {
    int fs = fsopen("tmpfs", FSOPEN_CLOEXEC);
    if (fs < 0) die("fsopen");
    if (fsconfig(fs, FSCONFIG_SET_STRING, "source", "tmpfs", 0) < 0) die("fsconfig source");
    if (fsconfig(fs, FSCONFIG_SET_STRING, "mode", "755", 0) < 0) die("fsconfig mode");
    if (fsconfig(fs, FSCONFIG_SET_STRING, "size", "65536k", 0) < 0) die("fsconfig size");
    if (fsconfig(fs, FSCONFIG_CMD_CREATE, NULL, NULL, 0) < 0) die("fsconfig create");
    int mnt = fsmount(fs, FSMOUNT_CLOEXEC, MOUNT_ATTR_NOSUID | MOUNT_ATTR_STRICTATIME);
    if (mnt < 0) die("fsmount");
    close(fs);
    return mnt;
}

static void defaults(int root) {
    for (size_t i = 0; i < N_DEV; i++) {
        if (mknodat(root, DEVICES[i].name, S_IFCHR | DEVICES[i].mode, makedev(DEVICES[i].major, DEVICES[i].minor)) < 0)
            die("mknodat");
        if (fchmodat(root, DEVICES[i].name, DEVICES[i].mode, 0) < 0) die("fchmodat");
    }
}

static void links(int root) {
    for (size_t i = 0; i < N_LINK; i++)
        if (symlinkat(LINKS[i].target, root, LINKS[i].name) < 0) die("symlinkat");
}

static void points(int root) {
    for (size_t i = 0; i < N_MP; i++)
        if (mkdirat(root, MOUNTPOINTS[i], 0755) < 0) die("mkdirat");
}

// The check through the original: every entry's statx and xattrs, each link's target, the root's own.
static void check(int root) {
    char buf[4096], names[256], target[256];
    struct statx sx;
    if (statx(root, "", AT_EMPTY_PATH, STATX_BASIC_STATS, &sx) < 0) die("statx root");
    int dir = openat(root, ".", O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (dir < 0) die("openat root");
    for (;;) {
        long n = syscall(SYS_getdents64, dir, buf, sizeof buf);
        if (n < 0) die("getdents64");
        if (n == 0) break;
        for (long off = 0; off < n;) {
            struct dirent64 *d = (struct dirent64 *)(buf + off);
            off += d->d_reclen;
            if (!strcmp(d->d_name, ".") || !strcmp(d->d_name, "..")) continue;
            if (statx(dir, d->d_name, AT_SYMLINK_NOFOLLOW, STATX_BASIC_STATS, &sx) < 0) die("statx");
            // listxattrat (Linux 6.13): an entry's xattrs by its name in the directory, a device node's not opened.
            if (syscall(SYS_listxattrat, dir, d->d_name, AT_SYMLINK_NOFOLLOW, names, sizeof names) < 0 &&
                errno != ENOTSUP)
                die("listxattrat");
            if (S_ISLNK(sx.stx_mode) && readlinkat(dir, d->d_name, target, sizeof target) < 0) die("readlinkat");
        }
    }
    close(dir);
}

static void rmdirs(int root) {
    for (size_t i = 0; i < N_MP; i++)
        if (unlinkat(root, MOUNTPOINTS[i], AT_REMOVEDIR) < 0) die("unlinkat");
}

static int cmp(const void *a, const void *b) {
    uint64_t x = *(const uint64_t *)a, y = *(const uint64_t *)b;
    return x < y ? -1 : x > y;
}

int main(int argc, char **argv) {
    if (argc != 3) { fprintf(stderr, "usage: devfs_clone <cpu> <iterations>\n"); return 2; }
    int cpu = atoi(argv[1]);
    long iters = atol(argv[2]);
    if (iters <= 0 || iters > 10000000) { fprintf(stderr, "devfs_clone: iterations out of range\n"); return 2; }
    cpu_set_t set;
    CPU_ZERO(&set);
    CPU_SET(cpu, &set);
    if (sched_setaffinity(0, sizeof set, &set) < 0) die("sched_setaffinity");
    uint64_t *t[STEPS];
    for (int s = 0; s < STEPS; s++)
        if (!(t[s] = calloc((size_t)iters, sizeof(uint64_t)))) die("calloc");
    int original = make_fs();
    defaults(original);
    links(original);
    for (long i = 0; i < iters; i++) {
        uint64_t a = now();
        int m = make_fs();
        uint64_t b = now();
        defaults(m);
        uint64_t c = now();
        links(m);
        uint64_t d = now();
        points(m);
        uint64_t e = now();
        close(m);
        uint64_t f = now();
        int clone = open_tree(original, "", OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC | AT_EMPTY_PATH);
        if (clone < 0) die("open_tree clone");
        uint64_t g = now();
        points(clone);
        uint64_t h = now();
        close(clone);
        uint64_t j = now();
        check(original);
        uint64_t k = now();
        rmdirs(original);
        uint64_t l = now();
        t[MAKE][i] = b - a; t[NODES][i] = c - b; t[LINKS_][i] = d - c; t[POINTS][i] = e - d; t[CLOSE][i] = f - e;
        t[FRESH][i] = f - a; t[CLONE][i] = g - f; t[CPOINTS][i] = h - g; t[CCLOSE][i] = j - h; t[SCAN][i] = k - j;
        t[RMDIRS][i] = l - k; t[HANDON][i] = l - f;
    }
    printf("devfs_clone: cpu %d, %ld iterations; ns: p50 p99 mean\n", cpu, iters);
    for (int s = 0; s < STEPS; s++) {
        double sum = 0;
        for (long i = 0; i < iters; i++) sum += (double)t[s][i];
        qsort(t[s], (size_t)iters, sizeof(uint64_t), cmp);
        printf("  %-46s %8llu %8llu %10.0f\n", STEP[s], (unsigned long long)t[s][iters / 2],
               (unsigned long long)t[s][(iters * 99) / 100], sum / (double)iters);
    }
    return 0;
}
