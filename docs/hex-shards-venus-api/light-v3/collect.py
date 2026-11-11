import gzip,hashlib,io,json,pathlib,tarfile
root=pathlib.Path.home()/'.cache/hex-shards-venus-api-01a1c640-3f2e9fb1-v3'
proof=root/'proof';source=root/'source'
manifest=json.loads((proof/'source.json').read_text())
after={path:hashlib.sha256((source/path).read_bytes()).hexdigest() for path in manifest['files']}
assert after==manifest['files']
extra=set(str(p.relative_to(source)) for p in source.rglob('*') if p.is_file())-set(after)
assert extra=={'vendor/shards/Cargo.lock'},extra
data={'proof/'+p.name:p.read_bytes() for p in proof.iterdir() if p.is_file()}
data['proof/source-after.json']=(json.dumps({'files':after,'only_private_generated_file':'vendor/shards/Cargo.lock'},indent=2)+'\n').encode()
data['proof/private-Cargo.lock']=(source/'vendor/shards/Cargo.lock').read_bytes()
for path in manifest['files']:
    if path.startswith('vendor/shards/crates/api/') or path in manifest.get('fixture_files',{}):
        data['source/'+path]=(source/path).read_bytes()
data['driver.py']=(root/'driver.py').read_bytes()
members={name:{'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()} for name,raw in sorted(data.items())}
data['MANIFEST.json']=(json.dumps(members,indent=2)+'\n').encode()
out=io.BytesIO()
with gzip.GzipFile(fileobj=out,mode='wb',mtime=0) as gz,tarfile.open(fileobj=gz,mode='w') as archive:
    for name,raw in sorted(data.items()):
        member=tarfile.TarInfo(name);member.size=len(raw);member.mode=0o644;archive.addfile(member,io.BytesIO(raw))
raw=out.getvalue();retained=root/'retained';retained.mkdir();p=retained/'proof.tar.gz'
with p.open('xb') as f:f.write(raw)
print(json.dumps({'path':str(p),'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'members':len(data)}))
