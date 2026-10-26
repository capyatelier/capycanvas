#requires -Version 7.0
param(
    [ValidateSet('Debug','Release')][string]$Configuration = 'Release',
    [Parameter(Mandatory)][string]$Output,
    [string]$BuildDirectory
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
. (Join-Path $repo 'apps/layer-windows/scripts/WindowsArchitecture.ps1')
$build = Resolve-WindowsArchitecture native
if (!$BuildDirectory) { $BuildDirectory = Get-WindowsBuildDirectory $repo $build $Configuration }
$BuildDirectory = (Resolve-Path -LiteralPath $BuildDirectory).Path
$Output = [IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Force $Output | Out-Null
$saved = @{}
foreach ($name in 'LAYER_TEST_SOFTWARE_GPU','WGPU_ADAPTER_NAME','LAYER_GPU_INDEX','CAPY_STORAGE_DIR','PATH','RUST_MIN_STACK','CARGO_TERM_COLOR') {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name)
}
$cargoProfileArgs = @(if ($Configuration -eq 'Release') { '--release' })
$results = @()
$ownedRuntime = @()
$testDirectories = @{}
function Run-Tests([string]$Name, [string]$Binary, [string[]]$Arguments) {
    $env:CAPY_STORAGE_DIR = Join-Path $Output ('storage-'+[Guid]::NewGuid().ToString('N'))
    $started = Get-Date
    Push-Location $testDirectories[$Binary]
    try {
        & $Binary @Arguments 2>&1 |
            Tee-Object -FilePath (Join-Path $Output "$Name.log") | Out-Host
        $code = $LASTEXITCODE
    } finally { Pop-Location }
    if (!(Select-String -LiteralPath (Join-Path $Output "$Name.log") -Pattern '^running [1-9][0-9]* tests?$' -Quiet)) {
        $code = 1
        Write-Warning "$Name did not execute any tests"
    }
    $script:results += @{name=$Name;exit=$code;seconds=((Get-Date)-$started).TotalSeconds}
}
Push-Location $repo
try {
    & (Join-Path $PSScriptRoot 'test-guest-gpu.ps1') |
        Tee-Object -FilePath (Join-Path $Output 'guest-driver-harness.log') | Out-Host
    & (Join-Path $PSScriptRoot 'test-hardware-harness.ps1') |
        Tee-Object -FilePath (Join-Path $Output 'hardware-harness.log') | Out-Host
    $env:LAYER_TEST_SOFTWARE_GPU = $null
    $env:LAYER_GPU_INDEX = $null
    $env:WGPU_ADAPTER_NAME = $null
    $env:RUST_MIN_STACK = '8388608'
    $env:CARGO_TERM_COLOR = 'never'
    $null = Enter-WindowsDeveloperShell $build
    $env:PATH = $BuildDirectory+';'+$env:PATH
    $metadata = & cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve Cargo test directory' }
    $rustProfile = if ($Configuration -eq 'Release') { 'release' } else { 'debug' }
    $deps = Join-Path $metadata.target_directory "$($build.target)/$rustProfile/deps"
    New-Item -ItemType Directory -Force $deps | Out-Null
    foreach ($name in 'dxcompiler.dll','dxil.dll') {
        $source = Join-Path $BuildDirectory $name
        if ($name -eq 'dxil.dll' -and !(Test-Path -LiteralPath $source)) { continue }
        $destination = Join-Path $deps $name
        if (Test-Path -LiteralPath $destination) {
            if ((Get-FileHash -LiteralPath $source).Hash -ne (Get-FileHash -LiteralPath $destination).Hash) {
                throw "Different DXC runtime already present: $destination"
            }
        } else {
            Copy-Item -LiteralPath $source -Destination $destination
            $ownedRuntime += $destination
        }
    }
    $adapters = @(& cargo run --locked --target $build.target @cargoProfileArgs -p layer-render-wgpu --example brush_frames -- --adapters)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot enumerate renderer adapters' }
    $adapters | Set-Content (Join-Path $Output 'adapters.txt')
    $hardware = @($adapters | Where-Object { $_ -match 'device_type: (DiscreteGpu|IntegratedGpu)' -and $_ -match 'backend: Dx12' })
    if ($hardware.Count -ne 1) { throw "Expected exactly one hardware D3D12 adapter: $($hardware -join '; ')" }
    if ($hardware[0] -notmatch 'LAYER_GPU_INDEX=(\d+):.*name: "([^"]+)"') { throw 'Cannot identify the hardware adapter' }
    $env:LAYER_GPU_INDEX = $Matches[1]
    $env:WGPU_ADAPTER_NAME = $Matches[2]
    @{adapter=$hardware[0];index=$env:LAYER_GPU_INDEX;name=$env:WGPU_ADAPTER_NAME;software_features=$false} |
        ConvertTo-Json | Set-Content (Join-Path $Output 'hardware.json')
    $artifacts = @(& cargo test --locked --target $build.target @cargoProfileArgs -p layer-render-wgpu -p layer-windows --lib --no-run --message-format=json)
    if ($LASTEXITCODE -ne 0) { throw 'Hardware test compilation failed' }
    $artifacts | Set-Content (Join-Path $Output 'test-artifacts.jsonl')
    $binaries = @{}
    foreach ($line in $artifacts) {
        $artifact = $line | ConvertFrom-Json
        if ($artifact.reason -eq 'compiler-artifact' -and 'software-adapter-tests' -in $artifact.features) {
            throw "Software GPU feature enabled in $($artifact.target.name)"
        }
        if ($artifact.reason -eq 'compiler-artifact' -and $artifact.profile.test -and $artifact.executable) {
            $binaries[$artifact.target.name] = $artifact.executable
            $testDirectories[$artifact.executable] = Split-Path -Parent $artifact.manifest_path
        }
    }
    if (!$binaries.layer_render_wgpu -or !$binaries.layer_windows) { throw 'Missing hardware test binaries' }
    @($binaries.Keys | Sort-Object | ForEach-Object {
        @{name=$_;path=$binaries[$_];working_directory=$testDirectories[$binaries[$_]];sha256=(Get-FileHash -LiteralPath $binaries[$_]).Hash}
    }) | ConvertTo-Json | Set-Content (Join-Path $Output 'test-binaries.json')
    Run-Tests 'renderer' $binaries.layer_render_wgpu @('--test-threads=8','--nocapture')
    $renderer = $results[-1]
    if ($renderer.exit -ne 0) {
        $lines = @(Get-Content -LiteralPath (Join-Path $Output 'renderer.log'))
        $start = [array]::LastIndexOf($lines, 'failures:')
        $failed = @(if ($start -ge 0) { $lines[($start + 1)..($lines.Count - 1)] | ForEach-Object { if ($_ -match '^    (\S+)$') { $Matches[1] } } })
        if ($failed.Count -and $failed.Count -le 50) {
            $renderer.parallel_failures = $failed
            Run-Tests 'renderer-alone' $binaries.layer_render_wgpu (@('--exact') + $failed + @('--test-threads=1','--nocapture'))
            if ($results[-1].exit -eq 0) { $renderer.exit = 0 }
        }
    }
    Run-Tests 'direct-contact' $binaries.layer_render_wgpu @('direct_contact_paint_and_erase_match_destination_deposition','--ignored','--test-threads=1','--nocapture')
    Run-Tests 'fine-aggregate' $binaries.layer_render_wgpu @('fine_aggregate_pencil_color_pipeline_prepares_on_hardware','--ignored','--test-threads=1','--nocapture')
    Run-Tests 'renderer-d3d12' $binaries.layer_render_wgpu @('d3d12_','--ignored','--test-threads=1','--nocapture')
    $env:LAYER_GPU_INDEX = $null
    Run-Tests 'documents-d3d12' $binaries.layer_windows @('d3d12_','--ignored','--test-threads=1','--nocapture')
    if (@($results | Where-Object { $_.exit -ne 0 }).Count) { throw 'Hardware GPU tests failed; see results.json and suite logs' }
} finally {
    @{results=$results;limitations=@('HDR output requires a physical HDR display','Physical pen, touch, mixed-DPI and multiple-display checks require physical devices','No performance target is established by this suite')} |
        ConvertTo-Json -Depth 5 | Set-Content (Join-Path $Output 'results.json')
    foreach ($entry in $saved.GetEnumerator()) { [Environment]::SetEnvironmentVariable($entry.Key, $entry.Value) }
    foreach ($path in $ownedRuntime) { Remove-Item -LiteralPath $path -Force }
    Pop-Location
}
