function Install-WindowsGpuDriver([string]$Directory) {
    & pnputil.exe /add-driver (Join-Path $Directory '*.inf') /subdirs /install
    $code = $LASTEXITCODE
    if ($code -notin 0,259,3010) { throw "GPU driver installation failed: $code" }
    if ($code -eq 259) {
        $present = @(Get-PnpDevice -PresentOnly -Class Display |
            Where-Object Status -eq 'OK' | Select-Object -ExpandProperty InstanceId)
        $drivers = @(Get-CimInstance Win32_PnPSignedDriver | Where-Object {
            $_.DeviceID -in $present -and $_.DeviceID -like 'PCI\*' -and
            $_.IsSigned -and $_.DriverProviderName -and $_.DriverProviderName -notmatch '^Microsoft'
        })
        if (!$drivers.Count) { throw 'The bundle installed no matching driver and no healthy vendor GPU driver is present' }
    }
}
