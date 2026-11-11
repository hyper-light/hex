import gzip,hashlib,io,json,pathlib,tarfile
root=pathlib.Path.home()/'.cache/hex-device-driver-01a1c640-6483c6f2-v1'
proof=root/'proof';source=root/'source'
data={('proof/'+p.name):p.read_bytes() for p in proof.iterdir() if p.is_file()}
for name,path in [('leaf.rs','crates/hex-node/src/placement/device_binding_tests.rs'),('placement.rs','crates/hex-node/src/placement.rs'),('placement-devices.rs','crates/hex-node/src/placement/devices.rs'),('runtime-catalog.rs','crates/hex-runtime/src/devices.rs')]:
    data['source/'+name]=(source/path).read_bytes()
data['driver.py']=(root/'driver.py').read_bytes()
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
