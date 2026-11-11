#!/usr/bin/env python3
"""SOURCE ONLY: normal Kiln collection into a fresh private caller cache.

This transports and hashes data, never installs/loads/extracts executable SDK
objects. It is not a laptop build/test or a native/GPU validation result.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import selectors
import shlex
import signal
import subprocess
import sys
import tarfile
import time

from alternate_00aad_io import read_json, regular_hash, require, write_json

CACHE = Path('/Users/adalundhe/.cache/hex-native-firecracker')
REMOTE_CACHE = '/home/adalundhe/.cache/hex-native-firecracker'
MAX_ARCHIVE = 472712455 + 16777216
ROOT = '01a1aa77778471339bea3beff90d6bbd'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reviewed-collect-admission', type=Path, required=True)
    parser.add_argument('--export-result', type=Path, required=True)
    parser.add_argument('--root', type=Path, required=True)
    args = parser.parse_args()
    admission = read_json(args.reviewed_collect_admission)
    require(admission['reviewed'] is True and admission['reviewed_by'] == ROOT and admission['stage'] == 'collect' and
            admission['quiet_window_reviewed'] is True and admission['quiet_window_receipt'] is not None,
            'ROOT_collect_command_and_quiet_ACK_required')
    exported = read_json(args.export_result)
    require(exported['stage'] == 'export' and exported['exit_code'] == 0 and
            exported['child_reaped'] is True and exported['child_owned_birth_absent'] is True and
            not exported['owned_scope_members'] and not exported['worker_observation_errors'],
            'actual_successful_export_result')
    receipt = exported['operation_receipt']
    pin = receipt['archive']
    require(receipt['schema'] == 'hex-native-fc-held451-export-receipt-v1' and receipt['state'] == 'complete' and
            receipt['source_input_sha256'] == '14e35dbed0d79273b963f8f27c1973d53ff997e6a02bd8b99fbe6c00deb91412' and
            receipt['object_count'] == 1596 and receipt['object_bytes'] == 472712455 and
            0 < pin['bytes'] <= MAX_ARCHIVE and re.fullmatch(r'[0-9a-f]{64}', pin['sha256']), 'exact_archive_pin')
    require(admission['export_receipt_pin'] == exported['export_receipt_pin'] and
            admission['export_launcher_pin'] == exported['launcher_final_pin'], 'actual_export_receipt_pins')
    work = args.root.absolute()
    require(work.parent.resolve() == CACHE.resolve() and
            re.fullmatch(r'held451-acquired-[a-z0-9-]{1,60}', work.name) and not work.exists(),
            'fresh_private_caller_cache')
    work.mkdir(mode=0o700)
    free = os.statvfs(work)
    require(free.f_bavail * free.f_frsize >= 30 * 1073741824 + MAX_ARCHIVE, 'caller_free_space_floor')
    remote_bundle = REMOTE_CACHE + '/held451-collect-bundle-2359-v1'
    remote_admission = admission['remote_admission_path']
    require(remote_admission == remote_bundle + '/reviewed.collect.json', 'exact_remote_collect_admission')
    remote = ['/usr/bin/timeout', '--signal=TERM', '--kill-after=3s', '200s', '/usr/bin/nice', '-n', '10',
              '/usr/bin/python3', '-B', remote_bundle + '/export_held451_objects.py', '--stage', 'collect',
              '--admission', remote_admission, '--root', admission['root'],
              '--seal', remote_bundle + '/held451-export.seal.json']
    command = ' '.join(shlex.quote(value) for value in remote)
    argv = ['/Users/adalundhe/.local/bin/kiln', 'run', '--quiet', '--on', '451397fcc585f7f7cb9aaf00e42064bb',
            '--needs', 'linux/amd64,cpus=1,mem=4g,disk=12g', '--no-sync', '--timeout', '210', '--', command]
    write_json(work / 'command.json', {'argv': argv, 'archive_expected': pin, 'reviewed_admission': admission,
               'export_result_pin': regular_hash(args.export_result), 'caller_guard_seconds': 230,
               'local_interpreter': regular_hash(sys.executable), 'kiln_executable': regular_hash(argv[0]),
               'required_external_tool_guard_seconds': 240})
    partial = work / 'held1596.tar.partial'
    actual, received = hashlib.sha256(), 0
    deadline = time.monotonic() + 230
    proc = None
    with partial.open('xb') as output, (work / 'kiln.raw.log').open('xb') as log:
        try:
            proc = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                    stderr=log, start_new_session=True)
            write_json(work / 'child.json', {'PID': proc.pid, 'process_group': proc.pid,
                       'ownership': 'exact unreaped Popen child; never selected by command line'})
            require(proc.stdout is not None, 'owned_collection_pipe')
            with selectors.DefaultSelector() as ready:
                ready.register(proc.stdout, selectors.EVENT_READ)
                while True:
                    remaining = deadline - time.monotonic()
                    require(remaining > 0, 'caller_collection_deadline')
                    require(ready.select(remaining), 'caller_pipe_deadline')
                    chunk = os.read(proc.stdout.fileno(), min(65536, pin['bytes'] + 1 - received))
                    if not chunk:
                        break
                    received += len(chunk)
                    require(received <= pin['bytes'], 'archive_transport_grew')
                    output.write(chunk)
                    actual.update(chunk)
            proc.stdout.close()
            code = proc.wait(timeout=max(0.1, deadline - time.monotonic()))
            require(code == 0 and received == pin['bytes'] and actual.hexdigest() == pin['sha256'],
                    'collection_exit_extent_or_hash')
            output.flush()
            os.fchmod(output.fileno(), 0o444)
            os.fsync(output.fileno())
        except (OSError, ValueError, subprocess.SubprocessError) as error:
            if proc is not None and proc.returncode is None:
                try:
                    os.killpg(proc.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    proc.wait(timeout=2)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(proc.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    proc.wait(timeout=2)
            write_json(work / 'failure.json', {'cause': str(error), 'received_bytes': received,
                       'received_sha256': actual.hexdigest(), 'private_partial': str(partial),
                       'remote_cleanup': 'unverified on transport failure; require exact remote scope receipt'})
            raise
    archive = work / 'held1596.tar'
    os.link(partial, archive, follow_symlinks=False)
    os.unlink(partial)
    # Only metadata is extracted after the entire canonical archive hash matches.
    # No object/SDK files, modes or owners are installed by this caller.
    with tarfile.open(archive, 'r:') as tar:
        count, data, names = 0, None, set()
        for member in tar:
            count += 1
            require(count <= 1597 and member.isfile() and member.mode == 0o444 and
                    member.name not in names and (member.name == 'manifest.json' or
                    re.fullmatch(r'objects/[0-9a-f]{64}', member.name)), 'export_tar_regular_member_count')
            names.add(member.name)
            if member.name == 'manifest.json':
                require(data is None and 0 < member.size <= 16777216, 'single_bounded_import_manifest')
                selected = tar.extractfile(member)
                require(selected is not None, 'import_manifest_stream')
                data = selected.read(16777217)
                require(len(data) == member.size and hashlib.sha256(data).hexdigest() ==
                        receipt['import_manifest']['sha256'], 'actual_import_manifest_hash')
        require(count == 1597 and data is not None, 'complete_tar_member_count')
    with (work / 'manifest.json').open('xb') as output:
        output.write(data)
        output.flush()
        os.fsync(output.fileno())
    directory_fd = os.open(work, os.O_RDONLY)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)
    result = {'archive': regular_hash(archive), 'import_manifest': regular_hash(work / 'manifest.json'),
              'normal_kiln_exit': 0, 'child_reaped': True, 'objects': 1596, 'object_bytes': 472712455,
              'remote_cleanup': 'see remote collect launcher-final; local success alone is not native cleanup proof',
              'SDK_materialization_or_compile_or_VM_GPU': False}
    write_json(work / 'acquisition-receipt.json', result)
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError, tarfile.TarError) as error:
        print(json.dumps({'error': 'held451_archive_acquisition_failed', 'cause': str(error),
                          'cleanup': 'retain private partial/raw; do not infer remote absence'}), file=sys.stderr)
        raise SystemExit(1)
