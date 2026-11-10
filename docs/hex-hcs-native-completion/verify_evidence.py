#!/usr/bin/env python3
"""Read retained evidence bytes and identities; never build, invoke HCS, or execute a test."""
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile

base = Path(__file__).resolve().parent
index = json.loads((base / 'raw-index.json').read_text())
blob = (base / 'raw-evidence.tar.gz').read_bytes()
assert hashlib.sha256(blob).hexdigest() == index['archive_sha256']
raw = {}
with tarfile.open(fileobj=io.BytesIO(blob), mode='r:gz') as bundle:
    for member in bundle.getmembers():
        name = PurePosixPath(member.name)
        assert member.isfile() and len(name.parts) == 1 and name.name in index['files']
        assert name.name not in raw
        data = bundle.extractfile(member).read()
        expected = index['files'][name.name]
        assert len(data) == expected['bytes']
        assert hashlib.sha256(data).hexdigest() == expected['sha256']
        raw[name.name] = data
assert set(raw) == set(index['files'])
readback = json.loads((base / 'source-readback.json').read_text())
repo = base.parents[1]
for name, expected in readback['source_sha256'].items():
    assert hashlib.sha256((repo / name).read_bytes()).hexdigest() == expected
registration = b'#[cfg(all(test, windows))]\n#[path = "assigned_native_tests.rs"]\nmod assigned_native_tests;\n\n'
system = (repo / 'crates/hex-runtime/src/windows/oci/sys.rs').read_bytes()
assert system.count(registration) == 1
original = subprocess.check_output(['git', '-C', str(repo), 'show', readback['base'] + ':crates/hex-runtime/src/windows/oci/sys.rs'])
assert system.replace(registration, b'') == original
runs = []
for tag, expected_exit in [('v1', 101), ('v2', 0)]:
    result = json.loads(raw[tag + '-result.json'])
    manifest = json.loads(raw[tag + '-source-hashes.json'])
    transfer = json.loads(raw[tag + '-transfer.json'])
    assert result['source_revision'] == transfer['source_revision']
    assert result['archive_sha256'] == transfer['archive_sha256']
    assert hashlib.sha256(raw[tag + '-source-hashes.json']).hexdigest() == result['source_manifest_sha256']
    assert result['native_invocations'] == result['executed_tests'] == 0
    assert result['remaining_temp_entries'] == 0
    assert len(result['phases']) == 1
    phase = result['phases'][0]
    assert phase['exit_code'] == expected_exit and phase['child_reaped']
    log = raw[tag + '-' + phase['name'] + '.log']
    assert hashlib.sha256(log).hexdigest() == phase['log_sha256']
    assert all(manifest[name] == value for name, value in result['source_hashes'].items())
    if tag == 'v2':
        assert result['source_hashes'] == readback['source_sha256']
    runs.append({'source_revision': result['source_revision'], 'compiler_exit': expected_exit,
                 'cargo_pid': phase['owned_pid'], 'reaped': True, 'log_sha256': phase['log_sha256']})
print(json.dumps({'raw_files': len(raw), 'raw_archive_sha256': index['archive_sha256'],
                  'runs': runs, 'source_matches': True, 'production_unchanged': True,
                  'executed_tests': 0, 'native_invocations': 0}, indent=2))
