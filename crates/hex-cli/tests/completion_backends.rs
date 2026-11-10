//! AC-T7.3.1/4.1: ROOT-only actual typed backend/socket/checked-GPU/recovery fixture.
//! No test self-enables hardware. Missing hardware/configuration is an explicit failure.
//! The external ROOT runner bounds the whole process, including native cleanup uncertainty.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros
)]

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio as HostStdio};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use hex_api::{BoxId, Class, DeviceId, Exit, Program, Runtime, Stdio};
use hex_client::Client;
use hex_core::schedule::{self as dto, Payload};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name]
        .as_str()
        .unwrap_or_else(|| panic!("UNVERIFIED missing text field {name}"))
}

fn count(value: &Value, name: &str) -> u64 {
    value[name]
        .as_u64()
        .unwrap_or_else(|| panic!("UNVERIFIED missing unsigned field {name}"))
}

fn bounded(path: &Path, budget: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    File::open(path)
        .unwrap()
        .take(budget.checked_add(1).unwrap())
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(
        u64::try_from(bytes.len()).unwrap() <= budget,
        "artifact exceeded caller budget"
    );
    bytes
}

fn digest(path: &Path) -> String {
    let mut file = File::open(path).unwrap();
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let size = file.read(&mut buffer).unwrap();
        if size == 0 {
            break;
        }
        digest.update(&buffer[..size]);
    }
    format!("{:x}", digest.finalize())
}

fn journal(value: Value) {
    let path = std::env::var_os("HEX_COMPLETION_PID_JOURNAL").expect("ROOT PID journal required");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    writeln!(file, "{value}").unwrap();
    file.sync_all().unwrap();
}

fn last_json(bytes: &[u8]) -> Value {
    String::from_utf8_lossy(bytes)
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str(line).ok())
        .expect("native witness produced no JSON result")
}

struct Service {
    name: String,
    child: Option<Child>,
    reader: Option<JoinHandle<()>>,
}

impl Service {
    fn start(row: &Value, hex: &Path, scope: &Path, wait: Duration, budget: u64) -> Self {
        let name = text(row, "name").to_owned();
        let ready = text(row, "ready").to_owned();
        let arguments: Vec<_> = row["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap())
            .collect();
        let mut command = Command::new(hex);
        command
            .args(&arguments)
            .stdin(HostStdio::null())
            .stdout(HostStdio::piped())
            .stderr(File::create(scope.join(format!("{name}.stderr"))).unwrap());
        for (name, value) in row["env"].as_object().unwrap() {
            command.env(name, value.as_str().unwrap());
        }
        let mut child = command.spawn().expect("actual backend/scheduler child");
        journal(
            json!({"event":"spawn","role":name,"pid":child.id(),"executable":hex,"args":arguments}),
        );
        let stdout = child.stdout.take().unwrap();
        let path = scope.join(format!("{name}.stdout"));
        let (sent, received) = mpsc::sync_channel(1);
        let reader = std::thread::spawn(move || {
            let mut log = File::create(path).unwrap();
            let mut source = BufReader::new(stdout.take(budget.checked_add(1).unwrap()));
            let mut total = 0_u64;
            let mut announced = false;
            loop {
                let mut line = String::new();
                let size = source.read_line(&mut line).unwrap();
                if size == 0 {
                    break;
                }
                total += u64::try_from(size).unwrap();
                assert!(total <= budget, "service output exceeded admitted bound");
                log.write_all(line.as_bytes()).unwrap();
                if !announced && line.trim_end() == ready {
                    let _ = sent.send(());
                    announced = true;
                }
            }
        });
        let mut service = Self {
            name,
            child: Some(child),
            reader: Some(reader),
        };
        if received.recv_timeout(wait).is_err() {
            service.stop();
            panic!("UNVERIFIED actual service did not announce readiness");
        }
        service
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let pid = child.id();
            let _ = child.kill(); // Exact Child handle only, never process-name discovery.
            let status = child
                .wait()
                .expect("owned service reaped; external ROOT deadline bounds OS wait");
            journal(json!({"event":"reaped","role":self.name,"pid":pid,"status":status.code()}));
        }
        if let Some(reader) = self.reader.take() {
            reader.join().expect("owned output reader joined");
        }
    }

