#!/usr/bin/env python3
"""Read only terminal owned Jupiter artifacts; preserve exact original bytes."""
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import sys

local, prefix = Path(sys.argv[1]), sys.argv[2]
meta = json.loads((local / 'transfer.json').read_text())
if meta.get('local_cli_exit') is None:
    raise ValueError('native check is not terminal')
free = int(subprocess.check_output(['df', '-k', '.'], text=True).splitlines()[-1].split()[3])
if free < 30 * 1024 * 1024:
    raise ValueError('hold collection below30GiB')
remote = meta['remote_root'].replace("'", "''")
script = r"""$ErrorActionPreference='Stop';$p='__ROOT__';$r=[IO.File]::ReadAllText($p+'\result.json')|ConvertFrom-Json;$names=@('result.json','run.json');foreach($phase in $r.phases){$names+=($phase.name+'.stdout.log');$names+=($phase.name+'.stderr.log')};foreach($n in $names){$b=[IO.File]::ReadAllBytes($p+'\'+$n);$hash=(Get-FileHash -LiteralPath ($p+'\'+$n) -Algorithm SHA256).Hash.ToLowerInvariant();[Console]::WriteLine('HEX_NATIVE_FILE '+$n+' '+$hash+' '+[Convert]::ToBase64String($b))}""".replace('__ROOT__', remote)
command = 'powershell -NoProfile -NonInteractive -EncodedCommand ' + base64.b64encode(script.encode('utf-16-le')).decode()
run = subprocess.run(['/Users/adalundhe/.local/bin/kiln', 'run', '--on', meta['runner'], '--needs', 'cpus=1,mem=4g,disk=12g', '--no-sync', '--timeout', '60', '--', command], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, check=True)
(local / 'collection-cli.log').write_bytes(run.stdout)
destination = Path('docs/research/measurements/hex-windows-device-attachment')
files = {}
for line in run.stdout.splitlines():
    if not line.startswith(b'HEX_NATIVE_FILE '):
        continue
    _, name, expected, encoded = line.split(b' ', 3)
    name, expected = name.decode('ascii'), expected.decode('ascii')
    if '/' in name or '\\' in name or name in files:
        raise ValueError('unexpected native evidence name')
    data = base64.b64decode(encoded, validate=True)
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError('native evidence hash mismatch')
    (local / name).write_bytes(data)
    with (destination / (prefix + '-' + name)).open('xb') as out:
        out.write(data)
    files[name] = expected
result = json.loads((local / 'result.json').read_text(encoding='utf-8-sig'))
required = {'result.json', 'run.json'} | {p['name'] + suffix for p in result['phases'] for suffix in ['.stdout.log', '.stderr.log']}
if set(files) != required:
    raise ValueError('native evidence collection incomplete')
for name in ['transfer.json', 'cli.log']:
    with (destination / (prefix + '-' + name)).open('xb') as out:
        out.write((local / name).read_bytes())
proof = {'source_revision': meta['source_revision'], 'remote_root': meta['remote_root'], 'files': files, 'free_kib_before': free}
(destination / (prefix + '-collection.json')).write_text(json.dumps(proof, indent=2) + '\n')
print(json.dumps({'source_revision': result['source_revision'], 'exit_code': result['exit_code'], 'phases': [{k: v for k, v in p.items() if k in ['name', 'exit_code', 'child_reaped', 'owned_pid', 'selected_lines', 'summaries', 'stdout_sha256', 'stderr_sha256']} for p in result['phases']], 'remaining_temp_entries': result['remaining_temp_entries']}, indent=2))
