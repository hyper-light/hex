//! The Windows calls the `oci` class makes that Rust can't make safely (proposed for CLAUDE.md §12): the
//! Host Compute Service's (`computecore.dll`), its storage calls (`computestorage.dll`) and the virtual-disk
//! calls (`virtdisk.dll`), each resolved at run time with `LoadLibraryExW` and `GetProcAddress` because
//! windows-sys links them at load time and a host without the Containers feature would then fail to start
//! the node at all (docs/research/runtime/windows-hcs.md §7); and, through windows-sys, opening a process by
//! pid, duplicating a handle into it, the host's version (`RtlGetVersion`), a file's version resource, and
//! a device interface class's members (`CM_Get_Device_Interface_List*`). Each `unsafe` block states the
//! invariant that makes it sound.

// The oci class's one place `unsafe` is allowed (proposed for CLAUDE.md §12); every block below carries
// its SAFETY comment.
#![allow(unsafe_code)]

use std::ffi::c_void;
use std::io;
use std::mem::{size_of, zeroed};
use std::os::windows::io::{AsRawHandle, BorrowedHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::ptr::{null, null_mut};
use std::sync::OnceLock;
use std::time::Duration;

use windows_sys::Wdk::System::SystemServices::RtlGetVersion;
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CM_Get_Device_Interface_List_SizeW,
    CM_Get_Device_Interface_ListW, CR_SUCCESS,
};
use windows_sys::Win32::Foundation::{
    DUPLICATE_SAME_ACCESS, DuplicateHandle, GENERIC_ALL, GetLastError, HANDLE, HLOCAL, HMODULE,
    INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::SECURITY_DESCRIPTOR;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileVersionInfoSizeW, GetFileVersionInfoW,
    OPEN_EXISTING, VS_FIXEDFILEINFO, VerQueryValueW,
};
use windows_sys::Win32::System::HostComputeSystem::{
    HCS_OPERATION, HCS_OPERATION_COMPLETION, HCS_PROCESS, HCS_PROCESS_INFORMATION, HCS_SYSTEM,
};
use windows_sys::Win32::System::JobObjects::IsProcessInJob;
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows_sys::Win32::System::SystemInformation::OSVERSIONINFOEXW;
use windows_sys::Win32::System::SystemServices::JOB_OBJECT_ASSIGN_PROCESS;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, TerminateProcess,
};
use windows_sys::core::{GUID, HRESULT, PCWSTR, PWSTR};

use hex_core::hcs::{self, Host, HostKind};

use crate::windows::sys::Handle;

/// `VER_NT_WORKSTATION`: a client SKU's product type (`OSVERSIONINFOEXW.wProductType`).
const VER_NT_WORKSTATION: u8 = 1;

/// The longest string Hex reads back from HCS, in UTF-16 units (a result or error document): far past any
/// document HCS returns for one container, bounded so a malformed one is never walked unbounded (§2).
const MAX_RESULT_UNITS: usize = 1 << 22;

/// The longest device-interface list Hex reads, in UTF-16 units (a probe's list of one class's members).
const MAX_INTERFACE_UNITS: u32 = 1 << 20;

/// A Windows error as an `io::Error`.
fn last_error() -> io::Error {
    // SAFETY: GetLastError reads the calling thread's own last-error value and returns it.
    io::Error::from_raw_os_error(i32::try_from(unsafe { GetLastError() }).unwrap_or(-1))
}

/// `s` as a NUL-terminated wide string. The caller has refused a NUL inside `s`.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// An HCS failure: its HRESULT, as `u32`, and the message its result document gave, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HcsFailure {
    pub code: u32,
    pub message: Option<String>,
}

impl HcsFailure {
    fn of(hr: HRESULT, doc: Option<String>) -> Self {
        Self {
            code: u32::from_ne_bytes(hr.to_ne_bytes()),
            // A ResultError document's message (`SchemaReference.md:3073-3076`), else the document whole.
            message: doc.map(|d| {
                serde_json::from_str::<serde_json::Value>(&d)
                    .ok()
                    .and_then(|v| {
                        v.get("ErrorMessage")
                            .and_then(|m| m.as_str())
                            .map(str::to_owned)
                    })
                    .unwrap_or(d)
            }),
        }
    }
}

type CreateOperation =
    unsafe extern "system" fn(*const c_void, HCS_OPERATION_COMPLETION) -> HCS_OPERATION;
