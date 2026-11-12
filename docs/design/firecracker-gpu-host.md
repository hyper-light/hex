# A GPU for a Firecracker microVM: the host's part (AC-T7.4.1)

Hex's `firecracker` class passes a selected PCI GPU into a VM through VFIO and never configures the
host to do it: no driver bind, module load or permission change (`firecracker/vfio.rs`). This note
states what the host's administrator provides, the two candidate GPUs on Hex's lab hosts, and what
a run on each would and would not prove. Decided by the native Firecracker lane with Hex's lead,
2026-11-11.

## The contract

The administrator, as root, before the node starts:

1. Turns the IOMMU on (both lab hosts have: AMD-Vi `ivhd0`, 31 groups).
2. Binds every function of the GPU's IOMMU group to `vfio-pci`, and catalogs the GPU with its
   companions (`gpu<TAB>key<TAB>model<TAB>size<TAB>vfio<TAB>sbdf[,companion]<TAB>group`). The
   adapter refuses a function bound to any other driver, and a group holding an uncatalogued one
   (`vfio::validate_function`, `vfio::plan`).
3. Gives `/dev/vfio/vfio` and `/dev/vfio/<group>` read-write to the user running `hex node`
   (`vfio::validate` opens both).
4. Lets that process lock each VM's whole guest memory: `RLIMIT_MEMLOCK` of at least the guest's
   memory, or `CAP_IPC_LOCK`. VFIO type1 pins and charges every DMA-mapped page against that limit
   ([vfio_iommu_type1.c at v7.3](https://github.com/torvalds/linux/blob/v7.3/drivers/vfio/vfio_iommu_type1.c#L770),
   `lock_cap` at L1777), and the VMM maps all guest memory for its devices
   (`vendor/firecracker/src/vmm/src/vfio.rs` `vfio_dma_map_guest_memory`). Both lab hosts give their
   runners' processes 8 MiB.

The guest serves what its VMM attached: the boot line carries the VM's owner and how many GPU
functions were passed (`hex.gpu.box=<owner> hex.gpu.pci=<count>`), the guest's PID 1 catalogs each
display or accelerator function its kernel lists by the guest's own PCI address, and the host joins
that address to its own selection only through the VMM's `GET /device-passthrough/{id}` witness
(`guest/gpu.rs`, `firecracker/observations.rs`).

## The lab's GPUs (read-only probe, 2026-11-11)

| Host | GPU | Function, IOMMU group | Reset | Display |
| --- | --- | --- | --- | --- |
| adacachy2, adacachy | NVIDIA 10de:2d04 | 0000:01:00.0 with audio 01:00.1, group 13 | FLR, bus | boot VGA, DP-3 attached on both |
| adacachy2 | AMD iGPU 1002:13c0 (GC 10.3.6) | 0000:79:00.0 alone, group 25 | bus only | none attached |
| adacachy | AMD iGPU 1002:13c0 | 0000:79:00.0 alone, group 25 | bus only | HDMI-A-2 attached |

No function is bound to `vfio-pci` on either host, and `vfio_pci` is not loaded.

## Option A: the AMD iGPU on adacachy2

```sh
modprobe vfio-pci
echo vfio-pci > /sys/bus/pci/devices/0000:79:00.0/driver_override
echo 0000:79:00.0 > /sys/bus/pci/devices/0000:79:00.0/driver/unbind
echo 0000:79:00.0 > /sys/bus/pci/drivers_probe
chown <node user> /dev/vfio/25
# and RLIMIT_MEMLOCK >= guest memory for the process that runs `hex node`:
# LimitMEMLOCK= in its unit, or prlimit --pid <pid> --memlock=<bytes>:<bytes>
```

Undo: unbind it again, `echo > .../driver_override`, then `echo 0000:79:00.0 >
/sys/bus/pci/drivers_probe` gives it back to amdgpu.

It would prove the attach, the witness, guest amdgpu with its embedded firmware, a RADV compute
dispatch through Hex's adapter, and the selected identity through restart and deletion. It cannot
prove a clean GPU between owners: 79:00.0's only reset is a secondary bus reset, and bus 0x79 also
holds 79:00.1 (audio), 79:00.2 (`ccp`) and 79:00.3-4 (xHCI), so the kernel refuses it
([pci.c at v7.3, `pci_parent_bus_reset`](https://github.com/torvalds/linux/blob/v7.3/drivers/pci/pci.c#L4934))
and vfio-pci proceeds without one
([vfio_pci_core.c](https://github.com/torvalds/linux/blob/v7.3/drivers/vfio/pci/vfio_pci_core.c#L612)).
The guest driver resets no APU at init
([nv.c at v6.18](https://github.com/torvalds/linux/blob/v6.18/drivers/gpu/drm/amd/amdgpu/nv.c#L542)).
Its VBIOS comes from the host. The VMM ignores ROM BAR access (`vendor/firecracker/src/vmm/src/vfio.rs`),
and firmware leaves a copy at the start of BAR0 only "if a discrete card is present"
([amdgpu_bios.c at v6.18](https://github.com/torvalds/linux/blob/v6.18/drivers/gpu/drm/amd/amdgpu/amdgpu_bios.c#L94)),
so GPU run r690's guest found none ("Unable to locate a BIOS ROM"). amdgpu reads the ACPI VFCT table
first. Hex's recorded patch
`vendor/patches/firecracker/0004-vfio-give-a-passthrough-function-s-video-bios-image-to-the.patch`
builds that table from the image file the administrator names for each function in the Firecracker
config (`vbios`: its path and SHA-256, checked at launch as the VMM binary is). Runs r761 and r985
found the image there.

## Option B: the NVIDIA GPU (either host)

The same commands for `0000:01:00.0` and `0000:01:00.1` (both in group 13; catalog the audio
function as the GPU's companion), with `/dev/vfio/13`. The host's display on DP-3 goes dark while
bound, and nothing may hold `/dev/nvidia*` when the driver lets go.

It would prove the same path with a function-level reset between owners. It needs a guest payload
not yet built: NVIDIA's open kernel modules for the guest kernel, its GSP firmware, the matching
userspace driver, and NVML in the guest for Hex's facts.

## One VM per boot for a function no reset clears

Decided by the native Firecracker lane with Hex's lead, 2026-11-11, after GPU run r940: the VM given
79:00.0 after r862's failed at its guest's PSP ring ("PSP create ring failed"). Nothing had reset the
GPU between the two VMs, so the second VM booted into the first one's state. That state, and the first
VM's VRAM, also reached the next tenant.

**The rule.** vfio-pci clears a function when its VM releases it in only two cases:
- A reset method of the function's own, in its `reset_method` list, resets it alone: `flr`, `af_flr`,
  `pm`, `device_specific` or `acpi`. The kernel tries the methods in that list
  ([pci.c at v7.3](https://github.com/torvalds/linux/blob/v7.3/drivers/pci/pci.c#L5197),
  `__pci_reset_function_locked`).
- The function's bus is reset whole, and every other device on that bus goes to the same VM.

**Why a listed `bus` method isn't enough.** The kernel probes the list once, at enumeration
([`pci_init_reset_methods`](https://github.com/torvalds/linux/blob/v7.3/drivers/pci/pci.c#L5245)). A
bus's first function is probed before its siblings are added, so it lists `bus`; 79:00.0 does. At reset
time, a bus reset refuses whenever another device is on the bus
([`pci_parent_bus_reset`](https://github.com/torvalds/linux/blob/v7.3/drivers/pci/pci.c#L4934)).
vfio-pci learns this at open from an actual attempt
([vfio_pci_core.c](https://github.com/torvalds/linux/blob/v7.3/drivers/vfio/pci/vfio_pci_core.c#L611)).
At the last release it resets the bus whole only where every device on the bus is vfio-pci's and unused
([`vfio_pci_dev_set_resettable`](https://github.com/torvalds/linux/blob/v7.3/drivers/vfio/pci/vfio_pci_core.c#L2459)).

**The evidence.** On adacachy2 (7.3.3-1-cachyos) the functions on bus 0x79 list these reset methods:

| Function | Driver | `reset_method` |
| --- | --- | --- |
| 79:00.0 | vfio-pci | `bus` |
| 79:00.1 | snd_hda_intel | `pm` |
| 79:00.2 | ccp | none (no file) |
| 79:00.3 | xhci_hcd | `pm` |
| 79:00.4 | xhci_hcd | `pm` |

So 79:00.0's bus reset is always refused.

**What Hex does** (`firecracker/vfio.rs` `once_per_boot`, `firecracker/single_use.rs`):
- Any other function goes to one VM per host boot.
- Its mark, `vfio-single-use/<sbdf>`, names the boot session the node stamps its records with. It
  is written durably before the VMM that opens the function starts.
- The mark lives in the node's runtime directory, the one every node of its user on the host shares:
  `/run/hex-node` for a root node, `$XDG_RUNTIME_DIR/hex-node` for another user's. So a node with
  another state directory sees it too.
- Only a node that reads where every node able to give the function out marks it gives it out
  (`FirecrackerRuntime::unavailable_once`, after the lead's review of 3cea2fcf):
  - A rootless node with no `$XDG_RUNTIME_DIR` keeps its runtime directory in its own state, so it
    gives no such function to any VM.
  - No node gives out one whose VFIO group interface, `/dev/vfio/<group>`, another user owns. That
    user's nodes mark it in their own runtime directory, and a root node, which can open any group,
    would never read it. So the nodes that can give a function out all read one directory.
  Both are refused by type before anything starts, and capacity lists the function unavailable with
  that reason.
- A later VM on that boot, any node's, is refused by type before anything starts: "no reset method
  ... the host must reboot". This includes a pool's observer, and a box after that observer.
- Capacity still lists the function, and lists it in `unavailable` with that reason. The scheduler
  never offers it, and a spent observer GPU loses its pool's fresh route.
- A later boot's session admits the function again. A boot session that can't be read refuses it.
- Where the mark can be lost, so can its refusal:
  - `$XDG_RUNTIME_DIR` is removed when its user's last session ends, unless that user lingers.
  - An administrator who gives a function's VFIO group to another user on a boot a VM had it: the
    new owner's nodes read their own directory.
- A function a reset clears keeps vfio-pci's reset at release, as before. That reset's outcome is
  invisible to Hex: the kernel reports it only at debug level, and to no process.

**What would falsify this:**
- a second VM on one boot running a function this rule marks single-use with no trace of the first;
- a VM failing on a function this rule clears.

## What would change this

- A guest amdgpu that refuses the VFCT image makes option A need another way to its VBIOS.
- A kernel or firmware that lists a reset of 79:00.0's own makes its GPU go to more than one VM per
  boot.
