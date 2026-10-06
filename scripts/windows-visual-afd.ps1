param([ValidateSet('Run', 'Cleanup')][string]$Action = 'Run')
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
# Manifest inspection only: no trace session, event collection or conversion.
# Before implementing level-4 capture, review every current level-0..4 template
# and verify a decoder, event versions and socket-address representation.
# https://learn.microsoft.com/en-us/windows/win32/winsock/winsock-tracing-levels
# https://learn.microsoft.com/en-us/windows/win32/winsock/winsock-tracing-event-details
# https://learn.microsoft.com/en-us/windows/win32/winsock/control-of-winsock-tracing
# https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/logman-create-trace
$root = $null
$diagnostic = $null
$testExit = 1
$statePath = if ($env:RUNNER_TEMP -and $env:THEME_SHARD -match '^[1-4]$') {
    Join-Path $env:RUNNER_TEMP ('paperboard-afd-manifest-owned-' + $env:THEME_SHARD + '.json')
} else { $null }
function Test-RegularPath([string]$Path) {
    $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    return (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0)
}
function Clear-OwnedManifest {
    if (-not $statePath -or -not (Test-Path -LiteralPath $statePath)) { return }
    try {
        if (-not (Test-RegularPath $env:RUNNER_TEMP) -or -not (Test-RegularPath $statePath)) { return }
        if ((Get-Item -LiteralPath $statePath).Length -gt 2048) { return }
        $state = Get-Content -LiteralPath $statePath -Raw | ConvertFrom-Json
        if ($state.name -notmatch '^paperboard-afd-manifest-[a-f0-9]{32}$') { return }
        $expected = Join-Path $env:RUNNER_TEMP $state.name
        if ($state.path -ne $expected) { return }
        if (Test-Path -LiteralPath $expected) {
            if (-not (Test-RegularPath $expected)) { return }
            # Never recurse, traverse reparse points, or delete unexpected files.
            foreach ($name in @('metadata.private.json', 'summary.json')) {
                $path = Join-Path $expected $name
                if (Test-Path -LiteralPath $path) {
                    if (-not (Test-RegularPath $path)) { return }
                    [IO.File]::Delete($path)
                }
            }
            [IO.Directory]::Delete($expected, $false)
        }
        [IO.File]::Delete($statePath)
    } catch { Write-Output 'AFD manifest cleanup unavailable.' }
}
if ($Action -eq 'Cleanup') { Clear-OwnedManifest; exit 0 }
try {
    try {
        if (-not $statePath -or -not (Test-RegularPath $env:RUNNER_TEMP)) { throw 'Unavailable' }
        if (Test-Path -LiteralPath $statePath) { throw 'Prior inspection still owned' }
        $name = 'paperboard-afd-manifest-' + [Guid]::NewGuid().ToString('N')
        $root = Join-Path $env:RUNNER_TEMP $name
        New-Item -ItemType Directory -Path $root -ErrorAction Stop | Out-Null
        @{ name = $name; path = $root } | ConvertTo-Json -Compress |
            Set-Content -LiteralPath $statePath -Encoding utf8NoBOM
        $events = [Collections.Generic.List[object]]::new()
        $issues = [Collections.Generic.List[string]]::new()
        try {
            $provider = Get-WinEvent -ListProvider 'Microsoft-Windows-Winsock-AFD' -ErrorAction Stop
            $scanned = 0
            foreach ($event in $provider.Events) {
                $scanned++
                if ($scanned -gt 4096 -or $events.Count -ge 256) { $issues.Add('metadata-limit'); break }
                $level = if ($null -eq $event.Level) { $null } else { [int]$event.Level.Value }
                if ($null -ne $level -and $level -gt 4) { continue }
                $fields = [Collections.Generic.List[object]]::new()
                $unsupported = $false
                if ($null -eq $level) { $issues.Add('provider-level-unavailable'); $unsupported = $true }
                try {
                    $template = [string]$event.Template
                    if ([string]::IsNullOrWhiteSpace($template)) { throw 'Missing template' }
                    if ($template.Length -gt 16384) { $issues.Add('metadata-limit'); throw 'Template limit' }
                    $settings = [Xml.XmlReaderSettings]::new()
                    $settings.DtdProcessing = [Xml.DtdProcessing]::Prohibit
                    $settings.XmlResolver = $null
                    $settings.MaxCharactersInDocument = 16384
                    $reader = [Xml.XmlReader]::Create([IO.StringReader]::new($template), $settings)
                    try { $xml = [Xml.XmlDocument]::new(); $xml.XmlResolver = $null; $xml.Load($reader) }
                    finally { $reader.Dispose() }
                    foreach ($node in $xml.DocumentElement.ChildNodes) {
                        if ($node.LocalName -ne 'data' -or $fields.Count -ge 16) { $unsupported = $true; break }
                        $scalar = $true
                        foreach ($attribute in $node.Attributes) {
                            if ($attribute.Name -notin @('name', 'inType', 'outType')) { $scalar = $false }
                        }
                        $fields.Add(@{ name = [string]$node.name; type = [string]$node.inType;
                            out_type = [string]$node.outType; scalar = $scalar })
                    }
                } catch { $unsupported = $true; $issues.Add('provider-template-unavailable') }
                $events.Add(@{ id = [int]$event.Id; version = [int]$event.Version;
                    level = $level; fields = @($fields.ToArray()); unsupported = $unsupported })
            }
        } catch { $issues.Add('provider-metadata-unavailable') }
        $metadata = @{ provider = 'Microsoft-Windows-Winsock-AFD';
            events = @($events.ToArray()); issues = @($issues | Select-Object -Unique) }
        $metadataJson = $metadata | ConvertTo-Json -Depth 8 -Compress
        while ([Text.Encoding]::UTF8.GetByteCount($metadataJson) -gt 262144 -and $events.Count -gt 0) {
            $events.RemoveAt($events.Count - 1)
            $metadata.events = @($events.ToArray())
            $metadata.issues = @('metadata-limit') + @($issues | Select-Object -Unique)
            $metadataJson = $metadata | ConvertTo-Json -Depth 8 -Compress
        }
        $inputPath = Join-Path $root 'metadata.private.json'
        $resultPath = Join-Path $root 'summary.json'
        [IO.File]::WriteAllText($inputPath, $metadataJson)
        & node (Join-Path $PSScriptRoot 'windows-visual-afd.mjs') $inputPath $resultPath *> $null
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $resultPath)) { throw 'Metadata unavailable' }
        if ((Get-Item -LiteralPath $resultPath).Length -gt 65536) { throw 'Metadata limit' }
        $diagnostic = [IO.File]::ReadAllText($resultPath)
    } catch { Write-Output 'AFD manifest inspection unavailable; continuing the unchanged theme command.' }
    npm run test:themes -- --shard "$env:THEME_SHARD/4" --output "test-results/windows-themes-$env:THEME_SHARD"
    $testExit = $LASTEXITCODE
} finally {
    # Inspection failures must never replace the saved theme result.
    try {
        if ($testExit -ne 0) {
            if (-not $diagnostic) {
                $diagnostic = '{"schema":1,"provider":"Microsoft-Windows-Winsock-AFD","capture":"unavailable","reason":"provider-metadata-unavailable","events":[],"events_lost":null,"buffers_lost":null,"circular_overwrite":null,"complete":false}'
            }
            $destination = Join-Path (Get-Location) 'test-results/windows-afd'
            New-Item -ItemType Directory -Force -Path $destination | Out-Null
            [IO.File]::WriteAllText((Join-Path $destination 'summary.json'), $diagnostic)
        }
    } catch { Write-Output 'AFD manifest summary unavailable.' }
    try {
        Clear-OwnedManifest
        # A state-file write failure can leave only our empty newly-created dir.
        if ($root -and (Test-Path -LiteralPath $root) -and (Test-RegularPath $root)) {
            [IO.Directory]::Delete($root, $false)
        }
    } catch { Write-Output 'AFD manifest cleanup unavailable.' }
}
exit $testExit