type CloseOperation = unsafe extern "system" fn(HCS_OPERATION);
type CancelOperation = unsafe extern "system" fn(HCS_OPERATION) -> HRESULT;
type WaitResult = unsafe extern "system" fn(HCS_OPERATION, u32, *mut PWSTR) -> HRESULT;
type WaitResultAndProcess = unsafe extern "system" fn(
    HCS_OPERATION,
    u32,
    *mut HCS_PROCESS_INFORMATION,
    *mut PWSTR,
) -> HRESULT;
type CreateSystem = unsafe extern "system" fn(
    PCWSTR,
    PCWSTR,
    HCS_OPERATION,
    *const SECURITY_DESCRIPTOR,
    *mut HCS_SYSTEM,
) -> HRESULT;
type OpenSystem = unsafe extern "system" fn(PCWSTR, u32, *mut HCS_SYSTEM) -> HRESULT;
type SystemOp = unsafe extern "system" fn(HCS_SYSTEM, HCS_OPERATION, PCWSTR) -> HRESULT;
type CloseSystem = unsafe extern "system" fn(HCS_SYSTEM);
type CreateProcess = unsafe extern "system" fn(
    HCS_SYSTEM,
    PCWSTR,
    HCS_OPERATION,
    *const SECURITY_DESCRIPTOR,
    *mut HCS_PROCESS,
) -> HRESULT;
type CloseProcess = unsafe extern "system" fn(HCS_PROCESS);
type ServiceProperties = unsafe extern "system" fn(PCWSTR, *mut PWSTR) -> HRESULT;
/// vmcompute's `DRIVER_INFO`: the filter driver (flavour 1) and an empty home folder, as hcsshim passes it
/// (`hcsshim/internal/wclayer/layerutils.go:35-43`).
#[repr(C)]
struct DriverInfo {
    flavour: i32,
    home: PCWSTR,
}

/// vmcompute's `WC_LAYER_DESCRIPTOR` (`hcsshim/internal/wclayer/layerutils.go:45-80`).
#[repr(C)]
struct LayerDescriptor {
    id: GUID,
    flags: u32,
    path: PCWSTR,
}

type LayerOp = unsafe extern "system" fn(*const DriverInfo, PCWSTR) -> HRESULT;
type PrepareLayer =
    unsafe extern "system" fn(*const DriverInfo, PCWSTR, *const LayerDescriptor, u32) -> HRESULT;
type LayerMountPath =
    unsafe extern "system" fn(*const DriverInfo, PCWSTR, *mut usize, *mut u16) -> HRESULT;

/// The HCS calls and vmcompute's layer calls, resolved once. The modules are never unloaded, so each pointer
/// stays valid for the process's life.
struct Api {
    create_operation: CreateOperation,
    close_operation: CloseOperation,
    cancel_operation: CancelOperation,
    wait_result: WaitResult,
    wait_result_and_process: WaitResultAndProcess,
    create_system: CreateSystem,
    open_system: OpenSystem,
    start_system: SystemOp,
    terminate_system: SystemOp,
    close_system: CloseSystem,
    create_process: CreateProcess,
    close_process: CloseProcess,
    service_properties: ServiceProperties,
    activate_layer: LayerOp,
    prepare_layer: PrepareLayer,
    layer_mount_path: LayerMountPath,
    unprepare_layer: LayerOp,
    deactivate_layer: LayerOp,
}

/// A module from System32, or what is missing.
fn module(name: &str) -> Result<HMODULE, String> {
    let w = wide(name);
    // SAFETY: w is a NUL-terminated wide string that outlives the call; no file handle is passed, and
    // LOAD_LIBRARY_SEARCH_SYSTEM32 searches System32 alone. A null return is an error, checked.
    let m = unsafe { LoadLibraryExW(w.as_ptr(), null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32) };
    if m.is_null() {
        return Err(format!("{name} isn't on this machine ({})", last_error()));
    }
    Ok(m)
}

/// Resolves export `name` of `module` as a function of type `$t` (its documented signature).
macro_rules! export {
    ($module:expr, $dll:literal, $name:literal, $t:ty) => {{
        // SAFETY: $name is a NUL-terminated ASCII export name and $module a module loaded above, never
        // freed; GetProcAddress returns null for a missing export, checked.
        let f = unsafe { GetProcAddress($module, concat!($name, "\0").as_ptr()) };
        match f {
            // SAFETY: the export is the documented function of signature $t (the HCS and virtdisk
            // references, docs/research/runtime/windows-hcs.md); a function pointer of one signature
            // transmuted to another of the same size is sound when the target is that function's true
            // signature, which the documentation states.
            Some(f) => unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, $t>(f) },
            None => return Err(format!("{} has no {} ({})", $dll, $name, last_error())),
        }
    }};
}

