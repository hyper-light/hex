import gzip,hashlib,io,json,pathlib,tarfile
root=pathlib.Path.home()/'.cache/hex-cdi-render-01a1c640-661cbd81-v1'
proof=root/'proof'
def sha(blob):return hashlib.sha256(blob).hexdigest()
data={}
for p in sorted(proof.rglob('*')):
    if p.is_file():data['proof/'+p.relative_to(proof).as_posix()]=p.read_bytes()
for rel in ['driver.py','baseline-bundle.rs']:
    data['retained-source/'+rel]=(root/rel).read_bytes()
for rel in ['crates/hex-core/src/bundle/device_edits.rs','crates/hex-core/src/bundle/device_edits/tests.rs','crates/hex-core/src/cdi.rs','crates/hex-core/src/cdi/tests.rs']:
    data['retained-source/'+rel]=(root/'source'/rel).read_bytes()
manifest={name:{'bytes':len(raw),'sha256':sha(raw)} for name,raw in sorted(data.items())}
data['MANIFEST.json']=(json.dumps(manifest,indent=2)+'\n').encode()
out=io.BytesIO()
with gzip.GzipFile(fileobj=out,mode='wb',mtime=0) as gz,tarfile.open(fileobj=gz,mode='w') as archive:
    for name,raw in sorted(data.items()):
        member=tarfile.TarInfo(name);member.size=len(raw);member.mode=0o644;archive.addfile(member,io.BytesIO(raw))
raw=out.getvalue();retained=root/'retained';retained.mkdir(exist_ok=True)
p=retained/'proof-v1.tar.gz'
with p.open('xb') as f:f.write(raw)
print(json.dumps({'path':str(p),'bytes':len(raw),'sha256':sha(raw),'members':len(data)}))
