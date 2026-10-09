#requires -Version 7.0
$ErrorActionPreference='Stop'
if (-not $IsWindows) { throw 'Native Windows required.' }
Add-Type -Path (Join-Path $PSScriptRoot 'owned-process.cs')
$node=(Get-Command node.exe -ErrorAction Stop).Source
$directory=Join-Path ([IO.Path]::GetTempPath()) ('dual-stack-job-test-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $directory | Out-Null
$child=$null
try {
  $script=Join-Path $directory 'parent.cjs'
  # The root exits while its descendant stays alive. Root exit alone must fail
  # the wrapper's completion test, and job termination must contain the child.
  "require('node:child_process').spawn(process.execPath,['-e','setTimeout(()=>{},30000)'],{stdio:'ignore'}).unref();" | Set-Content -LiteralPath $script -Encoding utf8NoBOM
  $child=[DualStackOwnedProcess]::Start($node,('"{0}"' -f $script),$directory,(Join-Path $directory 'stdout'),(Join-Path $directory 'stderr'))
  $watch=[Diagnostics.Stopwatch]::StartNew()
  while (-not $child.HasExited -and $watch.ElapsedMilliseconds -lt 5000) { Start-Sleep -Milliseconds 20 }
  if (-not $child.HasExited -or $child.ExitCode -ne 0) { throw 'Synthetic parent did not exit normally.' }
  if ($child.TreeExited -or $child.WaitForExit(100)) { throw 'Descendant escaped completion accounting.' }
  $child.Kill()
  if (-not $child.WaitForExit(5000) -or -not $child.TreeExited) { throw 'Owned tree failed to terminate.' }
  $child.Dispose(); $child=$null
  Write-Output '{"type":"owned-process-test","schema":1,"passed":true}'
} finally {
  if ($null -ne $child) {
    try { if (-not $child.TreeExited) { $child.Kill(); if (-not $child.WaitForExit(5000)) { throw 'Test cleanup unverified.' } } }
    finally { $child.Dispose() }
  }
  Remove-Item -LiteralPath $directory -Recurse -Force
}