impl Api {
    fn load() -> Result<Api, String> {
        let core = module("computecore.dll")?;
        let layers = module("vmcompute.dll")?;
        Ok(Api {
            create_operation: export!(
                core,
                "computecore.dll",
                "HcsCreateOperation",
                CreateOperation
            ),
            close_operation: export!(core, "computecore.dll", "HcsCloseOperation", CloseOperation),
            cancel_operation: export!(
                core,
                "computecore.dll",
                "HcsCancelOperation",
                CancelOperation
            ),
            wait_result: export!(
                core,
                "computecore.dll",
                "HcsWaitForOperationResult",
                WaitResult
            ),
            wait_result_and_process: export!(
                core,
                "computecore.dll",
                "HcsWaitForOperationResultAndProcessInfo",
                WaitResultAndProcess
            ),
            create_system: export!(
                core,
                "computecore.dll",
                "HcsCreateComputeSystem",
                CreateSystem
            ),
            open_system: export!(core, "computecore.dll", "HcsOpenComputeSystem", OpenSystem),
            start_system: export!(core, "computecore.dll", "HcsStartComputeSystem", SystemOp),
            terminate_system: export!(
                core,
                "computecore.dll",
                "HcsTerminateComputeSystem",
                SystemOp
            ),
            close_system: export!(
                core,
                "computecore.dll",
                "HcsCloseComputeSystem",
                CloseSystem
            ),
            create_process: export!(core, "computecore.dll", "HcsCreateProcess", CreateProcess),
            close_process: export!(core, "computecore.dll", "HcsCloseProcess", CloseProcess),
            service_properties: export!(
                core,
                "computecore.dll",
                "HcsGetServiceProperties",
                ServiceProperties
            ),
            activate_layer: export!(layers, "vmcompute.dll", "ActivateLayer", LayerOp),
            prepare_layer: export!(layers, "vmcompute.dll", "PrepareLayer", PrepareLayer),
            layer_mount_path: export!(layers, "vmcompute.dll", "GetLayerMountPath", LayerMountPath),
            unprepare_layer: export!(layers, "vmcompute.dll", "UnprepareLayer", LayerOp),
            deactivate_layer: export!(layers, "vmcompute.dll", "DeactivateLayer", LayerOp),
        })
    }
}

/// The calls, loaded on first use; `Err` says which module or export is missing.
fn api() -> Result<&'static Api, &'static str> {
    static API: OnceLock<Result<Api, String>> = OnceLock::new();
    API.get_or_init(Api::load).as_ref().map_err(String::as_str)
}

/// Whether HCS is here and answering: its calls load and `HcsGetServiceProperties` succeeds. `Err` says
/// what is missing, for the class's refusal.
pub fn available() -> Result<(), String> {
    let api = api().map_err(str::to_owned)?;
    let query = wide(r#"{"PropertyTypes":["Basic"]}"#);
    let mut result: PWSTR = null_mut();
    // SAFETY: query is a NUL-terminated wide string; result receives an HCS-allocated string, freed below.
    let hr = unsafe { (api.service_properties)(query.as_ptr(), &mut result) };
    let doc = take(result);
    if hr < 0 {
        return Err(format!(
            "the Host Compute Service doesn't answer ({:#010x}{})",
            u32::from_ne_bytes(hr.to_ne_bytes()),
            doc.map(|d| format!(": {d}")).unwrap_or_default()
        ));
    }
    Ok(())
}

/// Reads and frees a string HCS allocated (`LocalFree`, `HcsWaitForOperationResult.md:53`); `None` for null.
fn take(s: PWSTR) -> Option<String> {
    if s.is_null() {
        return None;
    }
    let mut len = 0usize;
    // SAFETY: s is a NUL-terminated wide string HCS allocated; reading stops at its NUL or at the bound,
    // within the allocation either way (a string past the bound is longer than the bound).
    while len < MAX_RESULT_UNITS && unsafe { *s.add(len) } != 0 {
        len = len.saturating_add(1);
    }
    // SAFETY: the len units before the NUL (or the bound) are initialized, read above.
    let text = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(s, len) });
    // SAFETY: s was allocated by HCS for the caller to free with LocalFree, once, here.
    unsafe { LocalFree(s as HLOCAL) };
    Some(text)
}

/// Milliseconds of `d` for an HCS wait, capped below `INFINITE`.
fn millis(d: Duration) -> u32 {
    u32::try_from(d.as_millis())
        .unwrap_or(0xFFFF_FFFE)
        .min(0xFFFF_FFFE)
}

