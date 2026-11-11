import base64,hashlib,io,json,pathlib,shlex,subprocess,tarfile
root=pathlib.Path('docs/hex-device-offer');meta=json.loads((root/'light-v2/proof-meta.json').read_text())
free=int(subprocess.check_output(['df','-k','.'],text=True).splitlines()[1].split()[3]);assert free>=30*1024*1024,free
remote='''import base64,pathlib
raw=pathlib.Path(PATH).read_bytes();text=base64.b64encode(raw).decode()
for i in range(0,len(text),1000):print(text[i:i+1000])
'''.replace('PATH',repr(meta['path']))
command=['/Users/adalundhe/.local/bin/kiln','run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=4g,disk=12g','--no-sync','--timeout','120','--','nice -n 10 python3 -c '+shlex.quote(remote)]
result=subprocess.run(command,capture_output=True,text=True);assert result.returncode==0,result.stderr
blob=base64.b64decode(''.join(result.stdout.split()),validate=True)
assert len(blob)==meta['bytes'] and hashlib.sha256(blob).hexdigest()==meta['sha256']
with (root/'proof-v2.tar.gz').open('xb') as f:f.write(blob)
with tarfile.open(fileobj=io.BytesIO(blob),mode='r:gz') as archive:
    members=archive.getmembers();assert len(members)==meta['members'];data={}
    for member in members:
        path=pathlib.PurePosixPath(member.name)
        assert member.isfile() and not path.is_absolute() and '..' not in path.parts and member.name not in data
        data[member.name]=archive.extractfile(member).read()
manifest=json.loads(data['MANIFEST.json']);assert set(manifest)==set(data)-{'MANIFEST.json'}
for path,row in manifest.items():
    raw=data[path];assert len(raw)==row['bytes'] and hashlib.sha256(raw).hexdigest()==row['sha256']
# The signed archive is the lossless raw authority; do not normalize libtest EOF whitespace.
(root/'source-and-logs-r22.json').write_text(json.dumps(manifest,indent=2)+'\n')
summary=json.loads(data['proof-clippy/summary.json'])
assert summary['source']=='a69d34264f7eb6214ea108f964a9cdcbe25ccc3b'
assert summary['production_pin']=='07c68e378b741e3ce6008e1cd612b5e9d9bc3ed2'
assert summary['exit']==0 and summary['reaped'] and summary['tests_executed']==0
assert hashlib.sha256(data['proof-clippy/clippy.stderr']).hexdigest()==summary['stderr_sha256']
assert hashlib.sha256(data['proof-clippy/clippy.stdout']).hexdigest()==summary['stdout_sha256']
stop=json.loads(data['proof/steering-stop.json'])
assert stop['recorded_owned_build_pid']==39653 and not stop['pid_exists_after_r22_stop']
assert stop['test_logs']==[] and not stop['tests_list_exists'] and not stop['summary_exists']
assert not any(name.startswith('proof/test-') for name in data)
for name in data:
    if name.startswith('source/'):
        path=name.removeprefix('source/')
        signed=subprocess.check_output(['git','show',summary['source']+':'+path])
        if path=='crates/hex-node/src/scheduler/tests.rs':
            assert data[name]==signed+b'\n#[path = "device_offer_tests.rs"]\nmod device_offer_tests;\n'
        else:assert data[name]==signed,path
parent=subprocess.check_output(['git','show',summary['production_pin']+':crates/hex-node/src/scheduler/tests.rs'])
registration=b'\n#[path = "device_offer_tests.rs"]\nmod device_offer_tests;\n'
assert data['proof/parent-before.rs']==parent
assert data['proof/parent-private.rs']==parent+registration
source_after=json.loads(data['proof/source-after.json'])
source_before=json.loads(data['proof/source.json'])
for path,digest in source_before['files'].items():
    if path!='crates/hex-node/src/scheduler/tests.rs':assert source_after['files'][path]==digest,path
record={'source':summary['source'],'production_pin':summary['production_pin'],'archive':meta,'safe_regular_members_verified':len(data),'all_member_hashes_verified':True,'source_matches_signed_commits':True,'private_registration_only':True,'test_clippy_exit':0,'tests_executed':0,'clippy_pid':summary['pid'],'clippy_reaped':True,'stopped_job':'r22','stopped_before_test_execution':True,'stopped_cargo_pid_absent':39653,'stderr_sha256':summary['stderr_sha256'],'local_free_kib':free,'transport_command':command,'transport_stderr':result.stderr}
(root/'proof-receipt-v2.json').write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps({key:value for key,value in record.items() if key not in ['transport_command','transport_stderr']}))
