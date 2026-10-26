$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'guest-gpu.ps1')
$cases = @(
    @{code=0;accepted=$true},
    @{code=3010;accepted=$true},
    @{code=259;provider='NVIDIA';present=$true;signed=$true;accepted=$true},
    @{code=259;provider='Microsoft';present=$true;signed=$true;accepted=$false},
    @{code=259;provider='NVIDIA';present=$false;signed=$true;accepted=$false},
    @{code=259;provider='NVIDIA';present=$true;signed=$false;accepted=$false},
    @{code=5;accepted=$false}
)
foreach ($case in $cases) {
    & {
        function pnputil.exe { $global:LASTEXITCODE = $case.code }
        function Get-PnpDevice {
            param([switch]$PresentOnly, [string]$Class)
            if (!$PresentOnly -or $Class -ne 'Display') { throw 'Driver validation must inspect present display devices' }
            if ($case.present) { [pscustomobject]@{Status='OK';InstanceId='PCI\VENDOR_GPU'} }
        }
        function Get-CimInstance {
            param([string]$ClassName)
            if ($ClassName -ne 'Win32_PnPSignedDriver') { throw 'Wrong driver inventory' }
            [pscustomobject]@{DeviceID='PCI\VENDOR_GPU';DriverProviderName=$case.provider;IsSigned=$case.signed}
        }
        $accepted = $true
        try { Install-WindowsGpuDriver 'C:\driver-test' } catch { $accepted = $false }
        if ($accepted -ne $case.accepted) { throw "Wrong GPU driver result for code $($case.code), provider $($case.provider), present $($case.present), signed $($case.signed)" }
    }
}
"Guest GPU driver result checks passed: $($cases.Count)"
