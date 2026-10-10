# Observation only. This never admits a snapshot or changes role classification.
# A signaled root and disappearing console hosts can remain briefly unavailable
# in job accounting. Prior qualified counts bound observation, not identity proof.
function Wait-OwnedRootAccounting {
  param($Before, [scriptblock]$ReadSnapshot, [scriptblock]$ElapsedMilliseconds, [scriptblock]$Pause)
  if ($null -eq $Before -or $Before.RootExited -or -not $Before.RootInJob -or
      -not $Before.AccountingLayoutValid -or $Before.DescendantExited -or
      -not $Before.DescendantInJob -or -not $Before.RolesQualified($true)) {
    throw 'Root accounting observation requires qualified live ownership.'
  }
  if ((& $ElapsedMilliseconds) -ge 5000) { throw 'Root accounting observation deadline.' }
  $state = & $ReadSnapshot
  if ((& $ElapsedMilliseconds) -ge 5000) { throw 'Root accounting observation deadline.' }
  while ($state.RootExited -and $state.AccountingLayoutValid -and
      -not $state.DescendantExited -and $state.DescendantInJob -and
      $state.ProcessListComplete -and $state.ProcessCountsConsistent -and
      $state.RootRoleCount -eq 0 -and $state.DescendantRoleCount -eq 1 -and
      $state.UnknownRoleCount -eq 0 -and $state.UnavailableRoleCount -ge 1 -and
      $state.TotalProcesses -eq $Before.TotalProcesses -and
      $state.EnumeratedProcesses -le $Before.EnumeratedProcesses -and
      $state.ConsoleHostRoleCount -le $Before.ConsoleHostRoleCount -and
      $state.UnavailableRoleCount -le ([int]$state.RootListed + $Before.ConsoleHostRoleCount - $state.ConsoleHostRoleCount) -and
      $state.EnumeratedProcesses -le 16 -and $state.ActiveProcesses -eq $state.EnumeratedProcesses -and
      $state.EnumeratedProcesses -eq ($state.UnavailableRoleCount + $state.DescendantRoleCount + $state.ConsoleHostRoleCount) -and
      (& $ElapsedMilliseconds) -lt 5000) {
    & $Pause
    if ((& $ElapsedMilliseconds) -ge 5000) { return $state }
    $next = & $ReadSnapshot
    if ((& $ElapsedMilliseconds) -ge 5000) { return $state }
    $state = $next
  }
  return $state
}
