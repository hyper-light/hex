# Concrete ROOT entry points

No native run, profiling or mutation in this file has been executed by L4.
Run admission/resource/pause instructions from CHARTER.md continue to apply.

Build the owned standalone fixtures from the final integrated source, retaining
the exact Cargo JSON compiler-artifact record and hashing the resulting binary:

    nice -n 10 cargo test -p hex-cli --test completion_backends --locked --no-run --message-format=json -j1
    nice -n 10 cargo test -p hex-gpu --test completion_telemetry --locked --no-run --message-format=json -j1
    nice -n 10 cargo test -p hex-gpu --test completion_windows_compute --locked --no-run --message-format=json -j1 --target x86_64-pc-windows-msvc

Build the Vulkan test-only workload on the actual host and separately for the
guest ABI/root image. Keep the compiler and Vulkan-Headers/loader pins; the
headers must provide Vulkan 1.1, VK_EXT_pci_bus_info and portability declarations.
On Apple link libobjc in addition to the actual Vulkan/MoltenVK loader:

    PINNED_GLSLANG_VALIDATOR -V --target-env vulkan1.1 scripts/scheduler-completion/compute.comp -o OWNED/compute.spv
    PINNED_CC -std=c11 -O2 -Wall -Wextra -Werror -I PINNED_VULKAN_HEADERS scripts/scheduler-completion/vulkan-witness.c -L PINNED_LOADER -lvulkan -o OWNED/vulkan-witness

The host and guest workload binary/shader hashes are distinct artifacts. Stage
the guest binary, shader, loader and native/vendor libraries into the image using
the existing image/rootfs tools. The workload never receives a PCI host path or
attachment authority. It receives the UUID obtained by the independent physical
join and verifies actual GPU writes against poisoned CPU-oracle outputs.

ROOT's lab JSON for prepare.py names exact binary pins for hex, python,
vulkan, native_executable and (full mode) telemetry. It also gives:

- backend, topology, proof (full or selected), scratch, wait_ms, byte_limit;
- ordinary services array with unique backend/scheduler roles, exact Hex args,
  env object and readiness line; backend/scheduler sockets and runner node ID;
- files array for the actual ordinary node/scheduler/backend configs, catalog,
  rootfs/bundle descriptors in the new scope; record_root, demand and typed payload;
- native_identity path/format/pointer naming the actual runtime/VMM receipt;
- cleanup_absent exact owned marker/config/socket/runtime paths;
- max_items, max_shader_bytes, words, rounds, challenge, gpu_wait_ns,
  guest_vulkan_program, guest_shader, guest_env and observer_timeout_seconds;
- for full mode, wrong_uuid, catalog path/before/after (same entries permuted),
  telemetry binary pin, and load_actor (pinned actual host-compute witness,
  explicit finite args/env, name and dispatch_started readiness line);
- for Firecracker, actual firecracker_api path and selected_host_sbdf.

The compiler/source/runtime pins must describe the final integrated candidate,
not merely the original b06b base. Config files are copied into the receipt;
prepare.py embeds the helper sources and copies source-pinned Hex/workload helpers
into the fixture's new private scope before executing them.

Native identity formats are json with an explicit object pointer, hex-guest
(the existing shared guest record), or hex-selected-guest (the dedicated selected
VMM record). Firecracker uses the selected host BoxRecord init pid/start_ticks.
Shards selected guests use the guest-record path plus .gpu/PHYSICAL_BOX. All
reads are actual records; a synthetic PID or expected executable label fails.
The independent OS observer then checks the actual process start/image/hash,
Linux container namespaces or KVM VM descriptor where applicable.

    python3 scripts/scheduler-completion/prepare.py --lab /ABS/lab.json --source FINAL_SHA --byte-limit ROOT_BOUND --output /ABS/new-fixture.json
    python3 scripts/scheduler-completion/run.py --root-authorized --source FINAL_SHA --binary /ABS/completion_backends --binary-sha256 SHA256 --config /ABS/new-fixture.json --backend oci --topology laptop --proof full --target x86_64-unknown-linux-gnu --timeout ROOT_SECONDS --cleanup-timeout ROOT_SECONDS --byte-limit ROOT_BOUND --output /ABS/new-run

Repeat with actual firecracker/shards and peer configurations. A selected-only
invocation records five stages and explicitly leaves foreign/wrong-ID/recovery
unverified; it cannot satisfy a full matrix cell. A full case needs a physically
different accessible host GPU as its positive foreign-device control. A host
without one can still run selected-only; lack of the second device remains a gap.

For Firecracker the selected host GPU is already bound to vfio-pci and cannot
simultaneously appear in the host Vulkan driver inventory. The fixture instead
joins selected host PCI -> the exact running VMM's physical-attachment API ->
actual guest PCI -> guest Vulkan deviceUUID. The API response and native VMM PID
are retained. A separate driver-bound host companion supplies the foreign control.
No harness command loads/unloads/rebinds a driver or changes IOMMU/runner setup.

The peer case uses the actual production peer transport/configuration and checks
the intended runner ID. For a non-Linux Firecracker origin, ROOT additionally
records the actual origin control path and its same physical placement/runner
receipt hash. It is a routed Linux execution proof; no local non-Linux GPU or
Firecracker support is inferred. The matrix excludes impossible non-Linux local
Firecracker cells and retains the peer-routing obligations.

Build a matrix manifest with source and cells of target/backend/topology/receipt.
Missing hardware cells carry an explicit reason. Gate verifies full native
receipts and computes all 39 actual/routed obligations:

    python3 scripts/scheduler-completion/gate.py --manifest /ABS/matrix.json --byte-limit ROOT_BOUND --output /ABS/matrix-result.json

ROOT guard controls use mutation.py with an exact file/old/new/original hash,
source pin, build argv, verify argv and relevant_failure oracle. For the shader
no-store control remove only the output assignment; GPU_oracle_mismatch must
fail, then exact source restoration and rebuilt green must pass. Source
restoration never overwrites bytes beyond original/this mutation. A compilation,
unavailable-hardware or deadline error is not a valid behavioral red.

    python3 scripts/scheduler-completion/mutation.py --root-authorized --source-root /ABS/private-integration --spec /ABS/no-store.json --output /ABS/new-mutation --byte-limit ROOT_BOUND --timeout ROOT_SECONDS --cleanup-timeout ROOT_SECONDS

On any timeout inspect the retained scope, owned-pids journal, actual native
records and raw logs. Reaping the test child is not guest/VMM/driver cleanup.
Only the production terminal/absence proof and native cleanup observer can close
that gap. Windows ARM VM remains paused; no template in this directory resumes it.
