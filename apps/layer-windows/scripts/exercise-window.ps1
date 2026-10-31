param([int]$ProcessId,
      [string]$Action='Stroke',
      [int]$X=400,[int]$Y=400,[int]$Width=1500,[int]$Height=1000,[switch]$DiscardUnsaved,[string]$StateDirectory,[long]$WindowHandle,[string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark')
$ErrorActionPreference='Stop'
if($Action -notin @('Stroke','Undo','Redo','Resize','Close','Test stroke','Test pan','Test backlog')){throw "Invalid window action: $Action"}
if($Executable){& (Join-Path $PSScriptRoot 'exercise-header.ps1') -Executable $Executable -Theme $Theme -FullscreenOnly;return}
if(!$ProcessId){throw 'Supply an executable or an owned process ID'}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyWindowExercise {
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window,out Rect rect);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
 [StructLayout(LayoutKind.Sequential)] public struct Mouse {public int dx,dy;public uint data,flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit,Size=40)] public struct Input {[FieldOffset(0)]public uint type;[FieldOffset(8)]public Mouse mouse;}
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
 [DllImport("user32.dll")] static extern int GetSystemMetrics(int code);
 [DllImport("user32.dll")] public static extern bool GetCursorPos(out Point point);
 [DllImport("user32.dll")] public static extern IntPtr WindowFromPoint(Point point);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 public static void Move(int x,int y) {
   var mouse=new Mouse {
     dx=(x-GetSystemMetrics(76))*65535/(GetSystemMetrics(78)-1),
     dy=(y-GetSystemMetrics(77))*65535/(GetSystemMetrics(79)-1),flags=0xC001};
   if(SendInput(1,new[]{new Input{mouse=mouse}},40)!=1)throw new Exception("Windows rejected mouse movement.");
 }
 public static void Button(uint flags) {
   if(SendInput(1,new[]{new Input{mouse=new Mouse{flags=flags}}},40)!=1)throw new Exception("Windows rejected mouse button input.");
 }
 [DllImport("user32.dll")] public static extern bool MoveWindow(IntPtr window,int x,int y,int w,int h,bool repaint);
}
'@
$p=Get-Process -Id $ProcessId
# Retain a queryable process handle before the HWND and process disappear.
$null=$p.Handle
if($WindowHandle -and $Action -notin @('Resize','Close')){throw 'An explicit window is supported only for resize or close.'}
$drawing=$null;$drawingResult=[ref]$drawing
Wait-Until {$drawingResult.Value=Owned-DrawingWindow $p $WindowHandle $StateDirectory;[bool]$drawingResult.Value} 'The app has no owned drawing window.'
$handle=$drawing.Handle
$windowOwner=[uint32]0
[CapyWindowExercise]::GetWindowThreadProcessId($handle,[ref]$windowOwner)|Out-Null
if($windowOwner -ne $ProcessId){throw 'The drawing window does not belong to the owned process.'}
[CapyWindowExercise]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$rect=New-Object CapyWindowExercise+Rect
[CapyWindowExercise]::GetWindowRect($handle,[ref]$rect)|Out-Null
if($Action -eq 'Close') {
    Assert-RendererHealthy $ProcessId $drawing
    $dirty=$false
    $windowOnly=$false
    if($DiscardUnsaved){
        if($p.ProcessName -ne 'CapyCanvas'){throw 'Discard requires a controlled CapyCanvas review.'}
        if(!$StateDirectory){$StateDirectory=Split-Path -Parent $p.Path}
        $windows=Read-Snapshot (Join-Path $StateDirectory "windows-$ProcessId.json")
        $window=@($windows.windows|Where-Object hwnd -eq ([int64]$handle))[0]
        if($windows.process_id -ne $ProcessId -or !$window){throw 'Discard requires the traced window of this review.'}
        $windowOnly=$WindowHandle -and @($windows.windows).Count -gt 1
        $stateFile=Join-Path $StateDirectory "ui-state-$ProcessId-$($window.id).json"
        $snapshot=Read-Snapshot $stateFile
        if($snapshot.process_id -ne $ProcessId -or !$snapshot.model.windows_isolated_settings){
            throw 'Discard is only available for an isolated review with a matching trace.'
        }
        $dirty=$snapshot.model.state.document_file.modified
    }
    if(![CapyWindowApi]::PostMessage($handle,0x10,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'The owned drawing window did not accept Close.'}
    if($DiscardUnsaved){
        $watch=[Diagnostics.Stopwatch]::StartNew();$lastEpoch=$null;$lastDecision=$null
        $root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
        while(!$p.HasExited){
          if($windowOnly -and ![CapyWindowExercise]::IsWindow($handle)){break}
          try {
            $snapshot=Read-Snapshot $stateFile
            if($snapshot.process_id -ne $ProcessId -or !$snapshot.model.windows_isolated_settings){throw 'Discard trace ownership changed'}
            $epoch=$snapshot.model.state.document_file.epoch
            if($epoch -ne $lastEpoch){$lastEpoch=$epoch;$watch.Restart()}
            $discard=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
                [System.Windows.Automation.AndCondition]::new(
                    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,'Discard Changes'),
                    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button)))
            if($discard -and $discard.Current.IsEnabled -and $lastDecision -ne $epoch){
                $discard.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke();$lastDecision=$epoch;$watch.Restart()
            }
            if($watch.Elapsed.TotalSeconds -gt 5){throw 'A drawing close exceeded five seconds after authorization'}
          }catch{$p.Refresh();if($p.HasExited -or ($windowOnly -and ![CapyWindowExercise]::IsWindow($handle))){break};throw}
            Start-Sleep -Milliseconds 75;$p.Refresh()
        }
    }
    if($windowOnly -and !$p.HasExited){
        if([CapyWindowExercise]::IsWindow($handle)){throw 'The owned drawing window did not close.'}
        Record-CapyFixtureClose $ProcessId $handle.ToInt64() $true
        'Completed Close';return
    }
    if(!$p.WaitForExit(5000)){throw 'Close exceeded five seconds after authorization.'}
    $code=$p.ExitCode
    if($null -eq $code){throw "Native review exit status is unavailable"}
    if($code -ne 0){throw ("Native review exit code: 0x{0:X8}" -f [uint32]($code -band 0xffffffffL))}
    Record-CapyFixtureClose $ProcessId $handle.ToInt64() $false
} elseif($Action -eq 'Resize') {
    if(![CapyWindowExercise]::MoveWindow($handle,$rect.left,$rect.top,$Width,$Height,$true)){throw 'Resize failed.'}
    $review=$p
    $traceDirectory=if($StateDirectory){$StateDirectory}else{Split-Path -Parent $p.Path}
    $registration=Read-Snapshot (Join-Path $traceDirectory "windows-$ProcessId.json")
    $registered=@($registration.windows|Where-Object hwnd -eq $handle.ToInt64())
    if($registration.process_id -ne $ProcessId -or $registered.Count -ne 1){throw 'Resize has no unique owned drawing trace.'}
    $statePath=Join-Path $traceDirectory "ui-state-$ProcessId-$($registered[0].id).json"
    Wait-Until {
        $outer=New-Object CapyWindowExercise+Rect;$client=New-Object CapyWindowExercise+Rect
        if(![CapyWindowExercise]::GetWindowRect($handle,[ref]$outer) -or ![CapyWindowExercise]::GetClientRect($handle,[ref]$client)){throw 'Resize bounds are unavailable.'}
        $snapshot=Read-Snapshot $statePath;$model=$snapshot.model;$scale=[CapyWindowExercise]::GetDpiForWindow($handle)/96.
        $snapshot.process_id -eq $ProcessId -and $snapshot.window_id -eq $registered[0].id -and $model.windows_isolated_settings -and
            $outer.right-$outer.left -eq $Width -and $outer.bottom-$outer.top -eq $Height -and
            [Math]::Abs($model.layout.viewport[0]*$scale-$client.right) -le 1 -and [Math]::Abs($model.layout.viewport[1]*$scale-$client.bottom) -le 1 -and
            [Math]::Abs($model.state.camera.viewport[0]-$client.right) -le 1 -and [Math]::Abs($model.state.camera.viewport[1]-$client.bottom) -le 1
    } 'The requested native resize did not receive its model viewport acknowledgement.'
} elseif($Action -in @('Undo','Redo','Test stroke','Test pan','Test backlog')) {
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
    $condition=New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty,$Action)
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{$button=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition);if(!$button){Start-Sleep -Milliseconds 100}}while(!$button -and $watch.Elapsed.TotalSeconds -lt 10)
    if(!$button){throw 'Command button is missing.'}
    $pattern=$button.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern)
    $pattern.Invoke()
} else {
    [CapyWindowExercise]::SetForegroundWindow($handle)|Out-Null
    [CapyWindowExercise]::Move($rect.left+$X,$rect.top+$Y)|Out-Null
    Start-Sleep -Milliseconds 50
    $actual=New-Object CapyWindowExercise+Point
    [CapyWindowExercise]::GetCursorPos([ref]$actual)|Out-Null
    $target=[uint32]0
    [CapyWindowExercise]::GetWindowThreadProcessId([CapyWindowExercise]::WindowFromPoint($actual),[ref]$target)|Out-Null
    if($target -ne $ProcessId){throw "Pointer targets process $target instead of the app; mouse test was not run."}
    [CapyWindowExercise]::Button(2)
    try {
        for($i=1;$i -le 40;$i++){
            [CapyWindowExercise]::Move($rect.left+$X+$i*5,$rect.top+$Y+[int](35*[Math]::Sin($i/6.0)))|Out-Null
            Start-Sleep -Milliseconds 8
        }
    } finally {[CapyWindowExercise]::Button(4)}
}
"Completed $Action"
