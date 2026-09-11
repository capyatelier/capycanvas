param([Parameter(Mandatory)][string]$Directory,[switch]$PassThru)
$ErrorActionPreference='Stop'
$meta=Get-Content -LiteralPath (Join-Path $Directory 'capture.json') -Raw | ConvertFrom-Json
$rows=@(Import-Csv -LiteralPath (Join-Path $Directory 'frames.csv'))
if(!$rows.Count){throw 'No captured frame records'}
if($rows | Where-Object {[int]$_.ProcessID -ne $meta.process_id}){throw 'Capture contains a different process'}
function Address([string]$Value){[Convert]::ToUInt64(($Value -replace '^0x',''),16)}
$addresses=@($meta.probe.swap_chain_addresses | ForEach-Object {Address $_})
$canvas=@($rows | Where-Object {(Address $_.SwapChainAddress) -in $addresses})
if(!$canvas.Count){throw 'No records match the native canvas swap chain'}
foreach($column in @('MsBetweenPresents','MsBetweenDisplayChange','MsUntilDisplayed')){
    if($column -notin $canvas[0].PSObject.Properties.Name){throw "Required PresentMon column is missing: $column"}
}
# Definitions: github.com/GameTechDev/PresentMon/blob/v2.5.1/README-ConsoleApplication.md
# Intervals are milliseconds. Do not invert a latency to report a frame rate.
function Samples([string]$Column,[switch]$PositiveOnly) {
    foreach($row in $canvas){
        $raw=$row.$Column
        if($raw -eq 'NA' -or $raw -eq ''){continue}
        $value=[double]::Parse($raw,[Globalization.CultureInfo]::InvariantCulture)
        if([double]::IsNaN($value) -or [double]::IsInfinity($value) -or $value -lt 0){throw "Invalid metric: $Column"}
        if(!$PositiveOnly -or $value -gt 0){$value}
    }
}
function Distribution([double[]]$Values) {
    if(!$Values.Count){return $null}
    $sorted=@($Values | Sort-Object)
    [pscustomobject]@{
        count=$sorted.Count
        mean_ms=($sorted | Measure-Object -Average).Average
        p50_ms=$sorted[[Math]::Ceiling($sorted.Count*.50)-1]
        p95_ms=$sorted[[Math]::Ceiling($sorted.Count*.95)-1]
        p99_ms=$sorted[[Math]::Ceiling($sorted.Count*.99)-1]
        max_ms=$sorted[-1]
    }
}
$present=Distribution @(Samples 'MsBetweenPresents' -PositiveOnly)
$display=Distribution @(Samples 'MsBetweenDisplayChange' -PositiveOnly)
$latency=Distribution @(Samples 'MsUntilDisplayed')
if(!$present -or !$display -or !$latency){throw 'Capture lacks presentation/display measurements'}
$notDisplayed=@($canvas | Where-Object {$_.MsUntilDisplayed -eq 'NA' -or $_.MsUntilDisplayed -eq ''}).Count
$result=[pscustomobject]@{
    scope='steady canvas; presentation diagnostics, not painting or input-latency acceptance'
    captured_canvas_records=$canvas.Count
    reported_displayed_records=$latency.count
    not_reported_displayed_records=$notDisplayed
    not_reported_displayed_percent=100.0*$notDisplayed/$canvas.Count
    present_modes=@($canvas.PresentMode | Select-Object -Unique)
    monitor_nominal_hz=$meta.display.refreshHz
    present_rate_hz=1000.0/$present.mean_ms
    displayed_frame_rate_hz=1000.0/$display.mean_ms
    observed_display_span_ms=$display.count*$display.mean_ms
    present_intervals=$present
    display_intervals=$display
    present_to_display_latency=$latency
    limitations=@(
        'Records without a display time are reported explicitly, not silently removed.'
        'Present-to-display timing excludes earlier input and drawing work.'
        'Check occlusion, concurrent workloads, capture completeness and binary identity separately.'
        'This summary does not establish a 120 Hz acceptance pass.'
    )
}
if($PassThru){$result}else{$result | ConvertTo-Json -Depth 5}
