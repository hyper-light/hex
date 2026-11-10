//! CP-T7.4: standalone allocation/timing probe, not production code or a performance claim.
//! All snapshots are cumulative. Inputs stay alive until every round and output has completed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::{self, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use hex_sim::index::FitIndex;
use hex_sim::model::{NodeId, NodeSpec, Resources};
use serde_json::{Value, json};

mod cycles;

struct CountedSystem;

#[global_allocator]
static ALLOCATOR: CountedSystem = CountedSystem;
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static DEALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static REALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static REQUESTED_BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE_BYTES: AtomicU64 = AtomicU64::new(0);
static ALLOCATION_FAILURES: AtomicU64 = AtomicU64::new(0);
static ACCOUNTING_INVALID: AtomicBool = AtomicBool::new(false);

fn add(counter: &AtomicU64, value: u64) {
    if counter.try_update(Ordering::Relaxed, Ordering::Relaxed, |old| old.checked_add(value)).is_err() {
        ACCOUNTING_INVALID.store(true, Ordering::Relaxed);
    }
}

fn subtract(counter: &AtomicU64, value: u64) {
    if counter.try_update(Ordering::Relaxed, Ordering::Relaxed, |old| old.checked_sub(value)).is_err() {
        ACCOUNTING_INVALID.store(true, Ordering::Relaxed);
    }
}

fn bytes(size: usize) -> u64 {
    match u64::try_from(size) {
        Ok(value) => value,
        Err(_) => { ACCOUNTING_INVALID.store(true, Ordering::Relaxed); 0 }
    }
}

// Best-effort, allocation-free diagnostics on the Unix hosts used by the external time(1) harness.
// This writes stderr; it performs no OS resource query and cannot recurse into GlobalAlloc.
#[cfg(unix)]
fn marker(text: &'static [u8]) {
    unsafe extern "C" {
        fn write(fd: std::os::raw::c_int, data: *const std::ffi::c_void, count: usize) -> isize;
    }
    // SAFETY: stderr is the conventional descriptor 2; text is an immutable live byte slice for
    // exactly count bytes. write never takes ownership, and failure/short write is tolerated.
    let _ = unsafe { write(2, text.as_ptr().cast(), text.len()) };
}

