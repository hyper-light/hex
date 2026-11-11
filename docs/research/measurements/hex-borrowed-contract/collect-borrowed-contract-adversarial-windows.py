import base64,hashlib,io,json,re,subprocess,tarfile,tomllib
from pathlib import Path
lead=Path.home()/'.cache/hex-lead';repo=lead/'worktrees/scheduler-live';dest=lead/'borrowed-contract-windows-adversarial-independent-v1';dest.mkdir(exist_ok=False)
ps=r"""
$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$base='C:\Users\corph\.cache\hex-windows-device-attachment\root-borrowed-'
$files=[ordered]@{};$digests=[ordered]@{};$counts=[ordered]@{}
foreach($tag in @('17c99c58','0116e44b','d9d547c7')){
 $root=$base+$tag+'-v1';$r=[IO.File]::ReadAllText($root+'\result.json')|ConvertFrom-Json
 $expectedExit=if($tag -eq 'd9d547c7'){0}else{1}
 if(-not $r.source.StartsWith($tag) -or -not $r.source_unchanged -or $r.temp_entries -ne 0 -or $r.exit -ne $expectedExit){throw 'result mismatch'}
 foreach($phase in $r.phases){
  $expected=if($tag -ne 'd9d547c7' -and $phase.name -eq 'new-test-clippy'){101}else{0}
  if($phase.exit -ne $expected -or -not $phase.reaped -or $phase.timed_out){throw 'phase/reaping mismatch'}
  foreach($stream in @('stdout','stderr')){
   $leaf=$phase.name+'.'+$stream+'.log';$property=$stream+'_sha256'
   if((Get-FileHash -LiteralPath (Join-Path $root $leaf) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $phase.$property){throw 'log changed'}
  }
 }
 $source=$root+'\source';$m=[IO.File]::ReadAllText($root+'\source-hashes.json')|ConvertFrom-Json;$n=0
 foreach($property in $m.PSObject.Properties){if((Get-FileHash -LiteralPath (Join-Path $source $property.Name) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $property.Value){throw 'post source changed'};$n++}
 if(@(Get-ChildItem -LiteralPath $source -Recurse -File -Force|Where-Object{$_.FullName -ne ($source+'\vendor\shards\Cargo.lock')}).Count -ne $n){throw 'source count changed'}
 $counts[$tag]=$n
 foreach($property in $r.binaries.PSObject.Properties){if((Get-FileHash -LiteralPath $property.Value.path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $property.Value.sha256){throw 'binary changed'}}
 $leaves=@('result.json','run.json','source-hashes.json','upstream.lock','resolved.lock')+@($r.phases|ForEach-Object{$_.name+'.stdout.log';$_.name+'.stderr.log'})
 foreach($leaf in $leaves){$key=$tag+'/'+$leaf;$bytes=[IO.File]::ReadAllBytes((Join-Path $root $leaf));$files[$key]=[Convert]::ToBase64String($bytes);$digests[$key]=(Get-FileHash -LiteralPath (Join-Path $root $leaf) -Algorithm SHA256).Hash.ToLowerInvariant()}
}
[Console]::WriteLine((@{files=$files;sha256=$digests;source_files_verified=$counts}|ConvertTo-Json -Depth 6 -Compress))

"""
args=[str(Path.home()/'.local/bin/kiln'),'run','--on','f766f09294f2e17e3b8a631ffd4e6a4b','--needs','cpus=1,mem=1g','--no-sync','--quiet','--','powershell -NoProfile -NonInteractive -EncodedCommand '+base64.b64encode(ps.encode('utf-16-le')).decode()]
with (dest/'cli.log').open('xb') as out:
 c=subprocess.Popen(args,stdout=out,stderr=subprocess.STDOUT);(dest/'submission.json').write_text(json.dumps({'pid':c.pid,'argv':args},indent=2));code=c.wait()
