$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$downloads = New-Item -ItemType Directory -Force (Join-Path $env:TEMP 'capycanvas-provision')

function Install-Download([string]$Uri, [string]$Arguments, [int[]]$Success = @(0)) {
    $installer = Join-Path $downloads (Split-Path $Uri -Leaf)
    Invoke-WebRequest $Uri -OutFile $installer -UseBasicParsing
    $process = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]@{ FileName = $installer; Arguments = $Arguments; UseShellExecute = $false })
    $process.WaitForExit()
    if ($process.ExitCode -notin $Success) { throw "$installer exited with $($process.ExitCode)." }
}

Add-MpPreference -ExclusionPath 'C:\capycanvas', (Join-Path $env:USERPROFILE '.cargo'), (Join-Path $env:USERPROFILE '.rustup')
$components = 'Microsoft.VisualStudio.Workload.VCTools', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', 'Microsoft.VisualStudio.Component.Windows11SDK.26100'
Install-Download 'https://aka.ms/vs/18/stable/vs_buildtools.exe' ("--quiet --wait --norestart --nocache " + (($components | ForEach-Object { "--add $_" }) -join ' ')) @(0, 3010)
Install-Download 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' '-y --profile minimal --component clippy'
$nuget = New-Item -ItemType Directory -Force (Join-Path $env:USERPROFILE '.local\tools\nuget')
Invoke-WebRequest 'https://dist.nuget.org/win-x86-commandline/latest/nuget.exe' -OutFile (Join-Path $nuget 'nuget.exe') -UseBasicParsing
Remove-Item -Recurse -Force $downloads
& (Join-Path $PSScriptRoot 'prepare.ps1')
