#requires -Version 7.0
param(
  [Parameter(Mandatory=$true)][ValidateSet('plain','randomized')][string]$Mode,
  [Parameter(Mandatory=$true)][string]$FixtureRepo,
  [ValidateRange(1,128)][int]$Batches=16
)
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$false
if (-not $IsWindows) { throw 'This opt-in diagnostic requires Windows.' }
$output=Join-Path $PSScriptRoot ('probe-' + $Mode + '-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $output -ErrorAction Stop | Out-Null
$metadata=[ordered]@{ schema=2; profile='pooled-event-overlapped'; exchanges=6; mode=$Mode; batches=$Batches; os_version=$null; os_build=$null; fixture_sha256=$null; source_sha256=$null; parser_sha256=$null; listener_verified=$false }
$status=[ordered]@{ schema=1; phase='initial'; compiler_exit=$null; native_exit=$null; validator_exit=$null; compiler_timeout=$false; native_timeout=$false; validator_timeout=$false; child_cleanup_complete=$true; process_tree_kill_attempted=$false; cleanup_unverified_reason=$null; failure_class=$null; complete=$false; passed=$false }
$compilerProcess=$null; $nativeProcess=$null; $validatorProcess=$null
function Save-EvidenceState {
  $metadata | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $output 'metadata.json') -Encoding utf8NoBOM
  $status | ConvertTo-Json -Compress | Set-Content -LiteralPath (Join-Path $output 'status.json') -Encoding utf8NoBOM
}
function Confirm-OwnedListener {
  $current=@(Get-NetTCPConnection -State Listen -LocalPort 3000 -ErrorAction Stop)
  if ($current.Count -ne 1 -or $current[0].LocalAddress -ne '127.0.0.1' -or $current[0].OwningProcess -ne $serverId) { throw 'Owned fixture listener changed.' }
  $owner=Get-Process -Id $serverId -ErrorAction Stop
  if ($owner.StartTime -ne $serverStarted -or [IO.Path]::GetFullPath($owner.Path) -ine [IO.Path]::GetFullPath($fixture)) { throw 'Owned fixture identity changed.' }
}
Save-EvidenceState
try {
  $status.phase='identity'; Save-EvidenceState
  $source=Join-Path $PSScriptRoot 'winsock-probe.cpp'
  $metadata.source_sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $source).Hash
  $metadata.parser_sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $PSScriptRoot 'response-parser.h')).Hash
  $os=Get-CimInstance Win32_OperatingSystem
  $metadata.os_version=$os.Version; $metadata.os_build=$os.BuildNumber; Save-EvidenceState
  $repo=(Resolve-Path -LiteralPath $FixtureRepo).Path
  $fixture=Join-Path $repo 'target/debug/examples/visual-fixtures.exe'
  if (-not (Test-Path -LiteralPath (Join-Path $repo 'apps/public/examples/visual-fixtures.rs')) -or -not (Test-Path -LiteralPath $fixture)) { throw 'Expected an already-built owned visual fixture checkout.' }
  $metadata.fixture_sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $fixture).Hash; Save-EvidenceState
  $listeners=@(Get-NetTCPConnection -State Listen -LocalPort 3000 -ErrorAction Stop)
  if ($listeners.Count -ne 1 -or $listeners[0].LocalAddress -ne '127.0.0.1') { throw 'Expected one loopback-only fixture listener.' }
  $server=Get-Process -Id $listeners[0].OwningProcess -ErrorAction Stop
  if ([IO.Path]::GetFullPath($server.Path) -ine [IO.Path]::GetFullPath($fixture)) { throw 'Listener is not the specified fixture.' }
  $serverId=$server.Id; $serverStarted=$server.StartTime
  $metadata.listener_verified=$true; Save-EvidenceState
  $status.phase='toolchain'; Save-EvidenceState
  $compiler=Get-Command cl.exe -ErrorAction Stop; $node=Get-Command node.exe -ErrorAction Stop
  $exe=Join-Path $output 'winsock-probe.exe'
  $status.phase='compile'; Save-EvidenceState
  $compilerArgs=@('/nologo','/std:c++17','/EHsc','/W4','/WX',('"{0}"' -f $source),('/Fe:"{0}"' -f $exe),'/link','Ws2_32.lib')
  $compilerProcess=Start-Process -FilePath $compiler.Source -ArgumentList $compilerArgs -WorkingDirectory $output -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $output 'compiler.stdout.txt') -RedirectStandardError (Join-Path $output 'compiler.stderr.txt')
  if (-not $compilerProcess.WaitForExit(120000)) {
    $status.compiler_timeout=$true
    # cl.exe may have linker descendants. Root exit cannot certify their exit,
    # including when cl.exe exits between this timeout and the finally block.
    $status.child_cleanup_complete=$false
    $status.cleanup_unverified_reason='compiler-timeout-descendants-unverified'
    $status.passed=$false
    Save-EvidenceState
    throw 'Compiler timeout.'
  }
  $status.compiler_exit=$compilerProcess.ExitCode; Save-EvidenceState
  if ($status.compiler_exit -ne 0) { throw 'Compile failed; no connections attempted.' }
  $status.phase='listener-recheck'; Save-EvidenceState; Confirm-OwnedListener
  $status.phase='native'; Save-EvidenceState
  $stdout=Join-Path $output 'output.jsonl'
  $nativeProcess=Start-Process -FilePath $exe -ArgumentList @($Mode,[string]$Batches) -PassThru -NoNewWindow -RedirectStandardOutput $stdout -RedirectStandardError (Join-Path $output 'native.stderr.txt')
  if (-not $nativeProcess.WaitForExit(40000)) { $status.native_timeout=$true; throw 'Native hard deadline exceeded.' }
  $status.native_exit=$nativeProcess.ExitCode; Save-EvidenceState
  $status.phase='validate'; Save-EvidenceState
  $validatorArgs=@(('"{0}"' -f (Join-Path $PSScriptRoot 'validate-output.mjs')),('"{0}"' -f $stdout))
  $validatorProcess=Start-Process -FilePath $node.Source -ArgumentList $validatorArgs -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $output 'validator.stdout.json') -RedirectStandardError (Join-Path $output 'validator.stderr.txt')
  if (-not $validatorProcess.WaitForExit(15000)) { $status.validator_timeout=$true; throw 'Validator timeout.' }
  $status.validator_exit=$validatorProcess.ExitCode
  $status.complete=$true; $status.passed=($status.native_exit -eq 0 -and $status.validator_exit -eq 0); $status.phase='finished'
} catch {
  $status.failure_class=$status.phase + '-error'
  Write-Output ('Probe stopped in phase ' + $status.phase + '; failure evidence retained.')
} finally {
  # Best effort only for process trees rooted in children owned by this invocation.
  # Kill(true) requests descendant termination; HasExited/WaitForExit certify only
  # the root. Without independent descendant tracking, forced cleanup is unverified.
  foreach ($entry in @(@{ process=$compilerProcess; key='compiler_exit' },@{ process=$nativeProcess; key='native_exit' },@{ process=$validatorProcess; key='validator_exit' })) {
    if ($null -eq $entry.process) { continue }
    try {
      if (-not $entry.process.HasExited) {
        $status.child_cleanup_complete=$false
        $status.passed=$false
        if ($null -eq $status.cleanup_unverified_reason) { $status.cleanup_unverified_reason='forced-process-tree-cleanup-unverified' }
        $status.process_tree_kill_attempted=$true
        $entry.process.Kill($true)
        if (-not $entry.process.WaitForExit(5000)) { throw 'Owned child did not exit.' }
      }
      $status[$entry.key]=$entry.process.ExitCode
      $entry.process.Dispose()
    } catch { $status.child_cleanup_complete=$false; $status.passed=$false }
  }
  Save-EvidenceState
}
# Reporting is separate from qualification: preserve this original exit decision.
$qualificationExit=0
if (-not $status.complete -or -not $status.passed -or -not $status.child_cleanup_complete) { $qualificationExit=1 }
$reporter=$null
$reporterLaunched=$false
$reporterExit=$null
$reporterTimeout=$false
$reporterCleanup=$true
try {
  $reportNode=Get-Command node.exe -ErrorAction Stop
  $reportArgs=@(('"{0}"' -f (Join-Path $PSScriptRoot 'validate-output.mjs')),'--diagnostics',('"{0}"' -f $output))
  # Only the fixed-schema reporter writes inherited stdout; arbitrary stderr is retained in the artifact.
  $reporter=Start-Process -FilePath $reportNode.Source -ArgumentList $reportArgs -PassThru -NoNewWindow -RedirectStandardError (Join-Path $output 'diagnostic.stderr.txt')
  $reporterLaunched=$true
  if (-not $reporter.WaitForExit(10000)) { $reporterTimeout=$true; $reporterCleanup=$false }
  else { $reporterExit=$reporter.ExitCode }
} catch {
  Write-Output '{"type":"pooled-diagnostics-unavailable","schema":1,"code":"reporter-launch-error"}'
} finally {
  if ($null -ne $reporter) {
    try {
      if (-not $reporter.HasExited) {
        $reporterCleanup=$false
        $reporter.Kill($true)
        if (-not $reporter.WaitForExit(5000)) { throw 'Reporter root did not exit.' }
      }
      $reporterExit=$reporter.ExitCode
      $reporter.Dispose()
    } catch { $reporterCleanup=$false }
  }
  $reporterHealthy=($reporterLaunched -and -not $reporterTimeout -and $reporterCleanup -and $reporterExit -eq 0)
  $finalExit=$qualificationExit
  if (-not $reporterHealthy) { $finalExit=1 }
  [ordered]@{ type='pooled-diagnostic-reporter'; schema=1; qualification_exit=$qualificationExit; exit=$reporterExit; launched=$reporterLaunched; timeout=$reporterTimeout; cleanup_complete=$reporterCleanup; final_exit=$finalExit } | ConvertTo-Json -Compress | Write-Output
}
exit $finalExit
