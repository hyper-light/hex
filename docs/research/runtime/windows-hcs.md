# The Windows `oci` class: Windows images, process-isolated, through the Host Compute Service

Source notes for the native plan's step 4 (kiln plan `01a194669ce477718e3b3b8c7be44138`): the `oci` class for
Windows images on Windows, beneath the native-platforms decision (`docs/design/runtime.md`) and its own decision
("the Windows `oci` class: an HCS silo holding a Hex job"). They say what the OCI runtime spec's Windows section
asks, how the Host Compute Service (HCS) gives it, how hcsshim, containerd's runhcs shim and Docker drive HCS, and
what each piece refuses, so the decision rests on them (CLAUDE.md §5).

Sources were read at fixed revisions; `path:line` is at that revision, and each quote is one line of its source,
copied exactly (some shortened with "…" inside the line):

- `runtime-spec/` — opencontainers/runtime-spec `469dd5b6233c9d0137d16be792978aebb302666e`.
- `hcsshim/` — microsoft/hcsshim `b1c1f43a3a6fb308cf3b1ba45affa835a693695c`.
- `VD/` — MicrosoftDocs/Virtualization-Documentation `85e96f5075e548ca24bc252e9f5ac0f39c318ef8` (`VD/hcs/` is its
  `virtualization/api/hcs/`, `VD/wc/` its `virtualization/windowscontainers/`).
- `moby/` — moby/moby `a2f4aa404016b392bebc44265ef4c30d92544e53`.
- `containerd/` — containerd/containerd `8a9c72699b5a29daa94cbf1bfe270f1bd1b52d12`.
- `win32/` — MicrosoftDocs/win32 `ca8274decead49403d70ceeed4af8d4ca74eaf11`; `sdk-api/` — MicrosoftDocs/sdk-api
  `656e9656bfaef34dd29cd7ee9e1146aec17e924c` (the revisions step 3's note read).
- `windows-sys/` — the `windows-sys` crate 0.61.2 Hex already links (crates.io), and `windows-link/` 0.2.1 beside it.

What only a Windows host can settle is marked **(probe)** and is measured on jupiter (Windows 11 Pro 25H2, build
26200) before the class claims it; §9 lists those.

## 1. What the OCI runtime spec asks of a Windows container

- It is HCS's to give (`runtime-spec/config-windows.md:4`): "The Windows container specification uses APIs
  provided by the Windows Host Compute Service (HCS) to fulfill the spec."
- Process isolation is the default (`config-windows.md:201`): "If omitted, the container MUST be run as a Windows
  Server container." — that is, a server silo on the host's kernel, not a Hyper-V utility VM.
- Layers (`config-windows.md:8`): "The list is ordered from topmost layer to base layer with the last entry being
  the scratch." (`:9`) "`layerFolders` MUST contain at least one entry."
- The root (`runtime-spec/config.md:36`): "On Windows, `path` MUST be a [volume GUID path][naming-a-volume]." and
  (`:44`) "On Windows, this field MUST be omitted or false." (`readonly`).
- Devices (`config-windows.md:31`): "Today, Windows only supports a value of `class`, which identifies `id` as a
  [device interface class GUID][interfaceGUID]." — a grant is by interface class, nothing else.
- Mounts: destination (`config.md:74`) "Windows: This value MUST be an absolute path." and (`:75`) "One mount
  destination MUST NOT be nested within another mount (e.g., c:\\foo and c:\\foo\\bar)."; source (`:82`) "Windows: a
  local directory on the filesystem of the container host. UNC paths and mapped drives are not supported."; options
  (`:87`) "Windows: runtimes MUST support `ro`, mounting the filesystem read-only when `ro` is given."
- Memory (`config-windows.md:62`): "**`limit`** *(uint64, OPTIONAL)* - sets limit of memory usage in bytes."
- CPU (`config-windows.md:80`): "The following parameters can be specified (mutually exclusive):" — `count`,
  `shares` (`:83`, "a value between 0 and 10,000"), `maximum` (`:84`, "the number of cycles per 10,000 cycles").
- The user (`config.md:474`): "**`username`** (string, OPTIONAL) specifies the user name for the process." — a
  Windows process runs as a named account, not a uid and gid.

## 2. HCS v2: the compute system, its document, its operations

- hcsshim's Argon create is HCS v2 through `computecore.dll` (`hcsshim/internal/hcsoci/create.go:21` imports
  `internal/hcs/v2`; `hcsshim/internal/hcs/v2/system.go:15` imports `internal/computecore`). Moby's Windows driver
  still uses the v1 `vmcompute.dll` schema (`moby/daemon/internal/libcontainerd/local/local_windows.go:177-186`).
