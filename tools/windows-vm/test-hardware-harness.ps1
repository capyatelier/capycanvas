$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'test-hardware-traces.ps1')
$directory = Join-Path ([IO.Path]::GetTempPath()) ('capy-hardware-harness-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $directory | Out-Null
try {
    foreach ($adapter in 'NVIDIA RTX PRO 6000 Blackwell (DiscreteGpu)','Intel Graphics (IntegratedGpu)') {
        @{model=@{windows_adapter=$adapter}}|ConvertTo-Json|Set-Content (Join-Path $directory 'ui-state-1-1.json')
        Assert-HardwareTraces $directory (Join-Path $directory 'adapters.json')
    }
    foreach ($adapter in 'Microsoft Basic Render Driver (Cpu)','Unknown (Other)',$null) {
        @{model=@{windows_adapter=$adapter}}|ConvertTo-Json|Set-Content (Join-Path $directory 'ui-state-1-1.json')
        $rejected = $false
        try { Assert-HardwareTraces $directory (Join-Path $directory 'adapters.json') } catch { $rejected = $true }
        if (!$rejected) { throw "Invalid adapter evidence accepted: $adapter" }
    }
    Remove-Item (Join-Path $directory 'ui-state-1-1.json')
    $rejected = $false
    try { Assert-HardwareTraces $directory (Join-Path $directory 'adapters.json') } catch { $rejected = $true }
    if (!$rejected) { throw 'Missing adapter evidence accepted' }
    'Hardware fixture evidence checks passed (2 accepted, 4 rejected).'
} finally {
    Remove-Item -LiteralPath $directory -Recurse -Force
}
