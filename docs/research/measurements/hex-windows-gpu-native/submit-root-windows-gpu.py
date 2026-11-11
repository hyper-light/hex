#!/usr/bin/env python3
"""Submit one explicitly assigned private Windows light run; never dispatch a queue."""
import base64
import gzip
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile

revision, tag, test_filter, expected_tests, clippy = sys.argv[1:6]
free = int(subprocess.check_output(['df', '-k', '.'], text=True).splitlines()[-1].split()[3])
if free < 30 * 1024 * 1024:
    raise ValueError('hold transfer below30GiB')
local = Path('/Users/adalundhe/.cache/hex-lead/transfers') / ('root-windows-gpu-' + tag)
local.mkdir(parents=True, exist_ok=False)
archive = gzip.compress(subprocess.check_output(['git', 'archive', '--format=tar', revision]), mtime=0)
sha = hashlib.sha256(archive).hexdigest()
(local / 'source.tar.gz').write_bytes(archive)
remote = 'C:\\Users\\corph\\.cache\\hex-windows-device-attachment\\root-gpu-' + tag
incoming = remote + '-incoming'
script = Path('/Users/adalundhe/.cache/hex-lead/fixtures/root-drivers-fff9d0b9/root-windows-gpu-native.ps1').read_text()
for key, value in {'__OWN_ROOT__': remote, '__ARCHIVE_SHA__': sha, '__SOURCE_REVISION__': revision, '__SOURCE_ARCHIVE_FILE__': incoming + '\\source.tar.gz', '__TEST_FILTER__': test_filter, '__EXPECTED_TESTS__': str(int(expected_tests)), '__RUN_CLIPPY__': '$true' if clippy == 'yes' else '$false', '__CHECK_SCOPE__': 'ROOT actual Windows AMD GPU inventory/sample; no HCS/VM or assignment proof'}.items():
    script = script.replace(key, value)
if '__' in script:
    raise ValueError('unexpanded driver placeholder')
(local / 'submitted-native-light.ps1').write_text(script)
buffer = io.BytesIO()
with tarfile.open(fileobj=buffer, mode='w:gz') as bundle:
    for name, data in [('source.tar.gz', archive), ('driver.ps1', script.encode())]:
        entry = tarfile.TarInfo(name)
        entry.size = len(data)
        bundle.addfile(entry, io.BytesIO(data))
transport = buffer.getvalue()
transport_sha = hashlib.sha256(transport).hexdigest()
(local / 'transport.tar.gz').write_bytes(transport)
bootstrap = r"""$ErrorActionPreference='Stop';$p='__INCOMING__';if(Test-Path -LiteralPath $p){throw 'private incoming path exists'};[IO.Directory]::CreateDirectory($p)|Out-Null;$f=$p+'\transport.tar.gz';$o=[IO.File]::Open($f,[IO.FileMode]::CreateNew);try{[Console]::OpenStandardInput().CopyTo($o)}finally{$o.Dispose()};if((Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash.ToLowerInvariant() -ne '__HASH__'){throw 'transport hash mismatch'};& C:\Windows\System32\tar.exe -xzf $f -C $p;if($LASTEXITCODE -ne 0){throw 'transport extraction failed'};$tokens=$null;$errors=$null;[Management.Automation.Language.Parser]::ParseFile($p+'\driver.ps1',[ref]$tokens,[ref]$errors)|Out-Null;if($errors.Count){throw ($errors|Out-String)};& ([scriptblock]::Create([IO.File]::ReadAllText($p+'\driver.ps1')))""".replace('__INCOMING__', incoming).replace('__HASH__', transport_sha)
(local / 'submitted-bootstrap.ps1').write_text(bootstrap)
command = 'powershell -NoProfile -NonInteractive -EncodedCommand ' + base64.b64encode(bootstrap.encode('utf-16-le')).decode()
meta = {'source_revision': revision, 'archive_sha256': sha, 'archive_bytes': len(archive), 'transport_sha256': transport_sha, 'runner': 'f766f09294f2e17e3b8a631ffd4e6a4b', 'needs': 'cpus=1,mem=4g,disk=12g', 'remote_root': remote, 'incoming_root': incoming, 'local_root': str(local), 'free_kib_before': free, 'test_filter': test_filter, 'expected_selected': int(expected_tests), 'driver_sha256': hashlib.sha256(script.encode()).hexdigest(), 'bootstrap_sha256': hashlib.sha256(bootstrap.encode()).hexdigest()}
def save():
    (local / 'transfer.json').write_text(json.dumps(meta, indent=2) + '\n')
save()
with (local / 'cli.log').open('xb') as log:
    child = subprocess.Popen(['/Users/adalundhe/.local/bin/kiln', 'run', '--on', meta['runner'], '--needs', meta['needs'], '--no-sync', '--stdin', '--timeout', '1800', '--', command], stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT)
    meta['local_cli_pid'] = child.pid
    save()
    child.communicate(transport)
    meta['local_cli_exit'] = child.returncode
    meta['cli_log_sha256'] = hashlib.sha256((local / 'cli.log').read_bytes()).hexdigest()
    save()
    print(json.dumps(meta, indent=2), flush=True)
    print((local / 'cli.log').read_text(errors='replace')[-16000:], flush=True)
    raise SystemExit(child.returncode)
