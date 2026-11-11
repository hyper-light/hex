#!/usr/bin/env python3
"""Run one immutable owned script through admitted Kiln stdin, with a finite outer guard."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
leaf, tag, seconds = sys.argv[1:4]
if Path(leaf).name != leaf or not tag.isalnum():
    raise ValueError('owned script/tag only')
script = (HERE / leaf).read_bytes()
local = HERE / 'runs' / tag
local.mkdir(parents=True, exist_ok=False)
(local / leaf).write_bytes(script)
needs = os.environ.get('HEX_NATIVE_NEEDS', 'cpus=1,mem=4g,disk=12g')
argv = [str(Path.home() / '.local/bin/kiln'), 'run', '--on', 'f766f09294f2e17e3b8a631ffd4e6a4b', '--needs', needs, '--no-sync', '--quiet', '--stdin', '--timeout', seconds, '--', 'powershell.exe -NoProfile -NonInteractive -Command "Invoke-Expression ([Console]::In.ReadToEnd())"']
meta = {'argv': argv, 'source_sha256': hashlib.sha256(script).hexdigest(), 'needs': needs, 'outer_budget_seconds': int(seconds)}
with (local / 'cli.log').open('xb') as output:
    child = subprocess.Popen(argv, stdin=subprocess.PIPE, stdout=output, stderr=subprocess.STDOUT)
    meta['pid'] = child.pid
    (local / 'submission.json').write_text(json.dumps(meta, indent=2)+'\n')
    print(json.dumps({'tag': tag, 'pid': child.pid, 'script_sha256': meta['source_sha256']}), flush=True)
    try:
        child.communicate(script, timeout=int(seconds)+30)
    except subprocess.TimeoutExpired:
        meta['timed_out'] = True
        child.terminate()
        child.wait(timeout=10)
    meta['exit'] = child.returncode
    meta['reaped'] = child.returncode is not None
    raw = (local / 'cli.log').read_bytes()
    meta['raw_sha256'] = hashlib.sha256(raw).hexdigest()
    (local / 'submission.json').write_text(json.dumps(meta, indent=2)+'\n')
    print(raw.decode(errors='replace')[-24000:], flush=True)
    raise SystemExit(child.returncode)
