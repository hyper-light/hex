"""Preserve terminal owned raw evidence losslessly, without editing or deleting originals."""
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import sys
import tarfile

here = Path(__file__).resolve().parent
for tag in sys.argv[1:]:
    if not tag.isalnum():
        raise ValueError('owned run tag required')
    root = here / 'runs' / tag
    submission = json.loads((root / 'submission.json').read_text())
    if not submission.get('reaped', submission.get('local_cli_reaped', False)):
        raise ValueError(f'{tag}: no terminal local reaping receipt')
    suffix = os.environ.get('HEX_NATIVE_EVIDENCE_ARCHIVE_SUFFIX', '')
    if suffix and not suffix.isalnum():
        raise ValueError('explicit alphanumeric additional archive suffix required')
    stem = 'raw-evidence' + ('-' + suffix if suffix else '')
    archive = root / (stem + '.tar.gz')
    index = root / (stem + '-index.json')
    if archive.exists() or index.exists():
        raise ValueError('existing raw archive must never be overwritten')
    files = sorted([f for f in root.rglob('*') if f.is_file() and (f.suffix == '.log' or 'raw' in f.relative_to(root).parts)])
    manifest = {}
    buffer = io.BytesIO()
    with gzip.GzipFile(fileobj=buffer, mode='wb', mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode='w') as output:
            for file in files:
                content = file.read_bytes()
                name = file.relative_to(root).as_posix()
                entry = tarfile.TarInfo(name)
                entry.size = len(content)
                entry.mtime = 0
                output.addfile(entry, io.BytesIO(content))
                manifest[name] = {'bytes': len(content), 'sha256': hashlib.sha256(content).hexdigest()}
    content = buffer.getvalue()
    archive.write_bytes(content)
    with tarfile.open(fileobj=io.BytesIO(content), mode='r:gz') as stored:
        for name, proof in manifest.items():
            member = stored.extractfile(name)
            if member is None or hashlib.sha256(member.read()).hexdigest() != proof['sha256']:
                raise ValueError('archived raw bytes differ')
    index.write_text(json.dumps({'archive_sha256': hashlib.sha256(content).hexdigest(), 'archive_bytes': len(content), 'members': manifest, 'originals_retained_in_place': True}, indent=2)+'\n')
    print(json.dumps({'tag': tag, 'members': len(manifest), 'archive_bytes': len(content), 'archive_sha256': hashlib.sha256(content).hexdigest()}))