    fn finish(&mut self) {
        if let Some(mut child) = self.child.take() {
            let pid = child.id();
            let status = child
                .wait()
                .expect("finite workload; external ROOT guard bounds OS wait");
            journal(
                json!({"event":"reaped","role":self.name,"pid":pid,"status":status.code(),"natural":true}),
            );
            assert!(status.success(), "owned native workload failed");
        }
        if let Some(reader) = self.reader.take() {
            reader.join().unwrap();
        }
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.stop();
    }
}

struct Lab {
    input: Value,
    backend: String,
    topology: String,
    scope: PathBuf,
    services: Vec<Service>,
    events: Vec<Value>,
    keys: Vec<dto::Key>,
    boxes: Vec<BoxId>,
    wait: Duration,
    budget: u64,
}

impl Lab {
    fn new(backend: &str, topology: &str, proof: &str) -> Self {
        assert_eq!(
            std::env::var("HEX_COMPLETION_ROOT_AUTHORIZED").as_deref(),
            Ok("1"),
            "UNVERIFIED fixture is ROOT-only"
        );
        let config = PathBuf::from(std::env::var_os("HEX_COMPLETION_CONFIG").expect("ROOT config"));
        // The existing wire ceiling also bounds this test's configuration.
        let bytes = bounded(&config, u64::try_from(hex_core::wire::MAX_MESSAGE).unwrap());
        let raw: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(text(&raw, "backend"), backend);
        assert_eq!(text(&raw, "topology"), topology);
        assert_eq!(text(&raw, "proof"), proof);
        let wait_ms = count(&raw, "wait_ms");
        let budget = count(&raw, "byte_limit");
        assert!(wait_ms > 0 && budget > 0);
        let scratch = Path::new(text(&raw, "scratch"));
        let scope = tempfile::Builder::new()
            .prefix("hex-completion-")
            .tempdir_in(scratch)
            .unwrap()
            .keep();
        journal(json!({"event":"scope","path":scope,"backend":backend,"topology":topology}));
        let escaped = serde_json::to_string(scope.to_str().unwrap()).unwrap();
        let escaped = escaped
            .strip_prefix('"')
            .unwrap()
            .strip_suffix('"')
            .unwrap();
        let expanded = String::from_utf8(bytes)
            .unwrap()
            .replace("@SCOPE@", escaped);
        let input: Value = serde_json::from_str(&expanded).unwrap();
        for file in input["files"].as_array().unwrap() {
            let path = Path::new(text(file, "path"));
            assert!(
                path.starts_with(&scope),
                "fixture may write only its own new scope"
            );
            assert!(
                !path
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir)),
                "fixture path cannot escape through parent components"
            );
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            if let Some(source) = file["copy_from"].as_str() {
                assert_eq!(
                    digest(Path::new(source)),
                    text(file, "sha256"),
                    "source-pinned fixture helper"
                );
                assert!(
                    fs::metadata(source).unwrap().len() <= budget,
                    "copied helper exceeds admitted byte bound"
                );
                fs::copy(source, path).unwrap();
                assert_eq!(digest(path), text(file, "sha256"));
            } else {
                let bytes = if let Some(contents) = file["text"].as_str() {
                    contents.as_bytes().to_vec()
                } else {
                    serde_json::to_vec_pretty(&file["json"]).unwrap()
                };
                assert!(u64::try_from(bytes.len()).unwrap() <= budget);
                fs::write(path, bytes).unwrap();
            }
        }
        Self {
            input,
            backend: backend.into(),
            topology: topology.into(),
            scope,
            services: Vec::new(),
            events: Vec::new(),
            keys: Vec::new(),
            boxes: Vec::new(),
            wait: Duration::from_millis(wait_ms),
            budget,
        }
    }

    fn class(&self) -> Class {
        serde_json::from_value(json!(self.backend))
            .expect("UNVERIFIED typed production Runtime backend not registered")
    }

    fn start_all(&mut self) {
        let hex = Path::new(text(&self.input["hex"], "path"));
        assert_eq!(
            digest(hex),
            text(&self.input["hex"], "sha256"),
            "source-pinned Hex executable"
        );
        for row in self.input["services"].as_array().unwrap() {
            self.services.push(Service::start(
                row,
                hex,
                &self.scope,
                self.wait,
                self.budget,
            ));
        }
        assert!(!self.services.is_empty());
    }

    fn restart(&mut self, role: &str) {
        let index = self
            .services
            .iter()
            .position(|service| service.name == role)
            .expect("owned service role");
        self.services[index].stop();
        let row = self.input["services"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| text(row, "name") == role)
            .unwrap();
        self.services[index] = Service::start(
            row,
            Path::new(text(&self.input["hex"], "path")),
            &self.scope,
            self.wait,
            self.budget,
        );
    }

    fn backend(&self) -> Client {
        Client::connect(Path::new(text(&self.input, "backend_socket"))).unwrap()
    }
    fn control(&self) -> Client {
        Client::connect(Path::new(text(&self.input, "scheduler_socket"))).unwrap()
    }

    fn emit(&mut self, stage: &str, mut value: Value) {
        value["stage"] = json!(stage);
        value["backend"] = json!(self.backend);
        value["topology"] = json!(self.topology);
        value["outcome"] = json!("verified");
        self.events.push(value);
        let path = std::env::var_os("HEX_COMPLETION_EVENTS").expect("ROOT event path");
        fs::write(path, serde_json::to_vec_pretty(&self.events).unwrap()).unwrap();
    }

    fn record(&self, id: &BoxId) -> Value {
        let path = hex_node::store::runtime_record(Path::new(text(&self.input, "record_root")), id);
        assert!(path.starts_with(&self.scope));
        let record: Value = serde_json::from_slice(&bounded(&path, self.budget)).unwrap();
        assert_eq!(record["spec"]["id"], json!(id));
        assert_eq!(
            record["phase"], "ready",
            "physical creation must be durable before workload dispatch"
        );
        record
    }

    fn selected(&self, id: &BoxId) -> Vec<DeviceId> {
        let selected: Vec<DeviceId> =
            serde_json::from_value(self.record(id)["spec"]["devices"].clone()).unwrap();
        assert_eq!(
            selected.len(),
            1,
            "this fixture requests exactly one physical GPU"
        );
        selected
    }

    fn holder(&self, id: &BoxId, selected: &[DeviceId]) {
        let backend = self.backend();
        assert!(
            backend.classes().contains(&self.class()),
            "actual typed Runtime capability"
        );
        let capacity = backend
            .capacity(self.class())
            .unwrap()
            .expect("actual backend physical capacity");
        capacity.check().unwrap();
        for device in selected {
            let rows: Vec<_> = capacity
                .devices
                .iter()
                .filter(|row| row.id == *device)
                .collect();
            assert_eq!(rows.len(), 1, "exact unique physical offer");
            assert_eq!(
                rows[0].holder.as_ref(),
                Some(id),
                "single production holder authority"
            );
        }
        assert_eq!(
            capacity
                .devices
                .iter()
                .filter(|row| row.holder.as_ref() == Some(id))
                .count(),
            selected.len()
        );
        assert_eq!(
            backend.inspect(id).unwrap().class,
            self.class(),
            "actual class, not scheduler label"
        );
    }

    fn host(&self, name: &str, substitutions: &[(&str, String)]) -> Value {
        let row = &self.input[name];
        let program = Path::new(text(row, "path"));
        assert_eq!(
            digest(program),
            text(row, "sha256"),
            "source-pinned host witness"
        );
        let arguments: Vec<_> = row["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| {
                let mut value = arg.as_str().unwrap().to_owned();
                for (key, replacement) in substitutions {
                    value = value.replace(key, replacement);
                }
                value
            })
            .collect();
        let stdout = self
            .scope
            .join(format!("host-{name}-{}.stdout", self.events.len()));
        let stderr = self
            .scope
            .join(format!("host-{name}-{}.stderr", self.events.len()));
        let mut child = Command::new(program)
            .args(&arguments)
            .stdin(HostStdio::null())
            .stdout(File::create(&stdout).unwrap())
            .stderr(File::create(&stderr).unwrap())
            .spawn()
            .unwrap();
        journal(
            json!({"event":"spawn","role":name,"pid":child.id(),"executable":program,"args":arguments}),
        );
        let pid = child.id();
        let status = child
            .wait()
            .expect("external ROOT run deadline bounds native command");
        journal(json!({"event":"reaped","role":name,"pid":pid,"status":status.code()}));
        assert!(
            status.success(),
            "native host witness failed; inspect retained {stderr:?}"
        );
        last_json(&bounded(&stdout, self.budget))
    }

    fn exec(&self, id: &BoxId, field: &str, uuid: &str) -> Value {
        let mut raw = serde_json::to_string(&self.input[field])
            .unwrap()
            .replace("@UUID@", uuid);
        raw = raw.replace("@BOX@", id.as_str());
        let program: Program = serde_json::from_str(&raw).unwrap();
        program.check().unwrap();
        let output = self.scope.join(format!("exec-{field}.stdout"));
        let error = self.scope.join(format!("exec-{field}.stderr"));
        let mut proc = self
            .backend()
            .exec(
                id,
                program,
                Stdio {
                    stdout: Some(File::create(&output).unwrap().into()),
                    stderr: Some(File::create(&error).unwrap().into()),
                    ..Stdio::null()
                },
            )
            .unwrap();
        journal(
            json!({"event":"runtime-exec","box":id,"reported_pid":proc.pid(),"field":field,
                       "note":"PID is remote/guest scoped; never signal it from host"}),
        );
        let end = proc.wait_for(self.wait).unwrap();
        if end.is_none() {
            proc.kill().unwrap();
            assert!(
                proc.wait_for(self.wait).unwrap().is_some(),
                "owned Runtime Proc reaped"
            );
        }
        assert_eq!(
            end,
            Some(Exit::Code(0)),
            "actual bounded workload exit; see {error:?}"
        );
        last_json(&bounded(&output, self.budget))
    }

    fn native_join(&self, id: &BoxId, selected: &[DeviceId]) -> (String, Value, Value) {
        let translation = if self.backend == "firecracker" {
            self.host(
                "physical_translation",
                &[
                    ("@BOX@", id.as_str().into()),
                    ("@KEY@", selected[0].key.to_string()),
                ],
            )
        } else {
            Value::Null
        };
        let inventory = if self.backend == "firecracker" {
            self.exec(id, "inventory", "")
        } else {
            self.host("host_inventory", &[])
        };
        let mut matches = Vec::new();
        for row in inventory["devices"]
            .as_array()
            .expect("independent actual host inventory")
        {
            if let Some(luid) = row["luid"].as_str() {
                let mask = count(row, "node_mask");
                assert!(
                    mask > 0 && mask.is_power_of_two(),
                    "linked LUID mask does not select one physical node"
                );
                assert_eq!(
                    inventory["devices"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|device| device["luid"].as_str() == Some(luid))
                        .count(),
                    1,
                    "LUID shared by multiple actual physical devices"
                );
            }
            if let Some(pci) = row["pci"].as_array() {
                assert_eq!(pci.len(), 4);
                let domain = pci[0].as_u64().unwrap();
                let bus = pci[1].as_u64().unwrap();
                let device = pci[2].as_u64().unwrap();
                let function = pci[3].as_u64().unwrap();
                assert!(
                    domain <= u64::from(u32::MAX) && bus <= 255 && device <= 31 && function <= 7
                );
                let key = (domain << 16) | (bus << 8) | (device << 3) | function;
                let exact = if translation.is_null() {
                    selected[0].key == key
                } else {
                    translation["native_key"] == json!(selected[0].key)
                        && translation["guest_pci"] == row["pci"]
                };
                if exact {
                    matches.push(text(row, "uuid").to_owned());
                }
            } else if row["metal_registry_id"].as_u64() == Some(selected[0].key) {
                matches.push(text(row, "uuid").to_owned());
            }
        }
        assert_eq!(
            matches.len(),
            1,
            "UNVERIFIED exact native-key-to-UUID join is missing/ambiguous"
        );
        (
            matches.pop().unwrap(),
            inventory["devices"].clone(),
            translation,
        )
    }

    fn compute(&self, id: &BoxId, uuid: &str) -> Value {
        let value = self.exec(id, "compute", uuid);
        assert_eq!(text(&value, "uuid"), uuid);
        let words = count(&value, "words");
        assert!(words > 0);
        assert_eq!(value["mismatches"], 0);
        assert_eq!(count(&value, "prefill_mismatches"), words);
        assert_eq!(value["hardware"], true);
        assert_eq!(value["dispatch_completed"], true);
        value
    }

    fn exercise(&mut self) {
        self.start_all();
        let payload: Payload = serde_json::from_value(self.input["payload"].clone()).unwrap();
        assert!(
            payload.spec.devices.is_empty(),
            "only production scheduler selects identities"
        );
        assert_eq!(
            payload.spec.class.class(),
            self.class(),
            "typed backend payload"
        );
        let reply = self
            .control()
            .schedule(dto::Command::Submit {
                demand: text(&self.input, "demand").into(),
                payload,
            })
            .unwrap();
        if let Some(key) = reply.key {
            self.keys.push(key);
        }
        if let Some(id) = &reply.physical {
            self.boxes.push(id.clone());
        }
        assert!(
            reply.refusals.is_empty() && reply.created && reply.tracked && !reply.ended,
            "{reply:?}"
        );
        assert_eq!(
            reply.at,
            Some(u32::try_from(count(&self.input, "runner")).unwrap())
        );
        let key = reply.key.unwrap();
        let id = reply.physical.unwrap();
        let selected = self.selected(&id);
        self.holder(&id, &selected);
        fs::write(
            self.scope.join("placement.json"),
            serde_json::to_vec(&json!({
                "source":self.input["source"],"key":key,"physical":id,"runner":reply.at.unwrap()
            }))
            .unwrap(),
        )
        .unwrap();
        if !self.input["origin_barrier"].is_null() {
            let origin = self.host("origin_barrier", &[]);
            assert_eq!(
                origin["origin_verified"], true,
                "actual peer origin must observe this physical box"
            );
        }
        let native = self.host("native_observer", &[("@BOX@", id.as_str().into())]);
        assert_eq!(native["backend"], self.backend);
        assert_eq!(native["native_process_verified"], true);
        fs::write(
            self.scope.join("native-proof.json"),
            serde_json::to_vec(&native).unwrap(),
        )
        .unwrap();
        self.emit("capability", json!({"runtime_class":self.class().name(),
            "native_process_verified":true,"native_executable_sha256":native["native_executable_sha256"]}));
        self.emit(
            "binding",
            json!({"selected":selected,"holder_count":1,"durable_before_execution":true,"physical":id,"key":key}),
        );
        let (uuid, native_inventory, translation) = self.native_join(&id, &selected);
        let mut compute = self.compute(&id, &uuid);
        compute["selected"] = json!(selected);
        compute["native_join_verified"] = json!(true);
        compute["native_inventory"] = native_inventory;
        compute["physical_translation"] = translation;
        self.emit("compute", compute);
        if text(&self.input, "proof") == "selected" {
            self.cleanup().expect("ordinary physical cleanup");
            self.emit(
                "cleanup",
                json!({"boxes":0,"holders":0,"owned_native_processes":0,"owned_artifacts":0}),
            );
            self.emit("complete", json!({"selected_stages":5}));
            return;
        }
        let inventory = self.host("host_inventory", &[]);
        let foreign: Vec<_> = inventory["devices"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| text(row, "uuid") != uuid && row["device_type"] != 4)
            .collect();
        assert!(
            !foreign.is_empty(),
            "UNVERIFIED second physical GPU absent; selected-only proof remains separate"
        );
        for row in &foreign {
            let positive = self.host(
                "foreign_host_compute",
                &[("@UUID@", text(row, "uuid").into())],
            );
            assert_eq!(positive["uuid"], row["uuid"]);
            assert_eq!(
                positive["dispatch_completed"], true,
                "foreign positive must dispatch on host"
            );
            assert_eq!(positive["mismatches"], 0);
            let denied = self.exec(&id, "reject", text(row, "uuid"));
            assert_eq!(denied["refused"], true);
            assert_eq!(denied["dispatches"], 0);
        }
        self.emit(
            "foreign",
            json!({"host_accessible":foreign.len(),"guest_accessible":0}),
        );
        let wrong = text(&self.input, "wrong_uuid").to_owned();
        assert!(
            inventory["devices"]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| text(row, "uuid") != wrong)
        );
        let denied = self.exec(&id, "reject", &wrong);
        assert_eq!(denied["refused"], true);
        assert_eq!(denied["dispatches"], 0);
        self.emit("wrong-id", denied);
        self.restart("scheduler");
        let status = self
            .control()
            .schedule(dto::Command::Status { key })
            .unwrap();
        assert!(status.created && status.tracked && status.refusals.is_empty());
        assert_eq!(status.physical.as_ref(), Some(&id));
        assert_eq!(self.selected(&id), selected);
        self.holder(&id, &selected);
        self.compute(&id, &uuid);
        let retained = self.host("native_observer", &[("@BOX@", id.as_str().into())]);
        assert_eq!(retained["pid"], native["pid"]);
        assert_eq!(
            retained["start_token"], native["start_token"],
            "scheduler restart recreated native work"
        );
        self.emit(
            "scheduler-recovery",
            json!({"selected":selected,"holder_count":1,
            "same_physical_box":true,"compute_verified":true}),
        );
        let before_load = self.host("load_before", &[]);
        let actor_row = &self.input["load_actor"];
        let actor_path = Path::new(text(actor_row, "path"));
        assert_eq!(
            digest(actor_path),
            text(actor_row, "sha256"),
            "source-pinned native load actor"
        );
        let mut actor = Service::start(actor_row, actor_path, &self.scope, self.wait, self.budget);
        let catalog = &self.input["catalog"];
        let catalog_path = Path::new(text(catalog, "path"));
        assert!(catalog_path.starts_with(&self.scope));
        let original = bounded(catalog_path, self.budget);
        assert_eq!(original, text(catalog, "before").as_bytes());
        assert_ne!(text(catalog, "before"), text(catalog, "after"));
        let before_text = text(catalog, "before");
        let after_text = text(catalog, "after");
        assert_eq!(before_text.lines().next(), after_text.lines().next());
        let mut old_rows: Vec<_> = before_text.lines().skip(1).collect();
        let mut new_rows: Vec<_> = after_text.lines().skip(1).collect();
        old_rows.sort_unstable();
        new_rows.sort_unstable();
        assert_eq!(
            old_rows, new_rows,
            "reorder preserves every exact catalog entry"
        );
        fs::write(catalog_path, text(catalog, "after")).unwrap();
        let changed = self.host("load_change", &[]);
        assert_ne!(
            before_load["order"], changed["order"],
            "actual measured load order did not change"
        );
        assert!(
            before_load["order"]
                .as_array()
                .is_some_and(|order| !order.is_empty())
        );
        assert!(
            changed["order"]
                .as_array()
                .is_some_and(|order| !order.is_empty())
        );
        assert!(
            actor.child.as_mut().unwrap().try_wait().unwrap().is_none(),
            "load actor already completed"
        );
        self.restart("backend");
        self.restart("scheduler");
        let status = self
            .control()
            .schedule(dto::Command::Status { key })
            .unwrap();
        assert!(status.created && status.tracked && status.refusals.is_empty());
        assert_eq!(status.physical.as_ref(), Some(&id));
        assert_eq!(self.selected(&id), selected);
        self.holder(&id, &selected);
        self.compute(&id, &uuid);
        let retained = self.host("native_observer", &[("@BOX@", id.as_str().into())]);
        assert_eq!(retained["pid"], native["pid"]);
        assert_eq!(
            retained["start_token"], native["start_token"],
            "runtime recovery recreated native work"
        );
        assert!(
            actor.child.as_mut().unwrap().try_wait().unwrap().is_none(),
            "load was not retained across recovery"
        );
        actor.finish();
        self.emit("runtime-recovery", json!({"selected":selected,"holder_count":1,
            "same_physical_box":true,"compute_verified":true,"catalog_reordered":true,"load_order_changed":true}));
        self.cleanup().expect("physical and ordinary owned cleanup");
        self.emit(
            "cleanup",
            json!({"boxes":0,"holders":0,"owned_native_processes":0,"owned_artifacts":0}),
        );
        self.emit("complete", json!({"selected_stages":9}));
    }

    fn cleanup(&mut self) -> Result<(), String> {
        for key in &self.keys {
            self.control()
                .schedule(dto::Command::Delete { key: *key })
                .map_err(|error| error.to_string())?;
        }
        for id in &self.boxes {
            if self
                .backend()
                .list()
                .map_err(|error| error.to_string())?
                .iter()
                .any(|row| row.id == *id)
            {
                return Err(format!("owned physical box remains: {id}"));
            }
            let capacity = self
                .backend()
                .capacity(self.class())
                .map_err(|error| error.to_string())?
                .ok_or("missing native cleanup capacity")?;
            if capacity
                .devices
                .iter()
                .any(|device| device.holder.as_ref() == Some(id))
            {
                return Err(format!("owned physical holder remains: {id}"));
            }
        }
        let result = self.host("cleanup_observer", &[]);
        if result["owned_native_processes"] != 0 || result["owned_artifacts"] != 0 {
            return Err(format!("actual native cleanup unverified: {result}"));
        }
        self.keys.clear();
        self.boxes.clear();
        for service in self.services.iter_mut().rev() {
            service.stop();
        }
        Ok(())
    }
}

