//! CP-T7.4/decision 9: repeat actual guest SDK Hello, without acquiring guest lifecycle ownership.
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::Shutdown;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hex_api::{Class, Refusal, Runtime};
use hex_client::guest::{Client, Duplex, Handle, Link};
use hex_runtime::guest::held;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

type Error = Box<dyn std::error::Error>;

fn invalid(why: impl Into<String>) -> Refusal {
    Refusal::Invalid {
        what: "guest Hello census",
        why: why.into(),
    }
}

#[derive(Debug)]
struct Inputs {
    root: PathBuf,
    record: PathBuf,
    receipt: PathBuf,
    vsock: PathBuf,
    vsock_root: Option<PathBuf>,
    iterations: u64,
    dial_nanos: u64,
    expected: Class,
    source_revision: String,
    profile: String,
}

fn budget(nanos: u64) -> Result<Duration, Refusal> {
    let duration = Duration::from_nanos(nanos);
    finite(duration)?;
    Ok(duration)
}

fn finite(duration: Duration) -> Result<(), Refusal> {
    if duration.is_zero()
        || duration == Duration::MAX
        || Instant::now().checked_add(duration).is_none()
    {
        return Err(invalid(
            "dial budget must be positive, finite and representable by the clock",
        ));
    }
    Ok(())
}

fn parse(arguments: impl Iterator<Item = OsString>) -> Result<Inputs, Error> {
    let mut values = BTreeMap::new();
    let mut arguments = arguments;
    while let Some(flag) = arguments.next() {
        let flag = flag
            .into_string()
            .map_err(|_| invalid("flag is not UTF-8"))?;
        if !matches!(
            flag.as_str(),
            "--owned-root"
                | "--record"
                | "--receipt"
                | "--vsock"
                | "--owned-vsock-root"
                | "--iterations"
                | "--dial-nanos"
                | "--expected-class"
                | "--source-revision"
                | "--profile"
        ) {
            return Err(invalid("unknown flag").into());
        }
        let value = arguments
            .next()
            .ok_or_else(|| invalid("flag needs a value"))?;
        if values.insert(flag, value).is_some() {
            return Err(invalid("duplicate flag").into());
        }
    }
    let required = |name: &str| {
        values
            .get(name)
            .ok_or_else(|| invalid(format!("{name} is required")))
    };
    let text = |name: &str| {
        required(name)?
            .to_str()
            .ok_or_else(|| invalid(format!("{name} is not UTF-8")))
    };
    let iterations: u64 = text("--iterations")?.parse()?;
    if iterations == 0 {
        return Err(invalid("iterations must be positive").into());
    }
    let dial_nanos: u64 = text("--dial-nanos")?.parse()?;
    budget(dial_nanos)?;
    let expected = match text("--expected-class")? {
        "oci" => Class::Oci,
        "process" => Class::Process,
        _ => {
            return Err(invalid(
                "expected class must be oci or process, the actual Runtime API classes",
            )
            .into());
        }
    };
    let source_revision = text("--source-revision")?;
    // A full Git SHA-1, matching the source-revision receipts used by the existing census fixture.
    if source_revision.len() != 40
        || !source_revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(
            invalid("source revision must be the full lowercase 40-hex Git revision").into(),
        );
    }
    let profile = text("--profile")?;
    if !matches!(profile, "debug" | "release") {
        return Err(invalid("profile must be explicitly debug or release").into());
    }
    Ok(Inputs {
        root: PathBuf::from(required("--owned-root")?),
        record: PathBuf::from(required("--record")?),
        receipt: PathBuf::from(required("--receipt")?),
        vsock: PathBuf::from(required("--vsock")?),
        vsock_root: values.get("--owned-vsock-root").map(PathBuf::from),
        iterations,
        dial_nanos,
        expected,
        source_revision: source_revision.to_owned(),
        profile: profile.to_owned(),
    })
}

fn contained(root: &Path, path: &Path) -> Result<(), Error> {
    if !path.is_absolute()
        || !path.starts_with(root)
        || path == root
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(invalid(
            "record/receipt/census/socket path must be explicitly inside its private root",
        )
        .into());
    }
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(invalid("owned path must not be a symbolic link").into());
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("owned path has no parent"))?;
    if !parent.canonicalize()?.starts_with(root) {
        return Err(invalid("owned path escapes through a parent link").into());
    }
    Ok(())
}

