
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
$root=Join-Path $env:USERPROFILE '.cache\hex-native-s4'
$report=[ordered]@{root=$root;exists=(Test-Path -LiteralPath $root)}
if(Test-Path -LiteralPath $root){
 $report.entries=@(Get-ChildItem -LiteralPath $root -Force | Select-Object Name,Mode,Length)
}
$rows=@()
foreach($leaf in @('layers.txt','mismatch-layers.txt')){
 $path=Join-Path $root $leaf
 $row=[ordered]@{name=$leaf;exists=(Test-Path -LiteralPath $path)}
 if(Test-Path -LiteralPath $path){
  $layers=[IO.File]::ReadAllText($path).Trim()
  $row.value=$layers
  $paths=@()
  foreach($layer in ($layers -split ';')){
   $paths+=@{path=$layer;exists=(Test-Path -LiteralPath $layer)}
  }
  $row.paths=$paths
 }
 $rows+=$row
}
$report.layers=$rows
[Console]::WriteLine(($report|ConvertTo-Json -Depth 6))
