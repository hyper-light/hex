//! The Windows `oci` class through the hex-api `Runtime` seam, in process (native plan step 4, its second
//! criterion): create, start and delete of a box whose init waits, each timed alone, `n` times after
//! `warmup` untimed rounds, on one image's layers. `bench/native/windows/oci_compare.ps1` runs the same
//! lifecycle through Docker's engine and containerd's runhcs shim on the same image in the same run, and
//! the record sets them side by side (CLAUDE.md §7).
//!
//!   windows_oci <out.json> <n> <warmup> <hex-bin-dir> <layer;layer;...>
//!
//! Layers are the image's read-only folders, topmost first; `hex-bin-dir` holds the `hex.exe` every box
//! maps as its launcher. It writes `{runtime, n, create_ms, start_ms, delete_ms}`, each the samples and
//! their p50, p90, p99 and max.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::disallowed_macros,
    clippy::print_stdout
)]

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("SKIP: the Windows oci seam bench runs on Windows");
}

#[cfg(target_os = "windows")]
fn main() {
    use std::path::PathBuf;
    use std::time::Instant;

    use hex_api::{Bounds, BoxId, BoxSpec, ClassSpec, Runtime};
    use hex_runtime::windows::WinRuntime;
    use serde_json::json;

    let args: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.into_string().expect("arguments are Unicode"))
        .collect();
    let [out, n, warmup, bin, layers] = args.as_slice() else {
        panic!("usage: windows_oci <out.json> <n> <warmup> <hex-bin-dir> <layer;layer;...>");
    };
    let (n, warmup): (usize, usize) = (n.parse().unwrap(), warmup.parse().unwrap());
    let layers: Vec<String> = layers
        .split(';')
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    let root = tempfile::tempdir().unwrap();
    let rt = WinRuntime::new().with_launcher(PathBuf::from(bin));
    let bundle = |i: usize| {
        let dir = root.path().join(format!("b{i}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut folders = layers.clone();
        folders.push(
            root.path()
                .join(format!("s{i}"))
                .to_str()
                .unwrap()
                .to_owned(),
        );
        let config = json!({
            "ociVersion": "1.2.0",
            "process": {"args": [r"C:\hex\hex.exe", "selftest-box", "wait"], "cwd": "C:\\"},
            "windows": {"layerFolders": folders},
        });
        std::fs::write(dir.join("config.json"), config.to_string()).unwrap();
        dir
    };
    let mut times: [Vec<f64>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for i in 0..warmup + n {
        let id = BoxId::new(&format!("hex-bench-{i}")).unwrap();
        let spec = BoxSpec {
            id: id.clone(),
            bounds: Bounds::default(),
            class: ClassSpec::Bundle(bundle(i)),
        };
        let t = Instant::now();
        rt.create(spec).expect("create");
        let create = t.elapsed();
        let t = Instant::now();
        rt.start(&id).expect("start");
        let start = t.elapsed();
        let t = Instant::now();
        rt.delete(&id).expect("delete");
        let delete = t.elapsed();
        if i >= warmup {
            for (k, d) in [create, start, delete].iter().enumerate() {
                times[k].push(d.as_secs_f64() * 1000.0);
            }
        }
    }
    let stats = |v: &[f64]| {
        let mut s = v.to_vec();
        s.sort_by(f64::total_cmp);
        // The nearest-rank percentile: the ⌈p/100 × n⌉-th smallest sample.
        let at = |p: usize| s[(p * s.len()).div_ceil(100).clamp(1, s.len()) - 1];
        json!({"p50": at(50), "p90": at(90), "p99": at(99), "max": s[s.len() - 1], "samples": v})
    };
    let record = json!({
        "runtime": "hex-seam",
        "n": n,
        "warmup": warmup,
        "layers": layers,
        "create_ms": stats(&times[0]),
        "start_ms": stats(&times[1]),
        "delete_ms": stats(&times[2]),
    });
    std::fs::write(out, serde_json::to_string_pretty(&record).unwrap()).unwrap();
    println!("{record}");
}
