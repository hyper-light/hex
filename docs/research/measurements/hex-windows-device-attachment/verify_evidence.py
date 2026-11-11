#!/usr/bin/env python3
"""Read back this lane's exact raw bytes/source identities; does not run builds or tests."""
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
import tarfile

base = Path(__file__).resolve().parent
proof = json.loads((base / 'verification.json').read_text())
index = json.loads((base / 'raw-index.json').read_text())
blob = (base / index['archive']).read_bytes()
assert hashlib.sha256(blob).hexdigest() == index['archive_sha256']
raw = {}
with tarfile.open(fileobj=io.BytesIO(blob), mode='r:gz') as bundle:
    for member in bundle.getmembers():
        assert member.isfile() and '/' not in member.name and '\\' not in member.name
        assert member.name in index['members'] and member.name not in raw
        data = bundle.extractfile(member).read()
        assert index['members'][member.name] == {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
        raw[member.name] = data
assert set(raw) == set(index['members'])
for path, sha in proof['source_sha256'].items():
    assert hashlib.sha256(Path(path).read_bytes()).hexdigest() == sha
for prefix in ['native-v9', 'light-v10']:
    result = json.loads(raw[prefix + '-result.json'].decode('utf-8-sig'))
    assert result['source_revision'] == proof['tested_source'] and result['exit_code'] == 0
    assert result['source_hashes'] == proof['source_sha256']
    assert all(phase['exit_code'] == 0 and phase['child_reaped'] for phase in result['phases'])
    assert result['remaining_temp_entries'] == 0
native = json.loads(raw['native-v9-result.json'].decode('utf-8-sig'))
assert native['phases'][0]['summaries'][0].startswith('test result: ok. 19 passed; 0 failed; 0 ignored;')
assert len(native['phases'][0]['selected_lines']) == 18
assert all(line.endswith(' ... ok') for line in native['phases'][0]['selected_lines'])
red = json.loads(raw['reclaim-red-v3-result.json'].decode('utf-8-sig'))
assert '0 passed; 1 failed; 0 ignored;' in red['phases'][0]['summaries'][0]
core = json.loads(raw['light-v5-result.json'])
assert core['phases'][0]['summaries'][0].startswith('test result: ok. 50 passed; 0 failed; 0 ignored;')
assert core['phases'][1]['exit_code'] == 0
assert not subprocess.check_output(['git', 'diff', '--name-only', core['source_revision'], proof['tested_source'], '--', 'crates/hex-core', 'crates/hex-api', 'Cargo.toml', 'Cargo.lock', 'vendor'])
print(json.dumps({'tested_source': proof['tested_source'], 'raw_members_verified': len(raw), 'raw_archive_sha256': index['archive_sha256'], 'HCS_data_units': '50/0/0ignored, separately at unchanged core source57', 'Windows_selected_units': '19/0/0ignored,18 assigned+1 incidental Catalog', 'source_hashes_match': True, 'final_compile_clippy_exit_codes': 0, 'owned_active_jobs': []}, indent=2))
