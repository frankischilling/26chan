#requires -Version 7.0
$ErrorActionPreference='Stop'
if (-not $IsWindows) { throw 'Native Windows required.' }
Add-Type -Path (Join-Path $PSScriptRoot 'owned-process.cs')
. (Join-Path $PSScriptRoot 'owned-process-accounting.ps1')
# Pure classifier checks use synthetic paths/counts; they prove no native role.
$system='C:\Windows\System32'
if (-not [DualStackOwnedProcess]::IsExpectedConsoleHost('C:\Windows\System32\conhost.exe',$system) -or -not [DualStackOwnedProcess]::IsExpectedConsoleHost('c:\WINDOWS\system32\CONHOST.EXE',$system)) { throw 'Exact system console-host classification failed.' }
foreach ($image in @('conhost.exe','C:\fake\conhost.exe','C:\Windows\System32\conhost.exe.evil','C:\Windows\System32\evilconhost.exe','C:\Windows\System32\sub\conhost.exe','C:\Windows\System32\..\System32\conhost.exe')) {
  if ([DualStackOwnedProcess]::IsExpectedConsoleHost($image,$system)) { throw 'Unverified console-host image admitted.' }
}
$roles=[DualStackOwnedProcess+OwnershipSnapshot]::new()
$roles.ProcessListComplete=$true; $roles.ProcessCountsConsistent=$true
$roles.EnumeratedProcesses=3; $roles.ActiveProcesses=3
$roles.RootRoleCount=1; $roles.DescendantRoleCount=1; $roles.ConsoleHostRoleCount=1
if (-not $roles.RolesQualified($true) -or $roles.RolesQualified($false)) { throw 'Role policy rejected valid synthetic ownership.' }
foreach ($field in @('ProcessListComplete','ProcessCountsConsistent')) {
  $roles.$field=$false
  if ($roles.RolesQualified($true)) { throw 'Incomplete role evidence admitted.' }
  $roles.$field=$true
}
foreach ($field in @('UnknownRoleCount','UnavailableRoleCount')) {
  $roles.$field=1
  if ($roles.RolesQualified($true)) { throw 'Unknown role evidence admitted.' }
  $roles.$field=0
}
foreach ($field in @('RootRoleCount','DescendantRoleCount','ConsoleHostRoleCount','ActiveProcesses','EnumeratedProcesses')) {
  $prior=$roles.$field; $roles.$field=$prior+1
  if ($roles.RolesQualified($true)) { throw 'Inconsistent role evidence admitted.' }
  $roles.$field=$prior
}
$roles.RootRoleCount=0; $roles.ActiveProcesses=2; $roles.EnumeratedProcesses=2
if (-not $roles.RolesQualified($false) -or $roles.RolesQualified($true)) { throw 'Post-root synthetic ownership policy failed.' }
$roles.ConsoleHostRoleCount=16; $roles.ActiveProcesses=17; $roles.EnumeratedProcesses=17
if ($roles.RolesQualified($false)) { throw 'Over-capacity role evidence admitted.' }
# Deterministic state-settlement controls. The exited root remains unavailable
# until a later complete snapshot removes it; the role policy stays unchanged.
function New-PendingRootState {
  $state=[DualStackOwnedProcess+OwnershipSnapshot]::new()
  $state.RootExited=$true; $state.RootListed=$true; $state.AccountingLayoutValid=$true
  $state.DescendantInJob=$true; $state.ProcessListComplete=$true; $state.ProcessCountsConsistent=$true
  $state.DescendantRoleCount=1; $state.ConsoleHostRoleCount=1; $state.UnavailableRoleCount=1
  $state.ActiveProcesses=3; $state.EnumeratedProcesses=3
  return $state
}
$pending=New-PendingRootState
if ($pending.RolesQualified($false)) { throw 'Pending root accounting was admitted.' }
$settled=New-PendingRootState
$settled.RootListed=$false; $settled.UnavailableRoleCount=0; $settled.ActiveProcesses=2; $settled.EnumeratedProcesses=2
$sequence=[Collections.Generic.Queue[object]]::new()
$sequence.Enqueue($pending); $sequence.Enqueue($pending); $sequence.Enqueue($settled)
$clock=@{ elapsed=4900; pauses=0 }
$observed=Wait-OwnedRootAccounting -ReadSnapshot { $sequence.Dequeue() } -ElapsedMilliseconds { $clock.elapsed } -Pause { $clock.elapsed+=20; $clock.pauses++ }
if (-not [object]::ReferenceEquals($observed,$settled) -or $clock.pauses -ne 2 -or -not $observed.RolesQualified($false)) { throw 'Delayed root accounting observation failed.' }
$clock=@{ elapsed=4980; pauses=0 }
$expired=Wait-OwnedRootAccounting -ReadSnapshot { $pending } -ElapsedMilliseconds { $clock.elapsed } -Pause { $clock.elapsed+=20; $clock.pauses++ }
if ($clock.pauses -ne 1 -or $clock.elapsed -ne 5000 -or $expired.RolesQualified($false)) { throw 'Root accounting deadline was extended or bypassed.' }
$clock=@{ elapsed=4980; pauses=0; reads=0 }
$expired=Wait-OwnedRootAccounting -ReadSnapshot {
  $clock.reads++
  if ($clock.reads -eq 1) { return $pending }
  $clock.elapsed=5001
  return $settled
} -ElapsedMilliseconds { $clock.elapsed } -Pause { $clock.elapsed+=1; $clock.pauses++ }
if ($clock.reads -ne 2 -or $expired.RolesQualified($false)) { throw 'A snapshot completing past the deadline was admitted.' }
$clock=@{ elapsed=5000; reads=0 }
$rejected=$false
try {
  $null=Wait-OwnedRootAccounting -ReadSnapshot { $clock.reads++; return $settled } -ElapsedMilliseconds { $clock.elapsed } -Pause { throw 'Expired observation was paused.' }
} catch {
  if ($_.Exception.Message -ne 'Root accounting observation deadline.') { throw }
  $rejected=$true
}
if (-not $rejected -or $clock.reads -ne 0) { throw 'An already-expired observation read or admitted evidence.' }
foreach ($finishedAt in @(5000,5001)) {
  $clock=@{ elapsed=4999; reads=0 }
  $rejected=$false
  try {
    $null=Wait-OwnedRootAccounting -ReadSnapshot { $clock.reads++; $clock.elapsed=$finishedAt; return $settled } -ElapsedMilliseconds { $clock.elapsed } -Pause { throw 'Late initial evidence was paused.' }
  } catch {
    if ($_.Exception.Message -ne 'Root accounting observation deadline.') { throw }
    $rejected=$true
  }
  if (-not $rejected -or $clock.reads -ne 1) { throw 'A late initial snapshot was admitted.' }
}
foreach ($field in @('RootExited','RootListed','AccountingLayoutValid','DescendantInJob','ProcessListComplete','ProcessCountsConsistent','UnknownRoleCount','UnavailableRoleCount')) {
  $invalid=New-PendingRootState
  if ($field -eq 'UnknownRoleCount') { $invalid.$field=1 }
  elseif ($field -eq 'UnavailableRoleCount') { $invalid.$field=2 }
  else { $invalid.$field=$false }
  $observed=Wait-OwnedRootAccounting -ReadSnapshot { $invalid } -ElapsedMilliseconds { 0 } -Pause { throw 'Unrelated or unverified role was retried.' }
  if (-not [object]::ReferenceEquals($observed,$invalid) -or $observed.RolesQualified($false)) { throw 'Unverified accounting state was accepted.' }
}
$node=(Get-Command node.exe -ErrorAction Stop).Source
$directory=Join-Path ([IO.Path]::GetTempPath()) ('dual-stack-job-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $directory | Out-Null
$child=$null; $descendant=$null
function Write-OwnedState {
  param([ValidateSet('before-parent-release','after-parent-exit','after-job-termination')][string]$Stage, $State)
  [ordered]@{
    type='owned-process-state'; schema=1; stage=$Stage
    root_exited=[bool]$State.RootExited; root_in_job=[bool]$State.RootInJob
    root_membership_observed=[bool]$State.RootMembershipObserved
    root_listed=[bool]$State.RootListed
    descendant_exited=[bool]$State.DescendantExited; descendant_in_job=[bool]$State.DescendantInJob
    accounting_layout_valid=[bool]$State.AccountingLayoutValid
    active_processes=[uint32]$State.ActiveProcesses; total_processes=[uint32]$State.TotalProcesses
    terminated_processes=[uint32]$State.TerminatedProcesses
    process_list_complete=[bool]$State.ProcessListComplete; process_counts_consistent=[bool]$State.ProcessCountsConsistent
    enumerated_processes=[uint32]$State.EnumeratedProcesses
    root_role_count=[uint32]$State.RootRoleCount; descendant_role_count=[uint32]$State.DescendantRoleCount
    console_host_role_count=[uint32]$State.ConsoleHostRoleCount
    unknown_role_count=[uint32]$State.UnknownRoleCount; unavailable_role_count=[uint32]$State.UnavailableRoleCount
  } | ConvertTo-Json -Compress | Write-Output
}
try {
  $script=Join-Path $PSScriptRoot 'owned-process-fixture.cjs'
  $child=[DualStackOwnedProcess]::Start($node,('"{0}" parent' -f $script),$directory,(Join-Path $directory 'stdout'),(Join-Path $directory 'stderr'))
  $readyPath=Join-Path $directory 'parent-ready.json'
  $watch=[Diagnostics.Stopwatch]::StartNew()
  while (-not (Test-Path -LiteralPath $readyPath) -and $watch.ElapsedMilliseconds -lt 5000) {
    if ($child.HasExited) { throw 'Synthetic parent exited before child acknowledgement.' }
    Start-Sleep -Milliseconds 20
  }
  if (-not (Test-Path -LiteralPath $readyPath)) { throw 'Synthetic child readiness timeout.' }
  $ready=Get-Content -Raw -LiteralPath $readyPath | ConvertFrom-Json
  if ($ready.schema -ne 1 -or $ready.child_ready -ne $true -or ($ready.pid -isnot [long] -and $ready.pid -isnot [int]) -or $ready.pid -le 0 -or $ready.pid -eq $child.Id) { throw 'Invalid synthetic child acknowledgement.' }
  # Keep the live process handle across parent exit, avoiding PID reuse checks.
  $descendant=Get-Process -Id $ready.pid -ErrorAction Stop
  $null=$descendant.Handle
  $before=$child.InspectOwnership($descendant)
  Write-OwnedState -Stage 'before-parent-release' -State $before
  if ($before.RootExited -or -not $before.RootInJob -or -not $before.AccountingLayoutValid -or $before.DescendantExited -or -not $before.DescendantInJob -or -not $before.RolesQualified($true)) { throw 'Synthetic child is not alive in the exact owned job.' }
  New-Item -ItemType File -Path (Join-Path $directory 'release-parent') | Out-Null
  $watch.Restart()
  while (-not $child.HasExited -and $watch.ElapsedMilliseconds -lt 5000) { Start-Sleep -Milliseconds 20 }
  if (-not $child.HasExited -or $child.ExitCode -ne 0) { throw 'Synthetic parent did not exit normally.' }
  # Reuse the release/exit deadline. A signaled root handle does not establish
  # that job accounting has removed that exact retained process yet.
  $after=Wait-OwnedRootAccounting -ReadSnapshot { $child.InspectOwnership($descendant) } -ElapsedMilliseconds { $watch.ElapsedMilliseconds } -Pause { Start-Sleep -Milliseconds 20 }
  Write-OwnedState -Stage 'after-parent-exit' -State $after
  if (-not $after.RootExited -or -not $after.AccountingLayoutValid -or $after.DescendantExited -or -not $after.DescendantInJob -or -not $after.RolesQualified($false)) { throw 'Synthetic descendant did not survive root exit in the owned job.' }
  if ($child.TreeExited -or $child.WaitForExit(100)) { throw 'Live descendant incorrectly counted as completed.' }
  $child.Kill()
  if (-not $child.WaitForExit(5000) -or -not $child.TreeExited -or -not $descendant.WaitForExit(5000)) { throw 'Owned tree failed to terminate.' }
  if ($descendant.ExitCode -ne 1) { throw 'Descendant did not exit through owned job termination.' }
  $stopped=$child.InspectOwnership($descendant)
  Write-OwnedState -Stage 'after-job-termination' -State $stopped
  if (-not $stopped.ProcessListComplete -or -not $stopped.ProcessCountsConsistent -or $stopped.ActiveProcesses -ne 0 -or $stopped.EnumeratedProcesses -ne 0) { throw 'Owned roles remain after job termination.' }
  $child.Dispose(); $child=$null
  Write-Output '{"type":"owned-process-test","schema":1,"passed":true}'
} finally {
  try {
    if ($null -ne $child) {
      try { if (-not $child.TreeExited) { $child.Kill(); if (-not $child.WaitForExit(5000)) { throw 'Test cleanup unverified.' } } }
      finally { $child.Dispose() }
    }
  } finally {
    if ($null -ne $descendant) { $descendant.Dispose() }
    Remove-Item -LiteralPath $directory -Recurse -Force
  }
}
