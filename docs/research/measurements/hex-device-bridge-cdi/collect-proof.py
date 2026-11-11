import gzip,hashlib,io,json,pathlib,tarfile
root=pathlib.Path.home()/'.cache/hex-cdi-01a1c640-9cb5953c-v1'
proof=root/'proof';source=root/'source'
summary=json.loads((proof/'summary.json').read_text())
assert summary['passes']==8 and summary['nonzero']==0 and summary['one_test_per_command']
assert all(value==0 for value in summary['checks'].values())
for row in summary['tests']:
    raw=(proof/row['log']).read_bytes()
    assert hashlib.sha256(raw).hexdigest()==row['log_sha256']
    assert b'running 1 test' in raw and b'test result: ok. 1 passed;' in raw
    assert row['reaped'] and row['ran_one']
data={('proof/'+p.name):p.read_bytes() for p in proof.iterdir() if p.is_file()}
for label,path in [('cdi.rs','crates/hex-core/src/cdi.rs'),('cdi-tests.rs','crates/hex-core/src/cdi/tests.rs'),('lib-private.rs','crates/hex-core/src/lib.rs')]:
    data['source/'+label]=(source/path).read_bytes()
manifest={name:{'bytes':len(blob),'sha256':hashlib.sha256(blob).hexdigest()} for name,blob in sorted(data.items())}
data['MANIFEST.json']=(json.dumps(manifest,indent=2)+'\n').encode()
out=io.BytesIO()
with gzip.GzipFile(fileobj=out,mode='wb',mtime=0) as gz,tarfile.open(fileobj=gz,mode='w') as archive:
    for name,blob in sorted(data.items()):
        member=tarfile.TarInfo(name);member.size=len(blob);member.mode=0o644;archive.addfile(member,io.BytesIO(blob))
blob=out.getvalue()
retained=root/'retained';retained.mkdir()
p=retained/'proof-v1.tar.gz'
with p.open('xb') as f:f.write(blob)
print(json.dumps({'path':str(p),'bytes':len(blob),'sha256':hashlib.sha256(blob).hexdigest(),'members':len(data)}))
