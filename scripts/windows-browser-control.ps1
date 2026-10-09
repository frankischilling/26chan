#requires -Version 7.0
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
if (-not $IsWindows -or $env:WINDOWS_BROWSER_CONTROL -ne '1' -or $env:THEME_SHARD -ne '7' -or
    $env:WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS -ne '1' -or $env:WINDOWS_VISUAL_NETLOG -ne '1') {
    throw 'Owned browser control configuration rejected.'
}
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$evidence = Join-Path $repo 'test-results/windows-browser-control-7'
# Refuse stale evidence and reparse traversal. Only these two directories are created.
$directory = $repo
foreach ($segment in @('test-results', 'windows-browser-control-7')) {
    $directory = Join-Path $directory $segment
    if ($segment -eq 'windows-browser-control-7' -and (Test-Path -LiteralPath $directory)) { throw 'Prior control evidence exists.' }
    if (-not (Test-Path -LiteralPath $directory)) { New-Item -ItemType Directory -Path $directory | Out-Null }
    $item = Get-Item -LiteralPath $directory -Force
    if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Unsafe evidence directory.' }
}
$state = [ordered]@{ schema = 1; phase = 'setup'; original_coordinator_exit = $null; hard_deadline = $false; forced_cleanup = $false; tree_exited = $false; cleanup_verified = $false }
$owned = $null
$exitCode = 1
$priorOwned = $env:WINDOWS_BROWSER_CONTROL_OWNED
$priorFixture = $env:VISUAL_FIXTURE_SERVER
try {
    Add-Type -Path (Join-Path $repo 'tests/fixtures/windows-dual-stack-connect-probe/owned-process.cs')
    $node = (Get-Command node.exe -ErrorAction Stop).Source
    $env:WINDOWS_BROWSER_CONTROL_OWNED = '1'
    $env:VISUAL_FIXTURE_SERVER = '1'
    $script = Join-Path $repo 'scripts/windows-browser-control.mjs'
    # The reused helper creates suspended, assigns the exact job, then resumes.
    # No raw streams are written. Original Playwright failure files remain enabled.
    $owned = [DualStackOwnedProcess]::Start($node, ('"{0}"' -f $script), $repo, 'NUL', 'NUL')
    $state.phase = 'running'
    $watch = [Diagnostics.Stopwatch]::StartNew()
    while (-not $owned.HasExited -and $watch.ElapsedMilliseconds -lt 1080000) { Start-Sleep -Milliseconds 100 }
    if ($owned.HasExited) {
        $exitCode = $owned.ExitCode
        $state.original_coordinator_exit = $exitCode
    } else { $state.hard_deadline = $true }
} catch { $state.phase = 'unavailable' }
finally {
    $env:WINDOWS_BROWSER_CONTROL_OWNED = $priorOwned
    $env:VISUAL_FIXTURE_SERVER = $priorFixture
    if ($null -ne $owned) {
        try {
            if (-not $owned.TreeExited) {
                $state.forced_cleanup = $true
                $owned.Kill()
                if (-not $owned.WaitForExit(5000)) { throw 'Owned cleanup deadline.' }
            }
            $state.tree_exited = $owned.TreeExited
            $state.cleanup_verified = $state.tree_exited
        } catch { $state.cleanup_verified = $false }
        finally { try { $owned.Dispose() } catch { $state.cleanup_verified = $false } }
    }
    if ($state.phase -ne 'unavailable') { $state.phase = 'finished' }
    $json = $state | ConvertTo-Json -Compress
    try {
        if ([Text.Encoding]::UTF8.GetByteCount($json) -le 8192) {
            $stream = [IO.File]::Open((Join-Path $evidence 'ownership.json'), [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
            try { $bytes = [Text.Encoding]::UTF8.GetBytes($json); $stream.Write($bytes, 0, $bytes.Length) } finally { $stream.Dispose() }
        }
    } catch { Write-Output 'Owned cleanup evidence unavailable.' }
    Write-Output ('Owned browser control exit: ' + $exitCode + '; cleanup verified: ' + $state.cleanup_verified)
}
# Diagnostic cleanup and collection never replace a completed original exit.
exit $exitCode
