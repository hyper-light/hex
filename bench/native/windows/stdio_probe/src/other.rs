//! `refuse-as <user> [node socket]`: the refusal check with the intruder a process of another user (native
//! plan step 8: only the caller may read or write its streams, and the node serves only its own user). The
//! user is a temporary standard account made for the run; its password comes on stdin, is held in memory
//! only, and is never printed. Its processes are started with `CreateProcessWithLogonW`, its profile not
//! loaded. Everything they touch is in `HEX_PROBE_DIR`, a folder they may read and write.
//!
//! What is checked, each printed as `refuse-as <check>: ...`:
//! - `acl`: whether the user can connect to an AF_UNIX socket of this user's at all (afunix.sys checks the
//!   socket file's write access at connect), the file this user's and SYSTEM's alone, nothing granted;
//! - `conn` and `pipe`: with the user granted the socket file, the node's own check (the peer's pid for a
//!   connection, `GetNamedPipeClientProcessId` for a pipe) refuses it while the caller is served;
//! - `node` (where a `hex node` socket is given): the user knocks on the node's socket, granted the file, and
//!   the node refuses it by name (its peer's user isn't the node's).

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::ptr::null;
use std::time::{Duration, Instant};

use shards_afunix::{UnixListener, UnixStream};
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CreateProcessWithLogonW, PROCESS_INFORMATION, STARTUPINFOW,
    WaitForSingleObject,
};

use super::{CANDIDATES, CHUNK, Waits, peer_pid, receive, serve};

/// A process started as the other user, waited out by its handle.
struct AsUser(HANDLE);

impl Waits for AsUser {
    fn wait(&mut self) {
        // SAFETY: the process handle CreateProcessWithLogonW returned, owned here; at most 60 s.
        let r = unsafe { WaitForSingleObject(self.0, 60_000) };
        assert!(
            r == WAIT_OBJECT_0,
            "the other user's process didn't end in 60 s"
        );
    }
}

