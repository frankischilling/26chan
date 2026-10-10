# All callbacks operate on the same owned job. A root exit is never tree proof.
function Complete-OwnedBrowserTree {
    param([scriptblock]$RootExited, [scriptblock]$TreeExited, [scriptblock]$WaitForExit,
        [scriptblock]$Kill, [scriptblock]$ElapsedMilliseconds, [Collections.IDictionary]$State)
    $result = if ($null -ne $State) { $State } else { [ordered]@{ forced_cleanup = $false; tree_exited = $false; cleanup_verified = $false } }
    $graceDeadline = $null
    if (-not (& $TreeExited)) {
        $rootHasExited = & $RootExited
        $remaining = [Math]::Max(0, 5000 - (& $ElapsedMilliseconds))
        if ($rootHasExited -and $remaining -gt 0) {
            # Give job accounting a short grace within the existing budget.
            # The wait's return value alone never establishes zero processes.
            $graceStarted = & $ElapsedMilliseconds
            $grantedGrace = [int][Math]::Min(250, [Math]::Max(0, 5000 - $graceStarted))
            if ($grantedGrace -gt 0) {
                $graceDeadline = $graceStarted + $grantedGrace
                $null = & $WaitForExit $grantedGrace
            }
        }
        if (-not (& $TreeExited)) {
            $result.forced_cleanup = $true
            & $Kill
            $remaining = [Math]::Max(0, 5000 - (& $ElapsedMilliseconds))
            if ($remaining -gt 0) { $null = & $WaitForExit ([int]$remaining) }
        }
    }
    $result.tree_exited = [bool](& $TreeExited)
    # Include the accounting read itself in the grace and overall deadlines.
    $finishedAt = & $ElapsedMilliseconds
    $graceOnTime = $null -eq $graceDeadline -or $finishedAt -le $graceDeadline
    $result.cleanup_verified = $result.tree_exited -and $graceOnTime -and $finishedAt -lt 5000
    return $result
}
