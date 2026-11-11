#!/usr/bin/env python3
"""Pack named lane raw evidence without normalizing bytes; retain originals outside Git."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile

base = Path(__file__).resolve().parent
relative = base.relative_to(Path.cwd())
names = subprocess.check_output(['git', 'ls-files', '--others', '--exclude-standard', '--', str(relative)], text=True).splitlines()
prefixes = ('light-v1-', 'light-v2-', 'light-v5-', 'light-v6-', 'light-v8-', 'light-v10-', 'native-v1-', 'native-v2-', 'native-v7-', 'native-v9-', 'reclaim-red-v3-', 'reclaim-green-v4-')
members = [Path(name) for name in names if Path(name).name.startswith(prefixes)]
manifest = {}
buffer = io.BytesIO()
with tarfile.open(fileobj=buffer, mode='w') as bundle:
    for member in sorted(members):
        if member.parent != base and member.resolve().parent != base:
            raise ValueError('raw member escapes the assigned evidence directory')
        if not member.is_file() or member.is_symlink():
            raise ValueError('raw member is not a regular file')
        data = member.read_bytes()
        entry = tarfile.TarInfo(member.name)
        entry.size = len(data)
        bundle.addfile(entry, io.BytesIO(data))
        manifest[member.name] = {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
blob = gzip.compress(buffer.getvalue(), mtime=0)
with tarfile.open(fileobj=io.BytesIO(blob), mode='r:gz') as bundle:
    for member in bundle.getmembers():
        data = bundle.extractfile(member).read()
        assert manifest[member.name] == {'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest()}
archive = base / 'raw-evidence.tar.gz'
with archive.open('xb') as out:
    out.write(blob)
proof = {'archive': archive.name, 'archive_sha256': hashlib.sha256(blob).hexdigest(), 'archive_bytes': len(blob), 'members': manifest, 'byte_policy': 'Every original raw byte retained, including CRLF, CLIXML, ANSI, trailing whitespace and libtest blank EOF; no normalization.'}
(base / 'raw-index.json').write_text(json.dumps(proof, indent=2) + '\n')
preserved = Path('/Users/adalundhe/.cache/hex-lead/transfers/windows-device-attachment-01a1c643-raw-originals-final')
preserved.mkdir(parents=True, exist_ok=False)
for member in members:
    destination = preserved / member.name
    member.rename(destination)
    assert hashlib.sha256(destination.read_bytes()).hexdigest() == manifest[member.name]['sha256']
(preserved / 'archive-index.json').write_text(json.dumps(proof, indent=2) + '\n')
print(json.dumps({'archive_sha256': proof['archive_sha256'], 'archive_bytes': len(blob), 'member_count': len(manifest), 'preserved_originals': str(preserved)}, indent=2))