fn case(backend: &str, topology: &str, proof: &str) {
    let mut lab = Lab::new(backend, topology, proof);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lab.exercise()));
    if let Err(original) = result {
        let cleanup = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lab.cleanup()));
        eprintln!(
            "completion_original_failure scope={:?} cleanup={cleanup:?}; preserve evidence/PID journal",
            lab.scope
        );
        std::panic::resume_unwind(original);
    }
    // Evidence stays outside scope; retain raw files in scope for ROOT review, never erase uncertain state.
    eprintln!("completion_verified_raw_scope {:?}", lab.scope);
}

macro_rules! native_case {
    ($name:ident, $backend:literal, $topology:literal) => {
        #[test]
        #[ignore = "ROOT-only actual GPU/backend/VM/crash; explicit finite envelope/config required"]
        fn $name() { case($backend, $topology, "full"); }
    };
}

native_case!(completion_oci_laptop, "oci", "laptop");
native_case!(completion_oci_peer, "oci", "peer");
native_case!(completion_firecracker_laptop, "firecracker", "laptop");
native_case!(completion_firecracker_peer, "firecracker", "peer");
native_case!(completion_shards_laptop, "shards", "laptop");
native_case!(completion_shards_peer, "shards", "peer");

macro_rules! selected_case {
    ($name:ident, $backend:literal, $topology:literal) => {
        #[test]
        #[ignore = "ROOT-only single-GPU positive; excludes full foreign/recovery proof"]
        fn $name() {
            case($backend, $topology, "selected");
        }
    };
}
selected_case!(completion_oci_laptop_selected, "oci", "laptop");
selected_case!(completion_oci_peer_selected, "oci", "peer");
selected_case!(
    completion_firecracker_laptop_selected,
    "firecracker",
    "laptop"
);
selected_case!(completion_firecracker_peer_selected, "firecracker", "peer");
selected_case!(completion_shards_laptop_selected, "shards", "laptop");
selected_case!(completion_shards_peer_selected, "shards", "peer");

/// AC-T7.4.1: fixture parsing is checked without touching any socket, driver or VM.
#[test]
fn completion_receipt_parser_uses_the_actual_last_json_result() {
    assert_eq!(
        last_json(b"dispatch_started\n{\"mismatches\":0}\n")["mismatches"],
        0
    );
    assert_eq!(
        serde_json::from_value::<Class>(json!("oci"))
            .unwrap()
            .name(),
        "oci"
    );
}
