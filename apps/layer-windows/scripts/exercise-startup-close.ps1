param(
    [Parameter(Mandatory)][string]$Executable,
    [ValidateNotNullOrEmpty()][int[]]$Delays = @(0, 1000, 2500, 5000)
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
if ($Delays | Where-Object { $_ -lt 0 -or $_ -gt 30000 }) {
    throw 'Close delays must be between zero and 30000 milliseconds.'
}
$Executable = (Resolve-Path -LiteralPath $Executable).Path
$directory = Split-Path -Parent $Executable
$repo = (Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$run = Join-Path $repo ('artifacts/windows/startup-close/' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run) | Out-Null
$results = @()
$review = $null
try {
    Enter-CapyEnvironment @('CAPY_TRACE_SHADER_JOBS', 'CAPY_FILTERS_DIR', 'CAPY_FILTERS_MODE')
    $env:CAPY_TRACE_UI = '1'
    $env:CAPY_TRACE_SHADER_JOBS = '1'
    $env:CAPY_TEST_DISPLAY = '1'
    $env:CAPY_TEST_PRIMARY = '1'
    foreach ($delay in $Delays) {
        $launch = Join-Path $run ([string]$results.Count)
        [IO.Directory]::CreateDirectory($launch) | Out-Null
        $env:CAPY_SETTINGS_DIRECTORY = Join-Path $launch 'profile'
        $stderr = Join-Path $launch 'shader-jobs.log'
        $review = Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
        # Retain the exact process handle through shutdown and final exit-code collection.
        $null = $review.Handle
        @{ process_id = $review.Id; delay_ms = $delay } | ConvertTo-Json | Set-Content (Join-Path $launch 'owner.json')
        Write-Output "Owned startup/close review $($review.Id), delay $delay ms"
        $startup = [Diagnostics.Stopwatch]::StartNew()
        Wait-Until { $model = Model; $model.brush_ready -and $model.windows_workspace.ready } 'Owned review did not become ready within 60 seconds.' 60
        $startup.Stop()
        if ($delay) { Start-Sleep -Milliseconds $delay }
        $watch = [Diagnostics.Stopwatch]::StartNew()
        if (!$review.CloseMainWindow()) { throw 'Owned review rejected the close request.' }
        $within = $review.WaitForExit(5000)
        # Gather evidence from this same process after a slow close, without
        # changing the five-second acceptance gate or restarting the app.
        if (!$within -and !$review.WaitForExit(20000)) {
            throw "Owned review $($review.Id) is still running after 25 seconds."
        }
        $watch.Stop()
        $review.Refresh()
        $pipelineTimes = @(Select-String -LiteralPath $stderr -Pattern '^pipeline end .*elapsed_ms=([0-9.]+)' | ForEach-Object {
            [double]::Parse($_.Matches[0].Groups[1].Value, [Globalization.CultureInfo]::InvariantCulture)
        })
        $result = [ordered]@{
            process_id = $review.Id
            delay_ms = $delay
            startup_ms = [Math]::Round($startup.Elapsed.TotalMilliseconds, 3)
            close_ms = [Math]::Round($watch.Elapsed.TotalMilliseconds, 3)
            within_five_seconds = $within
            exit_code = $review.ExitCode
            compiled_pipelines = $pipelineTimes.Count
            longest_pipeline_ms = ($pipelineTimes | Measure-Object -Maximum).Maximum
        }
        $results += [PSCustomObject]$result
        $results | ConvertTo-Json -Depth 4 | Set-Content (Join-Path $run 'results.json')
        $result | ConvertTo-Json -Compress | Write-Output
        $review.Dispose()
        $review = $null
    }
    if ($results | Where-Object { !$_.within_five_seconds -or $_.exit_code -ne 0 }) {
        throw 'Startup/close acceptance failed; see the local results for every owned launch.'
    }
    Write-Output 'All startup/close launches passed the five-second zero-exit gate. Painting cadence and physical input latency are separate acceptance tests.'
} finally {
    if ($null -ne $review) {
        $review.Refresh()
        if (!$review.HasExited) {
            $null = $review.CloseMainWindow()
            $null = $review.WaitForExit(5000)
        }
        $review.Dispose()
    }
    Exit-CapyEnvironment
}