/// One HCS operation: made, handed to `call`, waited on for at most `patience`. A wait that runs out is
/// cancelled (never abandoned, note §2) and waited on once more for the same time; an operation that still
/// hasn't finished is left open rather than closed under HCS's feet, and refused as timed out. Returns the
/// result document and, when `process` is asked for, the process information.
fn operation(
    api: &Api,
    patience: Duration,
    process: Option<&mut HCS_PROCESS_INFORMATION>,
    call: impl FnOnce(HCS_OPERATION) -> HRESULT,
) -> Result<Option<String>, HcsFailure> {
    // SAFETY: no context or callback: the operation is waited on synchronously below.
    let op = unsafe { (api.create_operation)(null(), None) };
    if op.is_null() {
        return Err(HcsFailure {
            code: 0x8037_011A,
            message: Some(format!("HcsCreateOperation failed: {}", last_error())),
        });
    }
    let hr = call(op);
    if hr < 0 {
        // SAFETY: the call failed synchronously, so the operation was never started; op is ours to close.
        unsafe { (api.close_operation)(op) };
        return Err(HcsFailure::of(hr, None));
    }
    let info: *mut HCS_PROCESS_INFORMATION = process.map_or(null_mut(), std::ptr::from_mut);
    let wait = || {
        let mut doc: PWSTR = null_mut();
        let hr = if info.is_null() {
            // SAFETY: op is a started operation; doc is a valid out-pointer.
            unsafe { (api.wait_result)(op, millis(patience), &mut doc) }
        } else {
            // SAFETY: op is a started operation; info points at the caller's HCS_PROCESS_INFORMATION,
            // borrowed for this call, and doc is a valid out-pointer.
            unsafe { (api.wait_result_and_process)(op, millis(patience), info, &mut doc) }
        };
        (hr, take(doc))
    };
    let timed_out = |hr: HRESULT| {
        hex_core::hcs::hcs_error(u32::from_ne_bytes(hr.to_ne_bytes()))
            == hex_core::hcs::HcsError::Timeout
    };
    let (mut hr, mut doc) = wait();
    if timed_out(hr) {
        // SAFETY: op is a started operation; cancelling one is the documented way to bound it.
        unsafe { (api.cancel_operation)(op) };
        (hr, doc) = wait();
        if timed_out(hr) {
            // Still running inside HCS after its cancel: closing it now could free what the call uses, so
            // the operation handle is left open (one per such failure) and the call refused.
            return Err(HcsFailure::of(hr, doc));
        }
    }
    // SAFETY: the operation has finished (its wait returned other than a timeout); op is ours to close.
    unsafe { (api.close_operation)(op) };
    if hr < 0 {
        return Err(HcsFailure::of(hr, doc));
    }
    Ok(doc)
}

/// A compute system, closed on drop (and, created with `ShouldTerminateOnLastHandleClosed`, ended then).
#[derive(Debug)]
pub struct System(HCS_SYSTEM);

// SAFETY: an HCS_SYSTEM handle is valid in any thread of the process; HCS serializes calls on it.
unsafe impl Send for System {}
// SAFETY: as above; Hex shares one only behind its own lock or for HCS calls HCS serializes.
unsafe impl Sync for System {}

impl Drop for System {
    fn drop(&mut self) {
        if let Ok(api) = api() {
            // SAFETY: self owns the handle and Drop runs once; HcsCloseComputeSystem drains in-flight calls.
            unsafe { (api.close_system)(self.0) };
        }
    }
}

/// Creates compute system `id` from `document`, waiting at most `patience`.
pub fn create_system(id: &str, document: &str, patience: Duration) -> Result<System, HcsFailure> {
    let api = api().map_err(|m| HcsFailure {
        code: 0x8037_0114,
        message: Some(m.to_owned()),
    })?;
    let (id, doc) = (wide(id), wide(document));
    let mut system: HCS_SYSTEM = null_mut();
    operation(api, patience, None, |op| {
        // SAFETY: id and doc are NUL-terminated wide strings that outlive the operation's wait; the
        // security descriptor is reserved (null); system receives the handle.
        unsafe { (api.create_system)(id.as_ptr(), doc.as_ptr(), op, null(), &mut system) }
    })
    .inspect_err(|_| {
        if !system.is_null() {
            // SAFETY: a handle HCS gave back before the create failed is ours to close.
            unsafe { (api.close_system)(system) };
        }
    })?;
    Ok(System(system))
}

