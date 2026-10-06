param([int]$ProcessId, [string]$Output = 'artifacts/windows/window.png', [switch]$ClientOnly, [switch]$Composed, [long]$WindowHandle=0)
$ErrorActionPreference='Stop'
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
$p=Get-Process -Id $ProcessId
$p.Refresh()
$handle=if($WindowHandle){[IntPtr]$WindowHandle}else{$p.MainWindowHandle}
if($handle -eq 0){throw 'The process has no main window yet.'}
$root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
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
[pscustomobject]@{title=$p.MainWindowTitle;responding=$p.Responding;elements=@($names)} | ConvertTo-Json -Depth 4
[CapyWindowCapture]::SetThreadDpiAwarenessContext([IntPtr](-4)) | Out-Null
$rect=New-Object CapyWindowCapture+Rect
if($ClientOnly){[CapyWindowCapture]::GetClientRect($handle,[ref]$rect)|Out-Null}else{[CapyWindowCapture]::GetWindowRect($handle,[ref]$rect)|Out-Null}
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
