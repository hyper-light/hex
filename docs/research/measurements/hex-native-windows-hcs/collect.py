#!/usr/bin/env python3
"""Retain original private native logs/source hashes, never source/target data or unrelated scopes."""
import base64
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import zipfile

HERE = Path(__file__).resolve().parent
for tag in sys.argv[1:]:
    if not tag.isalnum():
        raise ValueError('owned alphanumeric tag only')
    local = HERE / 'runs' / tag
    remote_leaf = {
        'hcscpu1': 'hcs-cpu-exec-v1', 'hcscpu2': 'hcs-cpu-exec-v2',
        'hcsgpu1': 'hcs-gpu-exec-v1', 'hcsgpu2': 'hcs-gpu-exec-v2',
        'hcsgpu1cleanup': 'hcs-gpu-exec-v1', 'fullimagecpu1': 'native-full-image-cpu-v1',
        'fullimagegpu1': 'native-full-image-gpu-v1', 'currentidentity1': 'current-identity-v1',
        'wholepost1': 'whole-envelope-meter-v2',
        'wholepost2': 'whole-envelope-meter-v3', 'nodeclic579': 'c579-node-cli-check-v1',
        'hcsevents1': 'owned-hcs-events-v1', 'absence3': 'owned-absence-v3',
        'nodecli93': '93-node-cli-check-v1', 'hcsbasic1': 'hcs-service-basic-v1',
        'wholepost3': 'whole-envelope-meter-v4', 'absence4': 'owned-absence-v4',
        'launcherloader1': 'native-launcher-loader-v1',
        'retainedmeter2': 'retained-image-meter-v2',
        'launcherimports1': 'launcher-imports-v1', 'launcherimports2': 'launcher-imports-v1',
        'cachedhelp1': 'cached-command-help-v1', 'cachedpreflight2': 'cached-import-preflight-v2',
        'wholeenvelope1': 'whole-envelope-meter-v1', 'vhdcapability1': 'vhd-capability-v1',
        'cachedimport3': 'cached-release-v3', 'cachedbody3': 'cached-image-import-v3',
        'cachedcontroller3': 'image-cached-controller-v3', 'cachedio1': 'cached-io-observer-v1',
        'importreceipts1': 'import-receipts-v1', 'verifystartup1': 'verify-startup-diagnostic-v1',
        'imageverify1': 'post-import-verify-v1', 'imageverify2': 'post-import-verify-v2',
        'imageverify3': 'post-import-verify-v3', 'imageverify4': 'post-import-verify-v4',
    }.get(tag, tag)
    script = r'''$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue';[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal';$r='C:\Users\corph\.cache\hex-native-windows-hcs\__TAG__';if(-not (Test-Path -LiteralPath $r)){throw 'owned run absent'};$files=@(Get-ChildItem -LiteralPath $r -File|Where-Object{$_.Extension -in @('.json','.log')});$scratch=Join-Path $r 'scratch';if(Test-Path -LiteralPath $scratch){$files+=@(Get-ChildItem -LiteralPath $scratch -File -Recurse|Where-Object{$_.Extension -in @('.json','.log','.owner') -or $_.Name -eq 'config.json'})};$output=Join-Path $r 'output';if(Test-Path -LiteralPath $output){$files+=@(Get-ChildItem -LiteralPath $output -File|Where-Object{$_.Extension -in @('.log','.txt')})};$index=[ordered]@{};$payload=[ordered]@{};foreach($file in $files){if($file.Length -gt 8388608){throw 'explicit collection per-file budget exceeded'};$relative=$file.FullName.Substring($r.Length+1).Replace('\','/');$bytes=[IO.File]::ReadAllBytes($file.FullName);$index[$relative]=@{bytes=$bytes.Length;sha256=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()};$payload[$relative]=[Convert]::ToBase64String($bytes)};$source=Join-Path $r 'source';$hashes=[ordered]@{};if(Test-Path -LiteralPath $source){foreach($file in (Get-ChildItem -LiteralPath $source -File -Recurse|Sort-Object FullName)){$relative=$file.FullName.Substring($source.Length+1).Replace('\','/');$hashes[$relative]=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}};$temp=Join-Path $r 'temp';$temporaries=@(if(Test-Path -LiteralPath $temp){Get-ChildItem -LiteralPath $temp -File -Recurse|ForEach-Object{@{path=$_.FullName;bytes=$_.Length;sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()}}});[ordered]@{root=$r;index=$index;files=$payload;source_hashes=$hashes;temp_files=$temporaries;utc=[DateTime]::UtcNow.ToString('o')}|ConvertTo-Json -Depth 20 -Compress'''.replace('__TAG__', remote_leaf)
    (local / 'collect.ps1').write_text(script)
    argv = [str(Path.home() / '.local/bin/kiln'), 'run', '--on', 'f766f09294f2e17e3b8a631ffd4e6a4b', '--needs', 'cpus=1,mem=4g,disk=12g', '--no-sync', '--quiet', '--stdin', '--timeout', '120', '--', 'powershell.exe -NoProfile -NonInteractive -Command "Invoke-Expression ([Console]::In.ReadToEnd())"']
    log = local / 'collection.cli.log'
    if log.exists():
        raise ValueError('collection already retained; choose new explicit collector rather than overwrite')
    with log.open('xb') as output:
        child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=output, stderr=subprocess.STDOUT)
        meta = {'pid': child.pid, 'argv': argv, 'script_sha256': hashlib.sha256(script.encode()).hexdigest()}
        (local / 'collection.json').write_text(json.dumps(meta, indent=2)+'\n')
        try:
            child.communicate(script.encode(), timeout=150)
        except subprocess.TimeoutExpired:
            child.terminate()
            child.wait(timeout=10)
            meta['timed_out'] = True
        meta['exit'] = child.returncode
        meta['reaped'] = child.returncode is not None
        raw = log.read_bytes()
        meta['raw_sha256'] = hashlib.sha256(raw).hexdigest()
        (local / 'collection.json').write_text(json.dumps(meta, indent=2)+'\n')
    if child.returncode:
        print(raw.decode(errors='replace')[-3000:])
        raise SystemExit(child.returncode)
    data = json.loads(next(line for line in raw.decode().splitlines() if line.startswith('{"root":')))
    originals = local / 'raw'
    originals.mkdir(exist_ok=False)
    for name, encoded in data.pop('files').items():
        if Path(name).is_absolute() or '..' in Path(name).parts:
            raise ValueError('unsafe collection member')
        content = base64.b64decode(encoded, validate=True)
        if hashlib.sha256(content).hexdigest() != data['index'][name]['sha256']:
            raise ValueError('raw native byte mismatch')
        into = originals / name
        into.parent.mkdir(parents=True, exist_ok=True)
        into.write_bytes(content)
    (local / 'native-index.json').write_text(json.dumps(data, indent=2)+'\n')
    meta['files'] = len(data['index'])
    meta['source_files'] = len(data['source_hashes'])
    (local / 'collection.json').write_text(json.dumps(meta, indent=2)+'\n')
    print(json.dumps({'tag': tag, 'raw_files': meta['files'], 'source_files': meta['source_files'], 'temp_files': data['temp_files'], 'exit': child.returncode}), flush=True)