/// Opens compute system `id` a node before this one made, where it still runs (`HcsOpenComputeSystem`,
/// with `GENERIC_ALL` as HCS's own sample reopens one: `VD/hcs/Reference/ComputeSystemSample.md:24-38`);
/// `None` where HCS has no system of that id. The handle takes every call the creating node's did.
pub fn open_system(id: &str) -> Result<Option<System>, HcsFailure> {
    let api = api().map_err(|m| HcsFailure {
        code: 0x8037_0114,
        message: Some(m.to_owned()),
    })?;
    let id = wide(id);
    let mut system: HCS_SYSTEM = null_mut();
    // SAFETY: id is a NUL-terminated wide string that outlives the call, which is synchronous (no
    // operation); system receives the handle, owned below on success.
    let hr = unsafe { (api.open_system)(id.as_ptr(), GENERIC_ALL, &mut system) };
    if hr < 0 {
        let f = HcsFailure::of(hr, None);
        return match hcs::hcs_error(f.code) {
            hcs::HcsError::NotFound => Ok(None),
            _ => Err(f),
        };
    }
    Ok(Some(System(system)))
}

impl System {
    fn op(&self, f: fn(&Api) -> SystemOp, patience: Duration) -> Result<(), HcsFailure> {
        let api = api().map_err(|m| HcsFailure {
            code: 0x8037_0114,
            message: Some(m.to_owned()),
        })?;
        let call = f(api);
        operation(api, patience, None, |op| {
            // SAFETY: self.0 is a live compute-system handle; options are null (none, documented).
            unsafe { call(self.0, op, null()) }
        })
        .map(|_| ())
    }

    /// Starts it (`HcsStartComputeSystem`).
    pub fn start(&self, patience: Duration) -> Result<(), HcsFailure> {
        self.op(|a| a.start_system, patience)
    }

    /// Ends it forcefully (`HcsTerminateComputeSystem`).
    pub fn terminate(&self, patience: Duration) -> Result<(), HcsFailure> {
        self.op(|a| a.terminate_system, patience)
    }

    /// Starts a process in it from `parameters` (a `ProcessParameters` document), returning HCS's handle
    /// for it, its pid, and the stdin and stdout pipes HCS made for it.
    pub fn create_process(
        &self,
        parameters: &str,
        patience: Duration,
    ) -> Result<HcsProcess, HcsFailure> {
        let api = api().map_err(|m| HcsFailure {
            code: 0x8037_0114,
            message: Some(m.to_owned()),
        })?;
        let params = wide(parameters);
        let mut process: HCS_PROCESS = null_mut();
        // SAFETY: HCS_PROCESS_INFORMATION is plain data; all-zero is a valid empty value.
        let mut info: HCS_PROCESS_INFORMATION = unsafe { zeroed() };
        let made = operation(api, patience, Some(&mut info), |op| {
            // SAFETY: self.0 is a live system; params is NUL-terminated and outlives the wait; the
            // security descriptor is reserved (null); process receives the handle.
            unsafe { (api.create_process)(self.0, params.as_ptr(), op, null(), &mut process) }
        });
        // The pipes and the process handle are ours from here, whatever the outcome, so they are owned
        // (and closed) either way.
        let handle = HcsProcessHandle(process);
        let own = |h: HANDLE| {
            (!h.is_null()).then(|| {
                // SAFETY: each non-null handle in info is a pipe end HCS made for this process, ours to
                // own, and each is taken once.
                unsafe { OwnedHandle::from_raw_handle(h as RawHandle) }
            })
        };
        let (stdin, stdout, _stderr) =
            (own(info.StdInput), own(info.StdOutput), own(info.StdError));
        made?;
        match (stdin, stdout) {
            (Some(stdin), Some(stdout)) => Ok(HcsProcess {
                handle,
                pid: info.ProcessId,
                stdin,
                stdout,
            }),
            _ => Err(HcsFailure {
                code: 0x8037_011D,
                message: Some("HCS gave the launcher no stdin or stdout pipe".to_owned()),
            }),
        }
    }
}

/// HCS's handle for a process it started, closed on drop.
#[derive(Debug)]
pub struct HcsProcessHandle(HCS_PROCESS);

/// What HCS gave back for a process it started.
#[derive(Debug)]
pub struct HcsProcess {
    pub handle: HcsProcessHandle,
    pub pid: u32,
    pub stdin: OwnedHandle,
    pub stdout: OwnedHandle,
}

// SAFETY: an HCS_PROCESS handle is valid in any thread of the process.
unsafe impl Send for HcsProcessHandle {}

impl Drop for HcsProcessHandle {
    fn drop(&mut self) {
        if !self.0.is_null()
            && let Ok(api) = api()
        {
            // SAFETY: self owns the handle and Drop runs once.
            unsafe { (api.close_process)(self.0) };
        }
    }
}

/// The longest mount path `GetLayerMountPath` may give, in UTF-16 units: a volume GUID path is 49, and
/// the longest Windows path 32,767 (CLAUDE.md §2: a longer answer is refused, not allocated).
const MAX_MOUNT_PATH: usize = 32_767;

