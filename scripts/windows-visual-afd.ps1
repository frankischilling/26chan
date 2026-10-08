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
# Public metadata APIs (no EventRecord access or private reflection):
# https://learn.microsoft.com/en-us/windows/win32/api/tdh/nf-tdh-tdhenumeratemanifestproviderevents
# https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.eventing.reader.providermetadata.id
# https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.eventing.reader.eventmetadata
# https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.eventing.reader.eventkeyword.value
# https://learn.microsoft.com/en-us/dotnet/api/system.diagnostics.eventing.reader.eventloglink
$root = $null
$diagnostic = $null
$testExit = 1
$statePath = if ($env:RUNNER_TEMP -and $env:THEME_SHARD -match '^[1-8]$') {
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
        $providerGuid = $null
        $eligible = 0
        $enumerationComplete = $true
        try {
            $provider = Get-WinEvent -ListProvider 'Microsoft-Windows-Winsock-AFD' -ErrorAction Stop
            try {
                if ($null -ne $provider.Id) { $providerGuid = $provider.Id.ToString('D') }
            } catch { $providerGuid = $null }
            $scanned = 0
            foreach ($event in $provider.Events) {
                $scanned++
                if ($scanned -gt 4096) { $issues.Add('metadata-limit'); $enumerationComplete = $false; break }
                $level = if ($null -eq $event.Level) { $null } else { [int]$event.Level.Value }
                if ($null -ne $level -and $level -gt 4) { continue }
                $eligible++
                if ($events.Count -ge 256) { $issues.Add('metadata-limit'); continue }
                # Keep each missing property null; a failed getter must not drop
                # the other metadata or turn a missing descriptor into zero.
                $opcode = $null
                $task = $null
                $channelName = $null
                try { if ($null -ne $event.Opcode) { $opcode = $event.Opcode.Value } } catch { }
                try { if ($null -ne $event.Task) { $task = $event.Task.Value } } catch { }
                try {
                    if ($null -ne $event.LogLink) {
                        $candidate = $event.LogLink.LogName
                        if ($candidate -is [string] -and $candidate -cmatch '^[A-Za-z][A-Za-z0-9_.-]{0,95}(?:/[A-Za-z][A-Za-z0-9_.-]{0,31})?\z') {
                            $channelName = $candidate
                        }
                    }
                } catch { }
                $keywordValues = [Collections.Generic.List[string]]::new()
                $keywordsComplete = $false
                try {
                    $keywords = $event.Keywords
                    if ($null -ne $keywords) {
                        $keywordsComplete = $true
                        foreach ($keyword in $keywords) {
                            if ($keywordValues.Count -ge 64) { $keywordsComplete = $false; break }
                            # Int64.ToString(X16) preserves all 64 two's-complement
                            # bits, including sign/high bits. No UInt32/JSON-number cast.
                            if ($null -eq $keyword -or $keyword.Value -isnot [long]) {
                                $keywordsComplete = $false; break
                            }
                            $keywordValues.Add('0x' + $keyword.Value.ToString('X16', [Globalization.CultureInfo]::InvariantCulture))
                        }
                    }
                } catch { $keywordsComplete = $false }
                $fields = [Collections.Generic.List[object]]::new()
                $rejections = [Collections.Generic.List[string]]::new()
                $unsupported = $false
                if ($null -eq $level) { $issues.Add('provider-level-unavailable'); $unsupported = $true }
                try {
                    $template = [string]$event.Template
                    if ([string]::IsNullOrWhiteSpace($template)) { throw 'Missing template' }
                    if ($template.Length -gt 16384) {
                        $issues.Add('metadata-limit'); $rejections.Add('template-limit'); throw 'Template limit'
                    }
                    $settings = [Xml.XmlReaderSettings]::new()
                    $settings.DtdProcessing = [Xml.DtdProcessing]::Prohibit
                    $settings.XmlResolver = $null
                    $settings.MaxCharactersInDocument = 16384
                    $reader = [Xml.XmlReader]::Create([IO.StringReader]::new($template), $settings)
                    try { $xml = [Xml.XmlDocument]::new(); $xml.XmlResolver = $null; $xml.Load($reader) }
                    finally { $reader.Dispose() }
                    foreach ($node in $xml.DocumentElement.ChildNodes) {
                        if ($node.LocalName -ne 'data') {
                            $unsupported = $true; $rejections.Add('template-node-unsupported'); break
                        }
                        if ($fields.Count -ge 16) {
                            $unsupported = $true; $rejections.Add('template-field-limit'); break
                        }
                        $scalar = $true
                        $attributes = [Collections.Generic.List[string]]::new()
                        foreach ($attribute in $node.Attributes) {
                            if ($attributes.Count -ge 16) {
                                $unsupported = $true; $scalar = $false
                                $rejections.Add('field-attribute-limit'); break
                            }
                            $attributes.Add([string]$attribute.Name)
                            if ($attribute.Name -notin @('name', 'inType', 'outType')) { $scalar = $false }
                        }
                        $fields.Add(@{ name = [string]$node.name; type = [string]$node.inType;
                            out_type = [string]$node.outType; scalar = $scalar;
                            attributes = @($attributes.ToArray());
                            count = $node.GetAttribute('count'); length = $node.GetAttribute('length') })
                    }
                } catch {
                    $unsupported = $true; $issues.Add('provider-template-unavailable')
                    $rejections.Add('template-unavailable')
                }
                $events.Add(@{ id = [int]$event.Id; version = [int]$event.Version;
                    level = $level; opcode = $opcode; task = $task; channel_name = $channelName;
                    keyword_values = @($keywordValues.ToArray()); keywords_complete = $keywordsComplete;
                    fields = @($fields.ToArray()); unsupported = $unsupported;
                    rejections = @($rejections.ToArray()) })
            }
        } catch { $issues.Add('provider-metadata-unavailable'); $enumerationComplete = $false }
        $tdh = @{ status = 'unavailable'; provider_guid = 'e53c6823-7bb8-44bb-90dc-3f86090d48a6';
            reason = 'tdh-provider-unverified'; api_status = $null; descriptors = @() }
        # Bind raw descriptors only to the verified AFD provider and eligible .NET
        # metadata identities. Display keyword names never supply a raw mask.
        if ($providerGuid -eq $tdh.provider_guid) {
            try {
                Add-Type -Path (Join-Path $PSScriptRoot 'windows-visual-afd-tdh.cs') -ErrorAction Stop
                $raw = [Paperboard.WindowsAfd.TdhMetadata]::Collect()
                $tdh.status = $raw.status
                $tdh.reason = $raw.reason
                $tdh.api_status = $raw.api_status
                if ($raw.status -eq 'ok') {
                    $keys = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
                    foreach ($event in $events) {
                        [void]$keys.Add(('{0}:{1}' -f $event.id, $event.version))
                    }
                    $filtered = [Collections.Generic.List[object]]::new()
                    foreach ($descriptor in $raw.descriptors) {
                        if (-not $keys.Contains(('{0}:{1}' -f $descriptor.id, $descriptor.version))) { continue }
                        # Keep duplicates for the parser to reject ambiguous joins.
                        if ($filtered.Count -ge 512) {
                            $tdh.status = 'unavailable'; $tdh.reason = 'tdh-descriptor-limit'; $tdh.api_status = $null
                            $filtered.Clear(); break
                        }
                        $filtered.Add($descriptor)
                    }
                    $tdh.descriptors = @($filtered.ToArray())
                }
            } catch {
                $tdh.status = 'unavailable'; $tdh.reason = 'tdh-collection-unavailable'; $tdh.api_status = $null; $tdh.descriptors = @()
            }
        }
        $metadata = @{ provider = 'Microsoft-Windows-Winsock-AFD'; provider_guid = $providerGuid; tdh = $tdh;
            events = @($events.ToArray()); issues = @($issues | Select-Object -Unique);
            events_total = $eligible; events_total_exact = $enumerationComplete }
        $metadataJson = $metadata | ConvertTo-Json -Depth 8 -Compress
        while ([Text.Encoding]::UTF8.GetByteCount($metadataJson) -gt 262144 -and $events.Count -gt 0) {
            $events.RemoveAt($events.Count - 1)
            $metadata.events = @($events.ToArray())
            # Trim descriptors with their .NET events, never the other way around.
            $remainingKeys = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
            foreach ($event in $events) { [void]$remainingKeys.Add(('{0}:{1}' -f $event.id, $event.version)) }
            $tdh.descriptors = @($tdh.descriptors | Where-Object {
                $remainingKeys.Contains(('{0}:{1}' -f $_.id, $_.version))
            })
            $metadata.issues = @('metadata-limit') + @($issues | Select-Object -Unique)
            $metadataJson = $metadata | ConvertTo-Json -Depth 8 -Compress
        }
        # Even an empty event list must never leave an oversized private envelope.
        if ([Text.Encoding]::UTF8.GetByteCount($metadataJson) -gt 262144) { throw 'Metadata limit' }
        $inputPath = Join-Path $root 'metadata.private.json'
        $resultPath = Join-Path $root 'summary.json'
        [IO.File]::WriteAllText($inputPath, $metadataJson)
        & node (Join-Path $PSScriptRoot 'windows-visual-afd.mjs') $inputPath $resultPath *> $null
        if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $resultPath)) { throw 'Metadata unavailable' }
        if ((Get-Item -LiteralPath $resultPath).Length -gt 65536) { throw 'Metadata limit' }
        $diagnostic = [IO.File]::ReadAllText($resultPath)
    } catch { Write-Output 'AFD manifest inspection unavailable; continuing the unchanged theme command.' }
    node scripts/windows-theme-stderr.mjs --shard "$env:THEME_SHARD/8" --output "test-results/windows-themes-$env:THEME_SHARD"
    $testExit = $LASTEXITCODE
} finally {
    # Inspection failures must never replace the saved theme result.
    try {
        # Retain the bounded sanitized inventory on one successful shard too.
        # Missing metadata stays optional; the unavailable fallback is for failures.
        if ($testExit -ne 0 -or ($env:THEME_SHARD -eq '1' -and $diagnostic)) {
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
