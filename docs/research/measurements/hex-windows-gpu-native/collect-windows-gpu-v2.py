import base64,hashlib,io,json,subprocess,tarfile
from pathlib import Path
lead=Path.home()/'.cache/hex-lead';repo=lead/'worktrees/scheduler-live';dest=lead/'windows-gpu-native-independent-v2';dest.mkdir(exist_ok=False)
ps=r"""$ProgressPreference='SilentlyContinue';$ErrorActionPreference='Stop';$root='C:\Users\corph\.cache\hex-windows-device-attachment\root-gpu-9190738c-v2';$r=[IO.File]::ReadAllText($root+'\result.json')|ConvertFrom-Json;if($r.exit_code-ne0-or-not$r.actual_gpu_required-or-not$r.source_unchanged-or$r.remaining_temp_entries-ne0){throw 'native GPU result failed'};$m=[IO.File]::ReadAllText($root+'\source-hashes.json')|ConvertFrom-Json;$source=$root+'\source';$n=0;foreach($p in $m.PSObject.Properties){if((Get-FileHash -LiteralPath (Join-Path $source $p.Name) -Algorithm SHA256).Hash.ToLowerInvariant()-ne$p.Value){throw 'source changed'};$n++};if(@(Get-ChildItem -LiteralPath $source -Recurse -File -Force).Count-ne$n){throw 'source count changed'};foreach($phase in $r.phases){if($phase.exit_code-ne0-or-not$phase.child_reaped){throw 'child not passed/reaped'};foreach($stream in @('stdout','stderr')){$leaf=$phase.name+'.'+$stream+'.log';$property=$stream+'_sha256';if((Get-FileHash -LiteralPath (Join-Path $root $leaf) -Algorithm SHA256).Hash.ToLowerInvariant()-ne$phase.$property){throw 'log hash mismatch'}}};$files=[ordered]@{};$hashes=[ordered]@{};foreach($leaf in @('result.json','run.json','source-hashes.json','windows-gpu.stdout.log','windows-gpu.stderr.log')){$b=[IO.File]::ReadAllBytes((Join-Path $root $leaf));$files[$leaf]=[Convert]::ToBase64String($b);$hashes[$leaf]=(Get-FileHash -LiteralPath (Join-Path $root $leaf) -Algorithm SHA256).Hash.ToLowerInvariant()};[Console]::WriteLine((@{files=$files;sha256=$hashes;source_files_verified=$n}|ConvertTo-Json -Depth 6 -Compress))"""
# Parse command only; the source/result readers never start an HCS system or a GPU program.
encoded=base64.b64encode(ps.encode('utf-16-le')).decode();args=[str(Path.home()/'.local/bin/kiln'),'run','--on','f766f09294f2e17e3b8a631ffd4e6a4b','--needs','cpus=1,mem=1g','--no-sync','--quiet','--','powershell -NoProfile -NonInteractive -EncodedCommand '+encoded]
with (dest/'cli.log').open('xb') as out:
 c=subprocess.Popen(args,stdout=out,stderr=subprocess.STDOUT);(dest/'submission.json').write_text(json.dumps({'pid':c.pid,'argv':args},indent=2)+'\n');exit_code=c.wait()
(dest/'cli.exit').write_text(str(exit_code)+'\n');assert exit_code==0
raw=(dest/'cli.log').read_text();obj=json.loads(raw)
for name,b64 in obj['files'].items():
 b=base64.b64decode(b64,validate=True);assert hashlib.sha256(b).hexdigest()==obj['sha256'][name];(dest/name).write_bytes(b)
source=json.loads((dest/'source-hashes.json').read_text());rev='9190738c332a442c91719aaa7857f9c3673a2769'
data=subprocess.check_output(['git','archive','--format=tar',rev],cwd=repo)
with tarfile.open(fileobj=io.BytesIO(data),mode='r:') as t:signed={m.name:hashlib.sha256(t.extractfile(m).read()).hexdigest() for m in t.getmembers() if m.isfile()}
assert source==signed
stderr=(dest/'windows-gpu.stderr.log').read_text(errors='replace');stdout=(dest/'windows-gpu.stdout.log').read_text(errors='replace');assert 'found Gpu { vendor: Amd' in stderr and 'sample 1:' in stderr and 'skipped:' not in stderr and '1 passed; 0 failed; 0 ignored;' in stdout
receipt={'source':rev,'source_files':obj['source_files_verified'],'all_post_sources_equal_signed_git':True,'sha256':obj['sha256'],'collector_exit':exit_code,'collector_reaped':c.returncode is not None}
(dest/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt));print(stderr[-2200:])
