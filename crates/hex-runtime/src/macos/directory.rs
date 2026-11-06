//! The box helper's directory work for a pool grown on demand (the lead's call, 2026-11-06): it lists the
//! pool's users, reads the ids held, makes an account from a fixed template, and keeps the peak of
//! concurrent boxes. Every tool runs by absolute path with a fixed argv and no shell; nothing a caller sends
//! reaches an argument but the account `hex_core::pool` named. No `unsafe`: these are subprocesses.
//!
//! Each account is written to the installer's manifest before it is made, so uninstall removes it even if
//! the helper stops part-way, and logged on the helper's stderr (launchd's log file).

use std::collections::BTreeSet;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Output};

use hex_core::pool::{Account, ROLE_UIDS, pool_index};

const DSCL: &str = "/usr/bin/dscl";
const DSEDITGROUP: &str = "/usr/sbin/dseditgroup";
const PS: &str = "/bin/ps";

/// Where the helper grows its pool: the pool's group, the installer's manifest, and the file it keeps the
/// peak in.
#[derive(Debug, Clone)]
pub struct Directory {
    /// The pool group's name and gid (`hexbox`, the installer's).
    pub group: String,
    pub gid: u32,
    /// The installer's manifest (`/var/db/hex-install/manifest`): each account made is a `user <name>` line.
    pub manifest: PathBuf,
    /// The peak of concurrent box-user boxes, one decimal line, kept across restarts.
    pub peak_file: PathBuf,
}

/// Runs `tool` with `args` (no shell) and returns its stdout, or an error naming the tool, its arguments and
/// its stderr.
fn run(tool: &str, args: &[&str]) -> io::Result<String> {
    let Output {
        status,
        stdout,
        stderr,
    } = Command::new(tool).args(args).output()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "{tool} {}: {status}: {}",
            args.join(" "),
            String::from_utf8_lossy(&stderr).trim()
        )));
    }
    String::from_utf8(stdout)
        .map_err(|e| io::Error::other(format!("{tool} printed non-UTF-8: {e}")))
}

/// `dscl . -list <path> <attribute>`'s lines as (name, number) pairs; a line that isn't one is skipped as
/// not a numbered record (dscl lists one record a line).
fn listed(path: &str, attribute: &str) -> io::Result<Vec<(String, u32)>> {
    Ok(run(DSCL, &[".", "-list", path, attribute])?
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let name = words.next()?;
            let n = words.next()?.parse().ok()?;
            Some((name.to_owned(), n))
        })
        .collect())
}

impl Directory {
    /// The pool's users: those `hex_core::pool` names, at a role uid, whose primary group is the pool's.
    pub fn pool_users(&self) -> io::Result<Vec<(String, u32)>> {
        let groups: Vec<(String, u32)> = listed("/Users", "PrimaryGroupID")?;
        Ok(listed("/Users", "UniqueID")?
            .into_iter()
            .filter(|(name, uid)| {
                pool_index(name).is_some()
                    && ROLE_UIDS.contains(uid)
                    && groups.iter().any(|(n, g)| n == name && *g == self.gid)
            })
            .collect())
    }

    /// Every id held now: users' uids, groups' gids and running processes' uids.
    pub fn taken_ids(&self) -> io::Result<BTreeSet<u32>> {
        let mut taken: BTreeSet<u32> = listed("/Users", "UniqueID")?
            .into_iter()
            .map(|(_, u)| u)
            .collect();
        taken.extend(
            listed("/Groups", "PrimaryGroupID")?
                .into_iter()
                .map(|(_, g)| g),
        );
        taken.extend(
            run(PS, &["-axo", "uid="])?
                .lines()
                .filter_map(|l| l.trim().parse::<u32>().ok()),
        );
        Ok(taken)
    }

    /// Makes `account` from the template: hidden, shell `/usr/bin/false`, home `/var/empty`, no password
    /// (`*`), primary group and member of the pool's group. Checks its uid is still no one's first (another
    /// installer may have taken it since install), and writes it to the manifest before making it.
    pub fn make_user(&self, account: &Account) -> io::Result<()> {
        if pool_index(&account.name).is_none() || !ROLE_UIDS.contains(&account.uid) {
            return Err(io::Error::other(format!(
                "{} at {} isn't a pool account",
                account.name, account.uid
            )));
        }
        let uid = account.uid.to_string();
        let holders = run(DSCL, &[".", "-search", "/Users", "UniqueID", &uid])?;
        if !holders.trim().is_empty() {
            return Err(io::Error::other(format!(
                "uid {uid} is taken: {}",
                holders.trim()
            )));
        }
        let mut manifest = OpenOptions::new().append(true).open(&self.manifest)?;
        writeln!(manifest, "user {}", account.name)?;
        manifest.sync_all()?;
        let record = format!("/Users/{}", account.name);
        let gid = self.gid.to_string();
        let real_name = format!("Hex box user {}", account.name);
        run(DSCL, &[".", "-create", &record])?;
        for (attribute, value) in [
            ("UniqueID", uid.as_str()),
            ("PrimaryGroupID", gid.as_str()),
            ("UserShell", "/usr/bin/false"),
            ("NFSHomeDirectory", "/var/empty"),
            ("IsHidden", "1"),
            ("Password", "*"),
            ("RealName", real_name.as_str()),
        ] {
            run(DSCL, &[".", "-create", &record, attribute, value])?;
        }
        run(
            DSEDITGROUP,
            &["-o", "edit", "-t", "user", "-a", &account.name, &self.group],
        )?;
        let _ = writeln!(
            io::stderr().lock(),
            "hex __box-helper: made box user {} (uid {uid}) for the pool",
            account.name
        );
        Ok(())
    }

    /// The kept peak, 0 when none is kept yet.
    pub fn peak(&self) -> io::Result<u32> {
        match std::fs::read_to_string(&self.peak_file) {
            Ok(text) => text
                .trim()
                .parse()
                .map_err(|e| io::Error::other(format!("{}: {e}", self.peak_file.display()))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(0),
            Err(e) => Err(e),
        }
    }

    /// Keeps `peak`, replacing the file whole (written beside it, then renamed).
    pub fn keep_peak(&self, peak: u32) -> io::Result<()> {
        let next = self.peak_file.with_extension("next");
        std::fs::write(&next, format!("{peak}\n"))?;
        std::fs::rename(&next, &self.peak_file)
    }
}
