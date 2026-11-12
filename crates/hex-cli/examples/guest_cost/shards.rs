//! The Shards class's driver (macOS HVF, Linux KVM): its guest booted by the configured `shards`, as a node
//! boots it, with runtime folders of the run's own; ready when its agent answers (the relay's token and
//! the node's hello, as `GuestRuntime`'s first dial is); its VMM read through its OS, and its own report
//! read from its agent's `oci` capacity.

use std::io::Read as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hex_api::{CapacityResources, Class};
use hex_client::guest::{Client, own_link, shards::ShardsLink};
use hex_runtime::guest::GuestConfig;
use sha2::{Digest, Sha256};
use shards_api::Guests as _;

use super::seam::{Driver, Envelope, Host, Lines, Patience};

pub struct Shards {
    config: GuestConfig,
    guests: shards_api::ConfiguredShards,
    home: PathBuf,
}

/// A ready guest: its VM, the agent's client, and its VMM's pid.
pub struct Guest {
    vm: Arc<dyn shards_api::Guest>,
    client: Client,
    pid: i32,
}

impl Shards {
    /// The guest the environment configures, as `hex node` reads it (`GuestConfig::from_env`).
    pub fn from_env(out: &Path) -> Result<Self, String> {
        Self::configured(GuestConfig::from_env().map_err(|e| e.to_string())?, out)
    }

    /// The `config` guest, booted with `out/runtime/{cache,home}` as its runtime folders (made private).
    fn configured(config: GuestConfig, out: &Path) -> Result<Self, String> {
        let runtime = out.join("runtime");
        let (cache, home) = (runtime.join("cache"), runtime.join("home"));
        for dir in [&runtime, &cache, &home] {
            std::fs::create_dir_all(dir).map_err(|e| format!("making {}: {e}", dir.display()))?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| format!("making {} private: {e}", dir.display()))?;
        }
        let absolute = |dir: &Path| std::path::absolute(dir).map_err(|e| e.to_string());
        let guests = shards_api::Shards::configured(absolute(&cache)?, absolute(&home)?)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            config,
            guests,
            home,
        })
    }

    fn stop(vm: &dyn shards_api::Guest, deadline: Instant) -> Result<(), String> {
        vm.stop().map_err(|e| format!("stopping the guest: {e}"))?;
        let left = deadline.saturating_duration_since(Instant::now());
        match vm.wait_for(left) {
            Ok(Some(_)) => Ok(()),
            Ok(None) => Err(format!("the guest didn't end within {left:?} of its stop")),
            Err(e) => Err(format!("waiting for the guest's end: {e}")),
        }
    }
}

/// The SHA-256 of the file at `path`, read in SHA-256's own block multiple.
fn digest(path: &Path) -> Result<String, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(super::run::hex(&hash.finalize()))
}

impl Driver for Shards {
    type Guest = Guest;

    fn class(&self) -> String {
        hex_runtime::guest::CLASS.to_owned()
    }

    fn describe(&self) -> Result<Lines, String> {
        let mut lines = Vec::new();
        for (name, path) in [
            ("SHARDS_BIN", &self.config.binary),
            ("HEX_GUEST_KERNEL", &self.config.kernel),
            ("HEX_GUEST_INITRD", &self.config.initrd),
        ] {
            lines.push((
                name.to_owned(),
                format!("{} sha256={}", path.display(), digest(path)?),
            ));
        }
        lines.push(("HEX_GUEST_CPUS".to_owned(), self.config.vcpus.to_string()));
        lines.push((
            "HEX_GUEST_MEMORY_MIB".to_owned(),
            self.config.memory_mib.to_string(),
        ));
        #[cfg(target_os = "macos")]
        lines.push((
            "host".to_owned(),
            hex_runtime::macos::machine_model().map_err(|e| e.to_string())?,
        ));
        #[cfg(target_os = "linux")]
        {
            let name = rustix::system::uname();
            lines.push((
                "host".to_owned(),
                format!(
                    "{} {}",
                    name.machine().to_string_lossy(),
                    name.release().to_string_lossy()
                ),
            ));
        }
        Ok(lines)
    }

    fn envelope(&self) -> Result<Envelope, String> {
        Ok(Envelope {
            resources: CapacityResources {
                cpu_milli: u64::from(self.config.vcpus)
                    .checked_mul(1000)
                    .ok_or("the vCPUs overflow")?,
                mem_bytes: self
                    .config
                    .memory_mib
                    .checked_mul(1 << 20)
                    .ok_or("the memory overflows")?,
                disk_bytes: 0,
            },
            source: "HEX_GUEST_CPUS and HEX_GUEST_MEMORY_MIB; no disk: the guest's root and state are in its RAM (docs/design/shards-guest.md §4)".to_owned(),
        })
    }

    fn patience(&self) -> Result<Patience, String> {
        Ok(Patience {
            bound: hex_runtime::guest::recorded_dial_bound().map_err(|e| e.to_string())?,
            source: format!(
                "the guest's recorded first dial bound, twice boot_ready of {} in bench/native/guest/records.tsv (hex_runtime::guest::recorded_dial_bound)",
                hex_runtime::guest::CLASS
            ),
        })
    }

