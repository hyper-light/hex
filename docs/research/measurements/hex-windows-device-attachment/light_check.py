#!/usr/bin/env python3
"""Exact private approved-runner HCS marker units and focused native/Windows compile/Clippy; archive on stdin."""
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import subprocess
import sys
import tarfile


def sha(data):
    return hashlib.sha256(data).hexdigest()


root = Path(sys.argv[1])
archive_sha, revision = sys.argv[2:4]
root.mkdir(parents=True, exist_ok=False)
archive = sys.stdin.buffer.read()
if sha(archive) != archive_sha:
    raise ValueError('exact source archive hash mismatch')
(root / 'source.tar.gz').write_bytes(archive)
source = root / 'source'
source.mkdir()
with tarfile.open(fileobj=io.BytesIO(archive), mode='r:gz') as bundle:
    for member in bundle.getmembers():
        path = PurePosixPath(member.name)
        if path.is_absolute() or '..' in path.parts or not (member.isfile() or member.isdir()):
            raise ValueError('archive has an escaping or nonregular member')
    bundle.extractall(source)
identities = {str(p.relative_to(source)): sha(p.read_bytes()) for p in sorted(source.rglob('*')) if p.is_file()}
(root / 'source-hashes.json').write_text(json.dumps(identities, indent=2)+'\n')
for name in ['target', 'cache/cargo', 'temp']:
    (root / name).mkdir(parents=True)
rustup = Path.home() / '.cache/hex-lead/rustup-scheduler-copy'
toolchain = rustup / 'toolchains/1.99.1-aarch64-apple-darwin/bin'
env = {k:v for k,v in os.environ.items() if not k.startswith(('DYLD_', 'HEX_DEBUG_', 'SHARDS_DEBUG_'))}
env.update({
    'PATH': str(toolchain)+os.pathsep+env['PATH'], 'RUSTUP_HOME': str(rustup),
    'RUSTC': str(toolchain/'rustc'), 'RUSTDOC': str(toolchain/'rustdoc'),
    'CARGO_HOME': str(root/'cache/cargo'), 'CARGO_TARGET_DIR': str(root/'target'),
    'CARGO_BUILD_JOBS': '1', 'RUST_TEST_THREADS': '1', 'RAYON_NUM_THREADS': '1',
    'TMPDIR': str(root/'temp'),
})
result = {'source_revision':revision, 'archive_sha256':archive_sha,
          'source_manifest_sha256':sha((root/'source-hashes.json').read_bytes()),
          'platform':platform.platform(), 'arch':platform.machine(), 'root':str(root),
          'needs':'cpus=1,mem=4g,disk=12g', 'phases':[],
          'scope':'HCS data units/core Clippy; installed-only Windows runtime/CLI compile and Clippy; no HCS/GPU/VM effects',
          'source_hashes':{p:identities[p] for p in json.loads((source/'docs/research/measurements/hex-windows-device-attachment/source-hashes.json').read_text())},
          'free_bytes_before':shutil.disk_usage(root).free}
for name in ['cargo', 'rustc']:
    result[name] = subprocess.check_output([str(toolchain/name),'--version'],env=env,text=True).strip()


def phase(name, args, required=()):
    command = ['nice','-n','10',str(toolchain/'cargo')]+args
    item = {'name':name,'command':command}
    result['phases'].append(item)
    with (root/(name+'.log')).open('xb') as log:
        child = subprocess.Popen(command,cwd=source,env=env,stdout=log,stderr=subprocess.STDOUT)
        item['owned_pid'] = child.pid
        (root/'run.json').write_text(json.dumps(result,indent=2)+'\n')
        try:
            item['exit_code'] = child.wait(timeout=900)
        except subprocess.TimeoutExpired:
            # Only this exact recorded child, never broad process matching or another lane.
            child.terminate()
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()
            item['exit_code'] = child.returncode
            item['timed_out'] = True
    item['child_reaped'] = child.returncode is not None
    raw = (root/(name+'.log')).read_bytes()
    text = raw.decode('utf-8',errors='replace')
    item.update({'log_sha256':sha(raw),'log_bytes':len(raw)})
    if required:
        item['selected_lines'] = [line for line in text.splitlines() if re.match(r'^test .+ \.\.\. (ok|FAILED|ignored)$',line)]
        item['summaries'] = [line for line in text.splitlines() if line.startswith('test result:')]
        item['required_tests'] = list(required)
        item['required_selected'] = all('test '+test+' ... ok' in text for test in required)
        item['nonzero_selection'] = bool(item['selected_lines'])
    (root/'run.json').write_text(json.dumps(result,indent=2)+'\n')
    return item['exit_code'] == 0 and not item.get('timed_out',False) and item.get('required_selected',True) and item.get('nonzero_selection',True)


# HCS/core sources and dependencies are byte-identical to the preserved48/0 +coreClippy0 run.
# This follow-up checks only the changed Windows implementation and real CLI constructor.
installed=(toolchain.parent/'lib/rustlib/x86_64-pc-windows-msvc/lib').is_dir()
result['windows_std_installed']=installed
if not installed:
    result['windows_compile_unrun']='existing Windows std absent; no install attempted'
    okay=False
else:
    okay=phase('windows-runtime-clippy',['clippy','-p','hex-runtime','--all-targets','--locked','--target','x86_64-pc-windows-msvc','--','-D','warnings'])
    if okay:
        okay=phase('windows-cli-check',['check','-p','hex-cli','--bin','hex','--locked','--target','x86_64-pc-windows-msvc'])
result['remaining_temp_entries']=len(list((root/'temp').iterdir()))
result['free_bytes_after']=shutil.disk_usage(root).free
result['exit_code']=0 if okay else 1
(root/'result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2),flush=True)
if not okay and result['phases']:
    last=result['phases'][-1]['name']
    print((root/(last+'.log')).read_text(errors='replace')[-18000:],flush=True)
sys.exit(result['exit_code'])
