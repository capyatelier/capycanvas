$ErrorActionPreference = 'Stop'
Start-Transcript (Join-Path $env:SystemRoot 'Temp\capycanvas-bootstrap.log')

function Set-Registry([string]$Key, [string]$Name, [string]$Type, [string]$Value) {
    & reg.exe add $Key /v $Name /t $Type /d $Value /f | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Could not set $Key\$Name." }
}

Remove-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\Winlogon' AutoLogonCount -ErrorAction SilentlyContinue
Set-Registry 'HKLM\SOFTWARE\Policies\Microsoft\Windows\Personalization' NoLockScreen REG_DWORD 1
Set-Registry 'HKLM\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU' NoAutoUpdate REG_DWORD 1
Set-Registry 'HKLM\SYSTEM\CurrentControlSet\Control\FileSystem' LongPathsEnabled REG_DWORD 1
Set-Registry 'HKLM\SOFTWARE\Microsoft\PowerShell\1\ShellIds\Microsoft.PowerShell' ExecutionPolicy REG_SZ RemoteSigned
powercfg /change standby-timeout-ac 0
powercfg /change monitor-timeout-ac 0
powercfg /hibernate off

Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0 | Out-Null
$ssh = New-Item -ItemType Directory -Force (Join-Path $env:ProgramData 'ssh')
$keys = Join-Path $ssh 'administrators_authorized_keys'
Copy-Item (Join-Path $PSScriptRoot 'administrators_authorized_keys') $keys
& icacls.exe $keys /inheritance:r /grant '*S-1-5-32-544:F' '*S-1-5-18:F' | Out-Null
Set-Registry 'HKLM\SOFTWARE\OpenSSH' DefaultShell REG_SZ (Join-Path $PSHOME 'powershell.exe')
New-NetFirewallRule -Name capycanvas-ssh -DisplayName 'OpenSSH for the Capy Canvas VM' -Protocol TCP -LocalPort 22 -Action Allow | Out-Null
Set-Service sshd -StartupType Automatic
Start-Service sshd
