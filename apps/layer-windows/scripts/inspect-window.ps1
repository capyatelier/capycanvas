param([int]$ProcessId, [string]$Output = 'artifacts/windows/window.png', [switch]$ClientOnly, [switch]$Composed, [long]$WindowHandle=0, [switch]$CaptureOnly)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyWindowCapture {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int left,top,right,bottom; }
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int x,y; }
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window, ref Point point);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
}
'@
$captureDpi=[CapyWindowCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))
try {
$p=Get-Process -Id $ProcessId
$p.Refresh()
$owned=@(if($WindowHandle){[pscustomobject]@{Handle=[IntPtr]$WindowHandle;Root=[System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$WindowHandle)}}else{
    $drawing=Owned-DrawingWindow $p
    if($drawing){$drawing}else{Owned-ProcessWindows $p|Where-Object {!$_.Root.Current.IsOffscreen -and !$_.Root.Current.BoundingRectangle.IsEmpty}}
})
if($owned.Count -ne 1){throw 'Specify one owned visible window to capture.'}
$handle=$owned[0].Handle;$root=$owned[0].Root
if(!$CaptureOnly){
for($attempt=1;;$attempt++){
    try{
        $nodes=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
        $names=foreach($node in $nodes){if($node.Current.Name){$node.Current.Name}}
        break
    }catch{
        if($attempt -ge 5){throw}
        Start-Sleep -Milliseconds 200
    }
}
}
$details=[ordered]@{title=$root.Current.Name;responding=$p.Responding}
if(!$CaptureOnly){$details.elements=@($names)}
[pscustomobject]$details | ConvertTo-Json -Depth 4
$rect=New-Object CapyWindowCapture+Rect
if($ClientOnly){$measured=[CapyWindowCapture]::GetClientRect($handle,[ref]$rect)}else{$measured=[CapyWindowCapture]::GetWindowRect($handle,[ref]$rect)}
if(!$measured -or $rect.right -le $rect.left -or $rect.bottom -le $rect.top){throw 'The owned window has no capture surface.'}
$bitmap=New-Object System.Drawing.Bitmap(($rect.right-$rect.left),($rect.bottom-$rect.top))
$graphics=[System.Drawing.Graphics]::FromImage($bitmap)
try {
    if($Composed){
        $origin=[CapyWindowCapture+Point]::new();$origin.x=$rect.left;$origin.y=$rect.top
        if($ClientOnly -and ![CapyWindowCapture]::ClientToScreen($handle,[ref]$origin)){throw 'Client origin unavailable'}
        $graphics.CopyFromScreen($origin.x,$origin.y,0,0,$bitmap.Size)
    }else{
        $dc=$graphics.GetHdc()
        try {
            $flags=if($ClientOnly){3}else{2}
            if(![CapyWindowCapture]::PrintWindow($handle,$dc,$flags)){throw 'Window capture failed.'}
        } finally {$graphics.ReleaseHdc($dc)}
    }
    if(![IO.Path]::IsPathRooted($Output)){$Output=Join-Path (Get-Location) $Output}
    $bitmap.Save($Output,[System.Drawing.Imaging.ImageFormat]::Png)
} finally {$graphics.Dispose();$bitmap.Dispose()}
} finally {[CapyWindowCapture]::SetThreadDpiAwarenessContext($captureDpi)|Out-Null}