- The document hcsshim sends for a process-isolated container (`hcsshim/internal/hcsoci/create.go:319-325`): `Owner`,
  `SchemaVersion` (`schemaversion.SchemaV21()`, `{Major: 2, Minor: 1}`, `hcsshim/internal/schemaversion/schemaversion.go:21-22`),
  `ShouldTerminateOnLastHandleClosed: true` and `Container`. No `HostingSystemId` (Argon is "no hosting system",
  `create.go:389-391`). The id is the create call's argument, not a field (`hcsdoc_wcow.go:164`).
- `ShouldTerminateOnLastHandleClosed` (`VD/hcs/SchemaReference.md:1353`): "If true, this system will be forcibly
  terminated when the last HCS_SYSTEM handle corresponding to it is closed." — so a node agent that dies takes its
  containers with it, as a job's kill-on-close does for the `process` class.
- `Container` (`hcsshim/internal/hcsoci/hcsdoc_wcow.go:143-521`; schema `VD/hcs/SchemaReference.md:1412-1431`):
  - `Storage{Path, Layers[{Id, Path}]}` — `Path` the scratch's volume GUID path (`hcsdoc_wcow.go:316-324`, refusing
    anything else: "Root.Path '%s' must be a volume GUID path in the format '\\?\Volume{GUID}\'"), `Layers` every
    read-only layer with its id (`:364-371`). HCS mounts nothing: the caller prepares the scratch volume first (§3).
  - `MappedDirectories[{HostPath, ContainerPath, ReadOnly}]` from the spec's mounts, `ReadOnly` for `ro`
    (`hcsdoc_wcow.go:53-60`). For process isolation the host path is resolved first, a symlink in it being HCS's to
    mis-resolve (`:62`): "HCS has a bug where it does not correctly resolve file (not dir) paths".
  - `AssignedDevices[{Type, InterfaceClassGuid}]` (`hcsdoc_wcow.go:525-549`; `VD/hcs/SchemaReference.md:1714`:
    `InterfaceClassGuid` "Only used when Type is ClassGuid").
  - `Memory{SizeInMB}` (`hcsdoc_wcow.go:255-257`) and `Processor{Count, Maximum, Weight}` (`:236-240`), mutually
    exclusive (`VD/hcs/SchemaReference.md:1464`). `Root.Readonly` is refused (`hcsdoc_wcow.go:306-307`):
    "readonly is not supported for Windows containers".
  - `GuestOs{HostName}` (`hcsdoc_wcow.go:167-170`).
- Every call is an operation: `HcsCreateOperation`, the call, then `HcsWaitForOperationResult(op, timeoutMs,
  &resultDocument)` (`VD/hcs/Reference/HcsWaitForOperationResult.md:27-32`), which on failure "can optionally receive
  an error JSON document" (`:50`). A timed-out wait is cancelled with `HcsCancelOperation`, never abandoned: an
  abandoned call "has been observed to crash the shim with EXCEPTION_ACCESS_VIOLATION"
  (`hcsshim/internal/computecore/computecore.go:104`), inside `computecore.dll` (`:105`). The result string is the caller's to
  free with `LocalFree` (`HcsWaitForOperationResult.md:53`; hcsshim uses `CoTaskMemFree`,
  `hcsshim/internal/interop/interop.go:12-16` — Hex follows the documentation).
