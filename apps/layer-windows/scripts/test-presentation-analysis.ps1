$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$directory=Join-Path $repo ('artifacts/windows/analysis-tests/'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $directory -Force | Out-Null
$metadata=@{
    process_id=12345
    display=@{refreshHz=120}
    probe=@{swap_chain_addresses=@('0x123abc')}
}
$metadata | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $directory 'capture.json')
$fixture=@'
Application,ProcessID,SwapChainAddress,PresentMode,MsBetweenPresents,MsBetweenDisplayChange,MsUntilDisplayed
CapyCanvas.exe,12345,0x123ABC,Composed: Flip,8.333333333333,NA,7
CapyCanvas.exe,12345,0x123ABC,Composed: Flip,8.333333333333,NA,NA
CapyCanvas.exe,12345,0x123ABC,Composed: Flip,8.333333333333,16.666666666667,8
CapyCanvas.exe,12345,0x123ABC,Composed: Flip,8.333333333333,8.333333333333,9
CapyCanvas.exe,12345,0x456DEF,Composed: Flip,100,100,100
'@
$file=Join-Path $directory 'frames.csv'
$fixture | Set-Content $file
$analyzer=Join-Path $PSScriptRoot 'analyze-presentation.ps1'
$result=& $analyzer -Directory $directory -PassThru
if($result.captured_canvas_records -ne 4 -or $result.reported_displayed_records -ne 3 -or $result.not_reported_displayed_records -ne 1){throw 'Canvas filtering or undisplayed-frame accounting is wrong'}
if([Math]::Abs($result.present_rate_hz-120) -gt .001 -or [Math]::Abs($result.displayed_frame_rate_hz-80) -gt .001){throw 'Submission and displayed-frame rates were conflated'}
if($result.present_to_display_latency.p99_ms -ne 9 -or $result.display_intervals.p99_ms -lt 16.66){throw 'Nearest-rank percentiles are incorrect'}
function Must-Reject([string]$Csv) {
    $Csv | Set-Content $file
    $rejected=$false
    try {& $analyzer -Directory $directory -PassThru | Out-Null}catch{$rejected=$true}
    if(!$rejected){throw 'Invalid capture was accepted'}
}
Must-Reject ($fixture.Replace('12345','54321'))
Must-Reject ($fixture.Replace('0x123ABC','0x789DEF'))
Must-Reject ($fixture.Replace(',9',',NaN'))
Must-Reject ($fixture.Replace('MsUntilDisplayed','MissingDisplayTime'))
[pscustomobject]@{
    canvas_identity='passed'
    missing_display_records='passed'
    separate_present_and_display_rates='passed'
    nearest_rank_percentiles='passed'
    invalid_capture_rejection='passed'
    fixtures='synthetic; no captured user data'
} | ConvertTo-Json