fn layer_call(f: impl FnOnce(&Api, &DriverInfo) -> HRESULT) -> Result<(), HcsFailure> {
    let api = api().map_err(|m| HcsFailure {
        code: 0x8037_0114,
        message: Some(m.to_owned()),
    })?;
    let home = [0u16];
    let info = DriverInfo {
        flavour: 1,
        home: home.as_ptr(),
    };
    let hr = f(api, &info);
    if hr < 0 {
        return Err(HcsFailure::of(hr, None));
    }
    Ok(())
}

/// `ActivateLayer`: attaches scratch folder `scratch`'s `sandbox.vhdx` as a volume (note §3).
pub fn activate_layer(scratch: &str) -> Result<(), HcsFailure> {
    let id = wide(scratch);
    // SAFETY: info and id are valid for the call: a DRIVER_INFO whose home is a NUL-terminated empty wide
    // string, and a NUL-terminated wide string.
    layer_call(|api, info| unsafe { (api.activate_layer)(info, id.as_ptr()) })
}

/// `PrepareLayer`: puts the storage filter over activated scratch `scratch`, its parents `parents`
/// topmost first, each named by [`hex_core::hcs::layer_guid`] as the container's document names it.
pub fn prepare_layer(scratch: &str, parents: &[String]) -> Result<(), HcsFailure> {
    let id = wide(scratch);
    let paths: Vec<Vec<u16>> = parents.iter().map(|p| wide(p)).collect();
    let descriptors: Vec<LayerDescriptor> = parents
        .iter()
        .zip(&paths)
        .map(|(p, w)| LayerDescriptor {
            id: GUID::from_u128(hex_core::hcs::layer_guid(p)),
            flags: 0,
            path: w.as_ptr(),
        })
        .collect();
    let count = u32::try_from(descriptors.len()).map_err(|_| HcsFailure {
        code: 0x8007_0057,
        message: Some(format!(
            "{} layers is more than PrepareLayer takes",
            descriptors.len()
        )),
    })?;
    // SAFETY: info and id as in activate_layer; descriptors holds count descriptors whose paths point into
    // paths, NUL-terminated wide strings, all of which outlive the call.
    layer_call(|api, info| unsafe {
        (api.prepare_layer)(info, id.as_ptr(), descriptors.as_ptr(), count)
    })
}

/// `GetLayerMountPath`: the prepared scratch's volume path, `\\?\Volume{GUID}` (its root without the
/// trailing backslash; `hex_core::hcs::volume_root` adds it).
pub fn layer_mount_path(scratch: &str) -> Result<String, HcsFailure> {
    let id = wide(scratch);
    let mut len = 0usize;
    // SAFETY: info and id as in activate_layer; len is a valid out-pointer, and a null buffer asks for
    // the length alone (hcsshim/internal/wclayer/getlayermountpath.go:26-30).
    layer_call(|api, info| unsafe {
        (api.layer_mount_path)(info, id.as_ptr(), &mut len, null_mut())
    })?;
    if len == 0 || len > MAX_MOUNT_PATH {
        return Err(HcsFailure {
            code: 0x8007_000D,
            message: Some(format!("GetLayerMountPath gave a length of {len}")),
        });
    }
    let mut buf = vec![0u16; len];
    // SAFETY: as above, with buf a buffer of len UTF-16 units, the length the call just gave.
    layer_call(|api, info| unsafe {
        (api.layer_mount_path)(info, id.as_ptr(), &mut len, buf.as_mut_ptr())
    })?;
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16(buf.get(..end).unwrap_or_default()).map_err(|_| HcsFailure {
        code: 0x8007_000D,
        message: Some("GetLayerMountPath gave a path that isn't UTF-16".to_owned()),
    })
}

/// `UnprepareLayer`: takes the storage filter off scratch `scratch`.
pub fn unprepare_layer(scratch: &str) -> Result<(), HcsFailure> {
    let id = wide(scratch);
    // SAFETY: info and id as in activate_layer.
    layer_call(|api, info| unsafe { (api.unprepare_layer)(info, id.as_ptr()) })
}

/// `DeactivateLayer`: detaches scratch `scratch`'s volume.
pub fn deactivate_layer(scratch: &str) -> Result<(), HcsFailure> {
    let id = wide(scratch);
    // SAFETY: info and id as in activate_layer.
    layer_call(|api, info| unsafe { (api.deactivate_layer)(info, id.as_ptr()) })
}

