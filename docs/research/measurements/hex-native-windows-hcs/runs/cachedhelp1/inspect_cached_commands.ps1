$ErrorActionPreference='Stop';$ProgressPreference='SilentlyContinue'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass='BelowNormal'
$root='C:\Users\corph\.cache\hex-native-windows-hcs\cached-command-help-v1'
if(Test-Path -LiteralPath $root){throw 'owned help scope exists'}
[void][IO.Directory]::CreateDirectory($root);[void][IO.Directory]::CreateDirectory(($root+'\temp'));$env:TEMP=$root+'\temp';$env:TMP=$env:TEMP
$ctr='C:\Users\corph\.cache\hex-native-s4\containerd\bin\ctr.exe'
if((Get-FileHash -LiteralPath $ctr -Algorithm SHA256).Hash -ne '457D4A4CF07DF0ADA953A5640CA9C2EA7B18F2F16E07BFA2469983072CCEAB70'){throw 'pinned already installed ctr differs'}
$guard='C:\Users\corph\.cache\hex-native-windows-hcs\hcsllvm1-incoming\guard.cs'
if((Get-FileHash -LiteralPath $guard -Algorithm SHA256).Hash.ToLowerInvariant() -ne '25fe189bb01f69f9cb3212a3fa31633b72edce36fa367d7e4c655ab3ebe61244'){throw 'guard source differs'}
Add-Type -TypeDefinition ([IO.File]::ReadAllText($guard))
$phases=@()
foreach($item in @(@{name='images';args='images --help'},@{name='unpack';args='images unpack --help'},@{name='import';args='images import --help'},@{name='content';args='content --help'})){
 $out=$root+'\'+$item.name+'.stdout.log';$err=$root+'\'+$item.name+'.stderr.log'
 $run=[NativeWindowsGuard]::Run($ctr,$item.args,$root,$out,$err,10000)
 $phases+=@{arguments=$item.args;guard=$run;stdout=[IO.File]::ReadAllText($out);stderr=[IO.File]::ReadAllText($err);stdout_sha256=(Get-FileHash -LiteralPath $out -Algorithm SHA256).Hash.ToLowerInvariant();stderr_sha256=(Get-FileHash -LiteralPath $err -Algorithm SHA256).Hash.ToLowerInvariant()}
}
$v=@{purpose='read-only actual pinned CLI command availability; no daemon/pipe connection or mutation';phases=$phases;native_workloads=0;server_starts=0;utc=[DateTime]::UtcNow.ToString('o')};$json=$v|ConvertTo-Json -Depth 20
[IO.File]::WriteAllText(($root+'\result.json'),$json,[Text.UTF8Encoding]::new($false));[Console]::WriteLine($json)
