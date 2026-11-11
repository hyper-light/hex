# ROOT integration boundary

Use exact source.json dependency cohort. Do not replace outside Recorded or API/wire files from this guest branch.

Existing initialization remains: verified GuestConfig::from_env_with_cost(), Shards::configured(exact_cache,exact_home), same RuntimePaths clone, GuestRuntime::new(provider,config).recorded_at(path).with_runtime_paths(paths).with_gpu_catalog(SAME SharedCatalog).with_runtime_cost(cost), WithGuest::configured, then Arc<Recorded<WithGuest<P>>>. Derive Weak<dyn Books> from that SAME existing Arc, bind_books, then call guest.prepare_backing() BEFORE ordinary recover/capacity. This persists Prepared scope only. SAME Index must reserve offer.allocation()+exact claims and invoke Runtime::initialize_backing(grant) through the existing append-only RPC before candidate admission. A caller-created equal tuple/capacity report is not an independently issued grant. Recovered Granted/Live uses original admitted receipt; no new grant or VM is invented. recover_backing observes the retained scope before ordinary inner recovery. Runtime::release_backing is the actual same-owner terminal path.

Windows native capacity must already contain its initialized NativeWindows route/masks; guest augmentation appends Linux shared/dedicated routes without replacing native HCS. L3 owns that producer. Mac ordinary Process routing is retained. Root7 Recorded runtime_for calls use route_for(&record.spec), which this source does not overwrite.

On Linux, ordinary CPU OCI remains native. Do not eagerly prepare/boot a CPU shared Shards pool merely because the dedicated Shards adapter is configured; explicit ClassShards CPU work remains per-box dedicated. GPU observer provisioning is a separate pending actual path, not a fake CPU pool or guest model row.

The configured native/probe cache/home pair must be identical; no ambient HOME/PATH/SHARDS_BIN fallback. The carried binary must include the reviewed --host-managed helper inheritance/parser source (full producer070964ea, API/ABIeff3), with matching carried VM/init ABI. The old GPU binary is not accepted evidence. Warm-up remains skipped whenever a cost or Books owner is initialized; no uncharged second VM starts.

For host measurement credit call Runtime::backing_usage(&key), whose exact API pin42de is already ROOT378192. Feed the existing Service native sampler and monotonic CPU-delta logic once. Do not add inner guest RSS or cumulative CPU time to host free capacity. All outer BoxSpec allocation/route literals are retained only where factual; the guest rewrite clears them before its native OCI request.

No CLI reachability, full envelope enforcement, actual VMM/renderer build or native termination result is claimed by this source. ROOT native testers/gates own those effects.
