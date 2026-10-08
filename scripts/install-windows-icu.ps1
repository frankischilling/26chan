# Local/CI development dependency. Production uses maintained OS ICU packages.
$ErrorActionPreference = 'Stop'
$taskRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$taskLocal = Join-Path $taskRoot '.local'
$icuRoot = Join-Path $taskLocal 'icu4c-74_2-Win64-MSVC2019'
$archive = Join-Path $taskLocal 'icu4c-74_2-Win64-MSVC2019.zip'
$archiveSha512 = 'cecb8a2cb05d4ed59b4d2f06eeb24a102ee6f75db0cee1970aa5ca6865677234fa031c337fba83c493c372617ac6efb8467bd059e6a99e6fcb395508c12ab00d'
$url = 'https://github.com/unicode-org/icu/releases/download/release-74-2/icu4c-74_2-Win64-MSVC2019.zip'
New-Item -ItemType Directory -Path $taskLocal -Force | Out-Null
if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) {
    Invoke-WebRequest -Uri $url -OutFile $archive
}
if ((Get-FileHash -LiteralPath $archive -Algorithm SHA512).Hash.ToLowerInvariant() -ne $archiveSha512) {
    throw 'Pinned ICU archive digest differs.'
}
Expand-Archive -LiteralPath $archive -DestinationPath $icuRoot -Force
$dlls = @{
    'icudt74.dll' = '088720972e0592c4ff5a81df0a274465cee83cbb47d044c731f22e661b983e6f'
    'icuin74.dll' = 'b27333428170f70b2f76c6e1f2ade2be6fa46e731d8e8b3be8a933b39bb806d2'
    'icuuc74.dll' = '44e7eb0a49fab63d8cb046963a8e06f5365219a986ed4006a7ddc05e5d81d2ec'
}
foreach ($entry in $dlls.GetEnumerator()) {
    $dll = Join-Path (Join-Path $icuRoot 'bin64') $entry.Key
    if ((Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash.ToLowerInvariant() -ne $entry.Value) {
        throw 'Pinned ICU runtime digest differs.'
    }
}
$env:RUST_ICU_LINK_SEARCH_DIR = Join-Path $icuRoot 'lib64'
$binPath = Join-Path $icuRoot 'bin64'
$env:PATH = "$binPath;$env:PATH"
if ($env:GITHUB_ENV -and $env:GITHUB_PATH) {
    "RUST_ICU_LINK_SEARCH_DIR=$env:RUST_ICU_LINK_SEARCH_DIR" | Out-File -LiteralPath $env:GITHUB_ENV -Encoding utf8 -Append
    $binPath | Out-File -LiteralPath $env:GITHUB_PATH -Encoding utf8 -Append
}
Write-Output 'Pinned ICU 74.2 development runtime verified.'
