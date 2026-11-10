import gzip,hashlib,io,json,pathlib,tarfile
root=pathlib.Path.home()/'.cache/hex-device-offer-01a1c640-a69d3426-v2'
proof=root/'proof';source=root/'source'
data={('proof/'+p.name):p.read_bytes() for p in proof.iterdir() if p.is_file()}
manifest=json.loads((proof/'source.json').read_text())
registration=b'\n#[path = "device_offer_tests.rs"]\nmod device_offer_tests;\n'
parent='crates/hex-node/src/scheduler/tests.rs'
after={name:hashlib.sha256((source/name).read_bytes()).hexdigest() for name in manifest['files']}
for name,digest in manifest['files'].items():
    if name!=parent:assert after[name]==digest,name
assert (source/parent).read_bytes()==(proof/'parent-before.rs').read_bytes()+registration
assert sorted(str(p.relative_to(source)) for p in source.rglob('*') if p.is_file())==sorted(manifest['files'])
data['proof/source-after.json']=(json.dumps({'files':after,'only_changed':parent,'all_others_match':True},indent=2)+'\n').encode()
selected=json.loads((source/'docs/hex-device-offer/pinned-sources.json').read_text())['sha256']
selected['crates/hex-node/src/scheduler/device_offer_tests.rs']=manifest['files']['crates/hex-node/src/scheduler/device_offer_tests.rs']
for path,digest in selected.items():
    raw=(source/path).read_bytes()
    if path!=parent:assert hashlib.sha256(raw).hexdigest()==digest,path
    data['source/'+path]=raw
data['driver.py']=(root/'driver.py').read_bytes()
data['driver-clippy-only.py']=(root/'driver-clippy-only.py').read_bytes()
for p in (root/'proof-clippy').iterdir():
    if p.is_file():data['proof-clippy/'+p.name]=p.read_bytes()
manifest={name:{'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()} for name,raw in sorted(data.items())}
data['MANIFEST.json']=(json.dumps(manifest,indent=2)+'\n').encode()
out=io.BytesIO()
with gzip.GzipFile(fileobj=out,mode='wb',mtime=0) as gz,tarfile.open(fileobj=gz,mode='w') as archive:
    for name,raw in sorted(data.items()):
        member=tarfile.TarInfo(name);member.size=len(raw);member.mode=0o644;archive.addfile(member,io.BytesIO(raw))
blob=out.getvalue();retained=root/'retained';retained.mkdir()
p=retained/'proof-v1.tar.gz'
with p.open('xb') as f:f.write(blob)
print(json.dumps({'path':str(p),'bytes':len(blob),'sha256':hashlib.sha256(blob).hexdigest(),'members':len(data)}))
