$ErrorActionPreference = 'Stop'
# Pure synthetic-buffer tests only: never invoke Collect or a native API.
Add-Type -Path (Join-Path $PSScriptRoot 'windows-visual-afd-tdh.cs') -ErrorAction Stop

function Assert-Equal($Actual, $Expected, [string]$Label) {
    if ($Actual -cne $Expected) { throw "Assertion failed: $Label" }
}
function New-Buffer([uint32]$Count, [int]$Length) {
    $bytes = [byte[]]::new($Length)
    if ($Length -ge 4) { [BitConverter]::GetBytes($Count).CopyTo($bytes, 0) }
    return ,$bytes
}
function Assert-Unavailable([byte[]]$Bytes, [uint32]$Size, [string]$Reason, [string]$Label) {
    $result = [Paperboard.WindowsAfd.TdhMetadata]::Decode($Bytes, $Size)
    Assert-Equal $result.status 'unavailable' "$Label status"
    Assert-Equal $result.reason $Reason "$Label reason"
    Assert-Equal $result.api_status $null "$Label no API status"
    Assert-Equal $result.descriptors.Length 0 "$Label empty descriptors"
    Assert-Equal $result.provider_guid 'e53c6823-7bb8-44bb-90dc-3f86090d48a6' "$Label provider"
}

$empty = [Paperboard.WindowsAfd.TdhMetadata]::Decode((New-Buffer 0 8), 8)
Assert-Equal $empty.status 'ok' 'empty header accepted'
Assert-Equal $empty.reason $null 'success has no reason'
Assert-Equal $empty.api_status $null 'empty decoder success has no API status'
Assert-Equal $empty.descriptors.Length 0 'empty header descriptors'

# Exercise every field, including byte/ushort boundaries and UInt64 high bits.
$bytes = New-Buffer 2 40
[BitConverter]::GetBytes([uint16]65535).CopyTo($bytes, 8)
$bytes[10] = 255; $bytes[11] = 254; $bytes[12] = 4; $bytes[13] = 253
[BitConverter]::GetBytes([uint16]65534).CopyTo($bytes, 14)
# First descriptor's keyword is exactly zero; second is high bit plus low bit.
$bytes[24] = 42; $bytes[26] = 1; $bytes[28] = 0; $bytes[32] = 1; $bytes[39] = 128
$result = [Paperboard.WindowsAfd.TdhMetadata]::Decode($bytes, 40)
Assert-Equal $result.status 'ok' 'two descriptors status'
Assert-Equal $result.api_status $null 'decoder success has no API status'
Assert-Equal $result.descriptors.Length 2 'two descriptors count'
$first = $result.descriptors[0]
Assert-Equal $first.id 65535 'id ushort'
Assert-Equal $first.version 255 'version byte'
Assert-Equal $first.channel 254 'channel byte'
Assert-Equal $first.level 4 'level byte'
Assert-Equal $first.opcode 253 'opcode byte'
Assert-Equal $first.task 65534 'task ushort'
Assert-Equal $first.keyword_mask '0x0000000000000000' 'zero keyword'
Assert-Equal $result.descriptors[1].id 42 'second descriptor offset'
Assert-Equal $result.descriptors[1].keyword_mask '0x8000000000000001' 'unsigned high-bit keyword'
$json = $result | ConvertTo-Json -Depth 8 -Compress | ConvertFrom-Json
Assert-Equal $json.api_status $null 'JSON decoder status remains null'
Assert-Equal ($json.descriptors[1].keyword_mask -is [string]) $true 'JSON mask remains a string'
Assert-Equal $json.descriptors[1].keyword_mask '0x8000000000000001' 'JSON exact unsigned mask'
for ($i = 32; $i -lt 40; $i++) { $bytes[$i] = 255 }
Assert-Equal ([Paperboard.WindowsAfd.TdhMetadata]::Decode($bytes, 40)).descriptors[1].keyword_mask '0xFFFFFFFFFFFFFFFF' 'all keyword bits'

# Extra allocated bytes are harmless only when outside the API's returned size.
$padded = New-Buffer 1 64
Assert-Equal ([Paperboard.WindowsAfd.TdhMetadata]::Decode($padded, 24)).status 'ok' 'returned size bounds allocation'
Assert-Unavailable $padded 64 'tdh-buffer-invalid' 'unexplained returned tail'
Assert-Unavailable $null 8 'tdh-buffer-invalid' 'null buffer'
Assert-Unavailable ([byte[]]::new(0)) 0 'tdh-buffer-invalid' 'empty buffer'
Assert-Unavailable (New-Buffer 0 7) 7 'tdh-buffer-invalid' 'short header'
Assert-Unavailable (New-Buffer 0 8) 9 'tdh-buffer-invalid' 'returned size exceeds allocation'
Assert-Unavailable (New-Buffer 0 8) ([uint32]::MaxValue) 'tdh-buffer-invalid' 'overflow returned size'
Assert-Unavailable (New-Buffer 1 23) 23 'tdh-buffer-invalid' 'truncated descriptor'
Assert-Unavailable (New-Buffer 2 24) 24 'tdh-buffer-invalid' 'count exceeds returned descriptors'
Assert-Unavailable (New-Buffer 0 24) 24 'tdh-buffer-invalid' 'unclaimed descriptor bytes'
Assert-Unavailable (New-Buffer 4097 65560) 65560 'tdh-descriptor-limit' 'count above cap'
Assert-Unavailable (New-Buffer ([uint32]::MaxValue) 8) 8 'tdh-descriptor-limit' 'overflow count'
Assert-Unavailable (New-Buffer 0 262145) 8 'tdh-buffer-invalid' 'oversized allocation'
Assert-Unavailable (New-Buffer 0 262144) 262145 'tdh-buffer-invalid' 'oversized returned size'
$reserved = New-Buffer 0 8
$reserved[4] = 1
Assert-Unavailable $reserved 8 'tdh-header-unsupported' 'nonzero reserved header'
$maximum = [Paperboard.WindowsAfd.TdhMetadata]::Decode((New-Buffer 4096 65544), 65544)
Assert-Equal $maximum.status 'ok' 'maximum descriptor count'
Assert-Equal $maximum.descriptors.Length 4096 'maximum descriptors retained'

# Duplicates are retained verbatim so the parser can reject an ambiguous join.
$duplicates = [Paperboard.WindowsAfd.TdhMetadata]::Decode((New-Buffer 2 40), 40)
Assert-Equal $duplicates.descriptors.Length 2 'duplicate descriptors retained'
Assert-Equal $duplicates.descriptors[0].id $duplicates.descriptors[1].id 'duplicate identities unchanged'
Write-Output 'AFD TDH synthetic decoder tests passed.'
