import hashlib,json,os,platform,signal,shutil,subprocess,sys
from pathlib import Path
lead=Path.home()/'.cache/hex-lead';base=lead/'borrowed-contract-d9d547c7-mac-v1';root=lead/'borrowed-observed-mutation-d9-v2';root.mkdir(exist_ok=False)
original=base/'source';source=root/'source';shutil.copytree(original,source)
leaf=source/'vendor/shards/crates/api/src/client.rs';before=leaf.read_bytes();needle=b'    held.check().map_err(process_failure)?;';assert before.count(needle)==1
changed=before.replace(needle,b'    let _ = &held; // ROOT private mutation: omit the final liveness guard.')
ending=b'    Ok(stream)\n}\n\n#[cfg(test)]\n#[path = "../../../docs/research/measurements/managed-borrowed-nowait/focused.rs"]';assert changed.count(ending)==1
changed=changed.replace(ending,b'    eprintln!("ROOT_forbidden_effect borrowed_stream_returned_after_target_death=true");\n'+ending)
leaf.write_bytes(changed)
(root/'mutation-client.rs').write_bytes(changed)
manifest=json.loads((base/'source-hashes.json').read_text());actual={str(p.relative_to(source)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(source.rglob('*')) if p.is_file() and str(p.relative_to(source))!='vendor/shards/Cargo.lock'}
assert {k for k in actual if actual[k]!=manifest.get(k)}=={'vendor/shards/crates/api/src/client.rs'}
short=lead/'hbo-d9-v2';short.mkdir(exist_ok=False)
rustup=lead/'rustup-scheduler-copy';tc=rustup/'toolchains/1.99.1-aarch64-apple-darwin/bin';env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','HEX_BORROWED_'))};env.update(PATH=str(tc)+os.pathsep+env['PATH'],RUSTUP_HOME=str(rustup),RUSTC=str(tc/'rustc'),RUSTDOC=str(tc/'rustdoc'),CARGO_HOME=str(base/'cache/cargo'),CARGO_TARGET_DIR=str(root/'target'),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RAYON_NUM_THREADS='1',TMPDIR=str(short))
guard=2*max(int(row.split('\t')[2]) for row in (source/'bench/native/guest/records.tsv').read_text().splitlines() if row.startswith('validation_run\t'))/1_000_000
r={'source':'d9d547c7d9211f6c42f1c2c884017a3981032889','platform':platform.platform(),'guard_seconds':guard,'changed_files':['vendor/shards/crates/api/src/client.rs'],'before_sha256':hashlib.sha256(before).hexdigest(),'after_sha256':hashlib.sha256(changed).hexdigest(),'phases':[]}
def run(name,cmd,expected):
 row={'name':name,'command':cmd};r['phases'].append(row)
 with(root/(name+'.log')).open('xb') as out:
  c=subprocess.Popen(['nice','-n','10',*cmd],cwd=source,env=env,stdout=out,stderr=subprocess.STDOUT,start_new_session=True);row['pid']=c.pid;(root/'run.json').write_text(json.dumps(r,indent=2))
  try:row['exit']=c.wait(timeout=guard)
  except subprocess.TimeoutExpired:row['timed_out']=True;os.killpg(c.pid,signal.SIGKILL);c.wait(timeout=guard);row['exit']=c.returncode
 row['reaped']=c.returncode is not None;raw=(root/(name+'.log')).read_bytes();row['sha256']=hashlib.sha256(raw).hexdigest();assert row['exit']==expected and not row.get('timed_out');return raw
try:
 raw=run('build',[str(tc/'cargo'),'test','--manifest-path','vendor/shards/Cargo.toml','-p','shards-api','--test','hex_borrowed_contract','--no-run','--message-format=json'],0)
 bins=[]
 for line in raw.splitlines():
  try:item=json.loads(line)
  except(ValueError,UnicodeDecodeError):continue
  if item.get('reason')=='compiler-artifact' and item.get('executable') and item['target']['name']=='hex_borrowed_contract':bins.append(item['executable'])
 assert len(bins)==1
 raw=run('missing-final-guard-observed',[bins[0],'--ignored','--exact','root_borrowed_target_death_before_ok_is_refused','--nocapture','--test-threads=1'],101)
 assert b'ROOT_forbidden_effect borrowed_stream_returned_after_target_death=true' in raw and b'0 passed; 1 failed; 0 ignored;' in raw
 r['forbidden_stream_return_observed']=True;r['exit']=0
except Exception as e:r.update(exit=1,error=str(e))
finally:
 r['original_source_unchanged']=all(hashlib.sha256((original/k).read_bytes()).hexdigest()==v for k,v in manifest.items())
 parent=short/'shards-tests'
 if parent.is_dir() and not list(parent.iterdir()):parent.rmdir()
 r['temp_entries']=len(list(short.iterdir()))
 if not r['original_source_unchanged'] or r['temp_entries']:r['exit']=1
 (root/'result.json').write_text(json.dumps(r,indent=2));print(json.dumps(r,indent=2),flush=True)
sys.exit(r['exit'])