(dest/'cli.exit').write_text(str(code));assert code==0
obj=json.loads((dest/'cli.log').read_text());rawdir=dest/'raw';rawdir.mkdir()
for name,b64 in obj['files'].items():
 assert name.count('/')==1 and not name.startswith('/') and '..' not in name
 b=base64.b64decode(b64,validate=True);assert hashlib.sha256(b).hexdigest()==obj['sha256'][name]
 p=rawdir/name;p.parent.mkdir(exist_ok=True);p.write_bytes(b)
registry=lambda b:{(p['name'],p['version'],p['checksum']) for p in tomllib.loads(b.decode())['package'] if p.get('source','').startswith('registry+')}
revisions={}
for tag in ['17c99c58','0116e44b','d9d547c7']:
 r=json.loads((rawdir/tag/'result.json').read_text());revision=subprocess.check_output(['git','rev-parse',r['source']],cwd=repo,text=True).strip();revisions[tag]=revision
 data=subprocess.check_output(['git','archive','--format=tar',revision],cwd=repo)
 with tarfile.open(fileobj=io.BytesIO(data),mode='r:') as t:signed={m.name:hashlib.sha256(t.extractfile(m).read()).hexdigest() for m in t.getmembers() if m.isfile()}
 assert json.loads((rawdir/tag/'source-hashes.json').read_text())==signed
 assert registry((rawdir/tag/'resolved.lock').read_bytes())<=registry((rawdir/tag/'upstream.lock').read_bytes())
read=lambda n:(rawdir/n).read_text(errors='replace')
assert 'single_element_loop' in read('17c99c58/new-test-clippy.stderr.log')
assert 'undocumented_unsafe_blocks' in read('0116e44b/new-test-clippy.stderr.log')
for name in ['finite-overflow','post-handshake-death','native-handles']: assert '1 passed; 0 failed; 0 ignored;' in read('d9d547c7/'+name+'.stdout.log')
assert 'accepted_connections=0' in read('d9d547c7/finite-overflow.stderr.log')
assert 'target_reaped_before_ok=true exact_exited_refusal=true' in read('d9d547c7/post-handshake-death.stderr.log')
handlelog=read('d9d547c7/native-handles.stderr.log')
counts=re.search(r'rounds=64 before=(\d+) after=(\d+) all_rounds_equal=true',handlelog);assert counts and counts[1]==counts[2]
cold=re.search(r'borrowed_windows_cold_handles before=(\d+) after=(\d+) equal=(true|false)',handlelog);assert cold
from collections import Counter
phases=dict(Counter(re.findall(r'borrowed_zero_ready phase=(\w+)',handlelog)))
buffer=io.BytesIO()
with tarfile.open(fileobj=buffer,mode='w:gz') as t:
 for p in sorted(rawdir.rglob('*')):
  if p.is_file():
   b=p.read_bytes();m=tarfile.TarInfo(str(p.relative_to(rawdir)));m.size=len(b);m.mtime=0;t.addfile(m,io.BytesIO(b))
proof=buffer.getvalue();(dest/'proof.tar.gz').write_bytes(proof)
receipt={'source':revisions['d9d547c7'],'revisions':revisions,'post_sources_equal_signed_git':True,'source_files':obj['source_files_verified'],'registry_subset_of_exact_seed':True,'new_windows_selected':3,'windows_handle_rounds':64,'handles_before':int(counts[1]),'handles_after':int(counts[2]),'cold_handles_before':int(cold[1]),'cold_handles_after':int(cold[2]),'cold_equal':cold[3]=='true','cold_delta_uncertain':cold[3]!='true','zero_returned_phase_counts':phases,'phase_counts_are_returned_error_tags_only':True,'proof_sha256':hashlib.sha256(proof).hexdigest(),'proof_bytes':len(proof),'collector_exit':code,'collector_reaped':c.returncode is not None,'sha256':obj['sha256']}
(dest/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps({k:v for k,v in receipt.items() if k!='sha256'},indent=2))