impl Drop for AsUser {
    fn drop(&mut self) {
        // SAFETY: owned here, closed once.
        unsafe { CloseHandle(self.0) };
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// Starts this probe with `args` as `user`, its profile not loaded, in `dir`.
fn spawn_as(user: &str, password: &[u16], dir: &Path, args: &[&str]) -> AsUser {
    let exe = std::env::current_exe().unwrap();
    let mut line = format!("\"{}\"", exe.display());
    for a in args {
        line.push_str(&format!(" \"{a}\""));
    }
    let mut cmd = wide(&line);
    let (u, d, cwd) = (wide(user), wide("."), wide(dir.to_str().unwrap()));
    // SAFETY: an all-zero STARTUPINFOW with its size is a valid start (no handles given).
    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = size_of::<STARTUPINFOW>() as u32;
    // SAFETY: written by the call.
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: every string is NUL-terminated and alive for the call; the command line is writable as the
    // call requires; flags 0 (no profile loaded); no environment block (the user's default).
    let ok = unsafe {
        CreateProcessWithLogonW(
            u.as_ptr(),
            d.as_ptr(),
            password.as_ptr(),
            0,
            null(),
            cmd.as_mut_ptr(),
            CREATE_NO_WINDOW,
            null(),
            cwd.as_ptr(),
            &si,
            &mut pi,
        )
    };
    assert!(
        ok != 0,
        "CreateProcessWithLogonW: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: the thread handle is ours and not needed.
    unsafe { CloseHandle(pi.hThread) };
    AsUser(pi.hProcess)
}

/// Leaves `path` this user's and SYSTEM's alone (inherited entries removed): what a socket file in a folder
/// of this user's own would have, whatever the shared folder grants.
fn restrict(path: &Path) {
    let me = std::env::var("USERNAME").unwrap();
    let out = std::process::Command::new("icacls")
        .arg(path)
        .args([
            "/inheritance:r",
            "/grant:r",
            &format!("{me}:F"),
            "*S-1-5-18:F",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "icacls: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// Gives `user` read and write on `path` (icacls), so a connect reaches the node's own check.
fn grant(path: &Path, user: &str) {
    let out = std::process::Command::new("icacls")
        .arg(path)
        .args(["/grant", &format!("{user}:(R,W)")])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "icacls: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

fn read_result(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| format!("no result ({e})"))
}

pub fn refuse_as(user: &str, node: Option<&str>) {
    let mut pass = String::new();
    std::io::stdin().lock().read_line(&mut pass).unwrap();
    // Without what a PowerShell pipe adds around it (a byte-order mark, the line's end); the password holds
    // neither (base64 and four characters, other_user.ps1).
    let given = pass.trim_start_matches('\u{feff}').trim();
    // Its length alone, so a password the pipe changed shows without the password being printed.
    println!(
        "refuse-as: a password of {} characters came on stdin",
        given.chars().count()
    );
    let password = wide(given);
    drop(pass);
    let dir = PathBuf::from(std::env::var("HEX_PROBE_DIR").expect("HEX_PROBE_DIR"));
    let path = dir.join(format!("s-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path).unwrap();
    restrict(&path);
    let p = path.to_str().unwrap().to_owned();

    // acl: the user knocks before anything is granted.
    let result = dir.join("acl.result");
    let _ = std::fs::remove_file(&result);
    spawn_as(
        user,
        &password,
        &dir,
        &["knock", &p, result.to_str().unwrap()],
    )
    .wait();
    let seen = read_result(&result);
    if seen.starts_with("connected") {
        drop(listener.accept().unwrap());
    }
    println!(
        "refuse-as acl: another user's connect to this user's socket file, nothing granted: {seen}"
    );

    grant(&path, user);
    let me = std::process::id();
    let payload = vec![0x5au8; CHUNK];
    let mut n = 0u32;
    for c in [1u8, 2u8] {
        let mut mine = UnixStream::connect(&path).unwrap();
        let (mut ctl, ()) = listener.accept().unwrap();
        assert_eq!(peer_pid(&ctl), me);
        let name = CANDIDATES[c as usize];
        let result = dir.join(format!("{name}.result"));
        let _ = std::fs::remove_file(&result);
        let r = result.to_str().unwrap().to_owned();
        let (p2, pw, d2) = (p.clone(), password.clone(), dir.clone());
        let intruder = move |door: &str| -> Box<dyn Waits> {
            Box::new(spawn_as(user, &pw, &d2, &["intrude", name, &p2, door, &r]))
        };
        let served = std::thread::scope(|sc| {
            let h = sc.spawn(|| {
                let mut buf = vec![0u8; CHUNK];
                receive(
                    &mut mine,
                    &path,
                    c as usize,
                    1 << 20,
                    &mut buf,
                    Instant::now(),
                )
                .2
            });
            let refused = serve(
                &listener,
                &mut ctl,
                me,
                c,
                1 << 20,
                &payload,
                &mut n,
                Some(&intruder),
            );
            (refused, h.join().unwrap())
        });
        println!(
            "refuse-as {name}: another user's process {} ; it says \"{}\" ; the caller got {} of {} bytes",
            if served.0 {
                "was REFUSED"
            } else {
                "WAS NOT REFUSED"
            },
            read_result(&result),
            served.1,
            1 << 20
        );
    }
    drop(listener);
    let _ = std::fs::remove_file(&path);

    if let Some(node) = node {
        let node = Path::new(node);
        grant(node, user);
        let result = dir.join("node.result");
        let _ = std::fs::remove_file(&result);
        spawn_as(
            user,
            &password,
            &dir,
            &["knock", node.to_str().unwrap(), result.to_str().unwrap()],
        )
        .wait();
        println!(
            "refuse-as node: another user's connect to hex node's socket, granted the file: {}",
            read_result(&result)
        );
    }
}

/// Connects to the AF_UNIX socket at `path` and writes what happened to `result`: `connected`, then
/// whatever came before the other end closed (as text), or why the connect failed.
pub fn knock(path: &str, result: &str) {
    let line = match UnixStream::connect(path) {
        Err(e) => format!("refused at connect: {e}"),
        Ok(mut s) => {
            let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
            let mut got = Vec::new();
            let mut buf = [0u8; 4096];
            while let Ok(k) = s.read(&mut buf) {
                if k == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..k]);
            }
            let text: String = String::from_utf8_lossy(&got)
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            format!(
                "connected; then {} bytes, then the end: {}",
                got.len(),
                text.trim()
            )
        }
    };
    let mut f = std::fs::File::create(result).unwrap();
    f.write_all(line.as_bytes()).unwrap();
}
