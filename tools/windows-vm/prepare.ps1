$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

if (!(Test-Path -LiteralPath (Join-Path $env:ProgramFiles 'PowerShell\7\pwsh.exe'))) {
    $release = Invoke-RestMethod 'https://api.github.com/repos/PowerShell/PowerShell/releases/latest' -UseBasicParsing
    $asset = $release.assets | Where-Object name -Like 'PowerShell-*-win-x64.msi' | Select-Object -First 1
    $installer = Join-Path $env:TEMP $asset.name
    Invoke-WebRequest $asset.browser_download_url -OutFile $installer -UseBasicParsing
    $process = [Diagnostics.Process]::Start([Diagnostics.ProcessStartInfo]@{ FileName = 'msiexec.exe'; Arguments = "/i `"$installer`" /quiet /norestart ADD_PATH=1"; UseShellExecute = $false })
    $process.WaitForExit()
    Remove-Item -LiteralPath $installer
    if ($process.ExitCode -notin 0, 3010) { throw "PowerShell 7 setup exited with $($process.ExitCode)." }
}

$desktop = 'HKCU:\Control Panel\Desktop'
$personalize = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize'
Set-ItemProperty $personalize AppsUseLightTheme 0 -Type DWord
Set-ItemProperty $personalize SystemUsesLightTheme 0 -Type DWord
foreach ($monitor in Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Control\GraphicsDrivers\Configuration') {
    $settings = New-Item -Force (Join-Path $desktop "PerMonitorSettings\$($monitor.PSChildName)")
    Set-ItemProperty $settings.PSPath DpiValue 2 -Type DWord
}
foreach ($name in 'Configuration', 'Connectivity', 'ScaleFactors') {
    $displays = "HKLM:\SYSTEM\CurrentControlSet\Control\GraphicsDrivers\$name"
    if (Test-Path $displays) { Get-ChildItem $displays | Remove-Item -Recurse -Force }
}
