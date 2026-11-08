// A co-tenant's mount stall across a node's start (goal 6 lever (4)'s A/B, goal6-mntns.md §12): a process outside
// Hex, in a user and mount namespace of its own (no root needed), mounts and unmounts a tmpfs in a loop for SECONDS
// about every 200 us, and writes each pair's start (CLOCK_MONOTONIC, the trace's `mono` clock, us) and length (us), one line each. Every
// mount and umount takes the kernel's one mount lock for write (v6.19 fs/namespace.c), so a long hold elsewhere
// shows as a long pair here. Usage: mount_cotenant SECONDS OUT
#define _GNU_SOURCE
#include <fcntl.h>
#include <sched.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

static long long now_us(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (long long)t.tv_sec * 1000000 + t.tv_nsec / 1000;
}

static int write_file(const char *path, const char *text) {
    int fd = open(path, O_WRONLY | O_CLOEXEC);
    if (fd < 0) return -1;
    ssize_t n = write(fd, text, strlen(text));
    close(fd);
    return n == (ssize_t)strlen(text) ? 0 : -1;
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: mount_cotenant SECONDS OUT\n");
        return 2;
    }
    long seconds = strtol(argv[1], NULL, 10);
    FILE *out = fopen(argv[2], "w");
    if (!out || seconds <= 0 || seconds > 600) {
        fprintf(stderr, "refusing: an output file and 1-600 seconds\n");
        return 2;
    }
    uid_t uid = getuid();
    gid_t gid = getgid();
    if (unshare(CLONE_NEWUSER | CLONE_NEWNS) != 0) {
        perror("unshare");
        return 3;
    }
    char map[64];
    snprintf(map, sizeof map, "0 %u 1", uid);
    if (write_file("/proc/self/setgroups", "deny") != 0 || write_file("/proc/self/uid_map", map) != 0) {
        perror("uid_map");
        return 3;
    }
    snprintf(map, sizeof map, "0 %u 1", gid);
    if (write_file("/proc/self/gid_map", map) != 0) {
        perror("gid_map");
        return 3;
    }
    if (mount(NULL, "/", NULL, MS_REC | MS_PRIVATE, NULL) != 0) {
        perror("make / private");
        return 3;
    }
    const char *dir = "/tmp";
    long long end = now_us() + seconds * 1000000LL, pairs = 0;
    // Bounded by the deadline and by a count, so a stuck clock can't run it forever (CLAUDE.md §2).
    while (now_us() < end && pairs < 100000000LL) {
        long long t0 = now_us();
        if (mount("cotenant", dir, "tmpfs", 0, "size=4k") != 0) {
            perror("mount");
            return 4;
        }
        if (umount2(dir, 0) != 0) {
            perror("umount");
            return 4;
        }
        fprintf(out, "%lld %lld\n", t0, now_us() - t0);
        pairs++;
        // A sample every ~200 us, not a saturating loop: the co-tenant measures the lock, it doesn't load it.
        struct timespec gap = {0, 200000};
        nanosleep(&gap, NULL);
    }
    fclose(out);
    return 0;
}
