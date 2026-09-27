param([ValidateSet('Debug','Release')][string]$Configuration = 'Debug')
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$packages = 'layer-core','layer-engine','layer-ui','layer-workspace','layer-host','layer-windows' | ForEach-Object { '-p', $_ }
& (Join-Path $PSScriptRoot 'build.ps1') -Configuration $Configuration
& (Join-Path $PSScriptRoot 'test-input.ps1')
Push-Location $repo
try {
    $env:LAYER_TEST_SOFTWARE_GPU = '1'
    & cargo test --locked @packages --lib --features layer-render-wgpu/software-adapter-tests
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' }
    $env:CAPY_SETTINGS_DIRECTORY = Join-Path $repo 'artifacts/windows/test-settings'
    & cargo test --locked -p layer-windows --lib --features layer-render-wgpu/software-adapter-tests d3d12_ -- --ignored --test-threads=1 --skip hdr --skip native_color
    if ($LASTEXITCODE -ne 0) { throw 'D3D12 document tests failed.' }
} finally { Pop-Location }
