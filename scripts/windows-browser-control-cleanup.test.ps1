# Pure deterministic controls. No process is launched, observed or terminated.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'windows-browser-control-cleanup.ps1')
function Invoke-CleanupControl {
    param([string]$Mode)
    $clock = @{ elapsed = 0; root = $true; tree = $false; kills = 0; waits = [Collections.Generic.List[int]]::new() }
    if ($Mode -eq 'already-empty') { $clock.tree = $true }
    if ($Mode -eq 'live-root') { $clock.root = $false }
    if ($Mode -eq 'expired') { $clock.elapsed = 5000 }
    if ($Mode -eq 'near-deadline') { $clock.elapsed = 4900 }
    $result = Complete-OwnedBrowserTree -RootExited { $clock.root } -TreeExited {
        if ($Mode -eq 'unavailable') { throw 'Synthetic accounting unavailable.' }
        if ($Mode -eq 'slow-zero-read' -and $clock.tree) { $clock.elapsed = 300 }
        return $clock.tree
    } -ElapsedMilliseconds { $clock.elapsed } -Kill {
        $clock.kills++
        if ($Mode -ne 'survivor') { $clock.tree = $true }
    } -WaitForExit {
        param($milliseconds)
        $clock.waits.Add($milliseconds)
        if ($Mode -in @('settled', 'slow-zero-read')) { $clock.elapsed += 20; $clock.tree = $true; return $true }
        if ($Mode -eq 'slow-grace-zero') { $clock.elapsed = 300; $clock.tree = $true; return $true }
        if ($Mode -eq 'late-zero') { $clock.elapsed = 5001; $clock.tree = $true; return $true }
        if ($Mode -eq 'misleading-wait' -and $clock.kills -eq 0) { $clock.elapsed += 20; return $true }
        if ($clock.kills -eq 0 -or $Mode -eq 'survivor') { $clock.elapsed += $milliseconds }
        else { $clock.elapsed += 10 }
        return $clock.tree
    }
    return @{ result = $result; clock = $clock }
}
$case = Invoke-CleanupControl 'already-empty'
if (-not $case.result.cleanup_verified -or $case.result.forced_cleanup -or $case.clock.waits.Count -ne 0 -or $case.clock.kills -ne 0) { throw 'An empty owned tree was changed.' }
$case = Invoke-CleanupControl 'settled'
if (-not $case.result.cleanup_verified -or $case.result.forced_cleanup -or $case.clock.kills -ne 0 -or $case.clock.waits.Count -ne 1 -or $case.clock.waits[0] -ne 250) { throw 'Root accounting did not receive its bounded grace.' }
$case = Invoke-CleanupControl 'persistent'
if (-not $case.result.forced_cleanup -or -not $case.result.tree_exited -or $case.clock.kills -ne 1 -or $case.clock.waits.Count -ne 2 -or $case.clock.waits[0] -ne 250 -or $case.clock.waits[1] -ne 4750) { throw 'A surviving owned process was not killed within the shared budget.' }
$case = Invoke-CleanupControl 'live-root'
if (-not $case.result.forced_cleanup -or $case.clock.kills -ne 1 -or $case.clock.waits.Count -ne 1 -or $case.clock.waits[0] -ne 5000) { throw 'A live root received an accounting grace.' }
$case = Invoke-CleanupControl 'misleading-wait'
if (-not $case.result.forced_cleanup -or $case.clock.kills -ne 1 -or $case.clock.waits[1] -ne 4980) { throw 'A wait return value replaced actual job accounting.' }
foreach ($mode in @('expired', 'near-deadline', 'survivor', 'late-zero')) {
    $case = Invoke-CleanupControl $mode
    if ($case.result.cleanup_verified) { throw 'Expired cleanup evidence was admitted.' }
    if ($mode -ne 'late-zero' -and $case.clock.kills -ne 1) { throw 'A remaining owned tree escaped termination.' }
    if ($mode -eq 'near-deadline' -and ($case.clock.waits.Count -ne 1 -or $case.clock.waits[0] -ne 100)) { throw 'The cleanup deadline was extended.' }
    if ($mode -eq 'expired' -and $case.clock.waits.Count -ne 0) { throw 'An expired cleanup received a new wait.' }
}
foreach ($mode in @('slow-grace-zero', 'slow-zero-read')) {
    $case = Invoke-CleanupControl $mode
    if ($case.result.cleanup_verified -or -not $case.result.tree_exited -or $case.result.forced_cleanup -or $case.clock.kills -ne 0 -or $case.clock.waits[0] -ne 250) { throw 'Late natural-zero evidence bypassed its granted grace.' }
}
$rejected = $false
try { $null = Invoke-CleanupControl 'unavailable' }
catch { if ($_.Exception.Message -ne 'Synthetic accounting unavailable.') { throw }; $rejected = $true }
if (-not $rejected) { throw 'Missing owned-tree evidence was accepted.' }
$clock = @{ elapsed = 0; kills = 0 }
$result = Complete-OwnedBrowserTree -RootExited { $clock.elapsed = 5000; return $true } -TreeExited { return $false } -ElapsedMilliseconds { $clock.elapsed } -WaitForExit { throw 'An expired root query added a wait.' } -Kill { $clock.kills++ }
if ($clock.kills -ne 1 -or $result.cleanup_verified) { throw 'A late root query escaped strict cleanup.' }
$state = [ordered]@{ forced_cleanup = $false; tree_exited = $false; cleanup_verified = $false }
$rejected = $false
try {
    $null = Complete-OwnedBrowserTree -State $state -RootExited { $false } -TreeExited { $false } -ElapsedMilliseconds { 0 } -WaitForExit { throw 'Kill failure must not wait.' } -Kill { throw 'Synthetic termination unavailable.' }
} catch { if ($_.Exception.Message -ne 'Synthetic termination unavailable.') { throw }; $rejected = $true }
if (-not $rejected -or -not $state.forced_cleanup -or $state.cleanup_verified) { throw 'A termination failure lost its forced-cleanup evidence.' }
Write-Output '{"type":"browser-control-cleanup-test","schema":1,"passed":true}'
