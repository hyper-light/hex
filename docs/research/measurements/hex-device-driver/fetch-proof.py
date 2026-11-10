import base64,hashlib,io,json,pathlib,shlex,subprocess,tarfile
root=pathlib.Path('docs/research/measurements/hex-device-driver');meta=json.loads((root/'light-v1/proof-meta.json').read_text())
free=int(subprocess.check_output(['df','-k','.'],text=True).splitlines()[1].split()[3]);assert free>=30*1024*1024,free
remote='''import base64,pathlib
raw=pathlib.Path(PATH).read_bytes();text=base64.b64encode(raw).decode()
for i in range(0,len(text),1000):print(text[i:i+1000])
'''.replace('PATH',repr(meta['path']))
command=['/Users/adalundhe/.local/bin/kiln','run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=4g,disk=12g','--no-sync','--timeout','120','--','nice -n 10 python3 -c '+shlex.quote(remote)]
result=subprocess.run(command,capture_output=True,text=True);assert result.returncode==0,result.stderr
blob=base64.b64decode(''.join(result.stdout.split()),validate=True)
assert len(blob)==meta['bytes'] and hashlib.sha256(blob).hexdigest()==meta['sha256']
with (root/'proof-v1.tar.gz').open('xb') as f:f.write(blob)
with tarfile.open(fileobj=io.BytesIO(blob),mode='r:gz') as archive:
    members=archive.getmembers();assert len(members)==meta['members'];data={}
    for member in members:
        path=pathlib.PurePosixPath(member.name)
        assert member.isfile() and not path.is_absolute() and '..' not in path.parts and member.name not in data
        data[member.name]=archive.extractfile(member).read()
manifest=json.loads(data['MANIFEST.json']);assert set(manifest)==set(data)-{'MANIFEST.json'}
for path,row in manifest.items():
    raw=data[path];assert len(raw)==row['bytes'] and hashlib.sha256(raw).hexdigest()==row['sha256']
dest=root/'raw-v1';assert not dest.exists();dest.mkdir()
for path,raw in data.items():
    target=dest/path;target.parent.mkdir(parents=True,exist_ok=True)
    with target.open('xb') as f:f.write(raw)
summary=json.loads(data['proof/summary.json'])
assert summary['source']=='6483c6f2b0cb109cd41d009aa3974962a238c1b8'
assert summary['production_pin']=='5e2cfbe7e09256ff6a7d979564717cfc6d8041f6'
assert summary['passes']==3 and summary['failed']==0 and summary['selected']==3 and summary['ignored']==0
assert all(value==0 for value in summary['checks'].values())
for row in summary['tests']:
    raw=data['proof/'+row['log']]
    assert row['passed'] and row['ran_one'] and row['reaped'] and row['exit_code']==0
    assert hashlib.sha256(raw).hexdigest()==row['log_sha256']
    for marker in [b'running 1 test',b'test result: ok. 1 passed; 0 failed; 0 ignored;',b'cleanup_boxes=0 cleanup_claims=0 hardware_effect=false']:assert marker in raw
for name,path in [('leaf.rs','crates/hex-node/src/placement/device_binding_tests.rs'),('placement.rs','crates/hex-node/src/placement.rs'),('placement-devices.rs','crates/hex-node/src/placement/devices.rs'),('runtime-catalog.rs','crates/hex-runtime/src/devices.rs')]:
    assert data['source/'+name]==subprocess.check_output(['git','show',summary['source']+':'+path])
parent=subprocess.check_output(['git','show',summary['production_pin']+':crates/hex-node/src/placement/tests.rs'])
registration=b'\n#[path = "device_binding_tests.rs"]\nmod device_binding_tests;\n'
assert data['proof/parent-before.rs']==parent
assert data['proof/parent-private.rs']==parent+registration
record={'source':summary['source'],'production_pin':summary['production_pin'],'archive':meta,'safe_regular_members_verified':len(data),'all_member_hashes_verified':True,'source_matches_signed_commits':True,'private_registration_only':True,'counted_passes':3,'failed':0,'ignored':0,'all_logs_nonzero_selected_and_cleanup_markers':True,'all_selected_child_reaps_verified':True,'binary_sha256':summary['binary_sha256'],'checks':summary['checks'],'local_free_kib':free,'transport_command':command,'transport_stderr':result.stderr}
(root/'proof-receipt.json').write_text(json.dumps(record,indent=2)+'\n')
print(json.dumps({key:value for key,value in record.items() if key not in ['transport_command','transport_stderr']}))
