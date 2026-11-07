param([ValidateSet('Debug','Release')][string]$Configuration = 'Release', [ValidateSet('native','x64','arm64')][string]$Architecture = 'native', [string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
. (Join-Path $PSScriptRoot 'WindowsArchitecture.ps1')
. (Join-Path $PSScriptRoot 'WindowsTestRuntime.ps1')
$build = Resolve-WindowsArchitecture $Architecture
$testBuildDirectory=if($OutputDirectory){[IO.Path]::GetFullPath($OutputDirectory)}else{Get-WindowsBuildDirectory $repo $build $Configuration}
$packages = 'layer-core','layer-engine','layer-color','layer-render','layer-ui','layer-workspace','layer-host','layer-windows' | ForEach-Object { '-p', $_ }
$cargoProfileArgs = @(if ($Configuration -eq 'Release') { '--release' })
& (Join-Path $PSScriptRoot '../tests/script-encoding.ps1')
& (Join-Path $repo 'tools/windows-vm/test-fixture-routing.ps1')
& (Join-Path $repo 'tools/windows-vm/test-prepare.ps1')
& (Get-Process -Id $PID).Path -NoProfile -File (Join-Path $PSScriptRoot '../tests/row-pointer-keepalive.ps1')
if($LASTEXITCODE -ne 0){throw 'Synthetic pointer keepalive regression failed.'}
& (Get-Process -Id $PID).Path -NoProfile -File (Join-Path $PSScriptRoot '../tests/renderer-health.ps1')
if($LASTEXITCODE -ne 0){throw 'Renderer health regression failed.'}
& (Join-Path $PSScriptRoot '../tests/capture-dpi.ps1')
& (Join-Path $PSScriptRoot '../tests/uia-owned-search.ps1')
foreach($shell in @((Get-Process -Id $PID).Path,(Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe'))|Select-Object -Unique){
    & $shell -NoProfile -File (Join-Path $PSScriptRoot '../tests/fixture-owner-census.ps1') -Helper (Join-Path $PSScriptRoot 'CapyUia.ps1') -Output (Join-Path $testBuildDirectory ('fixture-owner-'+[guid]::NewGuid().ToString('N')))
    if($LASTEXITCODE -ne 0){throw 'Registered fixture discovery regression failed.'}
    & $shell -NoProfile -File (Join-Path $PSScriptRoot '../tests/model-snapshot-reader.ps1')
    if($LASTEXITCODE -ne 0){throw 'Atomic snapshot reader regression failed.'}
}
& (Join-Path $PSScriptRoot '../tests/uia-projection-snapshot.ps1')
& (Join-Path $PSScriptRoot '../tests/canvas-editing-pixels.ps1')
& (Join-Path $PSScriptRoot '../tests/blender-document-pixels.ps1')
& (Join-Path $PSScriptRoot '../tests/object-fixture-pixels.ps1')
& (Join-Path $PSScriptRoot '../tests/layers-menu-targeting.ps1')
& (Join-Path $PSScriptRoot '../tests/presented-frame-wait.ps1')
& (Join-Path $PSScriptRoot '../tests/software-test-runtime.ps1')
& (Join-Path $PSScriptRoot 'build.ps1') -Configuration $Configuration -Architecture $build.architecture -OutputDirectory $testBuildDirectory
& (Join-Path $PSScriptRoot 'test-input.ps1') -Configuration $Configuration -OutputDirectory (Join-Path $testBuildDirectory 'tests')
& (Join-Path $PSScriptRoot '../tests/icon-orpc-driver.ps1')
$iconBuildDirectory=Join-Path $testBuildDirectory 'tests/icon-orpc-app'
& (Join-Path $PSScriptRoot 'build.ps1') -Configuration $Configuration -Architecture $build.architecture -ControlFixture Feedback -SkipRust -SkipRestore -OutputDirectory $iconBuildDirectory -IntermediateDirectory ($iconBuildDirectory+'.obj')
& (Join-Path $PSScriptRoot 'test-desktop.ps1') -Script (Join-Path $PSScriptRoot '../tests/icon-orpc.ps1') -Parameters @{Executable=(Join-Path $iconBuildDirectory 'CapyCanvas.exe');Output=(Join-Path $testBuildDirectory 'tests/icon-orpc')}
& (Join-Path $PSScriptRoot '../tests/runtime-staging.ps1') -Output (Join-Path $testBuildDirectory ('runtime-staging-'+[guid]::NewGuid().ToString('N')))
& (Join-Path $PSScriptRoot '../tests/architecture.ps1')
& (Join-Path $PSScriptRoot '../tests/application-menu-query.ps1')
& (Join-Path $PSScriptRoot '../tests/pen-capture.ps1')
$testPackages=Join-Path $repo 'artifacts/windows/test-packages'
if($build.architecture -eq 'arm64') {
    $nuget=Get-Command nuget -ErrorAction SilentlyContinue
    $nugetPath=if($nuget){$nuget.Source}else{Join-Path $env:USERPROFILE '.local/tools/nuget/nuget.exe'}
    & $nugetPath restore (Join-Path $PSScriptRoot '../tests/packages.config') -PackagesDirectory $testPackages -NonInteractive
    if($LASTEXITCODE -ne 0){throw 'Software test runtime restore failed.'}
}
$testRuntime=$null
Push-Location $repo
$previousSoftwareGpu = $env:LAYER_TEST_SOFTWARE_GPU
$previousAdapter = $env:WGPU_ADAPTER_NAME
$previousGpuIndex = $env:LAYER_GPU_INDEX
$previousStorage = $env:CAPY_STORAGE_DIR
$previousPath = $env:PATH
try {
    $metadata=& cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
    if($LASTEXITCODE -ne 0){throw 'Cannot resolve the Cargo test directory.'}
    $rustProfile=if($Configuration -eq 'Release'){'release'}else{'debug'}
    $testDirectory=Join-Path $metadata.target_directory "$($build.target)/$rustProfile/deps"
    $testRuntime=Install-WindowsSoftwareTestRuntime $testPackages $testDirectory $build.architecture
    $env:PATH = $testBuildDirectory+';'+$previousPath
    $env:LAYER_TEST_SOFTWARE_GPU = '1'
    $env:WGPU_ADAPTER_NAME = 'Microsoft Basic Render Driver'
    $env:LAYER_GPU_INDEX = $null
    $env:CAPY_STORAGE_DIR = Join-Path $repo ('artifacts/windows/test-storage/'+[Guid]::NewGuid().ToString('N'))
    & cargo test --locked --no-fail-fast --target $build.target @cargoProfileArgs @packages --lib --features layer-render-wgpu/software-adapter-tests -- --test-threads=1 --nocapture
    $failed = $LASTEXITCODE -ne 0
    $env:CAPY_STORAGE_DIR = Join-Path $repo ('artifacts/windows/test-storage/'+[Guid]::NewGuid().ToString('N'))
    & cargo test --locked --target $build.target @cargoProfileArgs -p layer-windows --lib --features layer-render-wgpu/software-adapter-tests -- color_storage::tests::hidden_profiles_leave_menus_until_shown_or_removed --ignored --exact --test-threads=1 --nocapture
    if ($LASTEXITCODE -ne 0) { $failed = $true }
    $env:CAPY_STORAGE_DIR = Join-Path $repo ('artifacts/windows/test-storage/'+[Guid]::NewGuid().ToString('N'))
    & cargo test --locked --target $build.target @cargoProfileArgs -p layer-windows --lib --features layer-render-wgpu/software-adapter-tests d3d12_ -- --ignored --test-threads=1 --nocapture --skip hdr --skip native_color
    if ($failed -or $LASTEXITCODE -ne 0) { throw 'Windows Rust or D3D12 document tests failed.' }
} finally {
    $env:LAYER_TEST_SOFTWARE_GPU = $previousSoftwareGpu
    $env:WGPU_ADAPTER_NAME = $previousAdapter
    $env:LAYER_GPU_INDEX = $previousGpuIndex
    $env:CAPY_STORAGE_DIR = $previousStorage
    $env:PATH = $previousPath
    Pop-Location
    Remove-WindowsSoftwareTestRuntime $testRuntime
}
