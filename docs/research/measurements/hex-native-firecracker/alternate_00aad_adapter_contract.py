"""SOURCE-ONLY boundary contract for ROOT's reviewed 4a -> 00aad adapter.

This is a plan input, not a build entrypoint. Nothing runs at import. ROOT must
join these boundaries into a separately sealed driver before any build. The
original 4a recipe/cohort stay unchanged. Namespace execution is UNRUN.
"""
import hashlib
import os
from pathlib import Path, PurePosixPath
import platform
import re

MACHINE_SHA256 = '01685e36c9b6fe2c3c404901afcda9424fa6584dd26fa2538fc6c132ed6cbc83'
BOOT_SHA256 = 'ae706cbc121308f52559c8c10e39cb341150db40f67122683f33899bcdd37a71'
CACHE = Path('/home/adalundhe/.cache/hex-native-firecracker')


def require(condition, why):
    if not condition:
        raise ValueError(why)


def observe_build_host():
    """Read build-host identity only; never a GPU/native-provider observation."""
    fields = {}
    for key, name, expected in [('machine', '/etc/machine-id', MACHINE_SHA256),
                                ('boot', '/proc/sys/kernel/random/boot_id', BOOT_SHA256)]:
        with Path(name).open('rb') as stream:
            data = stream.read(4097)
        require(0 < len(data) <= 4096, 'bounded_build_identity_read')
        value = hashlib.sha256(data.strip().lower()).hexdigest()
        require(value == expected, 'reviewed_build_' + key + '_mismatch')
        fields[key + '_sha256'] = value
    fields.update(kernel=platform.release(), arch=platform.machine(), uid=os.getuid(),
                  role='artifact-only build host; no selected GPU observation')
    require(fields['kernel'] == '7.3.3-1-cachyos' and fields['arch'] == 'x86_64' and
            fields['uid'] == 1000, 'reviewed_build_host_changed')
    return fields


def sdk_member(source):
    """Exact original FHS name for a 4a /usr byte pin, without host lookup."""
    path = PurePosixPath(source)
    require(str(path) == source and path.is_absolute() and '..' not in path.parts,
            'canonical_original_source')
    require(source.startswith('/usr/'), 'sdk_usr_source_only')
    return str(path).lstrip('/')


def object_source(objects, row):
    """Original source paths remain provenance labels, never host fallbacks."""
    digest = row['sha256']
    require(re.fullmatch(r'[0-9a-f]{64}', digest) is not None, 'object_hash')
    return Path(objects) / digest


def compiler_phase_argv(work, sdk, command):
    """Keep 4a /usr paths and flags; expose only the private SDK/work to tools.

    The outer host control driver records the real bwrap PID/birth, waits and
    checks its exact owned cgroup after each phase. PID1 inside this namespace
    is bwrap's reaper, not Hex or any VM/guest. No host /usr or GPU node is bound.
    """
    work, sdk = Path(work), Path(sdk)
    require(work.parent == CACHE and
            re.fullmatch(r'amd-root-build-alt00aa-[a-z0-9-]{1,40}', work.name), 'owned_work_root')
    require(sdk == work / 'input/private-sdk', 'owned_private_sdk')
    require(command and command[0] in ['/usr/bin/gcc', '/usr/bin/make', '/usr/bin/readelf'],
            'reviewed_compiler_phase_entrypoint')
    require(len(command) <= 128, 'phase_argument_count')
    env = {'PATH': str(work / 'tools/bin') + ':/usr/bin', 'HOME': str(work / 'home'),
           'TMPDIR': str(work / 'temp'), 'XDG_CACHE_HOME': str(work / 'cache'),
           'LC_ALL': 'C', 'LANG': 'C', 'PYTHONDONTWRITEBYTECODE': '1',
           'SOURCE_DATE_EPOCH': '0', 'KBUILD_BUILD_TIMESTAMP': '1970-01-01 00:00:00 UTC',
           'KBUILD_BUILD_USER': 'hex-native-fc', 'KBUILD_BUILD_HOST': 'private-preparation',
           'KBUILD_BUILD_VERSION': '1', 'CARGO_BUILD_JOBS': '1', 'RUST_TEST_THREADS': '1'}
    args = ['/usr/bin/bwrap', '--unshare-user', '--unshare-pid', '--unshare-net',
            '--unshare-ipc', '--unshare-uts', '--die-with-parent', '--new-session',
            '--cap-drop', 'ALL', '--clearenv', '--ro-bind', str(sdk / 'usr'), '/usr',
            '--symlink', 'usr/bin', '/bin', '--symlink', 'usr/bin', '/sbin',
            '--symlink', 'usr/lib', '/lib', '--symlink', 'usr/lib', '/lib64',
            '--proc', '/proc', '--dev', '/dev', '--bind', str(work), str(work),
            '--ro-bind', str(work / 'input'), str(work / 'input'),
            '--ro-bind', str(work / 'source'), str(work / 'source'), '--chdir', str(work)]
    for name, value in sorted(env.items()):
        args += ['--setenv', name, value]
    return args + ['--', *command]
