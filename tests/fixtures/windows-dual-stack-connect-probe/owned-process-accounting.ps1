# Observation only. This never admits a snapshot or changes role classification.
# An exited process can remain briefly listed in its job after its handle signals.
function Wait-OwnedRootAccounting {
  param([scriptblock]$ReadSnapshot, [scriptblock]$ElapsedMilliseconds, [scriptblock]$Pause)
  if ((& $ElapsedMilliseconds) -ge 5000) { throw 'Root accounting observation deadline.' }
  $state = & $ReadSnapshot
  if ((& $ElapsedMilliseconds) -ge 5000) { throw 'Root accounting observation deadline.' }
  while ($state.RootExited -and $state.RootListed -and $state.AccountingLayoutValid -and
      -not $state.DescendantExited -and $state.DescendantInJob -and
      $state.ProcessListComplete -and $state.ProcessCountsConsistent -and
      $state.RootRoleCount -eq 0 -and $state.DescendantRoleCount -eq 1 -and
      $state.UnknownRoleCount -eq 0 -and $state.UnavailableRoleCount -eq 1 -and
      $state.EnumeratedProcesses -le 16 -and $state.ActiveProcesses -eq $state.EnumeratedProcesses -and
      $state.EnumeratedProcesses -eq (1 + $state.DescendantRoleCount + $state.ConsoleHostRoleCount) -and
      (& $ElapsedMilliseconds) -lt 5000) {
    & $Pause
    if ((& $ElapsedMilliseconds) -ge 5000) { return $state }
    $next = & $ReadSnapshot
    if ((& $ElapsedMilliseconds) -ge 5000) { return $state }
    $state = $next
  }
  return $state
}
