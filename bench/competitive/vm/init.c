/* bench/competitive: the guest's init for vm_bench.py, the VM's only process. The kernel hands it the
 * words after `--` on its command line: "reboot" or "poweroff" ends the VM at once (the event its
 * monitor exits on); "pause" prints vm_bench.py's marker on the console and holds the VM forever. */
#include <string.h>
#include <sys/reboot.h>
#include <unistd.h>

int main(int argc, char **argv) {
    const char *action = argc > 1 ? argv[1] : "";
    if (strcmp(action, "pause") == 0) {
        static const char marker[] = "hexbench: paused\n";
        if (write(1, marker, sizeof marker - 1) < 0)
            return 1;
        for (;;)
            pause();
    }
    if (strcmp(action, "reboot") == 0)
        reboot(RB_AUTOBOOT);
    else if (strcmp(action, "poweroff") == 0)
        reboot(RB_POWER_OFF);
    else {
        static const char refused[] = "hexbench init: no action after --\n";
        (void)!write(2, refused, sizeof refused - 1);
    }
    /* Init returning panics the kernel; panic=1 then resets the VM, which the run's timeout bounds. */
    return 1;
}