- Lifecycle: `HcsCreateComputeSystem` → `HcsStartComputeSystem` (`VD/hcs/Reference/HcsStartComputeSystem.md:27-47`);
  `HcsShutDownComputeSystem` "Cleanly shuts down" and `HcsTerminateComputeSystem` "Forcefully terminates"
  (`HcsShutDownComputeSystem.md:22`, `HcsTerminateComputeSystem.md:22`); `HcsCloseComputeSystem`.
  `HCS_E_SYSTEM_ALREADY_STOPPED` answers a second stop (`HcsShutDownComputeSystem.md:63`).
- Leftovers are found by owner: `HcsEnumerateComputeSystems` with a `SystemQuery{Owners}` (`VD/hcs/SchemaReference.md:3352-3355`).
- The errors (`VD/hcs/Reference/HCSHResult.md:27-56`, bound in `windows-sys/src/Windows/Win32/Foundation/mod.rs:5170-5201`),
  among them `HCS_E_IMAGE_MISMATCH` 0x80370101 "The container operating system does not match the host operating
  system", `HCS_E_SERVICE_NOT_AVAILABLE` 0x80370114, `HCS_E_SYSTEM_ALREADY_STOPPED` 0x80370110,
  `HCS_E_OPERATION_TIMEOUT` 0x80370118. The legacy path surfaces the same codes NTSTATUS-shaped (0xC037xxxx,
  `hcsshim/internal/hcs/v2/errors.go:18,60`), and the image mismatch is documented as `(0xc0370101)`
  (`VD/wc/deploy-containers/version-compatibility.md:159`), so both forms are matched.

## 3. Layers: from folders to a scratch volume

