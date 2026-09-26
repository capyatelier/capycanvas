param([ValidateSet('Debug','Release')][string]$Configuration = 'Debug')
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$packages = 'layer-core','layer-engine','layer-ui','layer-host','layer-workspace','layer-windows' | ForEach-Object { '-p', $_ }
& (Join-Path $PSScriptRoot 'build.ps1') -Configuration $Configuration
& (Join-Path $PSScriptRoot 'test-input.ps1')
Push-Location $repo
try {
    & cargo test --locked @packages --lib
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' }
} finally { Pop-Location }
