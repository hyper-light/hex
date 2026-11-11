import re
import hashlib,io,json,os,platform,subprocess,sys,tarfile,time,shutil,tomllib
from pathlib import Path,PurePosixPath
root=Path(sys.argv[1]).expanduser();expected,revision,target=sys.argv[2:5]
root.mkdir(parents=True,exist_ok=False);data=sys.stdin.buffer.read()
if hashlib.sha256(data).hexdigest()!=expected:raise ValueError('archive SHA mismatch')
(root/'source.tar.gz').write_bytes(data);source=root/'source';source.mkdir()
with tarfile.open(fileobj=io.BytesIO(data),mode='r:gz') as bundle:
 for m in bundle.getmembers():
  p=PurePosixPath(m.name)
  if p.is_absolute() or '..' in p.parts or not (m.isfile() or m.isdir()):raise ValueError('nonregular member')
 bundle.extractall(source,filter='data')
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
manifest={str(p.relative_to(source)):sha(p) for p in sorted(source.rglob('*')) if p.is_file()}
(root/'source-before.json').write_text(json.dumps(manifest,indent=2)+'\n')
for file in source.rglob('*'):
 if file.is_file():file.chmod(0o444 | (file.stat().st_mode & 0o111))
for directory in sorted((p for p in source.rglob('*') if p.is_dir()),key=lambda p:len(p.parts),reverse=True):directory.chmod(0o555)
source.chmod(0o555)
for name in ['target','cargo','temp']:(root/name).mkdir()
# Seed only public immutable package archives and sparse-index metadata from ROOT's prior
# same-platform private job. The destination source, target and cargo home are all new.
# No old extracted source, target, binary, credentials or configuration are copied.
prior=Path.home()/'.cache/hex-lead/cohort-25d98e88-linux-producer-recovery-25d9/cargo'
locked=tomllib.loads((source/'Cargo.lock').read_text())
checks={(p['name'],p['version']):p['checksum'] for p in locked['package'] if p.get('source','').startswith('registry+')}
seeded=[]
if (prior/'registry/cache').exists():
 for file in sorted((prior/'registry/cache').glob('*/*.crate')):
  if file.is_symlink() or not file.is_file():raise ValueError('nonregular package cache member')
  name=file.name[:-6];key=next((key for key in checks if name==key[0]+'-'+key[1]),None)
  if key is None:continue
  digest=sha(file)
  if digest!=checks[key]:raise ValueError('cached package checksum differs from exact root lock')
  into=root/'cargo/registry/cache'/file.parent.name/file.name;into.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(file,into)
  seeded.append({'file':str(file.relative_to(prior)),'sha256':digest,'bytes':file.stat().st_size})
indexhash={}
if (prior/'registry/index').exists():
 for file in sorted((prior/'registry/index').rglob('*')):
  if file.is_symlink():raise ValueError('symlink in ROOT sparse registry cache')
  if not file.is_file():continue
  rel=file.relative_to(prior);into=root/'cargo'/rel;into.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(file,into);indexhash[str(rel)]=sha(file)
(root/'public-registry-seed.json').write_text(json.dumps({'source':str(prior),'packages':seeded,'index_hashes':indexhash,'extracted_source_or_binary_or_credentials_copied':False},indent=2)+'\n')
env={k:v for k,v in os.environ.items() if not k.startswith(('DYLD_','HEX_DEBUG_','SHARDS_DEBUG_'))}
env.update(CARGO_HOME=str(root/'cargo'),CARGO_TARGET_DIR=str(root/'target'),CARGO_BUILD_JOBS='1',RUST_TEST_THREADS='1',RAYON_NUM_THREADS='1',TMPDIR=str(root/'temp'),RUSTUP_AUTO_INSTALL='0')
if target=='mac':
 rustup=Path.home()/'.cache/hex-lead/rustup-scheduler-copy';tc=rustup/'toolchains/1.99.1-aarch64-apple-darwin/bin';cargo=tc/'cargo';rustc=tc/'rustc';rustdoc=tc/'rustdoc';env.update(PATH=str(tc)+os.pathsep+env['PATH'],RUSTUP_HOME=str(rustup))
