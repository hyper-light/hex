# oci_setup.ps1: readies jupiter for native plan step 4's runs, everything as plain processes (no services)
# under one folder, Docker Desktop untouched (the lead's ask to Ada, 2026-11-04):
#   - containerd's official Windows release (containerd.exe, ctr.exe, containerd-shim-runhcs-v1.exe),
#     run with its own root, state and pipe; the image pulled through its windows snapshotter, whose
#     layer folders are the image's layerFolders for Hex's tests and bench (research note §3);
#   - Docker's static Windows engine (dockerd.exe, docker.exe), run with its own data-root and pipe,
#     process isolation, the same image pulled into it.
# Each download's version and SHA-256 are written to versions.txt beside the record. Prints the layers
# ('HEX_WINDOWS_LAYERS=...') and writes them to layers.txt. Stop everything with oci_teardown.ps1.
#
#   powershell -NoProfile -File oci_setup.ps1 [-Root <dir>] [-Image <ref>]
param(
    [string]$Root = "$env:USERPROFILE\.cache\hex-native-s4",
    [string]$Image = "mcr.microsoft.com/windows/nanoserver:ltsc2025"
)
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
New-Item -ItemType Directory -Force -Path $Root | Out-Null
$Ctrd = "\\.\pipe\hex-native-s4-containerd"
$Dock = "npipe:////./pipe/hex-native-s4-docker"
$versions = Join-Path $Root "versions.txt"

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
tar.exe -xzf $tgz -C $cbin
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
$d = Start-Process -PassThru -WindowStyle Hidden -FilePath "$Root\docker\dockerd.exe" `
    -ArgumentList "--data-root", "$Root\docker-data", "--exec-root", "$Root\docker-exec", "-H", $Dock, `
    "--pidfile", "$Root\dockerd.pid.engine", "--exec-opt", "isolation=process" `
    -RedirectStandardError "$Root\dockerd.log"
$d.Id | Set-Content "$Root\dockerd.pid"
for ($i = 0; $i -lt 60; $i++) { & "$Root\docker\docker.exe" -H $Dock version *> $null; if ($?) { break }; Start-Sleep 1 }
& "$Root\docker\docker.exe" -H $Dock version | Add-Content $versions

# The image, into both stores.
& ctr -a $Ctrd images pull --platform windows/amd64 $Image
& "$Root\docker\docker.exe" -H $Dock pull $Image

# The image's layer folders: a prepared snapshot's mount names its parents, topmost first.
$chain = (& ctr -a $Ctrd images ls --quiet) | Select-Object -First 1
$top = (& ctr -a $Ctrd snapshots --snapshotter windows ls) | Select-String -Pattern "Committed" |
    ForEach-Object { ($_ -split '\s+')[0] }
$key = "hex-native-s4-layers"
& ctr -a $Ctrd snapshots --snapshotter windows rm $key 2>$null
# The topmost committed snapshot is the one no other committed snapshot names as its parent.
$parents = (& ctr -a $Ctrd snapshots --snapshotter windows ls) | ForEach-Object { ($_ -split '\s+')[1] }
$head = $top | Where-Object { $parents -notcontains $_ } | Select-Object -First 1
$mount = (& ctr -a $Ctrd snapshots --snapshotter windows prepare --mounts $key $head) -join " "
& ctr -a $Ctrd snapshots --snapshotter windows rm $key
"image $Image chain $chain head $head mount $mount" | Add-Content $versions
$m = [regex]::Match($mount, 'parentLayerPaths=(\[[^\]]*\])')
if (-not $m.Success) { throw "no parentLayerPaths in ctr's mount: $mount" }
$layers = ($m.Groups[1].Value | ConvertFrom-Json) -join ";"
$layers | Set-Content (Join-Path $Root "layers.txt")
"HEX_WINDOWS_LAYERS=$layers"