#[cfg(unix)]
fn private_root(path: &Path, owner: Option<u32>) -> Result<(PathBuf, u32), Error> {
    let meta = fs::symlink_metadata(path)?;
    // POSIX mode bits are an access invariant, not a resource/timing default.
    if !path.is_absolute()
        || !meta.file_type().is_dir()
        || meta.mode() & 0o777 != 0o700
        || owner.is_some_and(|uid| uid != meta.uid())
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(invalid(
            "private root must be an absolute non-link 0700 directory with the same owner",
        )
        .into());
    }
    Ok((path.canonicalize()?, meta.uid()))
}

#[cfg(not(unix))]
fn private_root(_path: &Path, _owner: Option<u32>) -> Result<(PathBuf, u32), Error> {
    Err(Refusal::Platform {
        what: "guest Hello census private run",
        platform: std::env::consts::OS,
        why: "this benchmark's private-directory permission proof currently requires Unix".into(),
    }
    .into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    record: PathBuf,
    pid: u32,
    started: u64,
    vsock: PathBuf,
    #[serde(default)]
    owned_vsock_root: Option<PathBuf>,
    declared_config: Value,
}

fn bounded_json(path: &Path) -> Result<(Receipt, Vec<u8>), Error> {
    let limit = u64::try_from(hex_core::wire::MAX_MESSAGE)?
        .checked_add(1)
        .ok_or_else(|| invalid("receipt bound overflow"))?;
    let mut bytes = Vec::new();
    File::open(path)?.take(limit).read_to_end(&mut bytes)?;
    if bytes.len() > hex_core::wire::MAX_MESSAGE {
        return Err(invalid("receipt exceeds the existing wire bound").into());
    }
    Ok((serde_json::from_slice(&bytes)?, bytes))
}

struct Verified {
    root: PathBuf,
    identity: shards_api::Identity,
    receipt_sha256: String,
    record_sha256: String,
    config: Value,
}

fn verify(input: &Inputs) -> Result<Verified, Error> {
    let (root, uid) = private_root(&input.root, None)?;
    contained(&root, &input.record)?;
    contained(&root, &input.receipt)?;
    let (receipt, bytes) = bounded_json(&input.receipt)?;
    let identity = held::read(&input.record)?
        .ok_or_else(|| invalid("the private run has no recorded guest"))?;
    if receipt.record != input.record
        || receipt.pid != identity.pid
        || receipt.started != identity.started
        || receipt.vsock != identity.vsock
        || input.vsock != identity.vsock
        || identity.pid == 0
        || identity.started == 0
    {
        return Err(invalid("explicit record/receipt/socket identities disagree").into());
    }
    let allowed = match &input.vsock_root {
        Some(path) => Some(private_root(path, Some(uid))?.0),
        None => None,
    };
    if allowed != receipt.owned_vsock_root {
        return Err(invalid("owned vsock root differs from its receipt").into());
    }
    contained(allowed.as_deref().unwrap_or(&root), &identity.vsock)?;
    let record_sha256 = digest(&input.record)?;
    Ok(Verified {
        root,
        identity,
        receipt_sha256: format!("{:x}", Sha256::digest(bytes)),
        record_sha256,
        config: receipt.declared_config,
    })
}

struct Hashing(Sha256);
impl Write for Hashing {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn digest(path: &Path) -> Result<String, Error> {
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut hash = Hashing(Sha256::new());
    if io::copy(&mut file.take(size), &mut hash)? != size {
        return Err(invalid("hashed input shortened while reading").into());
    }
    Ok(format!("{:x}", hash.0.finalize()))
}

fn census_path(root: &Path, iterations: u64) -> Result<(PathBuf, u64), Error> {
    let path = PathBuf::from(
        std::env::var_os(hex_node::census::ENV_FILE)
            .ok_or_else(|| invalid("explicit census ENV_FILE is required"))?,
    );
    let max = std::env::var_os(hex_node::census::ENV_MAX)
        .ok_or_else(|| invalid("explicit census ENV_MAX is required"))?;
    let max: u64 = max
        .to_str()
        .ok_or_else(|| invalid("census row bound is not UTF-8"))?
        .parse()?;
    if max == 0 || max < iterations {
        return Err(
            invalid("census row bound must hold at least the requested Hello count").into(),
        );
    }
    contained(root, &path)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    drop(options.open(&path)?);
    if !hex_node::census::install_from_env()? {
        return Err(invalid("SDK census installation did not succeed").into());
    }
    Ok((path, max))
}

// ShardsStream's tuple constructor is private and ShardsLink requires a lifecycle-owning Guest.
// This benchmark-only adapter forwards the existing stream; it introduces no copy/unsafe/FD trick.
struct BorrowedStream(Box<dyn shards_api::Stream>);
impl Read for BorrowedStream {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        self.0.read(bytes)
    }
}
impl Write for BorrowedStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}
impl Duplex for BorrowedStream {
    fn try_clone(&self) -> io::Result<Box<dyn Duplex>> {
        Ok(Box::new(Self(self.0.try_clone()?)))
    }
    fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.0.shutdown(how)
    }
    fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_write_timeout(timeout)
    }
    fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.0.set_read_timeout(timeout)
    }
    fn handle(&self) -> Handle<'_> {
        self.0.handle()
    }
    fn set_nonblocking(&self, on: bool) -> io::Result<()> {
        self.0.set_nonblocking(on)
    }
}