elif target=='linux':
 tc=Path.home()/'.rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin';cargo=tc/'cargo';rustc=tc/'rustc';rustdoc=tc/'rustdoc';env.update(PATH=str(tc)+os.pathsep+env['PATH'],RUSTUP_TOOLCHAIN='1.99.1')
else:raise ValueError('unknown target')
env.update(RUSTC=str(rustc),RUSTDOC=str(rustdoc))
scope=Path('/sys/fs/cgroup')/Path(next(line.split(':',2)[2] for line in Path('/proc/self/cgroup').read_text().splitlines() if line.startswith('0::'))).relative_to('/')
limits={name:(scope/name).read_text().strip() for name in ['cpu.max','memory.max','memory.swap.max','pids.max']}
if limits!={'cpu.max':'100000 100000','memory.max':'4294967296','memory.swap.max':'0','pids.max':'256'}:raise ValueError('owned scope resource envelope differs: '+str(limits))
(root/'owned-scope-limits.json').write_text(json.dumps({'cgroup':str(scope),'limits':limits},indent=2)+'\n')
version=subprocess.check_output([str(rustc),'-Vv'],cwd=source,env=env,text=True)
if 'release: 1.99.1\n' not in version:raise ValueError('wrong Rust release')

import importlib.util
helper=source/'docs/research/measurements/hex-completion-validation/native-gate-r05/root_native_gate_baseline.py'
if sha(helper)!='d1b42b03247b336c47a6724d6640c114c89c9ceca81d521db637220585759531':raise ValueError('immutable event/reap primitives changed')
spec=importlib.util.spec_from_file_location('root_a128_event_primitives',helper);base=importlib.util.module_from_spec(spec);spec.loader.exec_module(base)
os.chdir(source)  # The shared event/reap primitive inherits this exact archived source directory.


whole_deadline=time.monotonic()+3280
relative='crates/hex-node/src/recorded.rs'
needle='self.guest().owns_record(spec)';old='self.guest().uses_books()'
if (source/relative).read_text().count(needle)!=1:raise ValueError('exact production owner call missing or duplicated')
cells={'after':(source,root/'target',root/'cargo',root/'temp')}
for cell in ['mutant','restored']:
 cellroot=root/cell;cellroot.mkdir();cellsource=cellroot/'source';shutil.copytree(source,cellsource)
 for name in ['target','temp']:(cellroot/name).mkdir()
 shutil.copytree(root/'cargo',cellroot/'cargo') # Only verified public package archives/index exist before the first Cargo.
 cells[cell]=(cellsource,cellroot/'target',cellroot/'cargo',cellroot/'temp')
mutant=cells['mutant'][0]/relative;mutant.chmod(0o644);mutant.write_text(mutant.read_text().replace(needle,old));mutant.chmod(0o444)
cellmanifests={}
for cell,(cellsource,_,_,_) in cells.items():
 before={str(p.relative_to(cellsource)):sha(p) for p in sorted(cellsource.rglob('*')) if p.is_file()}
 delta=sorted(k for k in set(manifest)|set(before) if manifest.get(k)!=before.get(k))
 if delta!=([relative] if cell=='mutant' else []):raise ValueError('unexpected cell source delta '+cell)
 cellmanifests[cell]=before;(root/(cell+'-source-before.json')).write_text(json.dumps(before,indent=2)+'\n')
