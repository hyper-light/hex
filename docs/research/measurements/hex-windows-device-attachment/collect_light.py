#!/usr/bin/env python3
"""Collect only terminal owned light artifacts, preserving all raw bytes and hashes."""
import base64
import hashlib
import io
import json
from pathlib import Path, PurePosixPath
import shlex
import subprocess
import sys
import tarfile

local = Path(sys.argv[1])
prefix = sys.argv[2]
metadata = json.loads((local/'transfer.json').read_text())
if metadata.get('local_cli_exit') is None:
    raise ValueError('light check is not terminal')
free = int(subprocess.check_output(['df','-k','.'],text=True).splitlines()[-1].split()[3])
if free < 30*1024*1024:
    raise ValueError('hold bulk collection: local free below 30GiB')
remote = metadata['remote_root']
script = '''import base64,hashlib,io,json,sys,tarfile
from pathlib import Path
root=Path(sys.argv[1]); result=json.loads((root/'result.json').read_text())
names=['result.json','run.json','source-hashes.json']+[p['name']+'.log' for p in result['phases']]
buffer=io.BytesIO()
with tarfile.open(fileobj=buffer,mode='w:gz') as bundle:
 for name in names: bundle.add(root/name,arcname=name,recursive=False)
blob=buffer.getvalue()
print('HEX_DEVICE_LIGHT_ARTIFACTS_BEGIN')
print(json.dumps({'archive_sha256':hashlib.sha256(blob).hexdigest(),'archive_base64':base64.b64encode(blob).decode(),'files':{n:hashlib.sha256((root/n).read_bytes()).hexdigest() for n in names}}))
print('HEX_DEVICE_LIGHT_ARTIFACTS_END')
'''
command = 'python3 -c '+shlex.quote(script)+' '+shlex.quote(remote)
run = subprocess.run(['/Users/adalundhe/.local/bin/kiln','run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=4g,disk=12g','--no-sync','--timeout','60','--',command],stdout=subprocess.PIPE,stderr=subprocess.STDOUT,check=True)
(local/'collection-cli.log').write_bytes(run.stdout)
body = run.stdout.split(b'HEX_DEVICE_LIGHT_ARTIFACTS_BEGIN\n',1)[1].split(b'\nHEX_DEVICE_LIGHT_ARTIFACTS_END',1)[0]
proof = json.loads(body)
blob = base64.b64decode(proof.pop('archive_base64'),validate=True)
if hashlib.sha256(blob).hexdigest() != proof['archive_sha256']:
    raise ValueError('collected archive hash mismatch')
(local/'raw-evidence.tar.gz').write_bytes(blob)
destination = Path('docs/research/measurements/hex-windows-device-attachment')
with tarfile.open(fileobj=io.BytesIO(blob),mode='r:gz') as bundle:
    for member in bundle.getmembers():
        path=PurePosixPath(member.name)
        if not member.isfile() or len(path.parts)!=1 or path.name not in proof['files']:
            raise ValueError('unexpected evidence member')
        data=bundle.extractfile(member).read()
        if hashlib.sha256(data).hexdigest()!=proof['files'][path.name]:
            raise ValueError('raw evidence member hash mismatch')
        with (destination/(prefix+'-'+path.name)).open('xb') as out:
            out.write(data)
for name in ['transfer.json','cli.log']:
    with (destination/(prefix+'-'+name)).open('xb') as out:
        out.write((local/name).read_bytes())
proof.update({'source_revision':metadata['source_revision'],'remote_root':remote,'source_archive_sha256':metadata['archive_sha256'],'free_kib_before_collection':free})
(destination/(prefix+'-collection.json')).write_text(json.dumps(proof,indent=2)+'\n')
result=json.loads((destination/(prefix+'-result.json')).read_text())
expected = json.loads((destination/'source-hashes.json').read_text())
actual = json.loads((destination/(prefix+'-source-hashes.json')).read_text())
print(json.dumps({'source_revision':result['source_revision'],'exit_code':result['exit_code'],'phases':[{k:v for k,v in p.items() if k in ['name','exit_code','child_reaped','summaries','log_sha256','owned_pid','required_selected']} for p in result['phases']], 'remaining_temp_entries':result['remaining_temp_entries'],'current_candidate_source_matches':all(actual.get(p)==sha for p,sha in expected.items()),'collection_archive_sha256':proof['archive_sha256']},indent=2))
