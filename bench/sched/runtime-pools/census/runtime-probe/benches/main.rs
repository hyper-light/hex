//! Cache-only lead-run probe: actual Runtime inspection and identity-checked owned guest retirement.
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use hex_api::{BoxId, Refusal, Runtime};
use hex_runtime::guest::{GuestConfig, held};
use serde::{Deserialize, Serialize};
use shards_api::Guests;

type Error = Box<dyn std::error::Error>;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    record: PathBuf,
    pid: u32,
    started: u64,
    vsock: PathBuf,
    #[serde(default)]
    owned_vsock_root: Option<PathBuf>,
    declared_config: serde_json::Value,
}

fn invalid(why: impl Into<String>) -> Refusal {
    Refusal::Invalid {
        what: "runtime census probe",
        why: why.into(),
    }
}

fn owned(root: &Path, path: &Path) -> Result<(), Error> {
    if !path.is_absolute()
        || !path.starts_with(root)
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(
            invalid("guest record/receipt/socket must be contained in this private run").into(),
        );
    }
    if let Some(parent) = path.parent()
        && parent.exists()
        && !parent.canonicalize()?.starts_with(root)
    {
        return Err(invalid("owned path escapes through a parent link").into());
    }
    if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(invalid("owned guest paths must not be symbolic links").into());
    }
    Ok(())
}

#[cfg(unix)]
fn vsock_root(root: &Path, allowed: &Path, vsock: &Path) -> Result<PathBuf, Error> {
    if !allowed.is_absolute()
        || allowed
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(invalid("owned vsock root must be an explicit absolute directory").into());
    }
    let metadata = fs::symlink_metadata(allowed)?;
    let authority = fs::metadata(root)?;
    // POSIX permission bits: only this run's owner may access either private directory.
    if !metadata.file_type().is_dir()
        || metadata.mode() & 0o777 != 0o700
        || !authority.is_dir()
        || authority.mode() & 0o777 != 0o700
        || metadata.uid() != authority.uid()
    {
        return Err(invalid("owned vsock root must be a non-link 0700 directory owned by the private run root's owner").into());
    }
    let allowed = allowed.canonicalize()?;
    if !vsock.is_absolute()
        || vsock
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        || vsock.file_name() != Some(std::ffi::OsStr::new("v"))
    {
        return Err(invalid("owned vsock must be the exact allowed directory's v socket").into());
    }
    let parent = vsock
        .parent()
        .ok_or_else(|| invalid("owned vsock has no parent"))?;
    if !fs::symlink_metadata(parent)?.file_type().is_dir() || parent.canonicalize()? != allowed {
        return Err(
            invalid("owned vsock parent differs from the explicitly allowed directory").into(),
        );
    }
    // macOS spells /private/tmp as /tmp; bind the receipt to its canonical authority.
    owned(&allowed, &allowed.join("v"))?;
    Ok(allowed)
}

#[cfg(not(unix))]
fn vsock_root(_root: &Path, _allowed: &Path, _vsock: &Path) -> Result<PathBuf, Error> {
    Err(
        invalid("an external owned vsock root requires Unix ownership and mode verification")
            .into(),
    )
}

fn declared(config: &GuestConfig) -> Result<serde_json::Value, Error> {
    #[derive(Serialize)]
    struct Share<'a> {
        dir: &'a Path,
        read_only: bool,
    }
    #[derive(Serialize)]
    struct Declared<'a> {
        binary: &'a Path,
        kernel: &'a Path,
        initrd: &'a Path,
        vcpus: u32,
        memory_mib: u64,
        shares: Vec<Share<'a>>,
    }
    Ok(serde_json::to_value(Declared {
        binary: &config.binary,
        kernel: &config.kernel,
        initrd: &config.initrd,
        vcpus: config.vcpus,
        memory_mib: config.memory_mib,
        shares: config
            .shares
            .iter()
            .map(|share| Share {
                dir: &share.dir,
                read_only: share.read_only,
            })
            .collect(),
    })?)
}

fn parse() -> Result<BTreeMap<String, OsString>, Error> {
    let mut values = BTreeMap::new();
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let flag = flag
            .into_string()
            .map_err(|_| invalid("flag is not UTF-8"))?;
        if !matches!(
            flag.as_str(),
            "--socket"
                | "--inspect"
                | "--delete"
                | "--remember-guest"
                | "--stop-owned-guest"
                | "--owned-root"
                | "--owned-vsock-root"
                | "--receipt"
                | "--wait-nanos"
        ) {
            return Err(invalid("unknown probe flag").into());
        }
        let value = args
            .next()
            .ok_or_else(|| invalid("probe flag needs a value"))?;
        if values.insert(flag, value).is_some() {
            return Err(invalid("probe flag was repeated").into());
        }
    }
    Ok(values)
}

