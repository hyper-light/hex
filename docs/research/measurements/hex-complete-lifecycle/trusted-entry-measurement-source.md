Trusted-entry focused caller on exact ROOT786775e3

SOURCE ONLY / measurement only / no native invocation by L3. This checkpoint
completes the focused caller for cfd222a8971295fdb90409a906d0fecda20d1e3d.
It does not import the held 2ef/fab helper, create a worker protocol, change
ordinary runtime behavior or authorize any canonical release/refund. The five
new pure caller controls and the component's three controls are UNRUN. Local
rustfmt parsing and diff metadata are the only local checks.

Exact composition

- Source parent ROOT786775e3b119c5163d49aa12662c0207cb25cc29, then native sys
  component cfd222a8971295fdb90409a906d0fecda20d1e3d. Take its three named paths
  and this checkpoint's four named paths; do not take any full helper branch.
- L2 sole builder dea47d8425cccfa5a498414950909bcd3ea0f20c is a required source
  dependency. ROOT5688fd7516d260732796fbc53b45a8e66a2c46a5 already joined it
  with guest module/feature metadata; ROOTfabd73020cdd1c6e201d8028fcca5d85f28ef18c
  retains it. The builder itself stays in L2 ownership; no copy exists here.
- The builder calls actual load_runtime_inputs(cost_file,cost_sha256,evidence),
  GuestConfig.checked_runtime_allocation, and API prepare_native_boot/NativeBoot.
  Both images invoke that ONE builder from owned values. The native_startup view
  retains typed paths/kernel/ordered shares/console inputs and actual image
  declarations. This caller does not parse opaque Shards argv to recover them.
- Native-image API production4b0e3f4e3913047b5caf08e529f5f52c53102b95, final
  fixture API68941d44c7c39d27f0e9ade5d1d0e7169af53002; Hex loader production
  480d1d6a2c547ca38131c0850963f982e84feefb / a89737cf6b19daee65a092006cb821f499445a6d.
  NativeImages.cli denotes the foreign Shards CLI, not the trusted Hex image.
- ROOT reported builder2 PASS and Runtime/Node/CLI strict0 at5688, and earlier
  producer5 at its own exact cohort. Those results do not cover this caller/sys
  component and are not replayed or relabeled here.

