param(
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[a-z][a-z0-9-]{0,63}$')]
    [string]$Phase
)

$ErrorActionPreference = 'Stop'
$visualSnapshot = [ordered]@{ phase = $Phase; utc = [DateTime]::UtcNow.ToString('o') }
# Only aggregates leave the process: no endpoints, PIDs, command lines or files.
$visualConnections = $null
try {
    $visualConnections = @(Get-NetTCPConnection -ErrorAction Stop)
    $visualStates = [ordered]@{}
    foreach ($visualGroup in ($visualConnections | Group-Object State | Sort-Object Name)) {
        $visualStates[$visualGroup.Name] = $visualGroup.Count
    }
    $visualSnapshot.tcp_states = $visualStates
} catch { $visualSnapshot.tcp_states = $null }
$visualDynamicPorts = [ordered]@{}
foreach ($visualFamily in @('ipv4', 'ipv6')) {
    try {
        $visualRangeOutput = @(& "$env:SystemRoot\System32\netsh.exe" int $visualFamily show dynamicport tcp)
        if ($LASTEXITCODE -ne 0) { throw 'Dynamic port query failed' }
        $visualRangeNumbers = @($visualRangeOutput | ForEach-Object {
            if ($_ -match ':\s*([0-9]+)\s*$') { [int]$Matches[1] }
        })
        if ($visualRangeNumbers.Count -ne 2) { throw 'Dynamic port query was not recognized' }
        $visualStart = $visualRangeNumbers[0]
        $visualCount = $visualRangeNumbers[1]
        if ($visualStart -lt 1 -or $visualCount -lt 1 -or $visualStart + $visualCount -gt 65536) {
            throw 'Invalid dynamic port range'
        }
        $visualUsedPorts = $null
        if ($null -ne $visualConnections) {
            $visualFamilyNumber = if ($visualFamily -eq 'ipv4') { 2 } else { 23 }
            $visualUsedPorts = @($visualConnections | Where-Object {
                $_.LocalPort -ge $visualStart -and $_.LocalPort -lt $visualStart + $visualCount -and
                [int]([System.Net.IPAddress]::Parse($_.LocalAddress).AddressFamily) -eq $visualFamilyNumber
            } | Select-Object -ExpandProperty LocalPort -Unique).Count
        }
        $visualDynamicPorts[$visualFamily] = [ordered]@{
            start = $visualStart; count = $visualCount; observed_local_ports = $visualUsedPorts
        }
    } catch { $visualDynamicPorts[$visualFamily] = $null }
}
$visualSnapshot.dynamic_tcp_ports = $visualDynamicPorts
try {
    $visualEvents = @(Get-WinEvent -FilterHashtable @{
        LogName = 'System'; ProviderName = 'Microsoft-Windows-TCPIP'; Id = @(4227, 4231)
        StartTime = [DateTime]::Now.AddMinutes(-10)
    } -ErrorAction Stop)
    $visualSnapshot.recent_tcpip_exhaustion_events = $visualEvents.Count
} catch {
    $visualSnapshot.recent_tcpip_exhaustion_events = if ($_.FullyQualifiedErrorId -like 'NoMatchingEventsFound*') { 0 } else { $null }
}
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
$visualProcessResources = [ordered]@{}
foreach ($visualName in @('chrome', 'chrome-headless-shell', 'node', 'board-public', 'visual-fixtures')) {
    $visualProcesses = @(Get-Process -Name $visualName -ErrorAction SilentlyContinue)
    $visualProcessCounts[$visualName] = $visualProcesses.Count
    $visualHandles = 0L
    $visualPrivateBytes = 0L
    foreach ($visualProcess in $visualProcesses) {
        try {
            $visualHandles += $visualProcess.HandleCount
            $visualPrivateBytes += $visualProcess.PrivateMemorySize64
        } catch { }
    }
    $visualOwnedStates = $null
    if ($null -ne $visualConnections) {
        $visualOwnedStates = [ordered]@{}
        $visualIds = @($visualProcesses | Select-Object -ExpandProperty Id)
        foreach ($visualGroup in ($visualConnections | Where-Object { $_.OwningProcess -in $visualIds } | Group-Object State | Sort-Object Name)) {
            $visualOwnedStates[$visualGroup.Name] = $visualGroup.Count
        }
    }
    $visualProcessResources[$visualName] = [ordered]@{
        handles = $visualHandles; private_bytes = $visualPrivateBytes; tcp_states = $visualOwnedStates
    }
}
$visualSnapshot.process_counts = $visualProcessCounts
$visualSnapshot.process_resources = $visualProcessResources
$visualSnapshot | ConvertTo-Json -Depth 5 -Compress
