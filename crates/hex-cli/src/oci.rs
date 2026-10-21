//! `hex oci <verb>`: the OCI runtime command line, driving `hex_runtime`'s `oci` class. It is
//! runtime-spec compatible, so opencontainers/runtime-tools drives it as it drives runc or crun
//! (AC-T7.1.4). `hex __oci-init` is the re-exec'd container init and is handled in `main`.

use std::io::Write;
use std::path::PathBuf;

use hex_runtime::linux::oci::OciError;
use hex_runtime::linux::oci::run::Runtime;

/// Runs an `oci` subcommand (`args` is everything after `oci`); returns the process exit code.
pub fn main(args: &[String]) -> u8 {
    match run(args) {
        Ok(output) => {
            if !output.is_empty() {
                let _ = std::io::stdout().lock().write_all(output.as_bytes());
            }
            0
        }
        Err(e) => {
            let _ = writeln!(std::io::stderr().lock(), "hex oci: {e}");
            1
        }
    }
}

fn run(args: &[String]) -> Result<String, OciError> {
    let rt = Runtime::new(Runtime::default_root());
    let (verb, rest) = args.split_first().ok_or_else(usage)?;
    match verb.as_str() {
        "create" => {
            let (id, bundle, pid_file) = parse_create(rest)?;
            rt.create(&id, &bundle, pid_file.as_deref())?;
            Ok(String::new())
        }
        "start" => {
            rt.start(&one_id(rest)?)?;
            Ok(String::new())
        }
        "state" => {
            let state = rt.state(&one_id(rest)?)?;
            String::from_utf8(state.to_json()?)
                .map_err(|e| OciError::Broken(format!("state isn't utf-8: {e}")))
        }
        "kill" => {
            let (id, sig, all) = parse_kill(rest)?;
            rt.kill(&id, sig, all)?;
            Ok(String::new())
        }
        "delete" => {
            let (id, force) = parse_delete(rest)?;
            rt.delete(&id, force)?;
            Ok(String::new())
        }
        other => Err(OciError::Config(format!(
            "unknown oci command {other:?}; {}",
            usage()
        ))),
    }
}

fn usage() -> OciError {
    OciError::Config("usage: hex oci <create|start|state|kill|delete> [flags] <id>".to_owned())
}

/// Splits `rest` into flags (a leading run of `--flag [value]`) and positionals, taking the value
/// of each flag in `with_value`.
fn parse_flags<'a>(
    rest: &'a [String],
    with_value: &[&str],
) -> (Vec<(String, Option<String>)>, Vec<&'a String>) {
    let mut flags = Vec::new();
    let mut positionals = Vec::new();
    let mut i = 0;
    while let Some(arg) = rest.get(i) {
        if let Some(name) = arg.strip_prefix("--") {
            if let Some((k, v)) = name.split_once('=') {
                flags.push((k.to_owned(), Some(v.to_owned())));
            } else if with_value.contains(&name) {
                let v = rest.get(i.saturating_add(1)).cloned();
                flags.push((name.to_owned(), v));
                i = i.saturating_add(1);
            } else {
                flags.push((name.to_owned(), None));
            }
        } else {
            positionals.push(arg);
        }
        i = i.saturating_add(1);
    }
    (flags, positionals)
}

fn parse_create(rest: &[String]) -> Result<(String, PathBuf, Option<PathBuf>), OciError> {
    let (flags, positionals) = parse_flags(rest, &["bundle", "pid-file", "console-socket"]);
    let value = |name: &str| {
        flags
            .iter()
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| v.clone())
    };
    let bundle = value("bundle")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let pid_file = value("pid-file").map(PathBuf::from);
    let id = positionals
        .first()
        .ok_or_else(|| OciError::Config("create needs a container id".to_owned()))?;
    Ok(((*id).clone(), bundle, pid_file))
}

fn one_id(rest: &[String]) -> Result<String, OciError> {
    let (_, positionals) = parse_flags(rest, &[]);
    positionals
        .first()
        .map(|s| (*s).clone())
        .ok_or_else(|| OciError::Config("a container id is needed".to_owned()))
}

fn parse_kill(rest: &[String]) -> Result<(String, i32, bool), OciError> {
    let (flags, positionals) = parse_flags(rest, &[]);
    let all = flags.iter().any(|(k, _)| k == "all");
    let id = positionals
        .first()
        .map(|s| (*s).clone())
        .ok_or_else(|| OciError::Config("kill needs a container id".to_owned()))?;
    // The signal defaults to TERM, as runc's does; a second positional overrides it.
    let sig = match positionals.get(1) {
        Some(s) => signal_number(s)?,
        None => 15,
    };
    Ok((id, sig, all))
}

fn parse_delete(rest: &[String]) -> Result<(String, bool), OciError> {
    let (flags, positionals) = parse_flags(rest, &[]);
    let force = flags.iter().any(|(k, _)| k == "force");
    let id = positionals
        .first()
        .map(|s| (*s).clone())
        .ok_or_else(|| OciError::Config("delete needs a container id".to_owned()))?;
    Ok((id, force))
}

/// A signal's number from its name (`KILL`, `SIGKILL`) or a decimal number.
fn signal_number(s: &str) -> Result<i32, OciError> {
    if let Ok(n) = s.parse::<i32>() {
        return Ok(n);
    }
    let name = s.strip_prefix("SIG").unwrap_or(s).to_ascii_uppercase();
    let n = match name.as_str() {
        "HUP" => 1,
        "INT" => 2,
        "QUIT" => 3,
        "KILL" => 9,
        "USR1" => 10,
        "USR2" => 12,
        "TERM" => 15,
        "STOP" => 19,
        "CONT" => 18,
        other => return Err(OciError::Config(format!("unknown signal {other:?}"))),
    };
    Ok(n)
}

/// Runs the container init (`hex __oci-init`). Only reached on Linux.
pub fn init() -> u8 {
    hex_runtime::linux::oci::init::run()
}
