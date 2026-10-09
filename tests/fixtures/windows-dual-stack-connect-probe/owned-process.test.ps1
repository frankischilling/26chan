#requires -Version 7.0
$ErrorActionPreference='Stop'
if (-not $IsWindows) { throw 'Native Windows required.' }
Add-Type -Path (Join-Path $PSScriptRoot 'owned-process.cs')
$node=(Get-Command node.exe -ErrorAction Stop).Source
$directory=Join-Path ([IO.Path]::GetTempPath()) ('dual-stack-job-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $directory | Out-Null
$child=$null; $descendant=$null
function Write-OwnedState {
  param([ValidateSet('before-parent-release','after-parent-exit')][string]$Stage, $State)
  [ordered]@{
    type='owned-process-state'; schema=1; stage=$Stage
    root_exited=[bool]$State.RootExited; root_in_job=[bool]$State.RootInJob
    root_membership_observed=[bool]$State.RootMembershipObserved
    descendant_exited=[bool]$State.DescendantExited; descendant_in_job=[bool]$State.DescendantInJob
    accounting_layout_valid=[bool]$State.AccountingLayoutValid
    active_processes=[uint32]$State.ActiveProcesses; total_processes=[uint32]$State.TotalProcesses
    terminated_processes=[uint32]$State.TerminatedProcesses
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
  if ($before.RootExited -or -not $before.RootInJob -or -not $before.AccountingLayoutValid -or $before.DescendantExited -or -not $before.DescendantInJob -or $before.ActiveProcesses -ne 2) { throw 'Synthetic child is not alive in the exact owned job.' }
  New-Item -ItemType File -Path (Join-Path $directory 'release-parent') | Out-Null
  $watch.Restart()
  while (-not $child.HasExited -and $watch.ElapsedMilliseconds -lt 5000) { Start-Sleep -Milliseconds 20 }
  if (-not $child.HasExited -or $child.ExitCode -ne 0) { throw 'Synthetic parent did not exit normally.' }
  $after=$child.InspectOwnership($descendant)
  Write-OwnedState -Stage 'after-parent-exit' -State $after
  if (-not $after.RootExited -or -not $after.AccountingLayoutValid -or $after.DescendantExited -or -not $after.DescendantInJob -or $after.ActiveProcesses -ne 1) { throw 'Synthetic descendant did not survive root exit in the owned job.' }
  if ($child.TreeExited -or $child.WaitForExit(100)) { throw 'Live descendant incorrectly counted as completed.' }
  $child.Kill()
  if (-not $child.WaitForExit(5000) -or -not $child.TreeExited -or -not $descendant.WaitForExit(5000)) { throw 'Owned tree failed to terminate.' }
  if ($descendant.ExitCode -ne 1) { throw 'Descendant did not exit through owned job termination.' }
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
