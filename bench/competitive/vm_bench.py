#!/usr/bin/env python3
"""bench/competitive: microVM monitors run directly (Firecracker, Cloud Hypervisor), one VM per run.

  vm_bench.py <out.json> <n> <warmup> <lifecycles> <density cap> <density budget MiB> [vmm ...]

The VM analogue of a sandbox running `true`: each run boots one guest, whose init (vm/init.c) ends it
at once, the way its monitor exits on, and lasts from spawning the monitor to its exit. Density boots
guests whose init prints a marker and pauses forever. Both monitors boot the same kernel (the one
Firecracker's getting-started guide fetches; fetch.py) and initramfs, with the same memory, vCPUs and
command line. Each runs as a bare process, without its jailer or API socket: the cheapest way it boots
a VM, which is what a runtime built on it starts from. The harness, its phases and its cgroups are
sandbox_bench.py's; this file says how each monitor boots a VM.
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))  # beside this file even under -I
import sandbox_bench as sb  # noqa: E402

BIN = os.environ.get("HEXBENCH_BIN", "/opt/bin")
VM = os.environ.get("HEXBENCH_VM", "/run/vm")  # vmlinux and init.cpio (lab_inside.sh); the Firecracker configs
MEM_MIB, VCPUS = 128, 1
# Firecracker's documented boot arguments (its getting-started guide), quiet, then after `--` what the
# init does (vm/init.c). Firecracker exits when the guest resets through the keyboard controller
# (reboot=k); Cloud Hypervisor exits when it powers off, and restarts the VM on a reset.
CMDLINE = "console=ttyS0 reboot=k panic=1 quiet"
END = {"firecracker": "reboot", "cloud-hypervisor": "poweroff"}
MARKER = b"hexbench: paused"  # init.c prints it as it pauses


def firecracker(action):
    """Firecracker with the whole VM in a config file (`--no-api`), one per init action, from main()."""
    return [f"{BIN}/firecracker", "--no-api", "--config-file", f"{VM}/firecracker-{action}.json", "--level", "Error"]


def cloud_hypervisor(action):
    """Cloud Hypervisor from its command line; the serial port writes to its stdout, as Firecracker's does."""
    return [f"{BIN}/cloud-hypervisor", "--kernel", f"{VM}/vmlinux", "--initramfs", f"{VM}/init.cpio",
            "--cmdline", f"{CMDLINE} -- {action}", "--cpus", f"boot={VCPUS}", "--memory", f"size={MEM_MIB}M",
            "--serial", "tty", "--console", "off"]


VMMS = {
    "firecracker": (firecracker, "a KVM microVM per run, its own guest kernel; no jailer, no API socket"),
    "cloud-hypervisor": (cloud_hypervisor, "a KVM microVM per run, its own guest kernel; no API socket"),
}


def true_argv(vmm):
    """One VM whose init ends it at once."""
    return VMMS[vmm][0](END[vmm])


def sleep_argv(vmm):
    """One VM whose init pauses forever."""
    return VMMS[vmm][0]("pause")


def paused(_cg, console):
    """A VM is up once its init prints the marker on the serial console, as it pauses."""
    with open(console, "rb") as f:
        return MARKER in f.read()


def main():
    for action in ("reboot", "pause"):
        cfg = {"boot-source": {"kernel_image_path": f"{VM}/vmlinux", "initrd_path": f"{VM}/init.cpio",
                               "boot_args": f"{CMDLINE} -- {action}"},
               "drives": [], "machine-config": {"vcpu_count": VCPUS, "mem_size_mib": MEM_MIB}}
        with open(f"{VM}/firecracker-{action}.json", "w") as f:
            json.dump(cfg, f)
    sb.run(sys.argv[1:], VMMS, true_argv, sleep_argv, paused, lambda v: VMMS[v][1],
           {"guest": {"mem_mib": MEM_MIB, "vcpus": VCPUS, "cmdline": CMDLINE}})


if __name__ == "__main__":
    main()
