param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[a-z][a-z0-9-]{0,63}$')]
    [string]$Phase
)

$ErrorActionPreference = 'Stop'
$visualSnapshot = [ordered]@{ phase = $Phase; utc = [DateTime]::UtcNow.ToString('o') }
# Only aggregates leave the process: no endpoints, PIDs, command lines or files.
try {
    $visualConnections = @(Get-NetTCPConnection -ErrorAction Stop)
    $visualStates = [ordered]@{}
    foreach ($visualGroup in ($visualConnections | Group-Object State | Sort-Object Name)) {
        $visualStates[$visualGroup.Name] = $visualGroup.Count
    }
    $visualSnapshot.tcp_states = $visualStates
} catch { $visualSnapshot.tcp_states = $null }
try {
    $visualOperatingSystem = Get-CimInstance Win32_OperatingSystem -ErrorAction Stop
    $visualSnapshot.free_memory_kib = [long]$visualOperatingSystem.FreePhysicalMemory
    $visualSnapshot.total_memory_kib = [long]$visualOperatingSystem.TotalVisibleMemorySize
} catch { $visualSnapshot.free_memory_kib = $null; $visualSnapshot.total_memory_kib = $null }
try {
    $visualPool = Get-Counter '\Memory\Pool Nonpaged Bytes' -ErrorAction Stop
    $visualSnapshot.nonpaged_pool_bytes = [long]$visualPool.CounterSamples[0].CookedValue
} catch { $visualSnapshot.nonpaged_pool_bytes = $null }
$visualProcessCounts = [ordered]@{}
foreach ($visualName in @('chrome', 'chrome-headless-shell', 'node', 'board-public')) {
    $visualProcessCounts[$visualName] = @(Get-Process -Name $visualName -ErrorAction SilentlyContinue).Count
}
$visualSnapshot.process_counts = $visualProcessCounts
$visualSnapshot | ConvertTo-Json -Depth 3 -Compress
