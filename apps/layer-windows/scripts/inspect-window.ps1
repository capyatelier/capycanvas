param([int]$ProcessId, [string]$Output = 'artifacts/windows/window.png', [switch]$ClientOnly)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyWindowCapture {
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int left,top,right,bottom; }
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr context);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window, IntPtr dc, uint flags);
}
'@
$p=Get-Process -Id $ProcessId
$p.Refresh()
$handle=$p.MainWindowHandle
if($handle -eq 0){throw 'The process has no main window yet.'}
$root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
$nodes=$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)
$names=foreach($node in $nodes){if($node.Current.Name){$node.Current.Name}}
[pscustomobject]@{title=$p.MainWindowTitle;responding=$p.Responding;elements=@($names)} | ConvertTo-Json -Depth 4
[CapyWindowCapture]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$rect=New-Object CapyWindowCapture+Rect
if($ClientOnly){[CapyWindowCapture]::GetClientRect($handle,[ref]$rect)|Out-Null}else{[CapyWindowCapture]::GetWindowRect($handle,[ref]$rect)|Out-Null}
$bitmap=New-Object System.Drawing.Bitmap(($rect.right-$rect.left),($rect.bottom-$rect.top))
$graphics=[System.Drawing.Graphics]::FromImage($bitmap)
$dc=$graphics.GetHdc()
try {
    $flags=if($ClientOnly){3}else{2}
    if(![CapyWindowCapture]::PrintWindow($handle,$dc,$flags)){throw 'Window capture failed.'}
} finally {$graphics.ReleaseHdc($dc)}
if(![IO.Path]::IsPathRooted($Output)){$Output=Join-Path (Get-Location) $Output}
try {$bitmap.Save($Output,[System.Drawing.Imaging.ImageFormat]::Png)}
finally {$graphics.Dispose();$bitmap.Dispose()}
