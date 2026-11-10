import base64,hashlib,io,json,re,subprocess,tarfile,tomllib
from pathlib import Path
lead=Path.home()/'.cache/hex-lead';repo=lead/'worktrees/scheduler-live';dest=lead/'borrowed-contract-windows-independent-v1';dest.mkdir(exist_ok=False)
ps=r"""
$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop'
$base='C:\Users\corph\.cache\hex-windows-device-attachment\root-borrowed-8e5703b1-'
$files=[ordered]@{};$digests=[ordered]@{}
foreach ($tag in @('v1','v2','v3')) {
 $root=$base+$tag
 $r=[IO.File]::ReadAllText($root+'\result.json')|ConvertFrom-Json
 if ($r.source -ne '8e5703b13c8649c8cfbc95d6650163c173fad4ca' -or -not $r.source_unchanged -or $r.temp_entries -ne 0 -or $r.exit -ne $(if($tag -eq 'v3'){0}else{1})) { throw 'result identity/outcome mismatch' }
 foreach ($phase in $r.phases) {
  $expected=if($tag -eq 'v1' -and $phase.name -eq 'canonical-borrowed'){101}else{0}
  if($phase.exit -ne $expected -or -not $phase.reaped -or $phase.timed_out){throw 'phase exit/reaping mismatch'}
  foreach($stream in @('stdout','stderr')) {
   $leaf=$phase.name+'.'+$stream+'.log';$property=$stream+'_sha256'
   if((Get-FileHash -LiteralPath (Join-Path $root $leaf) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $phase.$property){throw 'phase log changed'}
  }
 }
 $leaves=@('result.json','run.json','source-hashes.json')+@($r.phases|ForEach-Object{$_.name+'.stdout.log';$_.name+'.stderr.log'})
 if($tag -eq 'v1'){$leaves+=@('upstream.lock','resolved.lock')}
 foreach($leaf in $leaves) {
  $key=$tag+'/'+$leaf;$b=[IO.File]::ReadAllBytes((Join-Path $root $leaf));$files[$key]=[Convert]::ToBase64String($b);$digests[$key]=(Get-FileHash -LiteralPath (Join-Path $root $leaf) -Algorithm SHA256).Hash.ToLowerInvariant()
 }
}
$original=$base+'v1';$source=$original+'\source';$m=[IO.File]::ReadAllText($original+'\source-hashes.json')|ConvertFrom-Json;$n=0
foreach($p in $m.PSObject.Properties){if((Get-FileHash -LiteralPath (Join-Path $source $p.Name) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $p.Value){throw 'post source changed'};$n++}
if(@(Get-ChildItem -LiteralPath $source -Recurse -File -Force|Where-Object{$_.FullName -ne ($source+'\vendor\shards\Cargo.lock')}).Count -ne $n){throw 'source count changed'}
foreach($p in $r.binaries.PSObject.Properties){if((Get-FileHash -LiteralPath $p.Value.path -Algorithm SHA256).Hash.ToLowerInvariant() -ne $p.Value.sha256){throw 'reused binary changed'}}
[Console]::WriteLine((@{files=$files;sha256=$digests;source_files_verified=$n}|ConvertTo-Json -Depth 6 -Compress))
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
source=json.loads((rawdir/'v1/source-hashes.json').read_text());revision='8e5703b13c8649c8cfbc95d6650163c173fad4ca'
data=subprocess.check_output(['git','archive','--format=tar',revision],cwd=repo)
with tarfile.open(fileobj=io.BytesIO(data),mode='r:') as t:signed={m.name:hashlib.sha256(t.extractfile(m).read()).hexdigest() for m in t.getmembers() if m.isfile()}
assert source==signed
for tag in ['v2','v3']: assert json.loads((rawdir/tag/'source-hashes.json').read_text())==source
registry=lambda b:{(p['name'],p['version'],p['checksum']) for p in tomllib.loads(b.decode())['package'] if p.get('source','').startswith('registry+')}
assert registry((rawdir/'v1/resolved.lock').read_bytes())<=registry((rawdir/'v1/upstream.lock').read_bytes())
read=lambda n:(rawdir/n).read_text(errors='replace')
assert '1 passed; 7 failed; 0 ignored;' in read('v1/canonical-borrowed.stdout.log') and 'InvalidInput' in read('v1/canonical-borrowed.stderr.log')
assert '8 passed; 0 failed; 0 ignored;' in read('v2/canonical-borrowed.stdout.log')
assert '0 passed; 0 failed; 0 ignored;' in read('v2/owned-zero.stdout.log')
assert '1 passed; 0 failed; 0 ignored;' in read('v3/public-lifetime.stdout.log') and 'dead_target_process_refusal=true' in read('v3/public-lifetime.stderr.log')
assert '1 passed; 0 failed; 0 ignored;' in read('v3/native-handles.stdout.log')
counts=re.search(r'rounds=64 before=(\d+) after=(\d+) all_rounds_equal=true',read('v3/native-handles.stderr.log'));assert counts and counts[1]==counts[2]
buffer=io.BytesIO()
with tarfile.open(fileobj=buffer,mode='w:gz') as t:
 for p in sorted(rawdir.rglob('*')):
  if p.is_file():
   b=p.read_bytes();m=tarfile.TarInfo(str(p.relative_to(rawdir)));m.size=len(b);m.mtime=0;t.addfile(m,io.BytesIO(b))
proof=buffer.getvalue();(dest/'proof.tar.gz').write_bytes(proof)
receipt={'source':revision,'post_sources_equal_signed_git':True,'source_files':len(source),'registry_subset_of_exact_seed':True,'selected_windows_borrowed':10,'windows_handle_rounds':64,'handles_before':int(counts[1]),'handles_after':int(counts[2]),'owned_zero_windows_selected':0,'owned_max_windows_run':False,'proof_sha256':hashlib.sha256(proof).hexdigest(),'proof_bytes':len(proof),'collector_exit':code,'collector_reaped':c.returncode is not None,'sha256':obj['sha256']}
(dest/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps({k:v for k,v in receipt.items() if k!='sha256'},indent=2))