    fn runtime(&self) -> &Path {
        &self.home
    }

    fn host_source(&self) -> String {
        if cfg!(target_os = "macos") {
            "the VMM's proc_pid_rusage: ri_user_time + ri_system_time, ri_lifetime_max_phys_footprint, ri_diskio_byteswritten".to_owned()
        } else {
            "the VMM's /proc/<pid>/stat utime + stime, /proc/<pid>/status VmHWM, /proc/<pid>/io write_bytes".to_owned()
        }
    }

    fn boot(&mut self, deadline: Instant) -> Result<Option<Guest>, String> {
        let span = deadline.saturating_duration_since(Instant::now());
        let vm: Arc<dyn shards_api::Guest> = Arc::from(
            self.guests
                .boot(&self.config.spec())
                .map_err(|e| format!("booting the guest: {e}"))?,
        );
        let ready = vm
            .identity()
            .map_err(|e| format!("naming the guest's VMM: {e}"))
            .and_then(|identity| i32::try_from(identity.pid).map_err(|e| e.to_string()))
            .and_then(|pid| {
                own_link(ShardsLink::new(Arc::clone(&vm), span))
                    .and_then(|link| Client::connect_before(link, deadline))
                    .map(|client| (client, pid))
                    .map_err(|why| format!("reaching the guest's agent: {why}"))
            });
        match ready {
            Ok((client, pid)) => Ok(Some(Guest { vm, client, pid })),
            // Whatever kept it from being ready, the guest is stopped first: none is left running.
            Err(why) => {
                let ran_out = Instant::now() >= deadline;
                let stopped = Instant::now()
                    .checked_add(span)
                    .ok_or_else(|| "the stop's wait overflows".to_owned())
                    .and_then(|at| Self::stop(vm.as_ref(), at));
                match (ran_out, stopped) {
                    (true, Ok(())) => Ok(None),
                    (false, Ok(())) => Err(why),
                    (_, Err(stop)) => Err(format!("{why}; then {stop}")),
                }
            }
        }
    }

    fn hold(&mut self, guest: &Guest, span: Duration) -> Result<Host, String> {
        let before = vmm(guest.pid)?;
        // The hold is a wait on the guest's end, for `span`: it must not come.
        if let Some(end) = guest
            .vm
            .wait_for(span)
            .map_err(|e| format!("holding the guest: {e}"))?
        {
            return Err(format!("the guest ended in its idle hold: {end:?}"));
        }
        let after = vmm(guest.pid)?;
        Ok(Host {
            cpu: after.cpu.saturating_sub(before.cpu),
            peak_bytes: after.peak_bytes,
            written_bytes: after.written_bytes,
        })
    }

    fn report(&mut self, guest: &Guest, deadline: Instant) -> Result<CapacityResources, String> {
        guest
            .client
            .capacity_before(Class::Oci, deadline)
            .map_err(|e| format!("reading the guest agent's capacity: {e}"))?
            .map(|capacity| capacity.free)
            .ok_or_else(|| "the guest's agent reports no oci capacity".to_owned())
    }

    fn tear_down(&mut self, guest: Guest, deadline: Instant) -> Result<(), String> {
        let Guest { vm, client, .. } = guest;
        drop(client);
        Self::stop(vm.as_ref(), deadline)
    }
}

/// The VMM's use so far: its CPU time, its peak memory, the bytes it wrote.
#[cfg(target_os = "macos")]
fn vmm(pid: i32) -> Result<Host, String> {
    let usage = hex_runtime::macos::sys::rusage(pid)
        .map_err(|e| format!("reading the VMM's use: {e}"))?
        .filter(|usage| !usage.ended)
        .ok_or("the guest's VMM has ended")?;
    let timebase = hex_runtime::macos::sys::Timebase::get().map_err(|e| e.to_string())?;
    Ok(Host {
        cpu: Duration::from_nanos(timebase.nanos(usage.cpu)),
        peak_bytes: usage.peak,
        written_bytes: usage.written_bytes,
    })
}

