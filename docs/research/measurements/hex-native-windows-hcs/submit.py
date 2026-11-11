#!/usr/bin/env python3
"""Transfer a pinned source and owned driver to the assigned admitted Windows runner."""
import base64
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
REVISION = os.environ.get('HEX_NATIVE_SOURCE_REVISION', '2c8855b220a36c57685e456a7b8c98d8368f7899')
RUNNER = 'f766f09294f2e17e3b8a631ffd4e6a4b'
NEEDS = os.environ.get('HEX_NATIVE_NEEDS', 'cpus=1,mem=4g,disk=12g')
if NEEDS not in ('cpus=1,mem=4g,disk=12g', 'cpus=1,mem=4g,disk=32g', 'cpus=1,mem=4g,disk=40g'):
    raise ValueError('fixed one-CPU/four-GiB explicit disk envelope required')
mode, tag = sys.argv[1:3]
if mode not in ('host', 'execute', 'controls', 'read', 'hcs', 'diagnostic', 'llddiagnostic', 'hcsbuild', 'preflight', 'preflight_pending') or not tag.isalnum():
    raise ValueError('explicit owned driver/tag required')
if NEEDS != 'cpus=1,mem=4g,disk=12g' and mode != 'preflight_pending':
    raise ValueError('changed envelope requires the separately reviewed successor driver')
local = HERE / 'runs' / tag
local.mkdir(parents=True, exist_ok=False)
free = int(subprocess.check_output(['df', '-k', str(REPO)], text=True).splitlines()[-1].split()[3])
if free < 30 * 1024 * 1024:
    raise ValueError('hold below local 30 GiB floor')
archive = gzip.compress(subprocess.check_output(['git', 'archive', REVISION], cwd=REPO), mtime=0)
source_sha = hashlib.sha256(archive).hexdigest()
(local / 'source.tar.gz').write_bytes(archive)
remote = 'C:/Users/corph/.cache/hex-native-windows-hcs/' + tag
incoming = remote + '-incoming'
driver = (HERE / (mode + '.ps1')).read_text()
for key, value in {'__ROOT__': remote, '__INCOMING__': incoming, '__REVISION__': REVISION, '__SOURCE_SHA__': source_sha, '__NEEDS__': NEEDS}.items():
    driver = driver.replace(key, value)
(local / 'driver.ps1').write_text(driver)
buffer = io.BytesIO()
with tarfile.open(fileobj=buffer, mode='w:gz') as bundle:
    for name, data in [('source.tar.gz', archive), ('driver.ps1', driver.encode()), ('guard.cs', (HERE / 'guard.cs').read_bytes()), ('artifact_verifier.cs', (HERE / 'artifact_verifier.cs').read_bytes()), ('policy-byte-equality.json', (HERE / 'policy-byte-equality.json').read_bytes())]:
        entry = tarfile.TarInfo(name)
        entry.size = len(data)
        entry.mtime = 0
        bundle.addfile(entry, io.BytesIO(data))
transport = buffer.getvalue()
(local / 'transport.tar.gz').write_bytes(transport)
transport_sha = hashlib.sha256(transport).hexdigest()
bootstrap = r"""$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';$p='__INCOMING__';if(Test-Path -LiteralPath $p){throw 'owned incoming exists'};[IO.Directory]::CreateDirectory($p)|Out-Null;$f=$p+'\transport.tar.gz';$o=[IO.File]::Open($f,[IO.FileMode]::CreateNew);try{[Console]::OpenStandardInput().CopyTo($o)}finally{$o.Dispose()};if((Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash.ToLowerInvariant() -ne '__TRANSPORT__'){throw 'transport mismatch'};& C:\Windows\System32\tar.exe -xzf $f -C $p;if($LASTEXITCODE -ne 0){throw 'transport extraction failed'};& ([scriptblock]::Create([IO.File]::ReadAllText($p+'\driver.ps1')))""".replace('__INCOMING__', incoming).replace('__TRANSPORT__', transport_sha)
(local / 'bootstrap.ps1').write_text(bootstrap)
command = 'powershell.exe -NoProfile -NonInteractive -EncodedCommand ' + base64.b64encode(bootstrap.encode('utf-16-le')).decode()
argv = [str(Path.home() / '.local/bin/kiln'), 'run', '--on', RUNNER, '--needs', NEEDS, '--no-sync', '--quiet', '--stdin', '--timeout', '1100', '--', command]
receipt = {'policy_revision': '0c598386ca1f7c65c6e0634dba77983e4abffff2', 'rules_sha256': 'df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3', 'role': 'assigned adversarial Windows/HCS', 'agent': '01a1c8aa921b72c89b7272fba2926a9c', 'coordinator': '01a1aa77778471339bea3beff90d6bbd', 'ownership': ['NEW docs/research/measurements/hex-native-windows-hcs/**', 'NEW crates/hex-cli/tests/native_windows_*', 'NEW crates/hex-gpu/tests/native_windows_*'], 'ack': 'ROOT immediate ACK/release user steering message before first edit/dispatch', 'source_revision': REVISION, 'runner': RUNNER, 'needs': NEEDS, 'priority': 'BelowNormal', 'jobs': 1, 'test_threads': 1, 'local_free_kib_before': free, 'source_archive_sha256': source_sha, 'source_archive_bytes': len(archive), 'driver_sha256': hashlib.sha256(driver.encode()).hexdigest(), 'transport_sha256': transport_sha, 'bootstrap_sha256': hashlib.sha256(bootstrap.encode()).hexdigest(), 'remote': remote, 'incoming': incoming, 'argv': argv, 'mode': mode}
(local / 'submission.json').write_text(json.dumps(receipt, indent=2) + '\n')
with (local / 'cli.log').open('xb') as log:
    child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=log, stderr=subprocess.STDOUT)
    receipt['local_cli_pid'] = child.pid
    (local / 'submission.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps({'local_cli_pid': child.pid, 'remote': remote, 'local': str(local), 'source_sha256': source_sha}), flush=True)
    try:
        child.communicate(transport, timeout=1160)
    except subprocess.TimeoutExpired:
        receipt['local_cli_timed_out'] = True
        child.terminate()
        try:
            child.wait(timeout=10)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=10)
    receipt['local_cli_exit'] = child.returncode
    receipt['local_cli_reaped'] = child.returncode is not None
    receipt['cli_log_sha256'] = hashlib.sha256((local / 'cli.log').read_bytes()).hexdigest()
    (local / 'submission.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print((local / 'cli.log').read_text(errors='replace')[-20000:], flush=True)
    raise SystemExit(child.returncode or int(receipt.get('local_cli_timed_out', False)))
