# ROOT-only private Store notifications. This script starts no child or workload.
param(
    [Parameter(Mandatory=$true)][string]$RecordRoot,
    [Parameter(Mandatory=$true)][double]$TimeoutSeconds,
    [Parameter(Mandatory=$true)][int]$EventLimit,
    [Parameter(Mandatory=$true)][int]$BufferBytes
)
$ErrorActionPreference = 'Stop'
$watcher = $null
try {
    if ($env:HEX_COMPLETION_ROOT_AUTHORIZED -ne '1') { throw 'ROOT authorization required' }
    if ([double]::IsNaN($TimeoutSeconds) -or [double]::IsInfinity($TimeoutSeconds) -or $TimeoutSeconds -le 0 -or $EventLimit -le 0 -or $BufferBytes -le 0) {
        throw 'Explicit finite notification bounds required'
    }
    [Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
    $root = [System.IO.Path]::GetFullPath($RecordRoot)
    if (-not [System.IO.Directory]::Exists($root)) { throw 'Private Store root missing' }
    $watcher = [System.IO.FileSystemWatcher]::new($root)
    $watcher.IncludeSubdirectories = $true
    $watcher.NotifyFilter = [System.IO.NotifyFilters]::FileName -bor [System.IO.NotifyFilters]::DirectoryName
    $watcher.InternalBufferSize = $BufferBytes
    if ($watcher.InternalBufferSize -ne $BufferBytes) { throw 'Filesystem API changed the requested buffer bound' }
    $errorId = 'hex-completion-creating-error'
    Register-ObjectEvent -InputObject $watcher -EventName Error -SourceIdentifier $errorId | Out-Null
    $watcher.EnableRaisingEvents = $true
    $clock = [System.Diagnostics.Stopwatch]::StartNew()
    [Console]::WriteLine('hex-completion-creating-ready')
    [Console]::Out.Flush()
    $count = 0
    while ($clock.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        $left = $TimeoutSeconds - $clock.Elapsed.TotalSeconds
        $milliseconds = [Math]::Ceiling($left * 1000)
        if ($milliseconds -gt [int]::MaxValue) { throw 'Notification deadline exceeds API milliseconds' }
        if ($milliseconds -le 0) { break }
        $event = $watcher.WaitForChanged([System.IO.WatcherChangeTypes]::All, [int]$milliseconds)
        if (Get-Event -SourceIdentifier $errorId -ErrorAction SilentlyContinue) { throw 'Filesystem notification overflow/error' }
        if ($event.TimedOut) { break }
        $count += 1
        if ($count -gt $EventLimit) { throw 'Creating notification event budget exceeded' }
        [Console]::WriteLine('hex-completion-creating-changed')
        [Console]::Out.Flush()
    }
} catch {
    [Console]::Error.WriteLine("UNVERIFIED Creating notifications: $_")
    exit 1
} finally {
    if ($null -ne $watcher) {
        $watcher.EnableRaisingEvents = $false
        Unregister-Event -SourceIdentifier 'hex-completion-creating-error' -ErrorAction SilentlyContinue
        Remove-Event -SourceIdentifier 'hex-completion-creating-error' -ErrorAction SilentlyContinue
        $watcher.Dispose()
    }
}
