#requires -Version 7.0
$ErrorActionPreference='Stop'
if (-not $IsWindows) { throw 'Native Windows required.' }
Add-Type -Path (Join-Path $PSScriptRoot 'owned-process.cs')
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
$node=(Get-Command node.exe -ErrorAction Stop).Source
$directory=Join-Path ([IO.Path]::GetTempPath()) ('dual-stack-job-test-' + [Guid]::NewGuid().ToString('N'))
$ownedDirectory=[IO.Path]::GetFullPath($directory)
New-Item -ItemType Directory -Path $directory | Out-Null
$child=$null; $descendant=$null
function Write-OwnedState {
  param([ValidateSet('before-parent-release','after-parent-exit','after-job-termination')][string]$Stage, $State)
  [ordered]@{
    type='owned-process-state'; schema=1; stage=$Stage
    root_exited=[bool]$State.RootExited; root_in_job=[bool]$State.RootInJob
    root_membership_observed=[bool]$State.RootMembershipObserved
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
  $after=$child.InspectOwnership($descendant)
  # An exited root can briefly remain in the native job accounting/list.
  # Use the remainder of the existing parent-exit deadline for that transition;
  # retain the exact role/membership assertion and fail if the descendant exits.
  while ($after.RootExited -and -not $after.DescendantExited -and $after.DescendantInJob -and
         $after.AccountingLayoutValid -and $after.UnknownRoleCount -eq 0 -and
         -not $after.RolesQualified($false) -and $watch.ElapsedMilliseconds -lt 5000) {
    Start-Sleep -Milliseconds 20
    $after=$child.InspectOwnership($descendant)
  }
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
    $item=Get-Item -LiteralPath $directory -Force
    if ([IO.Path]::GetFullPath($item.FullName) -ne $ownedDirectory -or
        [IO.Path]::GetDirectoryName($ownedDirectory) -ne ([IO.Path]::GetTempPath()).TrimEnd('\','/') -or
        $item.Name -notmatch '^dual-stack-job-test-[0-9a-f]{32}$' -or
        ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Owned test directory changed; refusing cleanup.' }
    Remove-Item -LiteralPath $ownedDirectory -Recurse -Force
  }
}
