#requires -Version 7.0
param([Parameter(Mandatory=$true)][ValidateSet('plain','randomized')][string]$Mode)
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$false
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$evidence=Join-Path $repo 'test-results/windows-pooled-transport'
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
$fixture=$null
$failure=$null
$cleaned=$false
try {
  $vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
  $install=& $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
  if ($LASTEXITCODE -ne 0 -or @($install).Count -ne 1 -or -not $install) { throw 'Installed MSVC toolchain unavailable.' }
  $devcmd=Join-Path $install 'Common7/Tools/VsDevCmd.bat'
  $lines=& cmd.exe /d /s /c "`"$devcmd`" -no_logo -arch=x64 -host_arch=x64 >nul && set"
  if ($LASTEXITCODE -ne 0) { throw 'MSVC developer environment failed.' }
  foreach ($line in $lines) {
    if ($line -match '^([^=]+)=(.*)$') { Set-Item -LiteralPath ('Env:' + $Matches[1]) -Value $Matches[2] }
  }
  $exe=Join-Path $repo 'target/debug/examples/visual-fixtures.exe'
  $fixture=Start-Process -FilePath $exe -WorkingDirectory $repo -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $evidence 'fixture.stdout.txt') -RedirectStandardError (Join-Path $evidence 'fixture.stderr.txt')
  $deadline=[Diagnostics.Stopwatch]::StartNew()
  $ready=$false
  while ($deadline.ElapsedMilliseconds -lt 20000) {
    if ($fixture.HasExited) { throw 'Owned fixture exited before listening.' }
    $listeners=@(Get-NetTCPConnection -State Listen -LocalPort 3000 -ErrorAction SilentlyContinue)
    if ($listeners.Count -gt 0) {
      if ($listeners.Count -ne 1 -or $listeners[0].OwningProcess -ne $fixture.Id -or $listeners[0].LocalAddress -ne '127.0.0.1') { throw 'Unexpected fixture listener owner.' }
      $ready=$true; break
    }
    Start-Sleep -Milliseconds 100
  }
  if (-not $ready) { throw 'Owned fixture startup deadline exceeded.' }
  & (Join-Path $PSScriptRoot 'run-probe.ps1') -Mode $Mode -FixtureRepo $repo -Batches 16
  $probeExit=$LASTEXITCODE
  $outputs=@(Get-ChildItem -LiteralPath $PSScriptRoot -Directory | Where-Object { $_.Name -like ('probe-' + $Mode + '-*') })
  if ($outputs.Count -ne 1) { throw 'Expected exactly one diagnostic evidence directory.' }
  $status=Get-Content -Raw -LiteralPath (Join-Path $outputs[0].FullName 'status.json') | ConvertFrom-Json
  if ($probeExit -ne 0 -or $status.schema -ne 1 -or $status.complete -ne $true -or $status.passed -ne $true -or $status.child_cleanup_complete -ne $true -or $status.compiler_exit -ne 0 -or $status.native_exit -ne 0 -or $status.validator_exit -ne 0 -or $status.compiler_timeout -ne $false -or $status.native_timeout -ne $false -or $status.validator_timeout -ne $false -or $null -ne $status.failure_class) { throw 'Native diagnostic did not qualify.' }
  & node (Join-Path $PSScriptRoot 'validate-output.mjs') (Join-Path $outputs[0].FullName 'output.jsonl')
  if ($LASTEXITCODE -ne 0) { throw 'Independent output validation failed.' }
} catch {
  $failure=$_.Exception
} finally {
  if ($null -ne $fixture) {
    try {
      if (-not $fixture.HasExited) { $fixture.Kill(); if (-not $fixture.WaitForExit(5000)) { throw 'Owned fixture did not exit.' } }
      $fixture.Dispose(); $cleaned=$true
    } catch { if ($null -eq $failure) { $failure=$_.Exception } }
  } else { $cleaned=$true }
  @{ schema=1; mode=$Mode; fixture_cleanup_complete=$cleaned; passed=($null -eq $failure -and $cleaned) } | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $evidence 'hosted-status.json') -Encoding utf8NoBOM
  [ordered]@{ type='pooled-hosted-cleanup'; schema=1; fixture_cleanup_complete=$cleaned; passed=($null -eq $failure -and $cleaned) } | ConvertTo-Json -Compress | Write-Output
}
if ($null -ne $failure) { throw $failure }
