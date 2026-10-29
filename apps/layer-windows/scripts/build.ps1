param(
    [ValidateSet('Debug','Release')][string]$Configuration = 'Release',
    [ValidateSet('dev','dev-perf','release')][string]$RustProfile,
    [ValidateSet('native','x64','arm64')][string]$Architecture = 'native',
    [string]$PackagesDirectory,
    [string]$OutputDirectory,
    [string]$IntermediateDirectory,
    [switch]$SkipRust,
    [ValidateSet('None','Number','Color','Bitmap','Popup','Feedback','LayerDrop')][string]$ControlFixture = 'None',
    [switch]$SkipRestore,
    [switch]$SoftwareAdapterTests,
    [switch]$ReleaseIdentity,
    [switch]$Run
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
. (Join-Path $PSScriptRoot 'WindowsArchitecture.ps1')
$build = Resolve-WindowsArchitecture $Architecture
$defaultProfile = if ($Configuration -eq 'Release') { 'release' } else { 'dev' }
if (!$RustProfile) { $RustProfile = $defaultProfile }
$RustProfile = $RustProfile.ToLowerInvariant()
if (!$PackagesDirectory) { $PackagesDirectory = Join-Path $repo 'artifacts/windows/packages' }
if (!$OutputDirectory) {
    $OutputDirectory = Get-WindowsBuildDirectory $repo $build $Configuration
    if ($RustProfile -ne $defaultProfile) { $OutputDirectory += "-$RustProfile" }
}
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
if (!$IntermediateDirectory) { $IntermediateDirectory = $OutputDirectory.TrimEnd('\','/') + '.obj' }
$IntermediateDirectory = [IO.Path]::GetFullPath($IntermediateDirectory)
$installation = Enter-WindowsDeveloperShell $build
$toolset = if (([version]$installation.installationVersion).Major -ge 18) { 'v145' } else { 'v143' }
if (!$SkipRestore) {
    $nuget = Get-Command nuget -ErrorAction SilentlyContinue
    $nugetPath = if ($nuget) {$nuget.Source} else {Join-Path $env:USERPROFILE '.local/tools/nuget/nuget.exe'}
    if (!(Test-Path -LiteralPath $nugetPath)) { throw 'nuget.exe is required; install the official CLI under ~/.local/tools/nuget or on PATH.' }
    & $nugetPath restore (Join-Path $repo 'apps/layer-windows/packages.config') -PackagesDirectory $PackagesDirectory -NonInteractive
    if ($LASTEXITCODE -ne 0) { throw 'NuGet restore failed.' }
}
Push-Location $repo
try {
    $metadata = & cargo metadata --locked --no-deps --format-version 1 | ConvertFrom-Json
    if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve the Cargo target directory.' }
    $profileDirectory = if ($RustProfile -eq 'dev') { 'debug' } else { $RustProfile }
    $rustDirectory = Join-Path $metadata.target_directory "$($build.target)/$profileDirectory"
    if (!$SkipRust) {
        $cargoArgs = @('build','--locked','--profile',$RustProfile,'--target',$build.target,'-p','layer-windows-link')
        $features = @(if ($SoftwareAdapterTests) { 'software-adapter-tests' }) + @(if ($ReleaseIdentity) { 'release-identity' })
        if ($features) { $cargoArgs += '--features', ($features -join ',') }
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
    }
    $fixture = switch ($ControlFixture) { Number {'true'} Color {'Color'} Bitmap {'Bitmap'} Popup {'Popup'} Feedback {'Feedback'} LayerDrop {'LayerDrop'} default {'false'} }
    & msbuild apps/layer-windows/CapyCanvas.vcxproj /m "/p:Configuration=$Configuration" "/p:Platform=$($build.platform)" "/p:CapyRustDirectory=$rustDirectory" "/p:CapyControlFixture=$fixture" "/p:PlatformToolset=$toolset" "/p:CapyPackages=$PackagesDirectory" "/p:OutDir=$OutputDirectory/" "/p:IntDir=$intermediateDirectory/" /v:minimal /nologo
    if ($LASTEXITCODE -ne 0) { throw 'WinUI build failed.' }
    Assert-WindowsBinaryArchitecture $OutputDirectory $build.architecture
    $executable = Join-Path $OutputDirectory 'CapyCanvas.exe'
    Write-Output $executable
    if ($Run) { Start-Process -FilePath $executable }
} finally { Pop-Location }
