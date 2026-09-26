param([ValidateSet('Debug','Release')][string]$Configuration = 'Debug')
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$packages = 'layer-core','layer-engine','layer-ui','layer-workspace','layer-windows' | ForEach-Object { '-p', $_ }
$gpu = 'export::tests','gpu::tests','open::tests','tasks::tests','tone::tests','window::tests' | ForEach-Object { '--skip', $_ }
& (Join-Path $PSScriptRoot 'build.ps1') -Configuration $Configuration
& (Join-Path $PSScriptRoot 'test-input.ps1')
Push-Location $repo
try {
    & cargo test --locked @packages --lib
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' }
    & cargo test --locked -p layer-host --lib -- @gpu
    if ($LASTEXITCODE -ne 0) { throw 'layer-host tests failed.' }
} finally { Pop-Location }