- hcsshim's process-isolated path (`hcsshim/internal/layers/wcow_mount.go:128-217`): create the scratch's
  `sandbox.vhdx` if absent, `ActivateLayer`, `PrepareLayer(scratch, parents)` (attaches the WCIFS filter over the
  parents), `GetLayerMountPath` (the `\\?\Volume{GUID}\` root), and a layer id per parent from `NameToGuid` of the
  folder's name (`hcsshim/internal/wclayer/layerid.go:22-23`). Teardown is `UnprepareLayer` then `DeactivateLayer`
  (`wcow_mount.go:121-126`). Every one of these is an **undocumented** `vmcompute.dll` export
  (`hcsshim/internal/wclayer/wclayer.go:9-27`), not bound by windows-sys.
- hcsshim wraps it in a timed retry for a Server 2019 bug (`wcow_mount.go:142,164-170`, "On 19h1+ this *shouldn't*
  be needed", `:140`) and a process-wide lock (`hcsshim/internal/wclayer/preparelayer.go:37`): "This lock is a
  temporary workaround for a Windows bug." Hex copies neither (CLAUDE.md §4): a failure is refused, typed.
- The **documented** storage API is `computestorage.dll` (`VD/hcs/Reference/APIOverview.md:132-149`):
  - `HcsFormatWritableLayerVhd` "creates and formats a partition that is intended to be used as a writable layer"
    (`HcsFormatWritableLayerVhd.md:22`);
  - `HcsGetLayerVhdMountPath` "Returns the volume path for a virtual disk of a writable container layer"
    (`HcsGetLayerVhdMountPath.md:22`), freed with `LocalFree` (`:42`);
  - `HcsInitializeWritableLayer` "initializes the writable layer for a container (e.g. the layer that captures the
    filesystem and registry changes caused by executing the container)" (`HcsInitializeWritableLayer.md:22`);
  - `HcsAttachLayerStorageFilter` "sets up the container storage filter on a layer directory. The storage filter
    provides the unified view to the layer and its antecedent layers." (`HcsAttachLayerStorageFilter.md:22`), and
    `HcsDetachLayerStorageFilter` undoes it (`HcsDetachLayerStorageFilter.md:22`).
  - Their `layerData` is `LayerData{SchemaVersion, Layers[{Id, Path, PathType}]}` (`VD/hcs/SchemaReference.md:2252-2259`).
  - All are bound by windows-sys from `computestorage.dll` (`windows-sys/src/Windows/Win32/System/HostComputeSystem/mod.rs:2,18,23,26,41`),
    and the virtual-disk calls from `virtdisk.dll` (`windows-sys/src/Windows/Win32/Storage/Vhd/mod.rs:4,10,13,31`,
    `ATTACH_VIRTUAL_DISK_FLAG_NO_DRIVE_LETTER` `:77`).
- **No source shows the documented sequence end to end** — nothing in hcsshim, containerd or moby calls
  `HcsInitializeWritableLayer` or `HcsAttachLayerStorageFilter` outside their wrappers
  (`hcsshim/computestorage/initialize.go:20`, `attach.go:21`). What does exist, each from documented calls:
  - a writable VHD made into a volume: `CreateVirtualDisk` → `AttachVirtualDisk` → `HcsFormatWritableLayerVhd` →
    `HcsGetLayerVhdMountPath` (`containerd/pkg/os/os_windows_test.go:130-160`);
  - a container's scratch as a byte copy of the base layer's `blank.vhdx`, which the base layer's setup made
    (`containerd/plugins/snapshots/windows/windows.go:259-270`; `hcsshim/computestorage/helpers.go:102`: "Create the
    differencing disk that will be what's copied for the final rw layer").
  So the documented sequence Hex runs is assembled from those: the scratch VHD (a copy of `blank.vhdx`) attached with
  no drive letter, its volume from `HcsGetLayerVhdMountPath`, `HcsInitializeWritableLayer(volume, layerData)`, then
  `HcsAttachLayerStorageFilter(volume, layerData)`, and the volume as `Storage.Path`. Whether each step takes the
  volume or a folder, whether the initialize is needed on a `blank.vhdx` copy, and whether a layer's `Id` must be
  HCS's `NameToGuid` of its name or only unique (hcsshim's CIM path uses the volume's own GUID: "since each mounted
  volume has a unique GUID, just return the same GUID as ID", `hcsshim/internal/wclayer/cim/mount.go:144`) is
  **(probe)**.
- Doc contradiction, recorded: `HcsFormatWritableLayerVhd.md:37` asks for "an unmounted virtual hard disk", while
  hcsshim says it mounts one itself if needed (`hcsshim/computestorage/format.go:15-20`) and containerd formats an
  attached one (`os_windows_test.go:144-152`). Copying `blank.vhdx` avoids formatting at create at all.
- Getting an image's layers without Docker's engine: containerd's windows snapshotter keeps them at
  `C:\ProgramData\containerd\root\io.containerd.snapshotter.v1.windows\snapshots\<n>` (`containerd/defaults/defaults_windows.go:27,55`;
  `containerd/plugins/snapshots/windows/common.go:59,70-71`), and a prepared snapshot's mount names the parents in
  order (`windows.go:323-357`, option `parentLayerPaths=`). Those folders are `layerFolders` minus the scratch.
- `layerchain.json` is Docker's, not HCS's (`moby/daemon/graphdriver/windows/windows.go:861-889`); HCS takes the parent
  list explicitly.

## 4. Processes: no suspended start, so a launcher Hex owns

- `HcsCreateProcess(system, processParameters, operation, securityDescriptor, &process)`
  (`VD/hcs/Reference/HcsCreateProcess.md:27-34`); `ProcessParameters` (`VD/hcs/SchemaReference.md:2780-2793`):
  `CommandLine`, `User`, `WorkingDirectory`, `Environment`, `CreateStdInPipe`/`CreateStdOutPipe`/`CreateStdErrPipe`,
  `EmulateConsole`. `HCS_PROCESS_INFORMATION{ProcessId, StdInput, StdOutput, StdError}`
  (`VD/hcs/Reference/HCS_PROCESS_INFORMATION.md:27-34`).
- **A process can't be created suspended through HCS**: the parameters have no such field, and the only "suspend" in
  the HCS reference is a VM's (`VD/hcs/SchemaReference.md:637`). hcsshim's own environment builder also drops
  variables with an empty value (`hcsshim/internal/cmd/cmd.go:197-201`) — a silent drop Hex doesn't copy (§3).
- So a member HCS starts runs at once, and anything it forks before the node could act would escape a job the node
  assigned afterward. The race-free way is the `process` class's own: create the member suspended inside its job with
  `PROC_THREAD_ATTRIBUTE_JOB_LIST` (`docs/research/runtime/windows-job-objects.md`), done *inside* the container by a
  small launcher Hex owns, which HCS starts and which starts the member.
- The pid HCS returns is a host pid: hcsshim opens silo pids with the host's `OpenProcess`
  (`hcsshim/internal/jobobject/jobobject.go:593-598`) and matches its exec pids against HCS's `ProcessList`
  (`hcsshim/cmd/containerd-shim-runhcs-v1/task_hcs.go:594-631`). No primary source states it **(probe)**.
- Handles cross into a process by `DuplicateHandle` with that process's handle opened for `PROCESS_DUP_HANDLE`
  (`sdk-api/sdk-api-src/content/handleapi/nf-handleapi-duplicatehandle.md:79`): "A handle to the process that is to
  receive the duplicated handle. The handle must have the PROCESS_DUP_HANDLE access right." So the node can give the
  launcher its box's job and the caller's three standard handles, and the member reads and writes the caller's pipes
  directly, as a `process` box's member does — no copier thread per stream.
- Signals: HCS has only console controls (`VD/hcs/SchemaReference.md:796-808`: CtrlC, CtrlBreak, CtrlClose,
  CtrlLogOff, CtrlShutdown); hcsshim maps TERM to `CtrlShutdown` and KILL to `HcsTerminateProcess`
  (`hcsshim/internal/signals/signal.go:191-195`), and Docker "fake[s] supporting regular stop and -9"
  (`moby/daemon/internal/libcontainerd/local/local_windows.go:713-749`). The `process` class already settled
  Windows' signal rule: KILL and TERM end the box's job (`hex_core::job::ends_job`).
- The container outlives its first program: hcsshim shuts it down itself when the init exits
  (`hcsshim/cmd/containerd-shim-runhcs-v1/task_hcs.go:641-654,695-757`).

## 5. Bounds: the silo is a job; a job nests

- Resource controls "are implemented on the parent job object associated with the container"
  (`VD/wc/manage-containers/resource-controls.md:16`): memory as `JOB_OBJECT_LIMIT_JOB_MEMORY` (`:26-30`), CPU count
  and percent as `JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP` (`:42,51`), shares as `WEIGHT_BASED` (`:60`). The doc's rows
  are the v1 fields; that v2 `SizeInMB` is the same control is inferred (hcsshim fills both from one value,
  `hcsdoc_wcow.go:244-257`) **(probe)**.
- The silo holds the container's own system processes, counted in its job: moby's test looks for "smss.exe",
  "csrss.exe", "wininit.exe", "services.exe", "lsass.exe", "CExecSvc.exe" (`moby/integration-cli/docker_cli_top_test.go:61-71`),
  and "each of them have > 20 system processes" (`moby/daemon/daemon_windows.go:42-43`). So a process bound on the
  silo's job can't be the box's `pids_max` (HEX-RT-6 bounds a box to 8), and its commit includes the system's.
- Opening the silo's job is by an implementation-detail name (`hcsshim/internal/hcs/v2/system.go:67`: "Implementation
  detail for silo naming, this should NOT be relied upon very heavily."), as SYSTEM only (`:435`: "by name, and only
  while running as SYSTEM."), through `NtOpenJobObject`, which windows-sys doesn't bind.
- Jobs nest (`win32/desktop-src/ProcThread/nested-jobs.md:27`): "If a process that is already in a job is added to
  another job, the jobs are nested by default if the system can form a valid job hierarchy and neither job sets UI
  limits". A process created in a job is in its parent's whole chain (`:43`): "a child process is associated with
  every job in the job chain of its parent process." Limits compose (`:47`): "The effective limit for child job can
  be more restrictive than the limit of its parent, but it cannot be less restrictive." — "Effective limits are
  enforced for … commit charge". Accounting rolls up (`:53`) and termination runs down (`:57`).
- So a job Hex creates on the host, handed to the launcher, holds the box's members *inside* the silo: the box's
  `memory_max`, `pids_max` and CPU bounds hold on its members exactly as the `process` class holds them (the same
  `hex_core::job` rule and measured margins), while the silo stays bounded by HCS for the whole container.
- Docker refuses `--pids-limit` on Windows ("invalid option: Windows does not support PidsLimit",
  `moby/daemon/daemon_windows.go:162`) and has no Windows memory minimum (`daemon_unix.go:67` is Linux's).

## 6. Devices and read-only mounts

- Devices are the container's only by grant (`VD/wc/deploy-containers/hardware-devices-in-containers.md:12`): "By
  default, Windows containers are given minimal access to host devices--just like Linux containers." A grant
  (`:39`) means "all devices which identify as implementing the requested class will be plumbed into the container",
  and only for process isolation (`:20`). An unknown class is "undefined behavior" (`:78-79`).
- hcsshim names `class` the legacy id type and `vpci-class-guid` the current one
  (`hcsshim/internal/uvm/virtual_device.go:21-28`); both become `{InterfaceClassGuid}`.
- Whether a device is visible is read by listing its class's interfaces: `CM_Get_Device_Interface_ListW`
  (`windows-sys/src/Windows/Win32/Devices/DeviceAndDriverInstallation/mod.rs:108,112`). Moby and containerd test a GPU
  grant by its side effect (the host's driver store appears: `moby/integration/container/devices_windows_test.go:109-118`),
  which needs a GPU-capable image.
- A read-only mapped directory (`VD/hcs/SchemaReference.md:2305`): `ReadOnly` "When set to true, the mapped directory
  in the compute system's guest will be read-only." hcsshim's test checks only that a delete and a copy fail
  (`hcsshim/test/functional/wcow_uvm_test.go:264-268`); the error a write gets is **(probe)**.

## 7. Host and image versions; the Containers feature

- Windows 11 runs Server 2022 images process-isolated
  (`VD/wc/deploy-containers/version-compatibility.md:17`): "**Starting with Windows 11 and Windows Server 2022 we
  are enabling the ability to run process-isolated WS2022 containers on Windows 11 hosts.**" Server 2025 images from
  24H2 (`:74`: "Supported from Windows 11 24H2 (`Build 2600`) onwards." — 24H2 is build 26100,
  `hcsshim/osversion/windowsbuilds.go:88-90`). Windows 10 runs none process-isolated (`:76-82`).
- Otherwise the build must match (`:98`): "With the exception of WS2022 + Windows 11, Windows Server containers are
  blocked from starting when the build number between the container host and the container image are different."
  Server hosts: 2025 runs 2025 and 2022; 2022 runs 2022 (`:24-56`).
- The two helpers disagree for a build-26200 host (jupiter): hcsshim's `CheckHostAndContainerCompat` refuses a
  Server 2022 image there (`hcsshim/osversion/platform_compat_windows.go:38-44`), containerd's copy allows it
  (`containerd/vendor/github.com/containerd/platforms/platform_windows_compat.go:94-97`: "a 26200 host, which is in the
  WS2025 generation"). Neither checks before create; HCS refuses at create. Hex checks first, by the documented
  table, and maps HCS's code to the same refusal; jupiter's answer for each image is **(probe)**.
- The image's build is its base layer's: the base layer holds the OS's files under `Files\`
  (`hcsshim/internal/wclayer/legacy.go:31-46`), so `Files\Windows\System32\ntdll.dll`'s version resource
  (`GetFileVersionInfoW`, `windows-sys/src/Windows/Win32/Storage/FileSystem/mod.rs:198`) gives it; the host's from
  `RtlGetVersion` (`windows-sys/src/Windows/Wdk/System/SystemServices/mod.rs:1473`), its product type telling a client
  (Windows 10 and 11) from a server.
- The Containers feature off: Docker checks for `vmcompute.dll` ("failed to load vmcompute.dll, ensure that the
  Containers feature is installed", `moby/daemon/daemon_windows.go:195-197`) and the `hns` and `vmcompute` services
  (`:202-204`). hcsshim loads `computecore.dll`/`computestorage.dll` lazily
  (`hcsshim/computestorage/zsyscall_windows.go:36,276-279`).
- **windows-sys links at load time**: `windows_link::link!` emits `#[link(name = $library, kind = "raw-dylib",
  modifiers = "+verbatim")]` (`windows-link/src/lib.rs:22`), so a binary importing `computecore.dll` through it would
  fail to load on a host without the feature (Windows' loader resolves imports before `main`) — the whole node down
  instead of one class refused. Hex resolves the HCS, storage and virtual-disk calls at run time instead:
  `LoadLibraryExW(…, LOAD_LIBRARY_SEARCH_SYSTEM32)` and `GetProcAddress` (`windows-sys/src/Windows/Win32/System/LibraryLoader/mod.rs:33,36`;
  `sdk-api/sdk-api-src/content/libloaderapi/nf-libloaderapi-getprocaddress.md:80`: "If the function fails, the return
  value is NULL."). A missing module or export is the class refused by name: the Containers feature is off, and the
  installer turns it on (Ada's call, 2026-10-31, `docs/design/runtime.md`).

## 8. Docker and containerd's runhcs shim, the step's comparisons

- containerd's Windows default runtime is `io.containerd.runhcs.v1`, process-isolated
  (`containerd/internal/cri/config/config_windows.go:57-69`). Its shim's create is `hcsoci.CreateContainer`
  (HcsCreateComputeSystem, `hcsshim/cmd/containerd-shim-runhcs-v1/task_hcs.go:120-191`); start is
  `HcsStartComputeSystem` then `HcsCreateProcess` (`exec_hcs.go:177-219`); delete is Shutdown, a 30 s wait, Terminate,
  a 30 s wait, then the layers released (`task_hcs.go:641-654,695-775`).
- Docker's create builds the v1 document through `vmcompute.dll` (`moby/daemon/internal/libcontainerd/local/local_windows.go:177-186,327`),
  and defaults client SKUs to Hyper-V isolation (`moby/daemon/daemon_windows.go:543-551`), so its comparison runs with
  `--isolation=process`.
- What the comparison needs on jupiter: Docker's engine in Windows-container mode and containerd with the runhcs shim,
  the same image (§7's matching tag) in each, run side by side with Hex's (the step's criterion 2).

## 9. What jupiter settles (probes, before the class claims them)

1. The documented scratch sequence (§3): which of `HcsInitializeWritableLayer` and `HcsAttachLayerStorageFilter` a
   `blank.vhdx` copy needs, on the volume path; and whether a layer `Id` need only be unique.
2. HCS pids are host pids, the launcher can be opened for `PROCESS_DUP_HANDLE` from the elevated node, and a job
   handed in nests under the silo's (§4, §5).
3. `Memory.SizeInMB` is the silo job's commit limit, and the silo's own baseline commit and process count for
   nanoserver and servercore (§5).
4. A write to a read-only mapped directory fails, and its error (§6); a class not granted lists no interfaces inside.
5. Which images HCS starts on build 26200: `ltsc2025` (26100) and `ltsc2022` (20348) (§7).
6. Whether a Rust binary built against the static C runtime runs in nanoserver as the launcher (nanoserver's VC
   runtime is not in these sources).
