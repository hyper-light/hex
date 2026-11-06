# Goal 6/17: SELinux mount labels and AppArmor profiles, runtime-tools on both modules (AC-T7.1.4)

The decision is docs/design/runtime.md, "SELinux mount labels and AppArmor profiles are applied; where the
module is off, skipped with a warning or refused under strict". Its sources are
docs/research/runtime/goal6-lsm.md. The bar is the lead's for goal 17 (2026-11-06): all the tests crun passes.
SELinux and AppArmor can't both run in one kernel (research §4), so the suite runs where each module is live.
Both runs use opencontainers/runtime-tools bd3b0bd and cp/t7-g6-lsm 7d78240, in one unit,
hex-lsm-final2-20261106T053914Z, whose 7 gates passed first (exit 0; 332 tests passed, 0 failed). Built, not
merged.

| Host | hex, default | hex, `HEX_OCI_LSM=strict` | crun 1.27 |
|---|---|---|---|
| fedora, SELinux enforcing | 28 | 27 | 28 |
| Ubuntu 24.04 guest, AppArmor | 30 | 29 | 30 |

By default hex passes exactly the tests crun passes, on each host, and hex passes no test crun fails. Under
strict the one difference on each host is the test whose module that host doesn't run, refused by name.

**What those passes mean.** On each host one LSM test passes with its label applied, and the other passes
because the container runs without the label, as crun's does, with hex warning of it. No one kernel can apply
both. runtimetest's own checks for these two tests don't look at the label (research §4); the applied labels
are proved below.

## Where SELinux runs: fedora, enforcing (se_lab.sh)

- 2026-11-06 05:40Z, on kiln-a094a12d.attlocal.net (fedora): kernel 6.19.10-300.fc44.x86_64, SELinux enforcing.
  Load average 13 (noisy, per the bench rules). Co-tenants: a build container, issue556oss-control-plane, two
  buildx builders, kagent-builder, kind-registry and kiln-t10-minio.
- The suite runs in run_hex.sh's privileged container (label=disable). The kernel runs SELinux there, but no
  selinuxfs is mounted in the container.
- `linux_mount_label`: hex applies the label under both policies, which the kernel accepts as each tmpfs's
  `context`. crun passes because it skips the label: it finds no `selinux` in the container's `/proc/mounts`
  (`src/libcrun/utils.c:885-906`, `:954-962`).
- `linux_process_apparmor_profile`: by default hex runs the container without the profile, as crun does
  (`set_apparmor_profile` returns 0 when AppArmor is off, `src/libcrun/utils.c:1111-1128`), and warns.
  Under strict it refuses: `process.apparmorProfile is set ("acme_secure_profile") but AppArmor isn't enabled on
  this host (/sys/module/apparmor/parameters/enabled doesn't read Y), and HEX_OCI_LSM=strict refuses rather than
  run the container without it`.

## Where AppArmor runs: Ubuntu 24.04 guest under KVM (aa_lab.sh)

- The same unit, on fedora. The guest is vmaa_build.sh's: Ubuntu 24.04, linux-image-virtual 6.8.0-31-generic
  (vmlinuz sha256 cee36e26…), AppArmor enabled (Y), and `acme_secure_profile` loaded in complain mode. Every
  test ran as root.
- hex is the release binary built for the guest in ubuntu:24.04 (sha256 1e87f8f1…). crun is 1.27's static release
  (sha256 2a80f801…). Ubuntu's own crun 1.14 refuses the suite's runtime-spec 1.3.0 ("unknown version
  specified"), so it can't be the reference.
- `linux_process_apparmor_profile`: hex applies the profile under both policies.
- `linux_mount_label`: by default hex runs the container unlabelled, as crun does, and warns. Under strict it
  refuses: `linux.mountLabel is set ("system_u:object_r:svirt_sandbox_file_t:s0:c715,c811") but SELinux isn't
  active here (no module claims a filesystem's seclabel option), and HEX_OCI_LSM=strict refuses rather than run
  the container without it`.

## The warning

A label skipped by default is never silent (CLAUDE.md §3). As oci_lsm's fedora run printed it:
`hex: warning: process.apparmorProfile ("hex-test-profile") is skipped: AppArmor isn't enabled on this host
(/sys/module/apparmor/parameters/enabled doesn't read Y). The container runs without it, as crun runs one;
HEX_OCI_LSM=strict refuses such a container instead`. The guest's run printed the same for `linux.mountLabel`.
runtime-tools keeps a runtime's stderr only when it fails, so its logs don't show the warning.

## Before this bar

Until 2026-11-06 hex refused a label under every policy: hex 27 and crun 28 on fedora
(hex-lsm-se2-20261105T051043Z), hex 29 and crun 30 in the guest (hex-lsm-aa3-20261105T050217Z).

## Proof that each label is applied (crates/hex-cli/tests/oci_lsm.rs)

runtimetest's own checks for these two tests are close to empty (research §4), so Hex's own test reads each
label back from inside the container:
- SELinux, fedora, rootless: `stat -c %C` on `/tmp`, `/dev` and a masked directory each read the spec's label
  `system_u:object_r:container_file_t:s0:c715,c811`. The AppArmor test there checks the container runs without
  the profile by default, and that strict refuses it by name.
- AppArmor, the guest, as root: `/proc/1/attr/apparmor/current` of the container's first process names
  `acme_secure_profile`. The mount-label test there checks the container runs unlabelled by default, and that
  strict refuses it by name.
- Each run also creates an unlabelled control container first, so a failure is the label's or the profile's.

## What the guest found, fixed in this tree

- Before Linux 6.15, `open_tree(OPEN_TREE_CLONE)` refuses a detached mount (v6.8 `fs/namespace.c:2588`), so
  the node's pooled empty masking tmpfs failed every masked directory on 6.8. `ProcSources` now probes the
  clone once and, where the kernel refuses, gives each masked directory its own tmpfs.
- The AppArmor enabled check had read `/sys` in the container's init, where `/sys` may not be mounted. It now
  asks the kernel through `/proc` (the scoped `current` attribute reads EINVAL where the module is off).
