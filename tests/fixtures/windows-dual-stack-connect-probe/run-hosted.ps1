#requires -Version 7.0
param([Parameter(Mandatory=$true)][ValidateSet('plain','randomized')][string]$Mode)
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$false
if (-not $IsWindows) { throw 'Native Windows required.' }
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
# A fresh directory per invocation prevents evidence from an earlier run qualifying.
$evidence=Join-Path $repo ('test-results/windows-dual-stack-connect/' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $evidence | Out-Null
if ($env:GITHUB_ENV) { 'DUAL_STACK_EVIDENCE=' + $evidence | Add-Content -LiteralPath $env:GITHUB_ENV }
# Compiler/fixture stderr can contain arbitrary text. It stays outside uploaded evidence.
$private=Join-Path ([IO.Path]::GetTempPath()) ('dual-stack-private-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $private | Out-Null
$status=[ordered]@{ schema=1; profile='dual-stack-connect'; mode=$Mode; phase='initial'; compiler_exit=$null; native_exit=$null; validator_exit=$null; compiler_timeout=$false; native_timeout=$false; validator_timeout=$false; listener_verified=$false; listener_rechecked=$false; child_cleanup_complete=$true; forced_cleanup=$false; complete=$false; qualified=$false; failure_class=$null }
$fixture=$null; $compiler=$null; $native=$null; $validator=$null; $discovery=$null; $environment=$null
function Save-Status { $status | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $evidence 'status.json') -Encoding utf8NoBOM }
function Confirm-Listener {
  if ($fixture.HasExited) { throw 'Owned fixture exited.' }
  $listeners=@(Get-NetTCPConnection -State Listen -LocalPort 3004 -ErrorAction SilentlyContinue)
  if ($listeners.Count -ne 1 -or $listeners[0].LocalAddress -ne '127.0.0.1' -or $listeners[0].OwningProcess -ne $fixture.Id) { throw 'Unexpected media listener.' }
  $owner=Get-Process -Id $fixture.Id -ErrorAction Stop
  try {
    if ($owner.StartTime -ne $fixture.StartTime -or [IO.Path]::GetFullPath($owner.Path) -ine [IO.Path]::GetFullPath($fixtureExe)) { throw 'Owned fixture identity changed.' }
  } finally { $owner.Dispose() }
}
Save-Status
try {
  $status.phase='toolchain'; Save-Status
  Add-Type -Path (Join-Path $PSScriptRoot 'owned-process.cs')
  $vswhere=Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
  $status.phase='toolchain-discovery'; Save-Status
  $discovery=[DualStackOwnedProcess]::Start($vswhere,'-latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath',$private,(Join-Path $private 'discovery.out'),(Join-Path $private 'discovery.err'))
  if (-not $discovery.WaitForExit(20000) -or $discovery.ExitCode -ne 0) { throw 'MSVC discovery failed.' }
  $install=@(Get-Content -LiteralPath (Join-Path $private 'discovery.out') | Where-Object { $_.Trim() })
  if ($install.Count -ne 1) { throw 'MSVC unavailable.' }
  $install=$install[0].Trim()
  $devcmd=Join-Path $install 'Common7/Tools/VsDevCmd.bat'
  $status.phase='toolchain-environment'; Save-Status
  $cmd=(Get-Command cmd.exe -ErrorAction Stop).Source
  # Export only the values cl.exe/link.exe need. Never capture the full runner
  # environment, which may also contain unrelated service credentials.
  $exportScript=Join-Path $private 'export-toolchain.ps1'
  @'
foreach ($name in @('PATH','INCLUDE','LIB','LIBPATH')) {
  Write-Output ($name + '=' + [Environment]::GetEnvironmentVariable($name))
}
'@ | Set-Content -LiteralPath $exportScript -Encoding utf8NoBOM
  $pwsh=Join-Path $PSHOME 'pwsh.exe'
  $cmdArguments='/d /s /c ""' + $devcmd + '" -no_logo -arch=x64 -host_arch=x64 >nul && "' + $pwsh + '" -NoLogo -NoProfile -File "' + $exportScript + '""'
  $environment=[DualStackOwnedProcess]::Start($cmd,$cmdArguments,$private,(Join-Path $private 'environment.out'),(Join-Path $private 'environment.err'))
  if (-not $environment.WaitForExit(20000) -or $environment.ExitCode -ne 0) { throw 'MSVC environment failed.' }
  $lines=Get-Content -LiteralPath (Join-Path $private 'environment.out')
  foreach ($line in $lines) { if ($line -match '^(PATH|INCLUDE|LIB|LIBPATH)=(.*)$') { Set-Item -LiteralPath ('Env:' + $Matches[1]) -Value $Matches[2] } }
  $cl=Get-Command cl.exe -ErrorAction Stop; $node=Get-Command node.exe -ErrorAction Stop
  $source=Join-Path $PSScriptRoot 'winsock-probe.cpp'
  $exe=Join-Path $private 'dual-stack-probe.exe'
  $status.phase='compile'; Save-Status
  $compilerArgs=@('/nologo','/std:c++17','/EHsc','/W4','/WX',('"{0}"' -f $source),('/Fe:"{0}"' -f $exe),'/link','Ws2_32.lib') -join ' '
  $compiler=[DualStackOwnedProcess]::Start($cl.Source,$compilerArgs,$private,(Join-Path $private 'compile.out'),(Join-Path $private 'compile.err'))
  if (-not $compiler.WaitForExit(120000)) { $status.compiler_timeout=$true; $status.child_cleanup_complete=$false; throw 'Compiler timeout.' }
  $status.compiler_exit=$compiler.ExitCode
  if ($compiler.ExitCode -ne 0) { throw 'Compile failed.' }
  $status.phase='fixture-start'; Save-Status
  # Fail before launching if any fixture port is occupied. No other process is stopped.
  foreach ($port in @(3000,3004)) { if (@(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue).Count) { throw 'Fixture port already occupied.' } }
  $fixtureExe=Join-Path $repo 'target/debug/examples/visual-fixtures.exe'
  $fixture=[DualStackOwnedProcess]::Start($fixtureExe,'',$repo,(Join-Path $private 'fixture.out'),(Join-Path $private 'fixture.err'))
  $watch=[Diagnostics.Stopwatch]::StartNew(); $ready=$false
  while ($watch.ElapsedMilliseconds -lt 20000) {
    if ($fixture.HasExited) { throw 'Fixture exited during startup.' }
    if (@(Get-NetTCPConnection -State Listen -LocalPort 3004 -ErrorAction SilentlyContinue).Count) { Confirm-Listener; $ready=$true; break }
    Start-Sleep -Milliseconds 100
  }
  if (-not $ready) { throw 'Fixture startup timeout.' }
  $status.listener_verified=$true
  $status.phase='native'; Save-Status
  Confirm-Listener
  $native=[DualStackOwnedProcess]::Start($exe,$Mode,$private,(Join-Path $evidence 'output.jsonl'),(Join-Path $private 'native.err'))
  if (-not $native.WaitForExit(40000)) { $status.native_timeout=$true; throw 'Native hard deadline.' }
  $status.native_exit=$native.ExitCode
  Confirm-Listener; $status.listener_rechecked=$true
  $status.phase='validate'; Save-Status
  $validatorArgs=@(('"{0}"' -f (Join-Path $PSScriptRoot 'validate-output.mjs')),('"{0}"' -f (Join-Path $evidence 'output.jsonl'))) -join ' '
  $validator=[DualStackOwnedProcess]::Start($node.Source,$validatorArgs,$private,(Join-Path $evidence 'validation.json'),(Join-Path $private 'validator.err'))
  if (-not $validator.WaitForExit(15000)) { $status.validator_timeout=$true; throw 'Validator timeout.' }
  $status.validator_exit=$validator.ExitCode
  $status.complete=$true
  $status.qualified=($status.native_exit -eq 0 -and $status.validator_exit -eq 0)
  $status.phase='finished'
} catch { $status.failure_class=$status.phase + '-error'; $status.child_cleanup_complete=$false } finally {
  foreach ($child in @($discovery,$environment,$compiler,$native,$validator)) {
    if ($null -eq $child) { continue }
    try {
      if (-not $child.TreeExited) {
        # Every child is contained before it runs. A forced kill still fails qualification.
        $status.child_cleanup_complete=$false; $status.forced_cleanup=$true
        $child.Kill($true); if (-not $child.WaitForExit(5000)) { throw 'Child cleanup timeout.' }
      }
    } catch { $status.child_cleanup_complete=$false } finally {
      try { $child.Dispose() } catch { $status.child_cleanup_complete=$false }
    }
  }
  if ($null -ne $fixture) {
    try {
      if (-not $fixture.TreeExited) { $fixture.Kill(); if (-not $fixture.WaitForExit(5000)) { throw 'Fixture cleanup timeout.' } }
    } catch { $status.child_cleanup_complete=$false } finally {
      try { $fixture.Dispose() } catch { $status.child_cleanup_complete=$false }
    }
  }
  $status.qualified=($status.qualified -and $status.child_cleanup_complete -and $null -eq $status.failure_class)
  Save-Status
  # Only fixed enums, booleans and numeric exit codes reach inherited stdout.
  $status | ConvertTo-Json -Compress | Write-Output
}
if (-not $status.qualified) { exit 1 }
exit 0
