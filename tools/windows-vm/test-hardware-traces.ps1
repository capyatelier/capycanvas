function Assert-HardwareTraces([string]$Directory, [string]$Output) {
    $adapters = @(Get-ChildItem -LiteralPath $Directory -Filter 'ui-state-*.json' -Recurse -File |
        ForEach-Object { (Get-Content -LiteralPath $_.FullName -Raw | ConvertFrom-Json).model.windows_adapter } |
        Where-Object { $_ } | Sort-Object -Unique)
    if (!$adapters.Count) { throw "No rendered adapter evidence in $Directory" }
    foreach ($adapter in $adapters) {
        if ($adapter -notmatch '\((DiscreteGpu|IntegratedGpu)\)$') {
            throw "Fixture did not use a hardware adapter: $adapter"
        }
    }
    @{hardware_gpu=$true;adapters=$adapters}|ConvertTo-Json|Set-Content -LiteralPath $Output
}
