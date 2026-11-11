import gzip,hashlib,io,json,pathlib,shlex,subprocess,tarfile
repo=pathlib.Path.cwd();evidence=repo/'docs/hex-shards-venus-api/light-v2'
commit='3f2e9fb10e4caca0de5114e1dd5471540b17e217';pin='0ba6a60f582d461e65673ad736b2ac658cee9b72'
assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==commit
assert subprocess.check_output(['git','rev-parse',commit+'^'],text=True).strip()==pin
free=int(subprocess.check_output(['df','-k','.'],text=True).splitlines()[1].split()[3]);assert free>=30*1024*1024,free
raw=subprocess.check_output(['git','archive','--format=tar',commit,'vendor/shards','docs/hex-shards-venus-api'])
upstream='33df95b08ad8f95bddeaa1c48acf0c64386e0eac'
seed=subprocess.check_output(['git','-C','/Users/adalundhe/Projects/shards/.claude/worktrees/g121','show',upstream+':Cargo.lock'])
files={}
with tarfile.open(fileobj=io.BytesIO(raw),mode='r') as archive:
    for member in archive.getmembers():
        if member.isdir():continue
        assert member.isfile(),member.name
        data=archive.extractfile(member).read();assert (repo/member.name).read_bytes()==data,member.name
        files[member.name]=hashlib.sha256(data).hexdigest()
extra={'driver.py':(evidence/'driver.py').read_bytes(),'proof/upstream-Cargo.lock':seed}
manifest={'commit':commit,'production_pin':pin,'files':files,'driver_sha256':hashlib.sha256(extra['driver.py']).hexdigest(),'upstream':upstream,'upstream_lock_sha256':hashlib.sha256(seed).hexdigest(),'policy':upstream,'rules_sha256':'df2acf47cf92996e9435f9a4988f10e9e7aaa21ca952a00069c044c7ed4f13d3'}
extra['proof/source.json']=(json.dumps(manifest,indent=2)+'\n').encode()
out=io.BytesIO()
with gzip.GzipFile(fileobj=out,mode='wb',mtime=0) as gz,tarfile.open(fileobj=gz,mode='w') as dst,tarfile.open(fileobj=io.BytesIO(raw),mode='r') as src:
    for member in src.getmembers():
        if member.isdir():continue
        data=src.extractfile(member).read();entry=tarfile.TarInfo('source/'+member.name);entry.size=len(data);entry.mode=member.mode;dst.addfile(entry,io.BytesIO(data))
    for name,data in extra.items():
        entry=tarfile.TarInfo(name);entry.size=len(data);entry.mode=0o644;dst.addfile(entry,io.BytesIO(data))
blob=out.getvalue();digest=hashlib.sha256(blob).hexdigest()
remote='''import pathlib,sys,io,tarfile,hashlib,json
root=pathlib.Path.home()/'.cache/hex-shards-venus-api-01a1c640-3f2e9fb1-v2'
assert not root.exists()
raw=sys.stdin.buffer.read();assert hashlib.sha256(raw).hexdigest()==EXPECTED
root.mkdir(parents=True);(root/'source-input.tar.gz').write_bytes(raw)
names=set()
with tarfile.open(fileobj=io.BytesIO(raw),mode='r:gz') as archive:
 for member in archive.getmembers():
  path=pathlib.PurePosixPath(member.name)
  assert member.isfile() and not path.is_absolute() and '..' not in path.parts and member.name not in names
  names.add(member.name);target=root/member.name;target.parent.mkdir(parents=True,exist_ok=True)
  with target.open('xb') as f:f.write(archive.extractfile(member).read())
  target.chmod(member.mode)
print(json.dumps({'root':str(root),'archive_bytes':len(raw),'archive_sha256':hashlib.sha256(raw).hexdigest(),'members':len(names)}))
'''.replace('EXPECTED',repr(digest))
command=['/Users/adalundhe/.local/bin/kiln','run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=4g,disk=12g','--no-sync','--stdin','--timeout','120','--','nice -n 10 python3 -c '+shlex.quote(remote)]
result=subprocess.run(command,input=blob,capture_output=True)
(evidence/'transfer.json').write_text(json.dumps({'command':command,'source':commit,'production_pin':pin,'archive_bytes':len(blob),'archive_sha256':digest,'local_free_kib':free,'upstream_lock_sha256':hashlib.sha256(seed).hexdigest(),'exit':result.returncode,'stdout':result.stdout.decode(errors='replace'),'stderr':result.stderr.decode(errors='replace')},indent=2)+'\n')
print(json.dumps({'exit':result.returncode,'bytes':len(blob),'sha256':digest,'stdout':result.stdout.decode(errors='replace')[-1600:],'stderr':result.stderr.decode(errors='replace')[-1600:]}))
raise SystemExit(result.returncode)
