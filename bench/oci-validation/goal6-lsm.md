# Goal 6: SELinux mount labels and AppArmor profiles applied, runtime-tools on both modules (AC-T7.1.4)

The decision is docs/design/runtime.md, "SELinux mount labels and AppArmor profiles are applied, and refused
where the module is off". Its sources are docs/research/runtime/goal6-lsm.md. SELinux and AppArmor can't both
run in one kernel (research §4), so the suite runs twice: once where each module is live. Both runs use
opencontainers/runtime-tools bd3b0bd and the same hex tree (cp/t7-g6-lsm, the tree after baf2a96 with the
fixes in this record). Built, not merged.

## Where SELinux runs: fedora, enforcing (se_lab.sh)

- Unit hex-lsm-se2-20261105T051043Z, 2026-11-05 05:10Z, on kiln-a094a12d.attlocal.net (fedora): kernel
  6.19.10-300.fc44.x86_64, SELinux enforcing. Load average 41 (noisy, per the bench rules). Co-tenants:
  hexbench-oci, issue556oss-control-plane, three buildx builders, kind-registry and kiln-t10-minio.
- The suite runs in run_hex.sh's privileged container (label=disable). The kernel runs SELinux there, but
  no selinuxfs is mounted in the container.
- hex (release, this tree): 27 pass. crun 1.27 (the host's): 28 pass. crun read 27 in the run just before,
  hex-lsm-final-20261105T050614Z, at the same load.
- Test by test, hex passes every test crun passes but one:
  - `linux_process_apparmor_profile`: hex refuses, by name: `process.apparmorProfile is set
    ("acme_secure_profile") but AppArmor isn't enabled on this host`. crun passes it by running the container
    without the profile (research §3: `set_apparmor_profile` returns 0 when AppArmor is off,
    `src/libcrun/utils.c:1111-1128`).
  - `linux_mount_label`: both pass. hex applies the label, which the kernel accepts as each tmpfs's
    `context`. crun passes because it skips the label: it finds no `selinux` in the container's
    `/proc/mounts` (`src/libcrun/utils.c:885-906`, `:954-962`).
- hex passes no test crun fails.

## Where AppArmor runs: Ubuntu 24.04 guest under KVM (aa_lab.sh)

- Unit hex-lsm-aa3-20261105T050217Z, 2026-11-05 05:02Z, on fedora (load average 41, the same co-tenants).
  The guest is vmaa_build.sh's: Ubuntu 24.04, linux-image-virtual 6.8.0-31-generic (vmlinuz sha256
  cee36e26…), AppArmor enabled (Y), and `acme_secure_profile` loaded in complain mode. Every test ran as root.
- hex (release, built for the guest in ubuntu:24.04, sha256 2664f603…): 29 pass. crun 1.27's static release
  (sha256 2a80f801…): 30 pass. Ubuntu's own crun 1.14 refuses the suite's runtime-spec 1.3.0 ("unknown version
  specified"), so it can't be the reference.
- Test by test, hex passes every test crun passes but one:
  - `linux_mount_label`: hex refuses, by name: `linux.mountLabel is set (...) but the kernel refused it as a
    tmpfs's context (Invalid argument)`. SELinux isn't active, so no module claims the key
    (`fs/fs_context.c:137-148`). crun passes it by running the container unlabelled (research §3).
  - `linux_process_apparmor_profile`: both pass, and hex applies the profile.
- hex passes no test crun fails.

## Across the two

Each of the 28 or 30 tests crun passes on a host, hex passes there too, but for the one test that host's
missing module can't serve. And each of those two tests passes, with its label actually applied, on the host
where its module runs. crun's 28 on one host include one label it silently skipped.

## Proof that each label is applied (crates/hex-cli/tests/oci_lsm.rs)

runtimetest's own checks for these two tests are close to empty (research §4), so Hex's own test reads each
label back from inside the container:
- SELinux, fedora, rootless (hex-lsm-se-20261105T030243Z and later units): `stat -c %C` on `/tmp`, `/dev` and
  a masked directory each read the spec's label `system_u:object_r:container_file_t:s0:c715,c811`. The
  AppArmor test there is the refusal by name.
- AppArmor, the guest, as root (hex-lsm-aa3-20261105T050217Z): `/proc/1/attr/apparmor/current` of the
  container's first process names `acme_secure_profile`. The mount-label test there is the refusal by name.
- Each run also creates an unlabelled control container first, so a failure is the label's or the profile's.

## What the guest found, fixed in this tree

- Before Linux 6.15, `open_tree(OPEN_TREE_CLONE)` refuses a detached mount (v6.8 `fs/namespace.c:2588`), so
  the node's pooled empty masking tmpfs failed every masked directory on 6.8. `ProcSources` now probes the
  clone once and, where the kernel refuses, gives each masked directory its own tmpfs.
- The AppArmor enabled check had read `/sys` in the container's init, where `/sys` may not be mounted. It now
  asks the kernel through `/proc` (the scoped `current` attribute reads EINVAL where the module is off).
