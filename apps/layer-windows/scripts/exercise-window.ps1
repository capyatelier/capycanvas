param([Parameter(Mandatory)][int]$ProcessId,
      [ValidateSet('Stroke','Undo','Redo','Resize','Close','Test stroke','Test pan','Test backlog')][string]$Action='Stroke',
      [int]$X=400,[int]$Y=400,[int]$Width=1500,[int]$Height=1000)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyWindowExercise {
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window,out Rect rect);
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
$handle=$p.MainWindowHandle
if(!$handle){throw 'The app has no main window.'}
[CapyWindowExercise]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$rect=New-Object CapyWindowExercise+Rect
[CapyWindowExercise]::GetWindowRect($handle,[ref]$rect)|Out-Null
if($Action -eq 'Close') {
    $p.CloseMainWindow()|Out-Null
    if(!$p.WaitForExit(5000)){throw 'Close exceeded five seconds.'}
} elseif($Action -eq 'Resize') {
    if(![CapyWindowExercise]::MoveWindow($handle,$rect.left,$rect.top,$Width,$Height,$true)){throw 'Resize failed.'}
} elseif($Action -in @('Undo','Redo','Test stroke','Test pan','Test backlog')) {
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
    $condition=New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty,$Action)
    $button=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
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