/// Opens process `pid` for `access`.
pub fn open_process(pid: u32, access: u32) -> io::Result<Handle> {
    // SAFETY: OpenProcess takes plain values and returns a handle or null, checked.
    let h = unsafe { OpenProcess(access, 0, pid) };
    if h.is_null() {
        return Err(last_error());
    }
    // SAFETY: h is the process handle just opened, owned by nothing else.
    Ok(unsafe { Handle::own(h) })
}

/// The access the node opens a launcher with: to duplicate handles into it and to end it.
pub const LAUNCHER_ACCESS: u32 =
    PROCESS_DUP_HANDLE | windows_sys::Win32::System::Threading::PROCESS_TERMINATE;
/// The access the node holds a member with: to wait on it, read its exit and job, and end it.
pub const MEMBER_ACCESS: u32 = PROCESS_SYNCHRONIZE
    | PROCESS_QUERY_LIMITED_INFORMATION
    | windows_sys::Win32::System::Threading::PROCESS_TERMINATE;
/// The access the launcher gets to the box's job: to start a member in it, and nothing else.
pub const JOB_ACCESS: u32 = JOB_OBJECT_ASSIGN_PROCESS;

/// Duplicates `handle` into `target` with `access` (or the same access where `None`), not inheritable;
/// returns its value in `target`.
pub fn duplicate_into(
    target: &Handle,
    handle: BorrowedHandle<'_>,
    access: Option<u32>,
) -> io::Result<u64> {
    let mut out: HANDLE = null_mut();
    // SAFETY: handle is a live handle of this process (borrowed for the call), target a process handle
    // with PROCESS_DUP_HANDLE; GetCurrentProcess is the pseudo-handle for this process; out receives the
    // value in target.
    let ok = unsafe {
        DuplicateHandle(
            GetCurrentProcess(),
            handle.as_raw_handle() as HANDLE,
            target.raw(),
            &mut out,
            access.unwrap_or(0),
            0,
            if access.is_some() {
                0
            } else {
                DUPLICATE_SAME_ACCESS
            },
        )
    };
    if ok == 0 {
        return Err(last_error());
    }
    Ok(out as usize as u64)
}

/// Ends `process` with `code` (a launcher that never replied).
pub fn terminate_process(process: &Handle, code: u32) -> io::Result<()> {
    // SAFETY: process is a live process handle with PROCESS_TERMINATE.
    if unsafe { TerminateProcess(process.raw(), code) } == 0 {
        return Err(last_error());
    }
    Ok(())
}

/// Whether `process` is in `job`.
pub fn in_job(process: &Handle, job: BorrowedHandle<'_>) -> io::Result<bool> {
    let mut result = 0;
    // SAFETY: process and job are live handles (job borrowed for the call); result is a valid out-pointer.
    if unsafe { IsProcessInJob(process.raw(), job.as_raw_handle() as HANDLE, &mut result) } == 0 {
        return Err(last_error());
    }
    Ok(result != 0)
}

/// Takes ownership, in the launcher, of the box's job and the member's three standard handles: the values
/// the node duplicated into this process for its one request (`super::launch`). Only the launcher calls
/// this, once, with the values its request carried; a value of 0 is refused.
pub fn adopt(job: u64, stdio: [u64; 3]) -> io::Result<(OwnedHandle, [OwnedHandle; 3])> {
    let own = |value: u64| {
        let raw = usize::try_from(value).map_err(io::Error::other)? as RawHandle;
        if raw.is_null() {
            return Err(io::Error::other("a null handle value"));
        }
        // SAFETY: the node duplicated this value into the launcher for this request alone (DuplicateHandle
        // into the launcher's process, `super::launch::start`), so it is a live handle here that nothing
        // else owns, and the launcher adopts each once.
        Ok(unsafe { OwnedHandle::from_raw_handle(raw) })
    };
    let [i, o, e] = stdio;
    Ok((own(job)?, [own(i)?, own(o)?, own(e)?]))
}

/// The host's build and kind (`RtlGetVersion`, which, unlike `GetVersionEx`, isn't shimmed by a manifest).
pub fn host() -> Host {
    // SAFETY: OSVERSIONINFOEXW is plain data; all-zero then its size set is the documented input.
    let mut v: OSVERSIONINFOEXW = unsafe { zeroed() };
    v.dwOSVersionInfoSize = u32::try_from(size_of::<OSVERSIONINFOEXW>()).unwrap_or(0);
    // SAFETY: v is an OSVERSIONINFOEXW with its size set, which RtlGetVersion accepts through its
    // OSVERSIONINFOW pointer (the structure begins with it); it fills v and returns STATUS_SUCCESS.
    unsafe { RtlGetVersion((&raw mut v).cast()) };
    Host {
        build: v.dwBuildNumber,
        kind: if v.wProductType == VER_NT_WORKSTATION {
            HostKind::Client
        } else {
            HostKind::Server
        },
    }
}

