#!/usr/bin/env python3
"""SOURCE ONLY: changed measurement copy of frozen exporter83586; not executed.

ROOT command review and a pinned actual quiet-window ACK are mandatory.
No package, compiler, GPU, VM, driver or451 source-mutation API is called.
Timing/counter overhead changes measurement source; no faster algorithm is claimed.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import stat
import subprocess
import sys
import time

from alternate_00aad_io import private_input, read_json, regular_hash, require, verify, write_json

BUNDLE = Path(__file__).resolve().parent
CACHE = Path('/home/adalundhe/.cache/hex-native-firecracker')
ROOT = '01a1aa77778471339bea3beff90d6bbd'
INPUT_SHA256 = '14e35dbed0d79273b963f8f27c1973d53ff997e6a02bd8b99fbe6c00deb91412'
COUNT = 1596
CONTENT_BYTES = 472712455
MAX_META = 16777216
MAX_ARCHIVE = CONTENT_BYTES + MAX_META
CHUNK = 65536
SOURCE_FILES = {'export_held451_objects.py', 'held451-export-inputs.json',
                'held451-export-controls.json', 'alternate_00aad_io.py'}
PROFILE_SAMPLE_LIMIT = COUNT * 2 + 1


def measured_fsync(profile, label, fd):
    started = time.monotonic_ns()
    completed = False
    try:
        os.fsync(fd)
        completed = True
    finally:
        elapsed = time.monotonic_ns() - started
        counter = profile['fsync'][label]
        counter['calls'] += 1
        counter['total_ns'] += elapsed
        if not completed:
            counter['errors'] += 1
        if label == 'source_event':
            samples = profile['source_event_samples_ns']
            if len(samples) < PROFILE_SAMPLE_LIMIT:
                samples.append(elapsed)
            else:
                # Refuse the measurement, without suppressing fsync or its error.
                profile['refusal'] = {'kind': 'SourceEventSamplesExceeded', 'limit': PROFILE_SAMPLE_LIMIT}


def measured_phase(profile, label, function, *args):
    started = time.monotonic_ns()
    try:
        return function(*args)
    finally:
        elapsed = time.monotonic_ns() - started
        counter = profile['phases'][label]
        counter['calls'] += 1
        counter['total_ns'] += elapsed


def finish_profile(profile, cpu_start, started):
    samples = sorted(profile['source_event_samples_ns'])
    profile['source_event_sample_count'] = len(samples)
    profile['status'] = 'refused' if profile['refusal'] is not None else 'observed'
    quantiles = {'p50': None, 'p90': None, 'p99': None, 'max': None}
    if samples and profile['refusal'] is None:
        # Nearest rank: ceil(n*p/100), one-based; no interpolation.
        for percent in [50, 90, 99]:
            rank = (len(samples) * percent + 100 - 1) // 100
            quantiles['p' + str(percent)] = samples[rank - 1]
        quantiles['max'] = samples[-1]
    profile['source_event_quantiles_ns'] = quantiles
    cpu_end = os.times()
    profile['own_os_times_cpu_seconds'] = {
        'start_user': cpu_start.user, 'start_system': cpu_start.system,
        'end_user': cpu_end.user, 'end_system': cpu_end.system,
        'user_delta': cpu_end.user - cpu_start.user,
        'system_delta': cpu_end.system - cpu_start.system}
    profile['worker_interval_ns'] = time.monotonic_ns() - started
    return profile


def tick(deadline):
    require(time.monotonic() < deadline, 'export_or_quiet_window_deadline')


def birth(pid):
    try:
        return Path('/proc', str(pid), 'stat').read_text().rsplit(')', 1)[1].split()[19]
    except FileNotFoundError:
        return None


def host_identity(controls):
    values = {}
    for key, name in [('machine', '/etc/machine-id'), ('boot', '/proc/sys/kernel/random/boot_id')]:
        with Path(name).open('rb') as stream:
            data = stream.read(4097)
        require(0 < len(data) <= 4096, 'host_identity_read_bound')
        values[key + '_sha256'] = hashlib.sha256(data.strip().lower()).hexdigest()
        require(values[key + '_sha256'] == controls[key + '_sha256'], 'reviewed451_host_identity')
    require(platform.system() == 'Linux' and platform.machine() == 'x86_64' and
            platform.release() == controls['kernel'] and os.getuid() == controls['uid'] == 1000,
            'reviewed451_platform_uid')
    return {**values, 'kernel': platform.release(), 'uid': os.getuid()}


def control_mappings(controls, deadline):
    known = {}
    for row in controls['mapped_file_allowlist']:
        for name in [row['source'], row.get('resolved', row['source'])]:
            if name in known:
                require(known[name]['sha256'] == row['sha256'], 'control_alias_conflict')
            known[name] = row
    with Path('/proc/self/maps').open('rb') as stream:
        data = stream.read(1048577)
    require(len(data) <= 1048576, 'control_mapping_budget')
    seen, result = set(), []
    for line in data.decode().splitlines():
        fields = line.split(None, 5)
        if len(fields) < 6 or not fields[5].startswith('/') or fields[5] in seen:
            continue
        tick(deadline)
        name = fields[5]
        require(len(seen) < 8192 and name in known, 'unlisted_export_control_mapping:' + name)
        verify(name, known[name])
        seen.add(name)
        result.append({'source': name, 'sha256': known[name]['sha256']})
    modules = []
    require(len(sys.modules) <= 4096, 'loaded_python_module_budget')
    for module_name, module in list(sys.modules.items()):
        for attribute in ['__file__', '__cached__']:
            name = getattr(module, attribute, None)
            if not isinstance(name, str) or not name.startswith('/') or not Path(name).exists() or name in seen:
                continue
            tick(deadline)
            if Path(name).parent == BUNDLE and Path(name).name in SOURCE_FILES:
                continue  # Already checked by the complete source seal.
            require(name in known, 'unlisted_python_control:' + name)
            verify(name, known[name])
            seen.add(name)
            modules.append({'module': module_name, 'attribute': attribute, 'source': name,
                            'sha256': known[name]['sha256']})
    return {'mapped_files': result, 'python_files': modules}


def owned_scope(admission):
    names = [line[3:] for line in Path('/proc/self/cgroup').read_text().splitlines() if line.startswith('0::')]
    require(len(names) == 1, 'unified_scope')
    path = Path('/sys/fs/cgroup') / names[0].lstrip('/')
    require(admission['unit'] + '.scope' in path.parts, 'exact_owned_export_scope')
    limits = {key: (path / key).read_text().strip()
              for key in ['cpu.max', 'memory.max', 'memory.swap.max', 'pids.max']}
    quota, period = limits['cpu.max'].split()
    require(quota != 'max' and 0 < int(quota) <= int(period) and limits['memory.max'] == '4294967296' and
            limits['memory.swap.max'] == '0' and limits['pids.max'] == '256' and
            os.getpriority(os.PRIO_PROCESS, 0) >= 10, 'all_export_controller_limits')
    return path, limits


def source_stat(state):
    # atime may be maintained by the filesystem on read; no no-atime privilege,
    # mount, ownership or source permission change is attempted.
    return {key: getattr(state, key) for key in ['st_dev', 'st_ino', 'st_size', 'st_mode',
                                                'st_uid', 'st_gid', 'st_mtime_ns', 'st_ctime_ns']}


def manifest_bytes(inputs):
    value = {'schema': 'hex-native-fc-imported-objects-v1', 'plan_sha256': inputs['source_plan_sha256'],
             'objects': [{'sha256': row['sha256'], 'bytes': row['bytes'],
                          'filename': row['archive_member'],
                          'provenance': {'kind': '451-original-byte-export-HELD',
                                         'original_source': row['source']}}
                         for row in inputs['objects']]}
    data = (json.dumps(value, sort_keys=True, indent=2, allow_nan=False) + '\n').encode()
    require(len(data) <= MAX_META, 'import_manifest_byte_budget')
    return data


def tar_header(name, size):
    name_bytes = name.encode('ascii')
    require(0 < len(name_bytes) <= 100 and 0 <= size <= 536870912, 'ustar_name_or_size')
    header = bytearray(512)
    header[:len(name_bytes)] = name_bytes
    for offset, length, value in [(100, 8, 0o444), (108, 8, 1000), (116, 8, 1000),
                                  (124, 12, size), (136, 12, 0)]:
        field = ('%0*o' % (length - 1, value)).encode() + b'\0'
        require(len(field) == length, 'ustar_integer_width')
        header[offset:offset + length] = field
    header[148:156] = b'        '
    header[156:157] = b'0'
    header[257:263] = b'ustar\0'
    header[263:265] = b'00'
    header[265:275] = b'hex-native'
    header[297:307] = b'hex-native'
    checksum = sum(header)
    header[148:156] = ('%06o' % checksum).encode() + b'\0 '
    return header


def export_objects(work, admission, inputs, deadline, profile):
    partial = work / 'output/held1596.tar.partial'
    archive = work / 'output/held1596.tar'
    manifest = manifest_bytes(inputs)
    expected_archive = sum(512 + row['bytes'] + (-row['bytes'] % 512) for row in inputs['objects'])
    expected_archive += 512 + len(manifest) + (-len(manifest) % 512) + 1024
    require(expected_archive <= MAX_ARCHIVE, 'archive_byte_budget')
    fd = os.open(partial, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_CLOEXEC | os.O_NOFOLLOW, 0o600)
    digest, written, verified = hashlib.sha256(), 0, 0
    events = work / 'raw/source-records.jsonl'
    event_bytes = 0
    with os.fdopen(fd, 'wb') as output, events.open('xb') as log:
        def emit(data):
            nonlocal written
            tick(deadline)
            written += len(data)
            require(written <= expected_archive, 'archive_grew')
            output.write(data)
            digest.update(data)

        def record_event(value):
            nonlocal event_bytes
            data = (json.dumps(value, sort_keys=True, allow_nan=False) + '\n').encode()
            event_bytes += len(data)
            require(event_bytes <= MAX_META, 'source_receipt_byte_budget')
            log.write(data)
            log.flush()
            measured_fsync(profile, 'source_event', log.fileno())

        for row in inputs['objects']:
            tick(deadline)
            original = Path(row['source'])
            event = {'index': verified, **row, 'state': 'source_started', 'archive_offset': written}
            object_digest, left = hashlib.sha256(), row['bytes']
            record_event(event)
            try:
                link_before = source_stat(original.lstat())
                event['source_path_before'] = link_before
                event['observed_resolution'] = str(original.resolve(strict=True))
                require(event['observed_resolution'] == row['expected_resolved'], 'original_source_resolution_changed')
                source_fd = os.open(row['expected_resolved'], os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
                with os.fdopen(source_fd, 'rb') as source:
                    before = source_stat(os.fstat(source.fileno()))
                    event['source_before'] = before
                    require(stat.S_ISREG(before['st_mode']) and before['st_size'] == row['bytes'], 'original_regular_extent')
                    emit(tar_header(row['archive_member'], row['bytes']))
                    while left:
                        tick(deadline)
                        data = source.read(min(CHUNK, left))
                        require(bool(data), 'original_source_short')
                        left -= len(data)
                        object_digest.update(data)
                        emit(data)
                    require(not source.read(1), 'original_source_grew')
                    after = source_stat(os.fstat(source.fileno()))
                    event['source_after'] = after
                require(before == after and link_before == source_stat(original.lstat()) and
                        str(original.resolve(strict=True)) == row['expected_resolved'], 'original_source_changed_during_read')
                require(object_digest.hexdigest() == row['sha256'], 'original_source_hash_mismatch:' + row['sha256'])
            except (OSError, ValueError) as error:
                record_event({**event, 'state': 'source_failed', 'actual_sha256': object_digest.hexdigest(),
                              'source_bytes_read': row['bytes'] - left, 'archive_bytes_written': written,
                              'cause': str(error)})
                raise
            emit(b'\0' * (-row['bytes'] % 512))
            record_event({**event, 'state': 'source_verified', 'actual_sha256': object_digest.hexdigest(),
                          'source_bytes_read': row['bytes'], 'verified': True})
            verified += 1
        require(verified == COUNT, 'complete_exact_export_set')
        emit(tar_header('manifest.json', len(manifest)))
        emit(manifest)
        emit(b'\0' * (-len(manifest) % 512))
        emit(b'\0' * 1024)
        require(written == expected_archive, 'exact_archive_extent')
        output.flush()
        os.fchmod(output.fileno(), 0o444)
        measured_fsync(profile, 'archive', output.fileno())
    archive_pin = regular_hash(partial)
    require(archive_pin['bytes'] == written and archive_pin['sha256'] == digest.hexdigest(), 'archive_final_hash')
    os.link(partial, archive, follow_symlinks=False)
    os.unlink(partial)  # Only the exact successfully published owned partial.
    with (work / 'output/manifest.json').open('xb') as stream:
        stream.write(manifest)
        stream.flush()
        measured_fsync(profile, 'manifest', stream.fileno())
    directory_fd = os.open(work / 'output', os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC)
    try:
        measured_fsync(profile, 'output_directory', directory_fd)
    finally:
        os.close(directory_fd)
    receipt = {'schema': 'hex-native-fc-held451-export-receipt-v1', 'state': 'complete',
               'object_count': verified, 'object_bytes': CONTENT_BYTES, 'source_input_sha256': INPUT_SHA256,
               'archive': {**archive_pin, 'source': str(archive)},
               'import_manifest': regular_hash(work / 'output/manifest.json'),
               'source_receipts': regular_hash(events), 'quiet_window_receipt': admission['quiet_window_receipt'],
               'operator_not_after_unix_ns': admission['operator_not_after_unix_ns'],
               'execution_kind': 'readonly object acquisition; no executable payload dispatch',
               'meaning': 'byte acquisition only; launcher-final exit/absence required separately'}
    write_json(work / 'output/export-receipt.json', receipt)
    return receipt


def collect_archive(work, admission, deadline):
    source_root = Path(admission['source_export_root'])
    require(source_root.parent == CACHE and re.fullmatch(r'held451-export-[a-z0-9-]{1,60}', source_root.name) and
            source_root != work and not source_root.is_symlink(), 'exact_prior_owned_export_root')
    for filename, key in [('launcher-final.json', 'export_launcher_pin'),
                          ('output/export-receipt.json', 'export_receipt_pin')]:
        verify(private_input(source_root / filename), admission[key])
    launcher = read_json(source_root / 'launcher-final.json')
    receipt = read_json(source_root / 'output/export-receipt.json')
    require(launcher['stage'] == 'export' and launcher['exit_code'] == 0 and
            launcher['child_reaped'] is True and launcher['child_owned_birth_absent'] is True and
            not launcher['owned_scope_members'] and not launcher['worker_observation_errors'], 'successful_reaped_export_required')
    require(receipt['schema'] == 'hex-native-fc-held451-export-receipt-v1' and receipt['state'] == 'complete' and
            receipt['object_count'] == COUNT and receipt['object_bytes'] == CONTENT_BYTES and
            receipt['source_input_sha256'] == INPUT_SHA256, 'exact_export_receipt')
    archive = private_input(source_root / 'output/held1596.tar')
    require(receipt['archive']['source'] == str(archive), 'fixed_archive_path')
    verify(archive, receipt['archive'])
    digest, copied = hashlib.sha256(), 0
    with archive.open('rb') as source:
        while copied < receipt['archive']['bytes']:
            tick(deadline)
            data = source.read(min(CHUNK, receipt['archive']['bytes'] - copied))
            require(bool(data), 'archive_stream_short')
            sys.stdout.buffer.write(data)
            copied += len(data)
            digest.update(data)
        require(not source.read(1), 'archive_stream_grew')
    sys.stdout.buffer.flush()
    require(digest.hexdigest() == receipt['archive']['sha256'], 'archive_stream_hash_mismatch')
    result = {'bytes': copied, 'sha256': digest.hexdigest(), 'prior_export_receipt': admission['export_receipt_pin'],
              'quiet_window_receipt': admission['quiet_window_receipt'], 'original_source_reads': 0,
              'meaning': 'streamed own frozen archive only; receiver must preserve partials on nonzero exit'}
    write_json(work / 'output/collection-receipt.json', result)
    return result


def worker(work, admission, stage, inputs, controls):
    left = (admission['operator_not_after_unix_ns'] - time.time_ns()) / 1e9
    deadline = time.monotonic() + min(175, left - 2)
    tick(deadline)
    profile_started = time.monotonic_ns()
    cpu_start = os.times()
    profile = {'schema': 'hex-native-fc-fsync-measurement-v1',
               'base_source_sha256': '83586fc225fbe7209eb250a6c8d21b8fb70dff97f82dd7dccb1f4f47bde00ad3',
               'source_event_sample_limit': PROFILE_SAMPLE_LIMIT, 'source_event_samples_ns': [],
               'refusal': None,
               'fsync': {label: {'calls': 0, 'errors': 0, 'total_ns': 0}
                         for label in ['source_event', 'archive', 'manifest', 'output_directory']},
               'phases': {label: {'calls': 0, 'total_ns': 0}
                          for label in ['control_mappings_before', 'operation', 'control_mappings_after']}}
    cg, limits = owned_scope(admission)
    write_json(work / 'scope.json', {'path': str(cg), 'limits': limits, 'PID': os.getpid(),
               'birth': birth(os.getpid()), 'nice': os.getpriority(os.PRIO_PROCESS, 0)})
    before = host_identity(controls)
    result = {'stage': stage, 'scope_path': str(cg), 'observation_errors': []}
    try:
        write_json(work / 'raw/control-mappings-before.json',
                   measured_phase(profile, 'control_mappings_before', control_mappings, controls, deadline))
        operation = export_objects if stage == 'export' else collect_archive
        receipt = (measured_phase(profile, 'operation', operation, work, admission, inputs, deadline, profile)
                   if stage == 'export' else measured_phase(profile, 'operation', operation, work, admission, deadline))
        result['operation_receipt'] = receipt
        require(host_identity(controls) == before, '451_host_identity_changed')
        write_json(work / 'raw/control-mappings-after.json',
                   measured_phase(profile, 'control_mappings_after', control_mappings, controls, deadline))
    finally:
        for name, observe in [('scope_members', lambda: (cg / 'cgroup.procs').read_text().split()),
                              ('host', lambda: host_identity(controls))]:
            try:
                result[name] = observe()
            except (OSError, ValueError) as error:
                result['observation_errors'].append({'observation': name, 'cause': str(error)})
        result['measurement_profile'] = finish_profile(profile, cpu_start, profile_started)
        write_json(work / 'worker-final.json', result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--stage', choices=['export', 'collect'], required=True)
    parser.add_argument('--admission', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--seal', type=Path, default=BUNDLE / 'held451-export.seal.json')
    parser.add_argument('--inside-owned-scope', action='store_true')
    args = parser.parse_args()
    admission = read_json(args.admission)
    require(admission['schema'] == 'hex-native-fc-held451-readonly-admission-v1' and
            admission['reviewed'] is True and admission['reviewed_by'] == ROOT and
            admission['stage'] == args.stage and admission['runner'] == '451397fcc585f7f7cb9aaf00e42064bb',
            'ROOT_command_source_and_stage_review_required')
    require(admission['quiet_window_reviewed'] is True and admission['quiet_window_receipt'] is not None,
            'actual_peer_quiet_ACK_required')
    pin = admission['quiet_window_receipt']
    require(str(pin['source']).startswith(str(CACHE) + '/'), 'private_quiet_receipt_path')
    verify(private_input(pin['source']), pin)
    require(type(admission['operator_not_after_unix_ns']) is int and
            0 < admission['operator_not_after_unix_ns'] - time.time_ns() <= 600000000000,
            'fresh_bounded_operator_window_deadline')
    for key in ['CPU_percent', 'memory_bytes', 'disk_bytes', 'swap_bytes', 'pids', 'nice',
                'jobs', 'test_threads', 'scope_seconds']:
        require(type(admission[key]) is int, 'integer_export_admission_field:' + key)
    require(admission['needs'] == 'linux/amd64,cpus=1,mem=4g,disk=12g' and
            admission['CPU_percent'] == 100 and admission['memory_bytes'] == 4294967296 and
            admission['disk_bytes'] == 12884901888 and admission['swap_bytes'] == 0 and
            admission['pids'] == 256 and admission['nice'] == 10 and admission['jobs'] == 1 and
            admission['test_threads'] == 1 and admission['scope_seconds'] == 180,
            'exact_readonly_export_envelope')
    require(regular_hash(args.seal)['sha256'] == admission['source_seal_sha256'], 'reviewed_export_source_seal')
    seal = read_json(args.seal)
    require(seal['schema'] == 'hex-native-fc-held451-source-seal-v1' and len(seal['files']) == len(SOURCE_FILES) and
            {row['bundle'] for row in seal['files']} == SOURCE_FILES, 'complete_export_source_set')
    for row in seal['files']:
        verify(BUNDLE / row['bundle'], row)
    input_path = BUNDLE / 'held451-export-inputs.json'
    require(regular_hash(input_path)['sha256'] == INPUT_SHA256, 'exact_held_object_manifest')
    inputs = read_json(input_path)
    require(inputs['schema'] == 'hex-native-fc-held451-export-inputs-v1' and len(inputs['objects']) == COUNT and
            sum(row['bytes'] for row in inputs['objects']) == CONTENT_BYTES and
            len({row['sha256'] for row in inputs['objects']}) == COUNT, 'exact_1596_export_set')
    for row in inputs['objects']:
        require(re.fullmatch(r'[0-9a-f]{64}', row['sha256']) and type(row['bytes']) is int and
                0 <= row['bytes'] <= 536870912 and row['archive_member'] == 'objects/' + row['sha256'] and
                row['archive_mode'] == 0o444 and Path(row['source']).is_absolute() and
                Path(row['expected_resolved']).is_absolute(), 'canonical_export_row')
    controls = read_json(BUNDLE / 'held451-export-controls.json')
    host_identity(controls)
    work = args.root.absolute()
    require(str(work) == admission['root'] and work.parent.resolve() == CACHE.resolve() and
            re.fullmatch(r'held451-(?:export|collect)-[a-z0-9-]{1,60}', work.name) and
            re.fullmatch(r'hex-native-fc-held451-[a-z0-9-]{1,60}', admission['unit']), 'fresh_owned_export_or_collect_root')
    if args.inside_owned_scope:
        worker(work, admission, args.stage, inputs, controls)
        return 0
    for row in controls['launcher'].values():
        verify(row['source'], row)
    require(not work.exists(), 'never_reuse_live_or_frozen_output')
    work.mkdir(mode=0o700)
    (work / 'raw').mkdir()
    (work / 'output').mkdir()
    write_json(work / 'admission.json', admission)
    available = os.statvfs(work)
    require(available.f_bavail * available.f_frsize >= admission['disk_bytes'], 'normal_disk_headroom')
    argv = ['/usr/bin/timeout', '--signal=TERM', '--kill-after=3s', '200s',
            '/usr/bin/systemd-run', '--user', '--scope', '--quiet', '--unit=' + admission['unit'],
            '--property=CPUQuota=100%', '--property=MemoryMax=4G', '--property=MemorySwapMax=0',
            '--property=TasksMax=256', '--property=RuntimeMaxSec=180s', '/usr/bin/nice', '-n', '10',
            '/usr/bin/python3', '-B', str(Path(__file__).resolve()), '--stage', args.stage,
            '--admission', str(args.admission.resolve()), '--root', str(work), '--seal', str(args.seal.resolve()),
            '--inside-owned-scope']
    write_json(work / 'launch.json', {'argv': argv, 'stage': args.stage, 'caller_seconds': 210,
               'admission_pin': regular_hash(args.admission), 'source_seal_pin': regular_hash(args.seal)})
    env = {'PATH': '/usr/bin', 'HOME': str(Path.home()), 'LC_ALL': 'C', 'LANG': 'C', 'PYTHONDONTWRITEBYTECODE': '1',
           'CARGO_BUILD_JOBS': '1', 'RUST_TEST_THREADS': '1', 'MAKEFLAGS': '-j1', 'OMP_NUM_THREADS': '1'}
    for name in ['XDG_RUNTIME_DIR', 'DBUS_SESSION_BUS_ADDRESS']:
        if name in os.environ:
            env[name] = os.environ[name]
    with (work / 'raw/worker.log').open('xb') as log:
        proc = subprocess.Popen(argv, env=env, stdin=subprocess.DEVNULL,
                                stdout=log if args.stage == 'export' else sys.stdout.buffer, stderr=log)
        identity = {'PID': proc.pid, 'birth': birth(proc.pid)}
        write_json(work / 'launcher-child.json', identity)
        code = proc.wait(timeout=205)
    result = {'stage': args.stage, 'exit_code': code, 'worker_log': regular_hash(work / 'raw/worker.log'),
              'child_reaped': True, 'child_owned_birth_absent': identity['birth'] is not None and birth(proc.pid) != identity['birth'],
              'child': identity, 'admission_pin': regular_hash(args.admission),
              'source_input_sha256': INPUT_SHA256, 'worker_observation_errors': [], 'owned_scope_members': None}
    final = work / 'worker-final.json'
    if final.is_file():
        receipt = read_json(final)
        cg = Path(receipt['scope_path'])
        require(str(cg).startswith('/sys/fs/cgroup/') and admission['unit'] + '.scope' in cg.parts, 'exact_cleanup_scope')
        result['owned_scope_path_absent'] = not cg.exists()
        result['owned_scope_members'] = (cg / 'cgroup.procs').read_text().split() if cg.exists() else []
        result['worker_observation_errors'] = receipt['observation_errors']
        if 'operation_receipt' in receipt:
            result['operation_receipt'] = receipt['operation_receipt']
            result['operation_receipt_pin'] = regular_hash(work / 'output' /
                ('export-receipt.json' if args.stage == 'export' else 'collection-receipt.json'))
            if args.stage == 'export':
                result['export_receipt_pin'] = result['operation_receipt_pin']
        if result['owned_scope_members'] or receipt['observation_errors'] or not result['child_owned_birth_absent']:
            code = 1
    else:
        result['cleanup'] = 'unverified: no worker final receipt; preserve private outputs/exact unit'
        code = 1
    result['exit_code'] = code
    write_json(work / 'launcher-final.json', result)
    presentation = {**result, 'launcher_final_pin': regular_hash(work / 'launcher-final.json')}
    print(json.dumps(presentation, sort_keys=True), file=sys.stdout if args.stage == 'export' else sys.stderr)
    return code


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        print(json.dumps({'error': 'readonly451_export_or_collection_failed', 'cause': str(error),
                          'partials': 'retain all private partial/archive/raw; no fallback',
                          'GPU_VM_build': False}), file=sys.stderr)
        raise SystemExit(1)
