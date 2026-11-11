import io,tarfile,gzip,subprocess,json,hashlib,pathlib,shlex
repo=pathlib.Path.cwd();evidence=repo/'docs/hex-device-offer/light-v2'
commit=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert commit=='a69d34264f7eb6214ea108f964a9cdcbe25ccc3b'
pin='07c68e378b741e3ce6008e1cd612b5e9d9bc3ed2'
assert subprocess.run(['git','merge-base','--is-ancestor',pin,commit]).returncode==0
free=int(subprocess.check_output(['df','-k','.'],text=True).splitlines()[1].split()[3]);assert free>=30*1024*1024,free
raw=subprocess.check_output(['git','archive','--format=tar',commit])
files={}
with tarfile.open(fileobj=io.BytesIO(raw),mode='r') as archive:
 for member in archive.getmembers():
  if member.isdir():continue
  assert member.isfile(),member.name
  files[member.name]=hashlib.sha256(archive.extractfile(member).read()).hexdigest()
leaf='crates/hex-node/src/scheduler/device_offer_tests.rs'
files[leaf]=hashlib.sha256((repo/leaf).read_bytes()).hexdigest()
assert all(hashlib.sha256((repo/name).read_bytes()).hexdigest()==sha for name,sha in files.items())
extra={'driver.py':(evidence/'driver.py').read_bytes()}
manifest={'commit':commit,'production_pin':pin,'files':files,'driver_sha256':hashlib.sha256(extra['driver.py']).hexdigest(),'policy':'33df95b08ad8f95bddeaa1c48acf0c64386e0eac','rules_sha256':'df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3'}
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
root=pathlib.Path.home()/'.cache/hex-device-offer-01a1c640-a69d3426-v2'
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
