import io,tarfile,gzip,subprocess,json,hashlib,pathlib,shlex
repo=pathlib.Path.cwd();evidence=repo/'docs/research/measurements/hex-device-driver/light-v1'
commit=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert commit=='6483c6f2b0cb109cd41d009aa3974962a238c1b8'
pin='5e2cfbe7e09256ff6a7d979564717cfc6d8041f6'
assert subprocess.check_output(['git','rev-parse',commit+'^'],text=True).strip()==pin
free=int(subprocess.check_output(['df','-k','.'],text=True).splitlines()[1].split()[3]);assert free>=30*1024*1024,free
raw=subprocess.check_output(['git','archive','--format=tar',commit])
files=json.loads((evidence.parent/'pinned-sources.json').read_text())['paths_sha256']
leaf='crates/hex-node/src/placement/device_binding_tests.rs'
files[leaf]=hashlib.sha256((repo/leaf).read_bytes()).hexdigest()
assert all(hashlib.sha256((repo/name).read_bytes()).hexdigest()==sha for name,sha in files.items())
extra={'driver.py':(evidence/'driver.py').read_bytes()}
manifest={'commit':commit,'production_pin':pin,'files':files,'driver_sha256':hashlib.sha256(extra['driver.py']).hexdigest(),'policy':'cbd39284937d024640c56db6f8d8ef2bd9b0ee24','rules_sha256':'df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3'}
extra['proof/source.json']=(json.dumps(manifest,indent=2)+'\n').encode()
out=io.BytesIO()
with gzip.GzipFile(fileobj=out,mode='wb',mtime=0) as gz,tarfile.open(fileobj=gz,mode='w') as dst,tarfile.open(fileobj=io.BytesIO(raw),mode='r') as src:
 for m in src.getmembers():
  if m.isdir():continue
  assert m.isfile(),m.name
  blob=src.extractfile(m).read();n=tarfile.TarInfo('source/'+m.name);n.size=len(blob);n.mode=m.mode;dst.addfile(n,io.BytesIO(blob))
 for name,blob in extra.items():
  n=tarfile.TarInfo(name);n.size=len(blob);n.mode=0o644;dst.addfile(n,io.BytesIO(blob))
blob=out.getvalue();sha=hashlib.sha256(blob).hexdigest()
remote='''import pathlib,sys,io,tarfile,hashlib,json
root=pathlib.Path.home()/'.cache/hex-device-driver-01a1c640-6483c6f2-v1'
assert not root.exists()
raw=sys.stdin.buffer.read();assert hashlib.sha256(raw).hexdigest()==EXPECTED
root.mkdir(parents=True);(root/'source-input.tar.gz').write_bytes(raw)
names=set()
with tarfile.open(fileobj=io.BytesIO(raw),mode='r:gz') as archive:
 for m in archive.getmembers():
  p=pathlib.PurePosixPath(m.name)
  assert m.isfile() and not p.is_absolute() and '..' not in p.parts and m.name not in names
  names.add(m.name);target=root/m.name;target.parent.mkdir(parents=True,exist_ok=True)
  with target.open('xb') as f:f.write(archive.extractfile(m).read())
  target.chmod(m.mode)
print(json.dumps({'root':str(root),'archive_bytes':len(raw),'archive_sha256':hashlib.sha256(raw).hexdigest(),'members':len(names)}))
'''.replace('EXPECTED',repr(sha))
cmd=['/Users/adalundhe/.local/bin/kiln','run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=4g,disk=12g','--no-sync','--stdin','--timeout','120','--','nice -n 10 python3 -c '+shlex.quote(remote)]
r=subprocess.run(cmd,input=blob,capture_output=True)
(evidence/'transfer.json').write_text(json.dumps({'command':cmd,'source':commit,'production_pin':pin,'archive_bytes':len(blob),'archive_sha256':sha,'local_free_kib':free,'exit_code':r.returncode,'stdout':r.stdout.decode(errors='replace'),'stderr':r.stderr.decode(errors='replace')},indent=2)+'\n')
print(json.dumps({'exit':r.returncode,'bytes':len(blob),'sha256':sha,'stdout':r.stdout.decode(errors='replace')[-1500:],'stderr':r.stderr.decode(errors='replace')[-1500:]}))
raise SystemExit(r.returncode)