result={'source':revision,'archive_sha256':expected,'scope':'ROOT shared outer owner AFTER / meaningful prior-predicate mutation / independently built restoration; pure model, no native runtime/VM/GPU','platform':platform.platform(),'rustc':version,'driver':base.birth(os.getpid()),'phases':[],'source_cell_delta':{cell:sorted(k for k in set(manifest)|set(m) if manifest.get(k)!=m.get(k)) for cell,m in cellmanifests.items()},'source_hashes':{cell:m[relative] for cell,m in cellmanifests.items()}}
(root/'event-evidence').mkdir();evidence=base.Evidence(root/'event-evidence')
shared='original_shared_create_uncertainty_keeps_outer_box_and_same_pool_reference'
phases=[('after-shared1','after','test',shared,1,0),('after-node-strict','after','clippy','',0,0),('after-production','after','production','',0,0),('mutant-shared1','mutant','test',shared,1,101),('restored-shared1','restored','test',shared,1,0)]
try:
 for name,cell,kind,selected,count,expected_code in phases:
  cellsource,celltarget,cellcargo,celltemp=cells[cell];os.chdir(cellsource);cellenv=env|{'CARGO_TARGET_DIR':str(celltarget),'CARGO_HOME':str(cellcargo),'TMPDIR':str(celltemp)}
  if kind=='test':command=['nice','-n','10',str(cargo),'test','-p','hex-node','--lib','--locked','--offline',selected,'--','--nocapture','--test-threads=1']
  elif kind=='clippy':command=['nice','-n','10',str(cargo),'clippy','-p','hex-node','--all-targets','--locked','--offline','--','-D','warnings']
  else:command=['nice','-n','10','bash','scripts/check-production.sh']
  _,observed=base.run_owned(command,cellenv,root/(name+'.stdout'),root/(name+'.stderr'),min(whole_deadline,time.monotonic()+900),evidence)
  stdout=(root/(name+'.stdout')).read_text(errors='replace');stderr=(root/(name+'.stderr')).read_text(errors='replace')
  counts=re.findall(r'test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',stdout)
  exact_completed=kind!='test' or len(counts)==1 and sum(int(x) for x in counts[0][1:])==count and int(counts[0][3])==0
  semantic=kind!='test' or (int(counts[0][1])==count if expected_code==0 and exact_completed else expected_code==101 and exact_completed and counts[0][1:]==('0','1','0') and 'shared create uncertainty lost the outer canonical Box/Pool reference' in stderr)
  row={'name':name,'cell':cell,'command':command,'observation':observed,'actual_counts':counts,'expected_exit':expected_code,'exact_completed':exact_completed,'expected_semantic_outcome':semantic}
  for stream in ('stdout','stderr'):
   path=root/(name+'.'+stream);row[stream]={'bytes':path.stat().st_size,'sha256':sha(path)}
  if kind=='test':
   paths=re.findall(r'Running unittests src/lib.rs \(([^\n]+)\)',stderr)
   if len(paths)!=1:row['binary_error']='exact test executable path not observed'
   else:
    binary=Path(paths[0]);row['binary']={'path':str(binary),'bytes':binary.stat().st_size,'sha256':sha(binary)}
  row['qualified']=observed['error'] is None and observed['reaped'] and observed['both_eof'] and observed['returncode']==expected_code and exact_completed and semantic and 'binary_error' not in row
  result['phases'].append(row);(root/'running.json').write_text(json.dumps(result,indent=2)+'\n')
  if not row['qualified']:break
finally:
 changes={};temp={}
 for cell,(cellsource,_,_,celltemp) in cells.items():
  after={str(p.relative_to(cellsource)):sha(p) for p in sorted(cellsource.rglob('*')) if p.is_file()};(root/(cell+'-source-after.json')).write_text(json.dumps(after,indent=2)+'\n');changes[cell]=sorted(k for k in set(after)|set(cellmanifests[cell]) if after.get(k)!=cellmanifests[cell].get(k));temp[cell]=len(list(celltemp.iterdir()))
 result.update(source_changes=changes,temp_entries=temp,native_invocations=0,event_errors=evidence.failures,restored_source_exact=cellmanifests['restored']==manifest)
 result['exit']=0 if len(result['phases'])==len(phases) and all(p['qualified'] for p in result['phases']) and not any(changes.values()) and not any(temp.values()) and not evidence.failures else 1
 evidence.journal.close();(root/'result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2),flush=True)
sys.exit(result['exit'])
