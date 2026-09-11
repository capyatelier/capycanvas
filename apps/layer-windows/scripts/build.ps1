param(
    [ValidateSet('Debug','Release')][string]$Configuration = 'Debug',
    [string]$PackagesDirectory,
    [switch]$SkipRust,
    [switch]$SkipRestore
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
if (!$PackagesDirectory) { $PackagesDirectory = Join-Path $repo 'artifacts/windows/packages' }
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
if (!(Test-Path -LiteralPath $vswhere)) { throw 'Install Visual Studio Build Tools with C++ and Windows application development support.' }
$installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (!$installation) { throw 'No Visual Studio C++ toolchain was found.' }
Import-Module (Join-Path $installation 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll')
Enter-VsDevShell -VsInstallPath $installation -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64'
$toolset = if ([int](& $vswhere -latest -products '*' -property installationVersion).Split('.')[0] -ge 18) { 'v145' } else { 'v143' }
if (!$SkipRestore) {
    $nuget = Get-Command nuget -ErrorAction SilentlyContinue
    $nugetPath = if ($nuget) {$nuget.Source} else {Join-Path $env:USERPROFILE '.local/tools/nuget/nuget.exe'}
    if (!(Test-Path -LiteralPath $nugetPath)) { throw 'nuget.exe is required; install the official CLI under ~/.local/tools/nuget or on PATH.' }
    & $nugetPath restore (Join-Path $repo 'apps/layer-windows/packages.config') -PackagesDirectory $PackagesDirectory -NonInteractive
    if ($LASTEXITCODE -ne 0) { throw 'NuGet restore failed.' }
}
Push-Location $repo
try {
    if (!$SkipRust) {
        $cargoArgs = @('build','--locked','-p','layer-windows')
        if ($Configuration -eq 'Release') { $cargoArgs += '--release' }
        & cargo @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw 'Rust build failed.' }
    }
    & msbuild apps/layer-windows/CapyCanvas.vcxproj /m "/p:Configuration=$Configuration" /p:Platform=x64 "/p:PlatformToolset=$toolset" "/p:CapyPackages=$PackagesDirectory" /v:minimal /nologo
    if ($LASTEXITCODE -ne 0) { throw 'WinUI build failed.' }
    Write-Output (Join-Path $repo "artifacts/windows/$Configuration/CapyCanvas.exe")
} finally { Pop-Location }
