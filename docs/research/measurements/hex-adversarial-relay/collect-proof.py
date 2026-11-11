import gzip,hashlib,io,json,pathlib,tarfile
home=pathlib.Path.home()
roots=[('v1',home/'.cache/hex-adversarial-relay-01a1c640-dc29-v1')]+[(v,home/('.cache/hex-adversarial-relay-01a1c640-'+v)) for v in ['v2','v3','v4','v5']]
data={}
summaries={}
for version,root in roots:
    proof=root/'proof'
    summary=json.loads((proof/'summary.json').read_text())
    summaries[version]=summary
    for test in summary['tests']:
        raw=(proof/test['log']).read_bytes()
        assert hashlib.sha256(raw).hexdigest()==test['log_sha256']
        assert b'running 1 test' in raw
        assert (b'test result: ok. 1 passed;' if test['exit_code']==0 else b'1 failed;') in raw
    for p in proof.iterdir():
        assert p.is_file()
        data[version+'/proof/'+p.name]=p.read_bytes()
    data[version+'/source/adversarial_relay_tests.rs']=(root/'source/crates/hex-node/src/scheduler/adversarial_relay_tests.rs').read_bytes()
    data[version+'/source/scheduler-tests-private.rs']=(root/'source/crates/hex-node/src/scheduler/tests.rs').read_bytes()
assert summaries['v5']['passes']==3 and summaries['v5']['nonzero']==0
assert summaries['v5']['scoped_clippy_exit']==0 and summaries['v5']['scoped_rustfmt_exit']==0
assert summaries['v5']['one_test_per_command'] and all(t['reaped'] for t in summaries['v5']['tests'])
manifest={name:{'bytes':len(blob),'sha256':hashlib.sha256(blob).hexdigest()} for name,blob in sorted(data.items())}
data['MANIFEST.json']=(json.dumps(manifest,indent=2)+'\n').encode()
out=io.BytesIO()
with gzip.GzipFile(fileobj=out,mode='wb',mtime=0) as compressed, tarfile.open(fileobj=compressed,mode='w') as tar:
    for name,blob in sorted(data.items()):
        member=tarfile.TarInfo(name);member.size=len(blob);member.mode=0o644
        tar.addfile(member,io.BytesIO(blob))
blob=out.getvalue()
retained=home/'.cache/hex-adversarial-relay-01a1c640-v5/retained'
retained.mkdir(exist_ok=True)
path=retained/'proofs-v1-v5.tar.gz'
if path.exists():
    assert path.read_bytes()==blob
else:
    with path.open('xb') as f:
        f.write(blob)
print(json.dumps({'path':str(path),'archive_bytes':len(blob),'archive_sha256':hashlib.sha256(blob).hexdigest(),'members':len(data),'final_summary':summaries['v5']}))
