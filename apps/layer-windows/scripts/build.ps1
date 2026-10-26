param(
    [ValidateSet('Debug','Release')][string]$Configuration = 'Release',
    [ValidateSet('native','x64','arm64')][string]$Architecture = 'native',
    [string]$PackagesDirectory,
    [string]$OutputDirectory,
    [string]$IntermediateDirectory,
    [switch]$SkipRust,
    [ValidateSet('None','Number','Color','Bitmap','Popup')][string]$ControlFixture = 'None',
    [switch]$SkipRestore,
    [switch]$SoftwareAdapterTests,
    [switch]$ReleaseIdentity,
    [switch]$Run
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
. (Join-Path $PSScriptRoot 'WindowsArchitecture.ps1')
$build = Resolve-WindowsArchitecture $Architecture
if (!$PackagesDirectory) { $PackagesDirectory = Join-Path $repo 'artifacts/windows/packages' }
if (!$OutputDirectory) { $OutputDirectory = Get-WindowsBuildDirectory $repo $build $Configuration }
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
if (!$IntermediateDirectory) { $IntermediateDirectory = $OutputDirectory.TrimEnd('\','/') + '.obj' }
$IntermediateDirectory = [IO.Path]::GetFullPath($IntermediateDirectory)
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (!(Test-Path -LiteralPath $vswhere)) { throw 'Install Visual Studio Build Tools with Desktop development with C++ and Windows SDK 10.0.26100.0.' }
$installation = & $vswhere -latest -products '*' -requires $build.component -format json | ConvertFrom-Json
if (!$installation) { throw "Install Visual Studio C++ tools for $($build.architecture) ($($build.component))." }
Import-Module (Join-Path $installation.installationPath 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $installation.installationPath -SkipAutomaticLocation -DevCmdArguments "-arch=$($build.architecture) -host_arch=$($build.hostArchitecture)"
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
    $rustProfile = if ($Configuration -eq 'Release') { 'release' } else { 'debug' }
    $rustDirectory = Join-Path $metadata.target_directory "$($build.target)/$rustProfile"
    if (!$SkipRust) {
        $cargoArgs = @('build','--locked','--target',$build.target,'-p','layer-windows-link')
        if ($Configuration -eq 'Release') { $cargoArgs += '--release' }
        $features = @(if ($SoftwareAdapterTests) { 'software-adapter-tests' }) + @(if ($ReleaseIdentity) { 'release-identity' })
        if ($features) { $cargoArgs += '--features', ($features -join ',') }
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
    }
    $fixture = switch ($ControlFixture) { Number {'true'} Color {'Color'} Bitmap {'Bitmap'} Popup {'Popup'} default {'false'} }
    & msbuild apps/layer-windows/CapyCanvas.vcxproj /m "/p:Configuration=$Configuration" "/p:Platform=$($build.platform)" "/p:CapyRustDirectory=$rustDirectory" "/p:CapyControlFixture=$fixture" "/p:PlatformToolset=$toolset" "/p:CapyPackages=$PackagesDirectory" "/p:OutDir=$OutputDirectory/" "/p:IntDir=$intermediateDirectory/" /v:minimal /nologo
    if ($LASTEXITCODE -ne 0) { throw 'WinUI build failed.' }
    Assert-WindowsBinaryArchitecture $OutputDirectory $build.architecture
    $executable = Join-Path $OutputDirectory 'CapyCanvas.exe'
    Write-Output $executable
    if ($Run) { Start-Process -FilePath $executable }
} finally { Pop-Location }