fn required<'a>(values: &'a BTreeMap<String, OsString>, flag: &str) -> Result<&'a OsString, Error> {
    values
        .get(flag)
        .ok_or_else(|| invalid(format!("{flag} is required")).into())
}

fn guest(values: &BTreeMap<String, OsString>, stop: bool) -> Result<serde_json::Value, Error> {
    let root = PathBuf::from(required(values, "--owned-root")?).canonicalize()?;
    let flag = if stop {
        "--stop-owned-guest"
    } else {
        "--remember-guest"
    };
    let record = PathBuf::from(required(values, flag)?);
    let receipt_path = PathBuf::from(required(values, "--receipt")?);
    owned(&root, &record)?;
    owned(&root, &receipt_path)?;
    let identity =
        held::read(&record)?.ok_or_else(|| invalid("this run has no recorded guest identity"))?;
    let owned_vsock_root = if let Some(allowed) = values.get("--owned-vsock-root") {
        Some(vsock_root(&root, &PathBuf::from(allowed), &identity.vsock)?)
    } else {
        owned(&root, &identity.vsock)?;
        None
    };
    let config = GuestConfig::from_env()?;
    let declared_config = declared(&config)?;
    if !stop {
        let receipt = Receipt {
            record,
            pid: identity.pid,
            started: identity.started,
            vsock: identity.vsock,
            owned_vsock_root,
            declared_config,
        };
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&receipt_path)?;
        serde_json::to_writer(&mut output, &receipt)?;
        output.write_all(b"\n")?;
        output.sync_all()?;
        return Ok(
            serde_json::json!({"remembered":true,"pid":receipt.pid,"started":receipt.started}),
        );
    }
    let mut data = Vec::new();
    File::open(&receipt_path)?
        .take(
            u64::try_from(hex_core::wire::MAX_MESSAGE)?
                .checked_add(1)
                .ok_or_else(|| invalid("receipt byte bound overflow"))?,
        )
        .read_to_end(&mut data)?;
    if data.len() > hex_core::wire::MAX_MESSAGE {
        return Err(invalid("guest receipt exceeds wire bound").into());
    }
    let receipt: Receipt = serde_json::from_slice(&data)?;
    if receipt.record != record
        || receipt.pid != identity.pid
        || receipt.started != identity.started
        || receipt.vsock != identity.vsock
        || receipt.owned_vsock_root != owned_vsock_root
        || receipt.declared_config != declared_config
    {
        return Err(invalid(
            "guest identity/configuration differs from this run's ownership receipt",
        )
        .into());
    }
    let nanos: u64 = required(values, "--wait-nanos")?
        .to_str()
        .ok_or_else(|| invalid("wait is not text"))?
        .parse()?;
    if nanos == 0 {
        return Err(invalid("operator wait budget must be positive").into());
    }
    match shards_api::Shards::new().attach(&identity, &config.spec()) {
        Ok(guest) => {
            guest.stop()?;
            let end = guest
                .wait_for(Duration::from_nanos(nanos))?
                .ok_or_else(|| invalid("owned guest did not end within operator budget"))?;
            Ok(
                serde_json::json!({"guest_stopped":true,"pid":identity.pid,"started":identity.started,"end":format!("{end:?}")}),
            )
        }
        Err(shards_api::Refusal::Gone { .. } | shards_api::Refusal::Ended(_)) => Ok(
            serde_json::json!({"guest_stopped":true,"pid":identity.pid,"started":identity.started,"already_gone":true}),
        ),
        Err(why) => Err(why.into()),
    }
}

fn main() -> Result<(), Error> {
    hex_node::census::install_from_env()?;
    let values = parse()?;
    let output = if values.contains_key("--remember-guest")
        || values.contains_key("--stop-owned-guest")
    {
        if values.contains_key("--remember-guest") && values.contains_key("--stop-owned-guest") {
            return Err(invalid("guest modes are mutually exclusive").into());
        }
        guest(&values, values.contains_key("--stop-owned-guest"))?
    } else {
        let socket = PathBuf::from(required(&values, "--socket")?);
        let client = hex_client::Client::connect(&socket)?;
        if let Some(id) = values.get("--delete") {
            client.delete(&BoxId::new(
                id.to_str().ok_or_else(|| invalid("box ID is not text"))?,
            )?)?;
        }
        let inspected = values
            .get("--inspect")
            .map(|id| -> Result<_, Error> {
                Ok(client.inspect(&BoxId::new(
                    id.to_str().ok_or_else(|| invalid("box ID is not text"))?,
                )?)?)
            })
            .transpose()?;
        serde_json::json!({"classes":client.classes(),"capacity":client.capacity(hex_api::Class::Oci)?,"boxes":client.list()?,"inspected":inspected})
    };
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer(&mut stdout, &output)?;
    stdout.write_all(b"\n")?;
    Ok(())
}
