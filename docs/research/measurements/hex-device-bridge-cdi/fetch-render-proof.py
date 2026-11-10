import base64,hashlib,io,json,pathlib,shlex,subprocess,sys,tarfile
version=sys.argv[1];assert version in ['v1','v2','v3']
evidence=pathlib.Path('docs/research/measurements/hex-device-bridge-cdi')
meta=json.loads((evidence/('render-light-'+version)/'proof-meta.json').read_text())
free=int(subprocess.check_output(['df','-k','.'],text=True).splitlines()[1].split()[3]);assert free>=30*1024*1024,free
remote='''import base64,pathlib
raw=pathlib.Path(PATH).read_bytes()
text=base64.b64encode(raw).decode()
for i in range(0,len(text),1000):print(text[i:i+1000])
'''.replace('PATH',repr(meta['path']))
cmd=['/Users/adalundhe/.local/bin/kiln','run','--on','d1323898b255a3ed435eb83fa0f7b0cf','--needs','cpus=1,mem=4g,disk=12g','--no-sync','--timeout','120','--','nice -n 10 python3 -c '+shlex.quote(remote)]
r=subprocess.run(cmd,capture_output=True,text=True);assert r.returncode==0,r.stderr
blob=base64.b64decode(''.join(r.stdout.split()),validate=True)
assert len(blob)==meta['bytes'] and hashlib.sha256(blob).hexdigest()==meta['sha256']
archive=evidence/('proof-render-'+version+'.tar.gz')
with archive.open('xb') as f:f.write(blob)
dest=evidence/('raw-render-'+version);assert not dest.exists();dest.mkdir()
with tarfile.open(fileobj=io.BytesIO(blob),mode='r:gz') as archive:
 members=archive.getmembers();assert len(members)==meta['members'];names=set();contents={}
 for m in members:
  path=pathlib.PurePosixPath(m.name)
  assert m.isfile() and not path.is_absolute() and '..' not in path.parts and m.name not in names
  names.add(m.name);contents[m.name]=archive.extractfile(m).read()
 manifest=json.loads(contents['MANIFEST.json']);assert set(manifest)==set(contents)-{'MANIFEST.json'}
 for name,row in manifest.items():
  raw=contents[name];assert len(raw)==row['bytes'] and hashlib.sha256(raw).hexdigest()==row['sha256']
 for name,raw in contents.items():
  p=dest/name;p.parent.mkdir(parents=True,exist_ok=True)
  with p.open('xb') as f:f.write(raw)
summary=json.loads(contents['proof/summary.json'])
for row in summary['tests']:
 raw=contents['proof/'+row['log']]
 assert hashlib.sha256(raw).hexdigest()==row['log_sha256'] and row['reaped']
 assert b'running 1 test' in raw and row['passed'] and b'test result: ok. 1 passed;' in raw
assert summary['failed']==0 and summary['passes']==(27 if version=='v1' else 26)
repo=pathlib.Path.cwd()
for name,rel in [('bundle/device_edits.rs','crates/hex-core/src/bundle/device_edits.rs'),('bundle/device_edits/tests.rs','crates/hex-core/src/bundle/device_edits/tests.rs'),('cdi.rs','crates/hex-core/src/cdi.rs'),('cdi/tests.rs','crates/hex-core/src/cdi/tests.rs')]:
 retained=contents['retained-source/crates/hex-core/src/'+name]
 want=subprocess.check_output(['git','show',summary['source']+':'+rel]);assert retained==want,(name,version)
for short,rel in [('bundle','crates/hex-core/src/bundle.rs'),('lib','crates/hex-core/src/lib.rs')]:
 assert contents['proof/source-before/'+short+'.rs']==subprocess.check_output(['git','show',summary['source']+':'+rel])
receipt={'version':version,'archive':meta,'source':summary['source'],'local_free_kib':free,'safe_members_verified':len(contents),'all_member_hashes_verified':True,'all_test_logs_pass_and_reap_verified':True,'source_matches_signed_commit':True,'checks':summary['checks'],'passes':summary['passes'],'failures':summary['failed'],'transport_command':cmd,'transport_stderr':r.stderr}
(evidence/('render-light-'+version)/'proof-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({k:v for k,v in receipt.items() if k not in ['transport_command','transport_stderr']}))
