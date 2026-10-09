#requires -Version 7.0
param(
  [Parameter(Mandatory=$true)][ValidateSet('serialized','overlap')][string]$Mode,
  [Parameter(Mandatory=$true)][ValidateSet('false','true')][string]$Randomize
)
$ErrorActionPreference='Stop'
$PSNativeCommandUseErrorActionPreference=$false
if (-not $IsWindows) { throw 'Native Windows required.' }
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
# A fresh directory per invocation prevents evidence from an earlier run qualifying.
$evidence=Join-Path $repo ('test-results/windows-receive-connect/' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $evidence | Out-Null
if ($env:GITHUB_ENV) { 'RECEIVE_CONNECT_EVIDENCE=' + $evidence | Add-Content -LiteralPath $env:GITHUB_ENV }
# Compiler/fixture stderr can contain arbitrary text. It stays outside uploaded evidence.
$private=Join-Path ([IO.Path]::GetTempPath()) ('receive-connect-private-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $private | Out-Null
$status=[ordered]@{ schema=1; profile='receive-connect'; mode=$Mode; randomize=$Randomize; outcome='harness-or-deadline-failure'; validation_outcome=$null; failure_kind=$null; first_failure=$null; first_native_operation_failure=$null; harness_failure=$false; parser_compiler_exit=$null; parser_exit=$null; validator_tests_exit=$null; phase='initial'; compiler_exit=$null; native_exit=$null; validator_exit=$null; compiler_timeout=$false; native_timeout=$false; validator_timeout=$false; listener_verified=$false; listener_rechecked=$false; child_cleanup_complete=$true; forced_cleanup=$false; complete=$false; qualified=$false; failure_class=$null; children=@(); compiler_diagnostics_available=$false }
$fixture=$null; $compiler=$null; $native=$null; $validator=$null; $discovery=$null; $environment=$null; $parserCompiler=$null; $parser=$null; $validatorTests=$null

function Save-Status { $status | ConvertTo-Json -Depth 5 -Compress | Set-Content -LiteralPath (Join-Path $evidence 'status.json') -Encoding utf8NoBOM }
function Export-FixtureEvidence {
  $log=Join-Path $private 'fixture.out'
  if ((Get-Item -LiteralPath $log).Length -gt 10485760) { throw 'Fixture output bound exceeded.' }
  $records=@(Get-Content -LiteralPath $log | Where-Object { $_.StartsWith('[owned-fixture-overlap] ') } | ForEach-Object { $_.Substring(24) })
  if ($records.Count -gt 128 -or @($records | Where-Object { $_.Length -gt 4096 }).Count) { throw 'Fixture record bound exceeded.' }
  [IO.File]::WriteAllLines((Join-Path $evidence 'fixture.jsonl'),[string[]]$records,[Text.UTF8Encoding]::new($false))
}
function Export-CompilerDiagnostics {
  # Never upload compiler prose, absolute paths, source snippets, or environment.
  # Read at most 64 KiB from each of four private streams; retain at most 64
  # allowlisted diagnostic records and a final artifact smaller than 16 KiB.
  $diagnostics=@(); $inputTruncated=$false; $recordsTruncated=$false
  $sources=@{
    'winsock-probe.cpp'='winsock-probe.cpp'; 'winsock-probe.obj'='winsock-probe.cpp'
    'response-parser.test.cpp'='response-parser.test.cpp'; 'response-parser.test.obj'='response-parser.test.cpp'
    'response-parser.h'='response-parser.h'; 'LINK'='linker'; 'cl'='compiler'
  }
  foreach ($logName in @('parser-compile.out','parser-compile.err','compile.out','compile.err')) {
    $path=Join-Path $private $logName
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { continue }
    $stream=[IO.File]::OpenRead($path)
    try {
      if ($stream.Length -gt 65536) { $inputTruncated=$true }
      $bytes=[byte[]]::new([int][Math]::Min($stream.Length,65536))
      $offset=0
      while ($offset -lt $bytes.Length) {
        $read=$stream.Read($bytes,$offset,$bytes.Length-$offset)
        if ($read -eq 0) { break }
        $offset+=$read
      }
      $text=[Text.Encoding]::UTF8.GetString($bytes,0,$offset)
    } finally { $stream.Dispose() }
    foreach ($line in ($text -split '\r?\n')) {
      if ($line -notmatch '^\s*(?<origin>[^\r\n]+?)\s*:\s*(?:Command line )?(?<severity>fatal error|error|warning)\s+(?<code>(?:C\d{4}|D\d{4}|LNK\d{4}))\s*:') { continue }
      $origin=$Matches.origin.Trim(); $severity=$Matches.severity.ToLowerInvariant(); $code=$Matches.code.ToUpperInvariant()
      $lineNumber=0; $columnNumber=0
      if ($origin -match '^(?<file>.+)\((?<line>\d{1,7})(?:,(?<column>\d{1,7}))?\)$') {
        $origin=$Matches.file
        $lineNumber=[int]$Matches.line
        if ($Matches.column) { $columnNumber=[int]$Matches.column }
      }
      $label=[IO.Path]::GetFileName($origin)
      if (-not $sources.ContainsKey($label)) { continue }
      if ($diagnostics.Count -ge 64) { $recordsTruncated=$true; continue }
      $diagnostics+=@([ordered]@{
        source=$sources[$label]; line=$lineNumber; column=$columnNumber
        severity=($severity -replace ' ','-'); code=$code
      })
    }
  }
  $record=[ordered]@{ schema=1; input_truncated=$inputTruncated; records_truncated=$recordsTruncated; diagnostics=$diagnostics }
  $json=$record | ConvertTo-Json -Depth 4 -Compress
  if ([Text.Encoding]::UTF8.GetByteCount($json) -gt 16384) { throw 'Sanitized compiler artifact bound exceeded.' }
  [IO.File]::WriteAllText((Join-Path $evidence 'compiler-diagnostics.json'),$json,[Text.UTF8Encoding]::new($false))
  $status.compiler_diagnostics_available=$true
}
function Invoke-EvidenceValidation {
  $status.phase='validate'; Save-Status
  $validatorArgs=@(('"{0}"' -f (Join-Path $PSScriptRoot 'validate-output.mjs')),('"{0}"' -f (Join-Path $evidence 'output.jsonl')),('"{0}"' -f (Join-Path $evidence 'fixture.jsonl'))) -join ' '
  $script:validator=[DualStackOwnedProcess]::Start($node.Source,$validatorArgs,$private,(Join-Path $evidence 'validation.json'),(Join-Path $private 'validator.err'))
  if (-not $validator.WaitForExit(15000)) { $status.validator_timeout=$true; throw 'Validator timeout.' }
  $status.validator_exit=$validator.ExitCode
  $status.complete=$true
  $validation=Get-Content -Raw -LiteralPath (Join-Path $evidence 'validation.json') | ConvertFrom-Json
  if ($validation.header.schedule -ne $Mode -or $validation.header.randomize -isnot [bool] -or $validation.header.randomize -ne ($Randomize -eq 'true')) { throw 'Validator arm mismatch.' }
  $status.qualified=($status.native_exit -eq 0 -and $status.validator_exit -eq 0 -and $validation.passed -eq $true)
  $outcomes=@('observed-native-10055','observed-native-failure','harness-or-deadline-failure','qualified-bounded-success','inconclusive')
  if ($validation.outcome -notin $outcomes) { throw 'Invalid validator outcome.' }
  $status.validation_outcome=$validation.outcome
  $status.outcome=$validation.outcome
  $status.failure_kind=$validation.failureKind
  $status.first_failure=$validation.firstFailure
  $status.first_native_operation_failure=$validation.firstNativeOperationFailure
  if ($validator.ExitCode -ne 0 -and $null -eq $status.failure_class) { $status.failure_class=$validation.outcome }
  if ($status.qualified -and $validation.outcome -ne 'qualified-bounded-success') { throw 'Validator qualification mismatch.' }
  $status.phase='finished'
}
function Confirm-Listener {
  if ($fixture.HasExited) { throw 'Owned fixture exited.' }
  $listeners=@(Get-NetTCPConnection -State Listen -LocalPort 3000 -ErrorAction SilentlyContinue)
  if ($listeners.Count -ne 1 -or $listeners[0].LocalAddress -ne '127.0.0.1' -or $listeners[0].OwningProcess -ne $fixture.Id) { throw 'Unexpected main fixture listener.' }
  $owner=Get-Process -Id $fixture.Id -ErrorAction Stop
  try {
    if ($owner.StartTime -ne $fixture.StartTime -or [IO.Path]::GetFullPath($owner.Path) -ine [IO.Path]::GetFullPath($fixtureExe)) { throw 'Owned fixture identity changed.' }
  } finally { $owner.Dispose() }
}
Save-Status
try {
  $status.phase='toolchain'; Save-Status
  Add-Type -Path (Join-Path $PSScriptRoot '../windows-dual-stack-connect-probe/owned-process.cs')
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
  $exe=Join-Path $private 'receive-connect-probe.exe'
  $parserExe=Join-Path $private 'response-parser.test.exe'
  $fixtureExe=Join-Path $repo 'target/debug/examples/visual-fixtures.exe'
  $manifest=[ordered]@{
    schema=1; profile='receive-connect'; mode=$Mode; randomize=$Randomize
    candidate=$env:CANDIDATE_SHA; runner_image=$env:ImageOS; runner_image_version=$env:ImageVersion
    runner_arch=[Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString(); runner_os=[Environment]::OSVersion.Version.ToString(); powershell=$PSVersionTable.PSVersion.ToString()
    compiler_version=$cl.Version.ToString(); node_version=$node.Version.ToString(); compiler_sha256=(Get-FileHash -LiteralPath $cl.Source -Algorithm SHA256).Hash
    build_flags='/std:c++17 /EHsc /W4 /WX'; rust_toolchain='1.94.0'; build_profile='debug'; cargo_locked=$true
    pools=20; lanes=6; starts=121; exchanges_per_pool=36; sentinel_exchanges=1; maximum_sockets=6
    streaming_delay_ms=250; retirement_gap_ms=100; work_ms=30000; cleanup_ms=35000; watchdog_ms=40000
    files=@()
  }
  $inputs=@('Cargo.toml','Cargo.lock','rust-toolchain.toml','apps/public/examples/visual-fixtures.rs',
    'tests/fixtures/windows-dual-stack-connect-probe/owned-process.cs',
    '.github/workflows/windows-receive-connect.yml')
  $inputs+=@(Get-ChildItem -LiteralPath (Join-Path $repo 'apps/public/examples/visual') -Filter '*.rs' | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName).Replace('\','/') })
  $inputs+=@(Get-ChildItem -LiteralPath $PSScriptRoot -File | ForEach-Object { [IO.Path]::GetRelativePath($repo,$_.FullName).Replace('\','/') })
  foreach ($relative in ($inputs | Sort-Object -Unique)) {
    $manifest.files+=@{ path=$relative; sha256=(Get-FileHash -LiteralPath (Join-Path $repo $relative) -Algorithm SHA256).Hash }
  }
  $manifest.files+=@{ path='target/debug/examples/visual-fixtures.exe'; sha256=(Get-FileHash -LiteralPath $fixtureExe -Algorithm SHA256).Hash }
  $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $evidence 'manifest.json') -Encoding utf8NoBOM
  $status.phase='parser-compile'; Save-Status
  $parserArgs=@('/nologo','/std:c++17','/EHsc','/W4','/WX',('"{0}"' -f (Join-Path $PSScriptRoot 'response-parser.test.cpp')),('/Fe:"{0}"' -f $parserExe)) -join ' '
  $parserCompiler=[DualStackOwnedProcess]::Start($cl.Source,$parserArgs,$private,(Join-Path $private 'parser-compile.out'),(Join-Path $private 'parser-compile.err'))
  if (-not $parserCompiler.WaitForExit(120000)) { throw 'Parser compiler timeout.' }
  $status.parser_compiler_exit=$parserCompiler.ExitCode
  if ($parserCompiler.ExitCode -ne 0) { throw 'Parser compile failed.' }
  $status.phase='parser-negative-tests'; Save-Status
  $parser=[DualStackOwnedProcess]::Start($parserExe,'',$private,(Join-Path $private 'parser.out'),(Join-Path $private 'parser.err'))
  if (-not $parser.WaitForExit(15000)) { throw 'Parser test timeout.' }
  $status.parser_exit=$parser.ExitCode
  if ($parser.ExitCode -ne 0) { throw 'Parser tests failed.' }
  $status.phase='validator-negative-tests'; Save-Status
  $testArgs='--test "' + (Join-Path $PSScriptRoot 'validate-output.test.mjs') + '"'
  $validatorTests=[DualStackOwnedProcess]::Start($node.Source,$testArgs,$private,(Join-Path $private 'validator-tests.out'),(Join-Path $private 'validator-tests.err'))
  if (-not $validatorTests.WaitForExit(15000)) { throw 'Validator tests timeout.' }
  $status.validator_tests_exit=$validatorTests.ExitCode
  if ($validatorTests.ExitCode -ne 0) { throw 'Validator tests failed.' }
  $status.phase='compile'; Save-Status
  $compilerArgs=@('/nologo','/std:c++17','/EHsc','/W4','/WX',('"{0}"' -f $source),('/Fe:"{0}"' -f $exe),'/link','Ws2_32.lib','Mswsock.lib') -join ' '
  $compiler=[DualStackOwnedProcess]::Start($cl.Source,$compilerArgs,$private,(Join-Path $private 'compile.out'),(Join-Path $private 'compile.err'))
  if (-not $compiler.WaitForExit(120000)) { $status.compiler_timeout=$true; throw 'Compiler timeout.' }
  $status.compiler_exit=$compiler.ExitCode
  if ($compiler.ExitCode -ne 0) { throw 'Compile failed.' }
  $manifest.files+=@{ path='receive-connect-probe.exe'; sha256=(Get-FileHash -LiteralPath $exe -Algorithm SHA256).Hash }
  $manifest.files+=@{ path='response-parser.test.exe'; sha256=(Get-FileHash -LiteralPath $parserExe -Algorithm SHA256).Hash }
  $manifest | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $evidence 'manifest.json') -Encoding utf8NoBOM
  $status.phase='fixture-start'; Save-Status
  # Fail before launching if any fixture port is occupied. No other process is stopped.
  foreach ($port in @(3000,3004)) { if (@(Get-NetTCPConnection -State Listen -LocalPort $port -ErrorAction SilentlyContinue).Count) { throw 'Fixture port already occupied.' } }
  # Child-scoped fixture gates only; no machine, security, or network setting changes.
  $names=@('WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS','WINDOWS_VISUAL_RECEIVE_CONNECT_PROBE','VISUAL_FIXTURE_MEDIA_PROFILE')
  $previous=@{}
  foreach ($name in $names) { $previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process') }
  try {
    [Environment]::SetEnvironmentVariable($names[0],'1','Process')
    [Environment]::SetEnvironmentVariable($names[1],'1','Process')
    [Environment]::SetEnvironmentVariable($names[2],'dual-loopback','Process')
    $fixture=[DualStackOwnedProcess]::Start($fixtureExe,'',$repo,(Join-Path $private 'fixture.out'),(Join-Path $private 'fixture.err'))
  } finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name,$previous[$name],'Process') }
  }
  $watch=[Diagnostics.Stopwatch]::StartNew(); $ready=$false
  while ($watch.ElapsedMilliseconds -lt 20000) {
    if ($fixture.HasExited) { throw 'Fixture exited during startup.' }
    if (@(Get-NetTCPConnection -State Listen -LocalPort 3000 -ErrorAction SilentlyContinue).Count) { Confirm-Listener; $ready=$true; break }
    Start-Sleep -Milliseconds 100
  }
  if (-not $ready) { throw 'Fixture startup timeout.' }
  $status.listener_verified=$true
  $status.phase='native'; Save-Status
  Confirm-Listener
  $native=[DualStackOwnedProcess]::Start($exe,($Mode + ' ' + $Randomize),$private,(Join-Path $evidence 'output.jsonl'),(Join-Path $private 'native.err'))
  if (-not $native.WaitForExit(40000)) { $status.native_timeout=$true; throw 'Native hard deadline.' }
  $status.native_exit=$native.ExitCode
  if ($native.ExitCode -ne 0) { $status.failure_class='native-failure'; $status.outcome='unvalidated-native-exit' }
  Confirm-Listener; $status.listener_rechecked=$true
  # Stop only this exact owned fixture job before reading its bounded evidence.
  $fixture.Kill()
  if (-not $fixture.WaitForExit(5000) -or -not $fixture.TreeExited) { throw 'Fixture cleanup timeout.' }
  Export-FixtureEvidence
} catch {
  $status.harness_failure=$true
  if ($status.outcome -notin @('observed-native-10055','observed-native-failure','unvalidated-native-exit')) { $status.outcome='harness-or-deadline-failure' }
  if ($null -eq $status.failure_class) { $status.failure_class=$status.phase + '-error' }
} finally {
  foreach ($entry in @(@('discovery',$discovery),@('environment',$environment),@('parser-compiler',$parserCompiler),@('parser',$parser),@('validator-tests',$validatorTests),@('compiler',$compiler),@('native',$native))) {
    $child=$entry[1]
    if ($null -eq $child) { continue }
    $childState=[ordered]@{ role=$entry[0]; forced=$false; tree_exited=$false; disposed=$false }
    try {
      if (-not $child.TreeExited) {
        # Every child is contained before it runs. A forced kill still fails qualification.
        $status.forced_cleanup=$true; $childState.forced=$true
        $child.Kill($true); if (-not $child.WaitForExit(5000)) { throw 'Child cleanup timeout.' }
      }
      $childState.tree_exited=$child.TreeExited
    } catch { $status.child_cleanup_complete=$false } finally {
      try { $child.Dispose(); $childState.disposed=$true } catch { $status.child_cleanup_complete=$false }
      $status.children+=@($childState)
    }
  }
  if ($null -ne $fixture) {
    $fixtureState=[ordered]@{ role='fixture'; termination='owned-job-stop'; forced=$false; tree_exited=$false; disposed=$false }
    try {
      if (-not $fixture.TreeExited) { $fixture.Kill(); if (-not $fixture.WaitForExit(5000)) { throw 'Fixture cleanup timeout.' } }
      $fixtureState.tree_exited=$fixture.TreeExited
    } catch { $status.child_cleanup_complete=$false } finally {
      try { $fixture.Dispose(); $fixtureState.disposed=$true } catch { $status.child_cleanup_complete=$false }
      $status.children+=@($fixtureState)
    }
    try { Export-FixtureEvidence } catch { $status.harness_failure=$true; if ($null -eq $status.failure_class) { $status.failure_class='fixture-evidence-error' } }
  }
  # Always inspect existing native output after owned cleanup, including when a
  # listener recheck or fixture shutdown failed. A harness failure never erases
  # independently corroborated transport evidence, and still blocks qualification.
  if ($null -ne $node -and (Test-Path -LiteralPath (Join-Path $evidence 'output.jsonl') -PathType Leaf)) {
    try { Invoke-EvidenceValidation } catch {
      $status.harness_failure=$true
      if ($status.outcome -notin @('observed-native-10055','observed-native-failure','unvalidated-native-exit')) { $status.outcome='harness-or-deadline-failure' }
      if ($null -eq $status.failure_class) { $status.failure_class='validation-error' }
    } finally {
      if ($null -ne $validator) {
        $validatorState=[ordered]@{ role='validator'; forced=$false; tree_exited=$false; disposed=$false }
        try {
          if (-not $validator.TreeExited) {
            $status.forced_cleanup=$true; $validatorState.forced=$true
            $validator.Kill(); if (-not $validator.WaitForExit(5000)) { throw 'Validator cleanup timeout.' }
          }
          $validatorState.tree_exited=$validator.TreeExited
        } catch { $status.child_cleanup_complete=$false } finally {
          try { $validator.Dispose(); $validatorState.disposed=$true } catch { $status.child_cleanup_complete=$false }
          $status.children+=@($validatorState)
        }
      }
    }
  }
  # Diagnostic export is best-effort and never replaces the original failure.
  try { Export-CompilerDiagnostics } catch { $status.compiler_diagnostics_available=$false }
  $status.qualified=($status.qualified -and -not $status.harness_failure -and $status.child_cleanup_complete -and -not $status.forced_cleanup -and $null -eq $status.failure_class)
  if (-not $status.child_cleanup_complete -or $status.forced_cleanup) { $status.harness_failure=$true }
  # A later cleanup problem cannot erase corroborated native operation failure.
  # Missing exposure stays inconclusive only when the harness itself completed.
  if ($status.harness_failure -and $status.outcome -notin @('observed-native-10055','observed-native-failure','unvalidated-native-exit')) { $status.outcome='harness-or-deadline-failure' }
  if (-not $status.qualified -and $status.outcome -eq 'qualified-bounded-success') { $status.outcome='harness-or-deadline-failure'; $status.harness_failure=$true }
  Save-Status
  # Validator-checked first failure and fixed launcher state reach stdout.
  $status | ConvertTo-Json -Depth 5 -Compress | Write-Output
}
if (-not $status.qualified) { exit 1 }
exit 0
