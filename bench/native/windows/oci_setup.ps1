# oci_setup.ps1: readies jupiter for native plan step 4's runs, everything as plain processes (no services)
# under one folder, Docker Desktop untouched (the lead's ask to Ada, 2026-11-04):
#   - containerd's official Windows release (containerd.exe, ctr.exe, containerd-shim-runhcs-v1.exe),
#     run with its own root, state and pipe; the image pulled through its windows snapshotter, whose
#     layer folders are the image's layerFolders for Hex's tests and bench (research note §3);
#   - Docker's static Windows engine (dockerd.exe, docker.exe), run with its own data-root and pipe,
#     process isolation, the same image pulled into it.
# Each download's version and SHA-256 are written to versions.txt beside the record. Prints the layers
# ('HEX_WINDOWS_LAYERS=...') and writes them to layers.txt, and a mismatched image's (Server 2019's) to
# mismatch-layers.txt ('HEX_WINDOWS_MISMATCH_LAYERS=...'). Stop everything with oci_teardown.ps1.
#
#   powershell -NoProfile -File oci_setup.ps1 [-Root <dir>] [-Image <ref>]
param(
    [string]$Root = "$env:USERPROFILE\.cache\hex-native-s4",
    [string]$Image = "mcr.microsoft.com/windows/nanoserver:ltsc2025",
    [string]$Mismatch = "mcr.microsoft.com/windows/nanoserver:ltsc2019"
)
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Ctrd = "\\.\pipe\hex-native-s4-containerd"
$Dock = "npipe:////./pipe/hex-native-s4-docker"
$versions = Join-Path $Root "versions.txt"

function Must([string]$what) { if ($LASTEXITCODE -ne 0) { throw "$what exited $LASTEXITCODE" } }

function Fetch([string]$url, [string]$to) {
    if (-not (Test-Path $to)) { Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $to }
    "$url sha256=$((Get-FileHash -Algorithm SHA256 $to).Hash)" | Add-Content $versions
}

# containerd: the latest release's Windows archive, its published checksum checked.
$tag = (Invoke-RestMethod -UseBasicParsing "https://api.github.com/repos/containerd/containerd/releases/latest").tag_name
$v = $tag.TrimStart("v")
$tgz = Join-Path $Root "containerd-$v-windows-amd64.tar.gz"
Fetch "https://github.com/containerd/containerd/releases/download/$tag/containerd-$v-windows-amd64.tar.gz" $tgz
# GitHub serves the .sha256sum as application/octet-stream, so Content is a byte[], read as ASCII here.
$sum = (Invoke-WebRequest -UseBasicParsing "https://github.com/containerd/containerd/releases/download/$tag/containerd-$v-windows-amd64.tar.gz.sha256sum").Content
if ($sum -is [byte[]]) { $sum = [Text.Encoding]::ASCII.GetString($sum) }
if (-not ([string]$sum).ToLower().Contains((Get-FileHash -Algorithm SHA256 $tgz).Hash.ToLower())) {
    throw "containerd $v's archive doesn't match its published sha256"
}
$cbin = Join-Path $Root "containerd"
New-Item -ItemType Directory -Force -Path $cbin | Out-Null
# System32's bsdtar by path: under kiln's Git Bash, PATH finds GNU tar first, which reads "C:" as a remote host.
& "$env:SystemRoot\System32\tar.exe" -xzf $tgz -C $cbin
foreach ($exe in "containerd.exe", "ctr.exe", "containerd-shim-runhcs-v1.exe") {
    if (-not (Test-Path (Join-Path $cbin "bin\$exe"))) { throw "containerd $v's archive has no bin\$exe" }
}
$env:PATH = "$cbin\bin;$env:PATH"
$c = Start-Process -PassThru -WindowStyle Hidden -FilePath "$cbin\bin\containerd.exe" `
    -ArgumentList "--root", "$Root\ctrd\root", "--state", "$Root\ctrd\state", "--address", $Ctrd `
    -RedirectStandardError "$Root\containerd.log"
$c.Id | Set-Content "$Root\containerd.pid"
for ($i = 0; $i -lt 60 -and -not (Test-Path $Ctrd); $i++) { Start-Sleep -Seconds 1 }
& ctr -a $Ctrd version | Add-Content $versions

# Docker's static engine: the newest stable zip listed.
$index = (Invoke-WebRequest -UseBasicParsing "https://download.docker.com/win/static/stable/x86_64/").Content
$zip = ([regex]::Matches($index, 'docker-(\d+\.\d+\.\d+)\.zip') | ForEach-Object { $_.Groups[1].Value } |
    Sort-Object { [version]$_ } | Select-Object -Last 1)