Real public entry points

    dispatch() -> Result<Option<u8>, Failure>
    run(Input<'_>, &mut dyn FnMut(&Identity) -> Result<(), Refusal>)
        -> Result<Report, Failure>

Input fields are public and all required: record:&BoxRecord, shared:&Shared,
config:&GuestConfig, cost_file:&Path, cost_sha256:[u8;32], evidence:&Path,
paths:&RuntimePaths, socket:&Path, console_kept:usize, bootstrap:&Path,
bootstrap_image:shards_api::ImageIdentity, deadline:Instant, journal:&mut File.
Shared is the existing macos::process::Shared, initialized with ROOT's real
learned seeds; this fixture supplies no timing/profile defaults.

The row must be an actual canonical Creating CPU ShardsDedicated Box with
native_scope, exact actual Allocation, no backing reference, no physical devices,
and init/run/exit absent. This narrow measurement cannot skip a Pool handoff.
The publisher is the existing caller's durable exact-Identity publication; it
must succeed before the first CONT. The measurement does not write Store itself,
construct a fake Process BoxId or supply a default RuntimeCost. The original row
and charge remain held even after successful known-controller cleanup.

ROOT registration only (not edited in this checkpoint)

1. Add to macos/process.rs:

    #[cfg(feature = "native-trusted-entry-measurement")]
    pub mod trusted_entry;

2. Add to macos/mod.rs:

    #[cfg(feature = "native-trusted-entry-measurement")]
    pub use process::trusted_entry;

3. ROOT5688 already declared Runtime's native-trusted-entry-measurement and CLI
   forwarding. Preserve those definitions and the guest::native_fixture module.
4. Insert this branch as the FIRST statement in CLI main(), before ANY standard
   argument collection/reader. ExitCode is already the existing main return type.
   All normal nonmatching invocations retain the existing CLI path.

    #[cfg(all(target_os = "macos", feature = "native-trusted-entry-measurement"))]
    match hex_runtime::macos::trusted_entry::dispatch() {
        Ok(Some(code)) => return ExitCode::from(code),
        Ok(None) => {}
        Err(failure) => {
            use std::io::Write as _;
            if let Err(write_error) = writeln!(
                std::io::stderr().lock(), "TRUSTED_ENTRY_ERROR {failure}"
            ) {
                let _ = writeln!(
                    std::io::stdout().lock(),
                    "TRUSTED_ENTRY_ERROR {failure}; DIAGNOSTIC_WRITE {write_error}"
                );
            }
            return ExitCode::FAILURE;
        }
    }

A matching private invocation always returns its result; it never falls through
to the normal CLI. The private selector is a measurement syntax, not an
authentication capability or a new privileged command. Std/dyld/library startup
still occurs before main; this does not establish that the Hex image has one
thread or cannot create an unknown pre-main descendant.

The parent caller supplies these existing variables to construct the sole Input:

    let report = hex_runtime::macos::trusted_entry::run(
        hex_runtime::macos::trusted_entry::Input {
            record: &creating_record, shared: &actual_shared,
            config: &actual_guest_config,
            cost_file: &actual_cost_file, cost_sha256: actual_cost_sha256,
            evidence: &actual_evidence, paths: &actual_runtime_paths,
            socket: &fixture_socket, console_kept: original_console_kept,
            bootstrap: &this_exact_hex_binary,
            bootstrap_image: actual_hex_image_identity,
            deadline: original_absolute_deadline, journal: &mut owned_journal,
        },
        &mut durable_original_identity_publisher,
    );

This is a constructor snippet, not an executable command with invented input
files/profile/limits. ROOT owns the outer finite guard, exact admitted input
bytes/digests and disposable fixture row, raw collection and eventual measurement.
The syntactic socket is passed to the real preparation API; this probe never
creates/dials a guest socket or starts a VMM. No input comes from ambient env.

Ownership and transitions

Parent: validate actual canonical input + same cost evidence -> retain verified
open CLI/kernel/Hex file witnesses -> prepare report storage/FDs/ImageEvents and
existing retained BoxProc -> allocate SAME ProcessBox -> native held Hex ->
immediate actual PID/birth facts -> genuine token -> durable publisher -> ONE
CONT of trusted Hex -> exact NOTE_EXEC + stopped/live same birth + fresh genuine
task-derived audit token -> validate scope/commitment/NPROC/source-FD report ->
foreign held -> owned abort and sole direct-controller reap. There is NO CONT of
the foreign image on any path. NOTE_EXEC and NOTE_EXIT are notifications only;
BoxProc uses NOWAIT and is the sole native wait/reap owner.

Measurement-specific owner preparation uses existing ProcessBox::prepared,
Scope::new(NativeRecord::Box,Ownership::Child), Prepared and Controller. It does
not start the ordinary cadence watcher, because that watcher may CONT its own
held PID across a same-PID exec. The private measurement has exactly one allowed
CONT and never exports the receipt as ManagedProcess. Ordinary prepare_managed,
watcher cadence, CPU accounting and runtime behavior are unchanged. No aggregate
CPU/backstop/profile equivalence is claimed from this preparation.

Entry: owned fixed input parse -> SAME builder -> exact prepared foreign argv/
environment/stdio -> prepare actual native attrs/actions and unexpected-return
BoxProc -> NPROC soft/hard0 readback -> child-only source FD flags -> SETEXEC|
START_SUSPENDED. No COW child runs this Rust path, and no pointer/opaque native
object is carried across exec. A genuine post-NOTE_EXEC token replaces the old
image token; PID/birth alone does not freeze XNU exec version.

If posix_spawn unexpectedly returns success with a positive child PID, the entry
binds its already prepared sole direct-parent BoxProc BEFORE any lookup or
publication. It immediately reports PID, then positive birth, token-fenced kill,
original-deadline wait and exact reap. Any error retains the waiter, original
error, cleanup error and diagnostic errors. A nonpositive/self returned PID is
reported as an invalid native return, never signaled numerically. No scan,
PID/PGID-signal fallback, second reaper or inferred child absence is used.

Parent raw PID/birth and ImageEvent flags/fflags/data precede relevant oracles.
Failures always run cleanup outside the operation closure; a cleanup-start/end
or report drain write error cannot skip that attempt. Failures pin the SAME
ProcessBox receipt and distinguish original/cleanup/diagnostic errors. Accessors
known_controller() and known_unexpected_child() expose only retained observations,
never authority reconstructed from console text. Reap is exact known-controller
cleanup only; Report has no TerminalScope/ManagedEnd/Released field. Unknown
pre-main or returned-child cleanup stays unknown. On cleanup failure ROOT retains
the error/receipt and charge; dropping the value is not a cleanup witness.

FD/input/deadline bounds

- Parent report stdout and diagnostic stderr are duplicates of the SAME private
  writer. Original parent writer/null descriptor flags are recorded before launch
  and checked unchanged at the held gate. Only the private read end is NONBLOCK.
- Trusted entry duplicates only std0/1/2 with CLOEXEC. Its report copy remains
  private and is closed by final exec. Final foreign stdin/stdout/stderr are null
  sources; source roles0/1/2 are fixed and observed independently after their
  final numbers are assigned. The entry clears CLOEXEC ONLY on these owned final
  source copies. Neither original parent's flags nor arbitrary FDs are changed.
- Private argv has22 fixed arguments (including argv0/selector) +3 per ordered
  Share(dir,read_only,at). Decimal representation uses u64's20 digit maximum;
  scope/digest encodings use fixed16/32 bytes. Hostile counts/modes/extra/truncated
  fields and nonabsolute/NUL paths refuse before the sole builder/native effect.
- The actual native kern.argmax bounds matching argv copies, retained report,
  both exec argument/env bytes + NULs + pointer arrays/terminators. Shares remain
  bounded by the existing architecture-derived MAX_SHARES. The512-byte drain
  chunk is POSIX PIPE_BUF's minimum, not a measured latency/throughput parameter.
- raw expiry is sampled BEFORE remaining original Instant. Entry and native
  events use that SAME CLOCK_UPTIME_RAW expiry, no new Instant/tolerance/period.
  Cleanup uses the same original remaining budget in existing retained BoxProc.
  Deadline expiry can leave cleanup incomplete; no larger guard or retry is added.
- Error detail storage is one fallibly pre-reserved record before actor effects;
  diagnostic slots are the fixed cleanup-start/end/drain stages. No new Store,
  owner table, persistent ledger, cache, thread/worker or admission manager exists.

Research, safety and evidence boundary

The new leaf contains no unsafe block or new native symbol. Every necessary new
unsafe call remains isolated in cfd222's existing sys.rs, with primary pins and
SAFETY invariants in trusted-entry-native-component.md/json and b467 comparison.
Apple XNU/Libsyscallf6217f891ac0bb64f3d375211650a4c1ff8ca1ea, Libc71bbe350ab79eef58113991d817ccc6165061a64,
Rust58cf8f9c35b355199727627d07ae34a4e2846d67 are exact primary revisions;
installed-kernel/libSystem/image equivalence is not claimed. Prior C8cd held/
NPROC/FD/QoS/jetsam data retains its own source attribution; it is not a result
for fresh Rust/Hex startup or multithreaded COW safety.

Verified open files are content witnesses, NOT immutable exec handles. This
measurement still executes named paths and cannot prove actual image bytes,
TOCTOU closure, dyld/main thread behavior, pre-main unknown descendants, fixed
workers, GPU artifact/service closure, HVF/Vulkan compatibility, aggregate CPU/
memory/resource limits, persistent helper charge, cold recovery or whole-scope
absence. The full helper/production worker launch remains held separately.

Five new pure controls (all UNRUN): fixed typed transport preserves share rights/
nonUTF8 paths/kept0/raw expiry; hostile carrier refuses extra rights/defaults;
stale scope/commitment/native report and FD roles refuse; exact ARG_MAX pointer/
NUL/environment boundary; bounded error record preserves native+cleanup+write
causes. Component3 remains UNRUN. No build/typecheck/strict/native/policy gate ran.

R01-R16 record: R01/R03/R04/R06/R07/R13 have source guards/bounds/typed transitions
above, with compilation and adversarial proof owed; R02/R05/R12 cite b467/cfd
alternatives and reuse the same native owner/waiter, no performance conclusion;
R08/R14 profiles/comparisons/allocations/fault data remain ROOT work; R09/R10
preserve06e escaped-descendant original and C mechanism raw, no production fix
or AFTER result claimed; R11 macOS arm64 only and other-platform gates unrun;
R15 implementer source only, no new agent/native campaign; R16 tiny named source
files only, no evidence/cache deletion or host changes. Policy revision
7d7e0855a5073ed5163b4fe7ed3355c9ed7c2506 / rules SHA256
 df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3
was reread in full after resume, with Hex charter/current handoff. Existing ROOT
source release/ACK persists; no additional permission wait was introduced.
