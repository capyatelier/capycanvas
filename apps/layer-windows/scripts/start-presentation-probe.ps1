param([string]$Executable)
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
if(!$Executable){$Executable=Join-Path $repo 'artifacts/windows/Release/CapyCanvas.exe'}
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$names=@('CAPY_PRESENT_PROBE','CAPY_TEST_DISPLAY','CAPY_SMOKE_TEST','CAPY_TRACE_INPUT','CAPY_TRACE_TRANSPORT')
$previous=@{}
foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
try {
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
    $env:CAPY_PRESENT_PROBE='1'
    $env:CAPY_TEST_DISPLAY='1'
    # This is an interactive drawing window. Keep the app at normal privilege;
    # only the separate ETW capture tool may need administrator rights.
    $app=Start-Process -FilePath $Executable -WorkingDirectory $directory -PassThru -RedirectStandardError (Join-Path $directory 'probe-runtime.stderr.log')
} finally {
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
$watch=[Diagnostics.Stopwatch]::StartNew()
$metadata=Join-Path $directory 'presentation-probe.json'
do {
    Start-Sleep -Milliseconds 200
    $app.Refresh()
    if($app.HasExited){throw "Presentation probe exited: $($app.ExitCode)"}
    $file=Get-Item -LiteralPath $metadata -ErrorAction SilentlyContinue
    $ready=$false
    if($file -and $file.LastWriteTime -ge $app.StartTime) {
        try {
            $info=Get-Content -LiteralPath $metadata -Raw | ConvertFrom-Json
            $ready=$info.process_id -eq $app.Id
        } catch { } # The single metadata write may still be finishing.
    }
} while(!$ready -and $watch.Elapsed.TotalSeconds -lt 50)
if(!$ready){throw "Probe readiness timed out; inspect the app's local stderr log."}
[pscustomobject]@{
    process_id=$app.Id
    scope='steady canvas presentation; not painting or input latency acceptance'
    capture_script=Join-Path $PSScriptRoot 'capture-presentation.ps1'
    viewport=$info.viewport
    maximum_frame_latency=$info.maximum_frame_latency
} | ConvertTo-Json