/// The VMM's use so far: its CPU time, its peak memory, the bytes it wrote.
#[cfg(target_os = "linux")]
fn vmm(pid: i32) -> Result<Host, String> {
    let read = |name: &str| {
        std::fs::read_to_string(format!("/proc/{pid}/{name}"))
            .map_err(|e| format!("reading the VMM's /proc/{pid}/{name}: {e}"))
    };
    // proc(5): after the command's closing parenthesis, utime and stime are the 12th and 13th fields.
    let stat = read("stat")?;
    let fields: Vec<&str> = stat
        .rsplit_once(')')
        .map(|(_, rest)| rest.split_whitespace().collect())
        .ok_or("the VMM's stat has no command")?;
    let tick = |at: usize| -> Result<u64, String> {
        fields
            .get(at)
            .and_then(|field| field.parse().ok())
            .ok_or_else(|| format!("the VMM's stat field {at} isn't a count"))
    };
    let ticks = tick(11)?.saturating_add(tick(12)?);
    let hz = rustix::param::clock_ticks_per_second();
    let field = |text: &str, name: &str| -> Result<u64, String> {
        text.lines()
            .find_map(|line| line.strip_prefix(name))
            .and_then(|value| value.trim().trim_end_matches("kB").trim().parse().ok())
            .ok_or_else(|| format!("the VMM's {name} isn't a count"))
    };
    Ok(Host {
        cpu: Duration::from_nanos(
            u64::try_from(u128::from(ticks) * 1_000_000_000 / u128::from(hz.max(1)))
                .map_err(|e| e.to_string())?,
        ),
        peak_bytes: field(&read("status")?, "VmHWM:")?.saturating_mul(1024),
        written_bytes: field(&read("io")?, "write_bytes:")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATIENCE: Duration = Duration::from_secs(30);

    fn config(dir: &Path) -> GuestConfig {
        for (name, bytes) in [
            ("shards", b"vmm".as_slice()),
            ("kernel", b"kernel".as_slice()),
            ("initrd", b"initrd".as_slice()),
        ] {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        GuestConfig {
            binary: dir.join("shards"),
            kernel: dir.join("kernel"),
            initrd: dir.join("initrd"),
            vcpus: 2,
            memory_mib: 512,
            gpu: false,
            shares: Vec::new(),
            staged: hex_client::guest::stage::STAGED.to_owned(),
        }
    }

    /// AC-T7.4.1: the driver's envelope is its configured guest's (vCPUs as milli-CPUs, MiB as bytes, no
    /// disk), its runtime folders are made private under the run's, and it names each image by path and
    /// SHA-256.
    #[test]
    fn the_driver_measures_its_configured_guest() {
        let dir = tempfile::tempdir().unwrap();
        let driver = Shards::configured(config(dir.path()), &dir.path().join("out")).unwrap();
        assert_eq!(
            driver.envelope().unwrap().resources,
            CapacityResources {
                cpu_milli: 2000,
                mem_bytes: 512 << 20,
                disk_bytes: 0,
            }
        );
        let home = std::fs::metadata(driver.runtime()).unwrap();
        assert_eq!(home.permissions().mode() & 0o777, 0o700);
        let lines = driver.describe().unwrap();
        let kernel = lines
            .iter()
            .find(|(name, _)| name == "HEX_GUEST_KERNEL")
            .unwrap();
        let expected = crate::run::hex(&Sha256::digest(b"kernel"));
        assert!(
            kernel.1.ends_with(&format!("sha256={expected}")),
            "{kernel:?}"
        );
        assert!(
            lines
                .iter()
                .any(|(name, value)| name == "HEX_GUEST_MEMORY_MIB" && value == "512")
        );
    }

    /// AC-T7.4.1: the VMM's use, read as the hold reads it, of a real busy process: expect its CPU to grow
    /// from nothing and its peak memory read.
    #[test]
    fn the_vmm_is_read_through_its_os() {
        let mut busy = std::process::Command::new("/usr/bin/yes")
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let pid = i32::try_from(busy.id()).unwrap();
        let deadline = Instant::now() + PATIENCE;
        let mut read = vmm(pid).unwrap();
        while read.cpu.is_zero() {
            assert!(Instant::now() < deadline, "the process never used CPU");
            std::thread::park_timeout(Duration::from_millis(1));
            read = vmm(pid).unwrap();
        }
        let _ = busy.kill();
        let _ = busy.wait();
        assert!(read.peak_bytes > 0, "{read:?}");
    }

    /// AC-T7.4.1 (light smoke, few boots): produce a pair over the configured Shards guest with SEEDABLE
    /// two, through the same census, evidence and output the measurement uses: expect it written and loaded
    /// as a node loads it, its envelope the configured guest's, and two guests in its evidence.
    /// Needs SHARDS_BIN, HEX_GUEST_KERNEL, HEX_GUEST_INITRD, HEX_GUEST_CPUS and HEX_GUEST_MEMORY_MIB, and
    /// GUEST_COST_SMOKE_OUT (a folder under ~/.cache).
    #[test]
    #[ignore = "boots two real guests: run with --ignored where a Shards guest is configured"]
    fn two_guests_make_a_pair_the_node_loads() {
        let out =
            PathBuf::from(std::env::var_os("GUEST_COST_SMOKE_OUT").expect("GUEST_COST_SMOKE_OUT"));
        let mut driver = Shards::from_env(&out).unwrap();
        let envelope = driver.envelope().unwrap().resources;
        let made =
            super::super::run::produce(&mut driver, &super::super::run::Plan { seedable: 2 }, &out)
                .unwrap();
        let (cost, _, rows) = hex_runtime::guest::load_runtime_evidence(
            &made.cost_file,
            made.cost_sha256,
            &made.evidence_file,
        )
        .unwrap();
        assert!(rows.is_empty());
        assert_eq!((cost.floor, cost.ceiling), (envelope, envelope));
        let evidence = std::fs::read_to_string(&made.evidence_file).unwrap();
        assert_eq!(
            evidence.lines().filter(|l| l.starts_with("ready ")).count(),
            2
        );
        println!("{}\n{evidence}", made.summary);
    }
}