$dz = Join-Path $Root "docker-$zip.zip"
Fetch "https://download.docker.com/win/static/stable/x86_64/docker-$zip.zip" $dz
Expand-Archive -Force -Path $dz -DestinationPath $Root
Remove-Item -ErrorAction SilentlyContinue "$Root\dockerd.pid.engine"
$d = Start-Process -PassThru -WindowStyle Hidden -FilePath "$Root\docker\dockerd.exe" `
    -ArgumentList "--data-root", "$Root\docker-data", "--exec-root", "$Root\docker-exec", "-H", $Dock, `
    "--pidfile", "$Root\dockerd.pid.engine", "--exec-opt", "isolation=process" `
    -RedirectStandardError "$Root\dockerd.log" -RedirectStandardOutput "$Root\dockerd.out.log"
$d.Id | Set-Content "$Root\dockerd.pid"
# The pipe, not `docker version`: a redirected native error is terminating under ErrorActionPreference Stop.
$DockPipe = "\\.\pipe\hex-native-s4-docker"
# dockerd restores HNS's networks before it listens (22 s on jupiter, 2026-11-06, longer after an abrupt
# end), so the wait is on its pipe or its exit, bounded at 300 s.
for ($i = 0; $i -lt 300 -and -not (Test-Path $DockPipe) -and -not $d.HasExited; $i++) { Start-Sleep -Seconds 1 }
if ($d.HasExited) { throw "dockerd exited $($d.ExitCode) before listening: see $Root\dockerd.log and dockerd.out.log" }
if (-not (Test-Path $DockPipe)) { throw "dockerd made no pipe in 300 s: see $Root\dockerd.log and dockerd.out.log" }
& "$Root\docker\docker.exe" -H $Dock version | Add-Content $versions
Must "docker version"

# The image, into both stores.
& ctr -a $Ctrd images pull --platform windows/amd64 $Image
Must "ctr images pull"
& "$Root\docker\docker.exe" -H $Dock pull $Image
Must "docker pull"

# An image's layer folders, from its own containerd namespace: a prepared snapshot's mount names its
# parents, topmost first, and the topmost committed snapshot is the one no other names as its parent.
function Layers([string]$ns, [string]$img, [string]$file, [string]$name) {
    $ls = (& ctr -a $Ctrd -n $ns snapshots --snapshotter windows ls)
    Must "ctr snapshots ls"
    $top = $ls | Select-String -Pattern "Committed" | ForEach-Object { ($_ -split '\s+')[0] }
    $parents = $ls | ForEach-Object { ($_ -split '\s+')[1] }
    $head = $top | Where-Object { $parents -notcontains $_ } | Select-Object -First 1
    $key = "hex-native-s4-layers"
    cmd /c "ctr -a $Ctrd -n $ns snapshots --snapshotter windows rm $key 2>nul"
    $mount = (& ctr -a $Ctrd -n $ns snapshots --snapshotter windows prepare --mounts $key $head) -join " "
    Must "ctr snapshots prepare"
    & ctr -a $Ctrd -n $ns snapshots --snapshotter windows rm $key
    Must "ctr snapshots rm"
    "image $img namespace $ns head $head mount $mount" | Add-Content $versions
    # ctr prints the mounts as JSON; the parents are one option's JSON array, topmost first.
    $opt = (($mount | ConvertFrom-Json) | ForEach-Object { $_.Options }) |
        Where-Object { $_ -like "parentLayerPaths=*" } | Select-Object -First 1
    if (-not $opt) { throw "no parentLayerPaths in ctr's mount: $mount" }
    $layers = ($opt.Substring("parentLayerPaths=".Length) | ConvertFrom-Json) -join ";"
    if (-not $layers) { throw "ctr's mount names no parent layers: $mount" }
    $layers | Set-Content (Join-Path $Root $file)
    "$name=$layers"
}
Layers "default" $Image "layers.txt" "HEX_WINDOWS_LAYERS"

# An image this host doesn't run, for the version refusal: Server 2019's, which no Windows 11 runs
# process-isolated (version-compatibility.md, research note §7). Into containerd only, its own namespace.
& ctr -a $Ctrd -n hex-mismatch images pull --platform windows/amd64 $Mismatch
Must "ctr images pull (mismatch)"
Layers "hex-mismatch" $Mismatch "mismatch-layers.txt" "HEX_WINDOWS_MISMATCH_LAYERS"
