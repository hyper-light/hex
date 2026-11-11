import io,tarfile,gzip,subprocess,json,hashlib,pathlib,shlex
repo=pathlib.Path.cwd();evidence=repo/'docs/research/measurements/hex-device-bridge-cdi/render-light-v2'
commit=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()
assert commit=='74f889be4a7022163e7cf1f7f6edddfebfcf1f73'
free=int(subprocess.check_output(['df','-k','.'],text=True).splitlines()[1].split()[3]);assert free>=30*1024*1024,free
raw=subprocess.check_output(['git','archive','--format=tar',commit])
extra={'driver.py':(evidence/'driver.py').read_bytes(),'baseline-bundle.rs':subprocess.check_output(['git','show','d4d96083:crates/hex-core/src/bundle.rs'])}
files={name:hashlib.sha256((repo/name).read_bytes()).hexdigest() for name in ['crates/hex-core/src/bundle.rs','crates/hex-core/src/bundle/device_edits.rs','crates/hex-core/src/bundle/device_edits/tests.rs','crates/hex-core/src/cdi.rs','crates/hex-core/src/cdi/tests.rs','crates/hex-core/src/lib.rs']}
manifest={'commit':commit,'files':files,'baseline_bundle_sha256':hashlib.sha256(extra['baseline-bundle.rs']).hexdigest(),'driver_sha256':hashlib.sha256(extra['driver.py']).hexdigest(),'policy':'cbd39284937d024640c56db6f8d8ef2bd9b0ee24','rules_sha256':'df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3'}
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
root=pathlib.Path.home()/'.cache/hex-cdi-render-01a1c640-74f889be-v2'
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
(evidence/'transfer.json').write_text(json.dumps({'command':cmd,'archive_bytes':len(blob),'archive_sha256':sha,'local_free_kib':free,'exit_code':r.returncode,'stdout':r.stdout.decode(errors='replace'),'stderr':r.stderr.decode(errors='replace')},indent=2)+'\n')
print(json.dumps({'exit':r.returncode,'bytes':len(blob),'sha256':sha,'stdout':r.stdout.decode(errors='replace')[-1500:],'stderr':r.stderr.decode(errors='replace')[-1500:]}))
raise SystemExit(r.returncode)
