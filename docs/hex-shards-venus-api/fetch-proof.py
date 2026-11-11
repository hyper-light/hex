import base64,hashlib,io,json,pathlib,shlex,subprocess,tarfile,sys
base=pathlib.Path('docs/hex-shards-venus-api');version=sys.argv[1]
assert version in ['v1','v2','v3']
meta=json.loads((base/('light-'+version)/'proof-meta.json').read_text())
free=int(subprocess.check_output(['df','-k','.'],text=True).splitlines()[1].split()[3]);assert free>=30*1024*1024,free
remote='''import base64,pathlib
raw=pathlib.Path(PATH).read_bytes();text=base64.b64encode(raw).decode()
for i in range(0,len(text),1000):print(text[i:i+1000])
'''.replace('PATH',repr(meta['path']))
command=['/Users/adalundhe/.local/bin/kiln','run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=4g,disk=12g','--no-sync','--timeout','120','--','nice -n 10 python3 -c '+shlex.quote(remote)]
result=subprocess.run(command,capture_output=True,text=True);assert result.returncode==0,result.stderr
blob=base64.b64decode(''.join(result.stdout.split()),validate=True)
assert len(blob)==meta['bytes'] and hashlib.sha256(blob).hexdigest()==meta['sha256']
destination=base/('proof-'+version+'.tar.gz')
if destination.exists():assert destination.read_bytes()==blob
else:
    with destination.open('xb') as f:f.write(blob)
data={}
with tarfile.open(fileobj=io.BytesIO(blob),mode='r:gz') as archive:
    assert len(archive.getmembers())==meta['members']
    for member in archive.getmembers():
        path=pathlib.PurePosixPath(member.name)
        assert member.isfile() and not path.is_absolute() and '..' not in path.parts and member.name not in data
        data[member.name]=archive.extractfile(member).read()
manifest=json.loads(data['MANIFEST.json']);assert set(manifest)==set(data)-{'MANIFEST.json'}
for name,row in manifest.items():assert len(data[name])==row['bytes'] and hashlib.sha256(data[name]).hexdigest()==row['sha256']
source=json.loads(data['proof/source.json']);assert source['commit']=='3f2e9fb10e4caca0de5114e1dd5471540b17e217'
after=json.loads(data['proof/source-after.json']);assert after['files']==source['files']
for name,raw in data.items():
    if name.startswith('source/'):
        path=name.removeprefix('source/')
        if path in source.get('fixture_files',{}):
            assert raw==subprocess.check_output(['git','-C','/Users/adalundhe/Projects/shards/.claude/worktrees/g121','show',source['upstream']+':'+path.removeprefix('vendor/shards/')]),path
        else:assert raw==subprocess.check_output(['git','show',source['commit']+':'+path]),path
if version in ['v1','v2']:
    assert data['proof/build.exit'].strip()==b'101'
    if version=='v1':assert b'no matching package named `triomphe` found' in data['proof/build.stderr']
    else:assert b'managed-borrowed-nowait/focused.rs' in data['proof/build.stderr']+data['proof/build.jsonl']
    assert not any(name.startswith('proof/test-') for name in data)
    selected=passed=failed=0;checks={'build':101};binary=None
else:
    summary=json.loads(data['proof/summary.json'])
    assert summary['source']==source['commit'] and summary['production_pin']==source['production_pin']
    assert summary['build_exit']==0 and summary['selected']==4 and summary['passes']==4 and summary['failed']==0 and summary['ignored']==0
    lock=json.loads(data['proof/lock-receipt.json']);assert lock['all_registry_versions_and_checksums_match_exact_upstream_seed']
    for row in summary['tests']:
        raw=data['proof/'+row['log']]
        assert row['passed'] and row['reaped'] and row['exit']==0
        assert hashlib.sha256(raw).hexdigest()==row['log_sha256']
        assert b'running 1 test' in raw and b'test result: ok. 1 passed; 0 failed; 0 ignored;' in raw
    assert len(summary['tests'])==4 and not any(row['target']=='real' for row in summary['tests'])
    assert summary['source_after']==source['files']
    selected=passed=4;failed=0;checks={'build':0};binary=summary['binaries']
(base/('source-and-logs-'+version+'.json')).write_text(json.dumps(manifest,indent=2)+'\n')
receipt={'source':source['commit'],'production_pin':source['production_pin'],'archive':meta,'safe_regular_members_verified':len(data),'all_member_hashes_verified':True,'source_matches_signed_commit':True,'api_code_matches_signed_source':True,'private_fixture_files':source.get('fixture_files',{}),'only_private_generated_source_file':'vendor/shards/Cargo.lock','selected':selected,'passed':passed,'failed':failed,'checks':checks,'binaries':binary,'real_vm_tests_executed':0,'native_gpu_effects':0,'local_free_kib':free,'transport_command':command,'transport_stderr':result.stderr}
(base/('proof-receipt-'+version+'.json')).write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({key:value for key,value in receipt.items() if key not in ['transport_command','transport_stderr','binaries']}))