struct BorrowedLink {
    identity: shards_api::Identity,
    patience: Duration,
}
impl Link for BorrowedLink {
    fn dial(&self, port: u32) -> Result<Box<dyn Duplex>, Refusal> {
        if port != hex_core::stream::CONTROL_PORT {
            return Err(invalid(
                "this Hello-only helper dials only the control port",
            ));
        }
        finite(self.patience)?;
        // Existing API checks PID+start before/after CONNECT. This never acquires a Guest owner.
        let stream =
            shards_api::connect_borrowed(&self.identity, port, self.patience).map_err(|why| {
                hex_client::guest::shards::refusal("borrowing the census guest connection", &why)
            })?;
        Ok(Box::new(BorrowedStream(stream)))
    }
}

fn run(input: Inputs) -> Result<Value, Error> {
    let verified = verify(&input)?;
    let executable = std::env::current_exe()?.canonicalize()?;
    let binary_sha256 = digest(&executable)?;
    let (census, max_rows) = census_path(&verified.root, input.iterations)?;
    let link: Arc<dyn Link> = Arc::new(BorrowedLink {
        identity: verified.identity.clone(),
        patience: budget(input.dial_nanos)?,
    });
    let (mut hello_wire_successes, mut class_validated) = (0u64, 0u64);
    let mut last_classes = Vec::new();
    let mut error = None;
    for _ in 0..input.iterations {
        match Client::connect(Arc::clone(&link)) {
            Ok(client) => {
                hello_wire_successes = hello_wire_successes
                    .checked_add(1)
                    .ok_or_else(|| invalid("Hello counter overflow"))?;
                let classes = client.classes();
                let expected = classes.contains(&input.expected);
                last_classes = classes.iter().map(|class| class.name()).collect();
                drop(client);
                if !expected {
                    error = Some(
                        "Hello did not advertise the explicitly expected API class".to_owned(),
                    );
                    break;
                }
                class_validated = class_validated
                    .checked_add(1)
                    .ok_or_else(|| invalid("class counter overflow"))?;
            }
            Err(why) => {
                error = Some(why.to_string());
                break;
            }
        }
    }
    let completed = error.is_none() && class_validated == input.iterations;
    Ok(
        json!({"kind":"actual-guest-sdk-hello-census","status":if completed {"completed"} else {"failed"},"error":error,
        "source_base":"bba3d814cfa8373d589482dfbaa38b9bfa792d03","source_revision":input.source_revision,
        "profile":input.profile,"declaration_authority":"source revision/profile are operator supplied; root verifies build receipt",
        "binary":{"path":executable,"sha256":binary_sha256},"owned_root":verified.root,
        "record":input.record,"record_sha256":verified.record_sha256,"receipt":input.receipt,"receipt_sha256":verified.receipt_sha256,
        "identity":{"pid":verified.identity.pid,"started":verified.identity.started,"vsock":verified.identity.vsock},
        "owned_vsock_root":input.vsock_root,"declared_config":verified.config,
        "iterations":input.iterations,"dial_budget_nanos":input.dial_nanos,"expected_class":input.expected.name(),
        "hello_wire_successes":hello_wire_successes,"class_validated_hellos":class_validated,"last_advertised_classes":last_classes,
        "census":{"path":census,"row_bound":max_rows,"producer":"existing SDK hooks only"},
        "hello_wait":"SDK measuring; root must own subprocess outer cancellation, kill/reap and guest cleanup",
        "workload_success":"not exercised by Hello","seed_installation":"not performed","guest_lifecycle_owned":false}),
    )
}

