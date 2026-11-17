param(
    [Parameter(Mandatory)][string]$Executable,
    [ValidateNotNullOrEmpty()][int[]]$Delays = @(0, 1000, 2500, 5000),
    [ValidateSet('dark','light')][string]$Theme = 'dark',
    [string]$Capy = 'bathing',
    [switch]$FailSettings
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
$settingsLock = $null
function Launch-Profile([string]$Directory) {
    $env:CAPY_STORAGE_DIR = Join-Path $Directory 'profile'
    $config = Join-Path $env:CAPY_STORAGE_DIR 'config'
    [IO.Directory]::CreateDirectory($config) | Out-Null
    [IO.File]::WriteAllText((Join-Path $config 'settings.json'),(@{theme=$Theme;zen_icon=$Capy}|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
}
function Launch-Mark([string]$Pose) { 'launch-mark-zen-' + ($Pose -replace '_','-') }
function Check-LaunchCover {
    $cover = Find 'launch-splash' -Within $root -NoPopups
    if (!$cover -or $cover.Current.IsOffscreen) { return }
    $others = @('looking_up','facing_forward','bathing','sleeping') | Where-Object { $_ -ne $Capy } | ForEach-Object { Launch-Mark $_ }
    $shown = @(foreach ($id in @('canvas-status','workspace-storage-status') + $others) {
        $element = Find $id -Within $root -NoPopups
        if ($element -and !$element.Current.IsOffscreen) { $id }
    })
    if ($shown -and !$cover.Current.IsOffscreen) { throw "The launch splash showed $($shown -join ', ') over the saved $Capy capy." }
}
function Launch-Capture([string]$Name) {
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -WindowHandle $drawing.Handle.ToInt64() -TraceDirectory $directory -Output (Join-Path $launch "$Name.png") -ClientOnly -Composed -CaptureOnly
}
try {
    Enter-CapyEnvironment @('CAPY_LATENCY_TRACE', 'CAPY_TRACE_SHADER_JOBS', 'CAPY_FILTERS_DIR', 'CAPY_FILTERS_MODE')
    $env:CAPY_TEST_DISPLAY = '1'
    $env:CAPY_TEST_PRIMARY = '1'
    $launch = Join-Path $run 'latency'
    [IO.Directory]::CreateDirectory($launch) | Out-Null
    Launch-Profile $launch
    if($FailSettings){
        $settingsLock=[IO.File]::Open((Join-Path $env:CAPY_STORAGE_DIR 'config/settings.json'),[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
        $env:CAPY_TRACE_UI='1'
    }else{$env:CAPY_LATENCY_TRACE = '1'}
    $review = Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $launch 'stderr.log')
    $null = $review.Handle
    Wait-Until { $script:drawing = Owned-DrawingWindow $review; $null -ne $drawing } 'Latency-only tracing did not publish the owned drawing window.' 60
    $script:root = $drawing.Root
    $launchAge=[Diagnostics.Stopwatch]::StartNew()
    $bootstrap=Read-Snapshot (Join-Path $directory "bootstrap-$($review.Id)-$($drawing.Handle.ToInt64()).json")
    $captionDelay=$bootstrap.launch_caption_delay_ms
    if(!$captionDelay){throw 'The launch bootstrap has no caption delay'}
    if($splash=Find 'launch-splash' -Within $root -NoPopups){Launch-Capture 'launch'}
    if($FailSettings){
        Wait-Until { $view=Model; $view.error -and !$view.launch.visible -and !$view.launch.finished } 'The launch error did not bypass the unfinished splash.'
        Wait-Until { $cover=Find 'launch-splash' -Within $root -NoPopups; !$cover -or $cover.Current.IsOffscreen } 'The launch error remained covered.'
        $errorStatus=Control 'canvas-status' -Arranged
        if(!$errorStatus.Current.Name){throw 'The uncovered launch error has no text'}
        Launch-Capture 'error'
        $settingsLock.Dispose();$settingsLock=$null
        if(!(Close-OwnedDrawing $review) -or !$review.WaitForExit(5000) -or $review.ExitCode -ne 0){throw 'The launch error review did not close cleanly'}
        $review.Dispose()
        $review=$null
        Write-Output 'Startup settings failure bypassed the splash and closed cleanly.'
        return
    }
    if(('launch-mark-' + $bootstrap.launch_mark.icon) -ne (Launch-Mark $Capy)){throw "The launch bootstrap chose $($bootstrap.launch_mark.icon) instead of the saved $Capy capy"}
    Wait-Until {
        Check-LaunchCover
        $cover=Find 'launch-splash' -Within $root -NoPopups;$mark=Find (Launch-Mark $Capy) -Within $root -NoPopups
        !$cover -or $cover.Current.IsOffscreen -or ($mark -and !$mark.Current.IsOffscreen)
    } "The launch splash did not show the saved $Capy capy."
    $captionCapture=@{done=$false}
    Wait-Until {
        Check-LaunchCover
        if(!$captionCapture.done -and $launchAge.ElapsedMilliseconds -ge ($captionDelay+200)){
            if($cover=Find 'launch-splash' -Within $root -NoPopups){if(!$cover.Current.IsOffscreen){Launch-Capture 'caption';$captionCapture.done=$true}}
        }
        Trace-File 'presentation-probe'
    } 'Latency-only renderer did not become ready.' 90
    Wait-Until { $cover=Find 'launch-splash' -Within $root -NoPopups; !$cover -or $cover.Current.IsOffscreen } 'The latency-only splash did not reveal its ready canvas.'
    Launch-Capture 'revealed'
    if ((Test-Path Env:CAPY_TRACE_UI) -or (Get-ChildItem -LiteralPath $directory -Filter "ui-state-$($review.Id)-*.json")) { throw 'Latency-only tracing enabled per-frame UI tracing.' }
    if (!(Close-OwnedDrawing $review) -or !$review.WaitForExit(5000) -or $review.ExitCode -ne 0) { throw 'Latency-only drawing did not close with zero exit within five seconds.' }
    $review.Dispose()
    $review = $null
    Write-Output 'Latency-only window discovery and close passed.'
    Remove-Item Env:CAPY_LATENCY_TRACE
    $env:CAPY_TRACE_UI = '1'
    $env:CAPY_TRACE_SHADER_JOBS = '1'
    foreach ($delay in $Delays) {
        $launch = Join-Path $run ([string]$results.Count)
        [IO.Directory]::CreateDirectory($launch) | Out-Null
        Launch-Profile $launch
        $stderr = Join-Path $launch 'shader-jobs.log'
        $review = Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
        # Retain the exact process handle through shutdown and final exit-code collection.
        $null = $review.Handle
        @{ process_id = $review.Id; delay_ms = $delay } | ConvertTo-Json | Set-Content (Join-Path $launch 'owner.json')
        Write-Output "Owned startup/close review $($review.Id), delay $delay ms"
        $startup = [Diagnostics.Stopwatch]::StartNew()
        Wait-Canvas { $model = Model; $model.brush_ready -and $model.windows_workspace.ready } 'Owned review did not become ready within 60 seconds.' 60
        $startup.Stop()
        Wait-Until { $view=Model; $view.launch.finished -and !$view.launch.visible -and $view.state.theme -eq $Theme } 'The shared launch did not finish on its ready presented frame.' 60
        $script:drawing=Owned-DrawingWindow $review
        $script:root=$drawing.Root
        Wait-Until { $cover=Find 'launch-splash' -Within $root -NoPopups; !$cover -or $cover.Current.IsOffscreen } 'The launch cover did not finish its reveal.'
        if(!(Model).canvas_ready -or !(Model).brush_ready){throw 'The revealed canvas lost its painting readiness'}
        Launch-Capture 'revealed'
        if ($delay) { Start-Sleep -Milliseconds $delay }
        $watch = [Diagnostics.Stopwatch]::StartNew()
        if (!(Close-OwnedDrawing $review)) { throw 'Owned review rejected the close request.' }
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
    if($settingsLock){$settingsLock.Dispose()}
    if ($null -ne $review) {
        $review.Refresh()
        if (!$review.HasExited) {
            $null = (Close-OwnedDrawing $review)
            $null = $review.WaitForExit(5000)
        }
        $review.Dispose()
    }
    Exit-CapyEnvironment
}