#[cfg(not(unix))]
fn marker(_: &'static [u8]) {}

fn failed_allocation() {
    add(&ALLOCATION_FAILURES, 1);
    marker(b"{\"event\":\"allocator_failure\",\"kind\":\"system_returned_null\"}\n");
}

// SAFETY: every operation forwards the caller's original pointer/layout directly to System.
// Accounting uses only nonallocating atomics, never changes a returned block, and never unwinds.
unsafe impl GlobalAlloc for CountedSystem {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: GlobalAlloc's caller supplies a valid nonzero layout, forwarded unchanged.
        let pointer = unsafe { System.alloc(layout) };
        if pointer.is_null() { failed_allocation(); } else {
            add(&ALLOCATIONS, 1);
            let size = bytes(layout.size()); add(&REQUESTED_BYTES, size); add(&LIVE_BYTES, size);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: same allocation contract; System additionally preserves zero initialization.
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if pointer.is_null() { failed_allocation(); } else {
            add(&ALLOCATIONS, 1);
            let size = bytes(layout.size()); add(&REQUESTED_BYTES, size); add(&LIVE_BYTES, size);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: the caller owns a live block from this allocator with this same layout. No
        // access to the block occurs after its one corresponding System deallocation.
        unsafe { System.dealloc(pointer, layout) };
        add(&DEALLOCATIONS, 1); subtract(&LIVE_BYTES, bytes(layout.size()));
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: caller supplies a live System block with its original layout and a valid nonzero
        // new_size. On null the original block remains live; otherwise System owns its replacement.
        let replacement = unsafe { System.realloc(pointer, layout, new_size) };
        if replacement.is_null() { failed_allocation(); } else {
            add(&REALLOCATIONS, 1); add(&REQUESTED_BYTES, bytes(new_size));
            if let Some(growth) = new_size.checked_sub(layout.size()) { add(&LIVE_BYTES, bytes(growth)); }
            else if let Some(shrink) = layout.size().checked_sub(new_size) { subtract(&LIVE_BYTES, bytes(shrink)); }
        }
        replacement
    }
}

#[derive(Clone, Copy)]
struct Snapshot {
    allocations: u64, deallocations: u64, reallocations: u64,
    requested_bytes: u64, live_bytes: u64, allocation_failures: u64, invalid: bool,
}

impl Snapshot {
    fn now() -> Self {
        Self {
            allocations: ALLOCATIONS.load(Ordering::Relaxed),
            deallocations: DEALLOCATIONS.load(Ordering::Relaxed),
            reallocations: REALLOCATIONS.load(Ordering::Relaxed),
            requested_bytes: REQUESTED_BYTES.load(Ordering::Relaxed),
            live_bytes: LIVE_BYTES.load(Ordering::Relaxed),
            allocation_failures: ALLOCATION_FAILURES.load(Ordering::Relaxed),
            invalid: ACCOUNTING_INVALID.load(Ordering::Relaxed),
        }
    }

    fn value(self) -> Value {
        json!({"allocations":self.allocations,"deallocations":self.deallocations,
            "reallocations":self.reallocations,"requested_bytes":self.requested_bytes,
            "live_bytes":self.live_bytes,"allocation_failures":self.allocation_failures,
            "accounting_invalid":self.invalid})
    }
}

fn phase(before: Snapshot, after: Snapshot, elapsed: Duration) -> Value {
    let difference = |a: u64, b: u64| b.checked_sub(a);
    let live = i128::from(after.live_bytes).checked_sub(i128::from(before.live_bytes));
    let allocations=difference(before.allocations,after.allocations);
    let deallocations=difference(before.deallocations,after.deallocations);
    let reallocations=difference(before.reallocations,after.reallocations);
    let requested=difference(before.requested_bytes,after.requested_bytes);
    let failures=difference(before.allocation_failures,after.allocation_failures);
    let valid=!before.invalid&&!after.invalid&&allocations.is_some()&&deallocations.is_some()
        &&reallocations.is_some()&&requested.is_some()&&failures.is_some()&&live.is_some();
    json!({"duration_ns":elapsed.as_nanos().to_string(),
        "allocations":allocations,"deallocations":deallocations,"reallocations":reallocations,
        "requested_bytes":requested,"allocation_failures":failures,
        "live_bytes_delta":live.map(|n|n.to_string()),"accounting_valid":valid})
}

#[derive(Clone, Copy)]
struct Inputs { nodes: u32, cpu_milli: u64, mem_bytes: u64, rounds: u64 }

fn inputs() -> Result<(Inputs, Option<cycles::Inputs>), String> {
    let mut arguments = std::env::args_os(); let _=arguments.next();
    let (mut nodes, mut cpu_milli, mut mem_bytes, mut rounds) = (None, None, None, None);
    let mut extension = cycles::Options::default();
    while let Some(flag) = arguments.next() {
        let flag = flag.to_str().ok_or("argument name is not UTF-8")?;
        if extension.parse(flag, &mut arguments)? { continue; }
        let target = match flag {
            "--nodes" => &mut nodes, "--cpu-milli" => &mut cpu_milli,
            "--mem-bytes" => &mut mem_bytes, "--rounds" => &mut rounds,
            _ => return Err("unknown argument; require --nodes --cpu-milli --mem-bytes --rounds".into()),
        };
        if target.is_some() { return Err(format!("duplicate {flag}")); }
        let value = arguments.next().ok_or_else(|| format!("missing value for {flag}"))?;
        *target = Some(value.to_str().ok_or("argument value is not UTF-8")?.parse::<u64>()
            .map_err(|_| format!("unsigned integer required for {flag}"))?);
    }
    let nodes = u32::try_from(nodes.ok_or("missing --nodes")?).map_err(|_| "node count exceeds public index ID range")?;
    let rounds = rounds.ok_or("missing --rounds")?;
    if rounds == 0 { return Err("--rounds must be positive".into()); }
    Ok((Inputs { nodes, cpu_milli: cpu_milli.ok_or("missing --cpu-milli")?, mem_bytes: mem_bytes.ok_or("missing --mem-bytes")?, rounds }, extension.finish()?))
}

fn input_nodes(input: Inputs) -> Result<Vec<NodeSpec>, String> {
    let mut nodes = Vec::new();
    let count = usize::try_from(input.nodes).map_err(|_| "node count exceeds usize")?;
    nodes.try_reserve_exact(count).map_err(|e| format!("input vector reservation failed (not SUT): {e}"))?;
    for id in 0..input.nodes {
        nodes.push(NodeSpec::new(NodeId(id), Resources {
            cpu_milli: input.cpu_milli, mem_bytes: input.mem_bytes, gpus: 0, disk_bytes: 0,
        }));
    }
    Ok(nodes)
}

fn panic_text(payload: &(dyn std::any::Any + Send)) -> &str {
    if let Some(text) = payload.downcast_ref::<String>() { text }
    else if let Some(text) = payload.downcast_ref::<&'static str>() { text }
    else { "non-string panic payload" }
}

fn emit(output: &mut impl Write, value: &Value) -> Result<(), String> {
    serde_json::to_writer(&mut *output, value).map_err(|e| e.to_string())?;
    output.write_all(b"\n").and_then(|()| output.flush()).map_err(|e| e.to_string())
}

fn run() -> Result<(), String> {
    let (input, cycles) = inputs()?;
    if let Some(workload) = cycles { return cycles::run(input, workload); }
    // Keep both input vector and reporting handle alive across all snapshots and rounds.
    let nodes = input_nodes(input)?;
    let mut output = io::stdout().lock();
    for round in 0..input.rounds {
        let before = Snapshot::now();
        let began = Instant::now();
        let constructed = catch_unwind(AssertUnwindSafe(|| FitIndex::new(std::hint::black_box(&nodes))));
        let construct_elapsed = began.elapsed();
        let after_constructor = Snapshot::now();
        let mut record = match constructed {
            Ok(Ok(index)) => {
                std::hint::black_box(&index);
                // No output formatting or input destruction between constructor/drop snapshots.
                let drop_began = Instant::now();
                let dropped = catch_unwind(AssertUnwindSafe(|| drop(index)));
                let drop_elapsed = drop_began.elapsed();
                let after_drop = Snapshot::now();
                let retained = after_constructor.live_bytes.checked_sub(before.live_bytes);
                let valid = !before.invalid && !after_constructor.invalid && !after_drop.invalid && retained.is_some()
                    &&after_drop.live_bytes>=before.live_bytes;
                let status = if dropped.is_err() {"drop_panic"} else if !valid {"accounting_error"} else {"ok"};
                let panic = dropped.as_ref().err().map(|payload| panic_text(payload.as_ref()));
                json!({"status":status,"panic":panic,"accounting_valid":valid,
                    "constructor":phase(before,after_constructor,construct_elapsed),
                    "constructor_retained_live_bytes":retained,
                    "drop":phase(after_constructor,after_drop,drop_elapsed),
                    "after_drop_live_bytes_delta":i128::from(after_drop.live_bytes).checked_sub(i128::from(before.live_bytes)).map(|n|n.to_string()),
                    "before":before.value(),"after_constructor":after_constructor.value(),"after_drop":after_drop.value()})
            }
            Ok(Err(error)) => json!({"status":"constructor_error","error":error.to_string(),
                "constructor":phase(before,after_constructor,construct_elapsed),"constructor_retained_live_bytes":null,
                "drop":null,"before":before.value(),"after_constructor":after_constructor.value()}),
            Err(payload) => json!({"status":"constructor_panic","panic":panic_text(payload.as_ref()),
                "constructor":phase(before,after_constructor,construct_elapsed),"constructor_retained_live_bytes":null,
                "drop":null,"before":before.value(),"after_constructor":after_constructor.value()}),
        };
        record["event"] = json!("round"); record["round"] = json!(round);
        record["inputs"] = json!({"nodes":input.nodes,"cpu_milli":input.cpu_milli,"mem_bytes":input.mem_bytes,"rounds":input.rounds});
        record["copied_bytes"] = Value::Null; record["copy_measurement"] = json!("unmeasured");
        let okay = record.get("status").and_then(Value::as_str) == Some("ok")
            && record.get("accounting_valid").and_then(Value::as_bool) == Some(true);
        emit(&mut output, &record)?;
        if !okay { return Err("round failed; see its JSON record and stderr markers".into()); }
    }
    std::hint::black_box(&nodes); // Explicitly retain original input allocations through the last output.
    Ok(())
}

fn main() -> ExitCode {
    let original = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        marker(b"{\"event\":\"panic\",\"kind\":\"panic_hook\"}\n");
        original(info);
    }));
    match catch_unwind(AssertUnwindSafe(run)) {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(error)) => {
            let _ = emit(&mut io::stderr().lock(), &json!({"event":"error","error":error}));
            ExitCode::FAILURE
        }
        Err(payload) => {
            let _ = emit(&mut io::stderr().lock(), &json!({"event":"error","kind":"harness_panic","panic":panic_text(payload.as_ref())}));
            ExitCode::FAILURE
        }
    }
}