fn main() -> ExitCode {
    let result = parse(std::env::args_os().skip(1)).and_then(run);
    let value = match result {
        Ok(value) => value,
        Err(why) => {
            json!({"kind":"actual-guest-sdk-hello-census","status":"failed","error":why.to_string(),"seed_installation":"not performed"})
        }
    };
    let completed = value.get("status").and_then(Value::as_str) == Some("completed");
    let wrote = serde_json::to_writer(io::stdout().lock(), &value)
        .map_err(io::Error::other)
        .and_then(|()| io::stdout().lock().write_all(b"\n"));
    if completed && wrote.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_macros)]
mod tests {
    use super::*;

    fn arguments() -> Vec<OsString> {
        [
            "--owned-root",
            "/owned",
            "--record",
            "/owned/guest",
            "--receipt",
            "/owned/receipt.json",
            "--vsock",
            "/owned/v",
            "--iterations",
            "1",
            "--dial-nanos",
            "1",
            "--expected-class",
            "oci",
            "--source-revision",
            "bba3d814cfa8373d589482dfbaa38b9bfa792d03",
            "--profile",
            "debug",
        ]
        .into_iter()
        .map(OsString::from)
        .collect()
    }

    #[test]
    fn cp_t7_4_hello_arguments_require_explicit_finite_counts_and_budget() {
        assert!(parse(arguments().into_iter()).is_ok());
        for (flag, bad) in [
            ("--iterations", "0"),
            ("--dial-nanos", "0"),
            ("--dial-nanos", "18446744073709551616"),
            ("--expected-class", "shards"),
            ("--profile", "guessed"),
        ] {
            let mut values = arguments();
            let at = values
                .iter()
                .position(|value| value == flag)
                .expect("test flag");
            values[at + 1] = OsString::from(bad);
            assert!(parse(values.into_iter()).is_err());
        }
        assert!(finite(Duration::ZERO).is_err());
        assert!(finite(Duration::MAX).is_err());
        assert!(parse(Vec::new().into_iter()).is_err());
        let mut duplicate = arguments();
        duplicate.extend([OsString::from("--iterations"), OsString::from("1")]);
        assert!(parse(duplicate.into_iter()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn cp_t7_4_hello_ownership_refuses_escape_links_and_foreign_receipt_without_dial()
    -> Result<(), Error> {
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
        let parent = PathBuf::from(
            std::env::var_os("HELLO_CENSUS_TEST_ROOT")
                .ok_or("explicit owned test root required")?,
        );
        let root = parent.join(format!("hello-ownership-{}", std::process::id()));
        fs::DirBuilder::new().mode(0o700).create(&root)?;
        struct Remove(PathBuf);
        impl Drop for Remove {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _remove = Remove(root.clone());
        let (canonical, _) = private_root(&root, None)?;
        assert!(contained(&canonical, &canonical.join("..")).is_err());
        let linked = canonical.join("linked");
        symlink(&canonical, &linked)?;
        assert!(contained(&canonical, &linked).is_err());
        assert!(contained(&canonical, &linked.join("outside")).is_ok()); // Same-root parent link stays contained.
        let escape = canonical.join("escape");
        symlink(parent.canonicalize()?, &escape)?;
        assert!(contained(&canonical, &escape.join("outside")).is_err());
        let mut values = arguments();
        for (flag, path) in [
            ("--owned-root", canonical.clone()),
            ("--record", canonical.join("guest")),
            ("--receipt", canonical.join("receipt.json")),
            ("--vsock", canonical.join("v")),
        ] {
            let at = values
                .iter()
                .position(|value| value == flag)
                .ok_or("test flag absent")?;
            values[at + 1] = path.into_os_string();
        }
        let input = parse(values.into_iter())?;
        held::write(
            &input.record,
            &shards_api::Identity {
                pid: 1,
                started: 1,
                vsock: input.vsock.clone(),
            },
        )?;
        fs::write(
            &input.receipt,
            serde_json::to_vec(&json!({"record":input.record,"pid":2,"started":1,
            "vsock":input.vsock,"owned_vsock_root":null,"declared_config":{}}))?,
        )?;
        assert!(verify(&input).is_err());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755))?;
        assert!(private_root(&root, None).is_err());
        Ok(())
    }
}