/// The build in `path`'s version resource (`VS_FIXEDFILEINFO.dwFileVersionLS`'s high word): for a base
/// layer's `ntdll.dll`, the image's Windows build.
pub fn file_build(path: &str) -> io::Result<u32> {
    let p = wide(path);
    let mut ignored = 0u32;
    // SAFETY: p is a NUL-terminated wide string; ignored is a valid out-pointer.
    let size = unsafe { GetFileVersionInfoSizeW(p.as_ptr(), &mut ignored) };
    if size == 0 {
        return Err(last_error());
    }
    let mut block = vec![0u8; usize::try_from(size).map_err(io::Error::other)?];
    // SAFETY: block holds size bytes, as GetFileVersionInfoW was told.
    if unsafe { GetFileVersionInfoW(p.as_ptr(), 0, size, block.as_mut_ptr().cast()) } == 0 {
        return Err(last_error());
    }
    let root = wide("\\");
    let mut info: *mut c_void = null_mut();
    let mut len = 0u32;
    // SAFETY: block is the version resource just read; VerQueryValueW points info into it.
    if unsafe { VerQueryValueW(block.as_ptr().cast(), root.as_ptr(), &mut info, &mut len) } == 0
        || info.is_null()
        || usize::try_from(len).unwrap_or(0) < size_of::<VS_FIXEDFILEINFO>()
    {
        return Err(io::Error::other("no fixed version information"));
    }
    // SAFETY: info points at a VS_FIXEDFILEINFO of len bytes inside block, which is alive; it may be
    // unaligned, so it is read unaligned.
    let fixed = unsafe { info.cast::<VS_FIXEDFILEINFO>().read_unaligned() };
    Ok(fixed.dwFileVersionLS >> 16)
}

/// The present devices implementing interface class `class` as seen from where this process runs: how
/// many `CM_Get_Device_Interface_List*` lists, and how many of those open. The probe for "a device not
/// granted isn't visible", read as reachable (the lead's ruling, 2026-11-06).
pub fn device_interfaces(class: &GUID) -> io::Result<DeviceView> {
    let mut len = 0u32;
    // SAFETY: class is a valid GUID; len is a valid out-pointer; no device instance filter.
    let cr = unsafe {
        CM_Get_Device_Interface_List_SizeW(
            &mut len,
            class,
            null(),
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    };
    if cr != CR_SUCCESS {
        return Err(io::Error::other(format!(
            "CM_Get_Device_Interface_List_SizeW: {cr}"
        )));
    }
    if len > MAX_INTERFACE_UNITS {
        return Err(io::Error::other(format!(
            "{len} units of device interfaces is past the bound"
        )));
    }
    let mut buf = vec![0u16; usize::try_from(len).map_err(io::Error::other)?];
    // SAFETY: buf holds len units, as told; class is a valid GUID.
    let cr = unsafe {
        CM_Get_Device_Interface_ListW(
            class,
            null(),
            buf.as_mut_ptr(),
            len,
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        )
    };
    if cr != CR_SUCCESS {
        return Err(io::Error::other(format!(
            "CM_Get_Device_Interface_ListW: {cr}"
        )));
    }
    // A double-NUL-terminated list of NUL-terminated names: each one listed, and opened to see whether
    // it is reachable from here.
    let mut view = DeviceView::default();
    for name in buf.split(|u| *u == 0).filter(|s| !s.is_empty()) {
        view.listed = view.listed.saturating_add(1);
        let path: Vec<u16> = name.iter().copied().chain([0]).collect();
        // A query open (no access asked): it opens a plumbed device and is refused one the container
        // wasn't given (jupiter, 2026-11-06: USB, keyboard, mouse and disk classes, 0 of n opened ungranted,
        // n of n granted). SAFETY: path is a NUL-terminated wide string that outlives the call; no security
        // attributes or template; the result is a handle or INVALID_HANDLE_VALUE, checked.
        let h = unsafe {
            CreateFileW(
                path.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            )
        };
        if h != INVALID_HANDLE_VALUE && !h.is_null() {
            // SAFETY: h is the handle just opened, owned by nothing else; dropping it closes it.
            drop(unsafe { Handle::own(h) });
            view.reachable = view.reachable.saturating_add(1);
        }
    }
    Ok(view)
}

/// A device interface class as seen from where this process runs: how many of its interfaces are listed,
/// and how many of those open (a process-isolated container lists the host's, and opens only those its
/// grant plumbed in: the lead's ruling on "visible", 2026-11-06).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DeviceView {
    pub listed: usize,
    pub reachable: usize,
}
