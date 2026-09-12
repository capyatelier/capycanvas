param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyWindowLifecycle {
    [DllImport("user32.dll")] public static extern bool ShowWindowAsync(IntPtr h,int command);
    [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/lifecycle/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{}
foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
    try {$snapshot=Get-Content (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json;if($snapshot.process_id -eq $review.Id){$snapshot.model}}catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=5) {
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do {
        if(& $Condition){return}
        $review.Refresh();if($review.HasExited){throw 'Lifecycle review exited unexpectedly'}
        Start-Sleep -Milliseconds 50
    }while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Button([string]$Name) {
    $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.AndCondition]::new(
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button)))
}
function Check-Caption {
    # Caption validation must observe a settled UI; filter discovery can still
    # be running after the brush and document become ready.
    Wait-Until {
        $captionStatus = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'canvas-status'))
        !$captionStatus -or $captionStatus.Current.IsOffscreen -or
            $captionStatus.Current.Name -notin @('Preparing canvas…','Preparing brushes…','Loading filters…')
    } 'Startup status did not settle before caption validation' 45
    $insets = @((Model).titlebar_insets)
    if ($insets.Count -ne 3 -or $insets[1] -le 0 -or $insets[2] -le 0 -or
        @($insets | Where-Object { [double]::IsNaN($_) -or [double]::IsInfinity($_) -or $_ -lt 0 }).Count) {
        throw 'Restored native caption measurements are invalid.'
    }
    $status = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'canvas-status'))
    if ($status -and !$status.Current.IsOffscreen) {
        throw ('Restored editor reports a native error: ' + $status.Current.Name)
    }
}
function Wait-Canvas {
    Wait-Until {
        $canvas=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
            [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Drawing canvas'))
        $canvas -and $canvas.Current.IsEnabled
    } 'Modal input gate did not clear'
    Check-Caption
}
function Minimize {
    [CapyWindowLifecycle]::ShowWindowAsync($handle,6)|Out-Null
    Wait-Until {[CapyWindowLifecycle]::IsIconic($handle)} 'Review did not minimize'
}
function Close-Decision([bool]$Maximized,[string]$Choice) {
    $review.CloseMainWindow()|Out-Null
    Wait-Until {![CapyWindowLifecycle]::IsIconic($handle)} 'Unsaved decision left its owner minimized'
    Wait-Until {$button=Button $Choice;$button -and !$button.Current.IsOffscreen} 'Unsaved decision is not visible'
    if([CapyWindowLifecycle]::IsZoomed($handle) -ne $Maximized){throw 'Unsaved decision changed the pre-minimize maximized state'}
    (Button $Choice).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    if($Choice -eq 'Cancel'){
        Wait-Until {$model=Model;$model -and !$model.state.document_file.busy} 'Cancelled close did not finish'
        if(!(Model).state.document_file.modified -or (Model).state.document_file.close_ready){throw 'Cancelled close lost the drawing'}
        Wait-Canvas
    }
}
function Check-Closed {
    if(!$review.WaitForExit(5000)){throw 'Lifecycle close exceeded five seconds after authorization'}
    if($review.ExitCode -ne 0){throw ("Native exit code: 0x{0:X8}" -f [uint32]($review.ExitCode -band 0xffffffffL))}
    if((Get-Item -LiteralPath $stderr).Length){throw 'Lifecycle runtime stderr requires inspection'}
}
try {
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1'
    $env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    foreach($scenario in @('startup','warming','clean','dirty')){
        $dirty=$scenario -eq 'dirty'
        $stderr=Join-Path $run ("$scenario.stderr.log")
        $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
        $null=$review.Handle # Keep exit-code observation valid after a fast close.
        [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/lifecycle-review.pid'),[string]$review.Id)
        Write-Output "Owned lifecycle review $($review.Id), scenario=$scenario"
        if($scenario -eq 'startup'){
            Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero} 'Startup window did not appear' 45
            $startupBeforeReady=!(Model).brush_ready
            $review.CloseMainWindow()|Out-Null
            Check-Closed
            continue
        }
        Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
        if(!(Model).windows_isolated_settings){throw 'Lifecycle review requires an isolated profile'}
        $handle=$review.MainWindowHandle
        $root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
        if($scenario -eq 'warming'){
            Start-Sleep -Milliseconds 1250 # Exercise close during speculative shader warmup.
            $review.CloseMainWindow()|Out-Null
            Check-Closed
            continue
        }
        if(!$dirty){
            Minimize
            $review.CloseMainWindow()|Out-Null
            Check-Closed
            continue
        }
        & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action 'Test stroke'
        Wait-Until {(Model).state.document_file.modified} 'Controlled stroke did not dirty the document'
        Minimize
        Close-Decision $false 'Cancel'
        [CapyWindowLifecycle]::ShowWindowAsync($handle,3)|Out-Null
        Wait-Until {[CapyWindowLifecycle]::IsZoomed($handle)} 'Review did not maximize'
        Minimize
        Close-Decision $true 'Cancel'
        [CapyWindowLifecycle]::ShowWindowAsync($handle,9)|Out-Null
        Wait-Until {![CapyWindowLifecycle]::IsZoomed($handle)} 'Review did not restore from maximized state'
        Minimize
        Close-Decision $false 'Discard Changes'
        Check-Closed
    }
    [PSCustomObject]@{
        startup_window_close='passed'
        shader_warmup_close='passed'
        startup_close_requested_before_brush_ready=$startupBeforeReady
        clean_minimized_close='passed'
        visible_unsaved_decision_from_minimized='passed'
        cancelled_close_preserves_drawing='passed'
        maximized_state_survives_minimize_and_prompt='passed'
        restored_caption_measurements_and_error_status='passed'
        discard_and_zero_exit='passed'
        scope='isolated native window state and controlled replay; not physical input, mixed DPI or presentation acceptance'
    }|ConvertTo-Json
} finally {
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
