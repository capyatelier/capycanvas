param([Parameter(Mandatory)][string]$Executable,[Parameter(Mandatory)][string]$FixtureFile)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=45
Add-Type -AssemblyName System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyColorCapture {
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window,out Rect rect);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,ref Point point);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr window);
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
}
'@
[CapyColorCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$FixtureFile=(Resolve-Path -LiteralPath $FixtureFile).Path
$fixture=Get-Content -LiteralPath $FixtureFile -Raw|ConvertFrom-Json
if($fixture.schema -ne 2 -or $fixture.name -notmatch '^(dark|light)-(128|160|226|360)$'){throw 'Expected the synthetic color fixture'}
$captureState=if($fixture.capture_state){[string]$fixture.capture_state}else{'default'}
if($captureState -notin @('default','readout-focus','shape-focus','swatch-focus','swap-focus','shape-hover','swatch-hover','swap-hover')){throw 'Unknown color capture state'}
if($captureState -ne 'default' -and $fixture.capture_target -notin @('color-readout','color-shape-0','color-background','color-swap')){throw 'Unknown capture target'}
$output=Split-Path -Parent $FixtureFile
$metadata=Join-Path $output ("native-"+$fixture.name+'.json')
$previousFixture=$env:CAPY_COLOR_FIXTURE
try{
    $env:CAPY_COLOR_FIXTURE=$FixtureFile
    $review=Start-Process -FilePath $Executable -WorkingDirectory (Split-Path -Parent $Executable) -WindowStyle Hidden -PassThru
}finally{$env:CAPY_COLOR_FIXTURE=$previousFixture}
$null=$review.Handle
[IO.File]::WriteAllText((Join-Path $output 'review-process.txt'),[string]$review.Id)
Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero} 'Color review window did not open'
$handle=$review.MainWindowHandle
Wait-Until {
    try{$script:report=Get-Content -LiteralPath $metadata -Raw|ConvertFrom-Json;$report.process_id -eq $review.Id}catch{$false}
} 'Color display, bounds and geometry did not settle'
$root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
$surface=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'color-review-surface'))
if(!$surface){throw 'Color review surface is missing from accessibility'}
$surfaceBounds=$surface.Current.BoundingRectangle
[CapyColorCapture]::SetForegroundWindow($handle)|Out-Null
if([CapyColorCapture]::GetForegroundWindow() -ne $handle){throw 'Review does not own foreground input'}
[CapyRowPointer]::Initialize([uint32]$review.Id)
try{
# Deliver a real mouse move; SetCursorPos alone need not enter WinUI pointer-over state.
# The fixture's one-pixel outer gap is outside every production control.
[CapyRowPointer]::Hover([int]$surfaceBounds.X+1,[int]$surfaceBounds.Y+1)
Start-Sleep -Milliseconds 100
if($captureState.EndsWith('-hover')){
    $target=$surface.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,[string]$fixture.capture_target))
    if(!$target){throw 'Hover target is missing'}
    $targetBounds=$target.Current.BoundingRectangle
    [CapyRowPointer]::Hover([int]($targetBounds.X+$targetBounds.Width*.5),[int]($targetBounds.Y+$targetBounds.Height*.5))
    Start-Sleep -Milliseconds 300
}
}finally{[CapyRowPointer]::Dispose()}
$bounds=$surface.Current.BoundingRectangle
$width=$fixture.width*$report.scale;$height=$fixture.height*$report.scale
if($bounds.Width -ne $width -or $bounds.Height -ne $height){throw "Incomplete color surface: $bounds"}
$client=[CapyColorCapture+Rect]::new();$origin=[CapyColorCapture+Point]::new()
if(![CapyColorCapture]::GetClientRect($handle,[ref]$client) -or ![CapyColorCapture]::ClientToScreen($handle,[ref]$origin)){throw 'Cannot measure review client'}
$offset=@([int]($bounds.X-$origin.x),[int]($bounds.Y-$origin.y))
$bitmap=[Drawing.Bitmap]::new($client.right,$client.bottom,[Drawing.Imaging.PixelFormat]::Format24bppRgb)
$graphics=[Drawing.Graphics]::FromImage($bitmap);$dc=$graphics.GetHdc()
try{if(![CapyColorCapture]::PrintWindow($handle,$dc,3)){throw 'Color control capture failed'}}
finally{$graphics.ReleaseHdc($dc);$graphics.Dispose()}
try{
    $bitmap.Save((Join-Path $output ("client-"+$fixture.name+'.png')),[Drawing.Imaging.ImageFormat]::Png)
    $rectangle=[Drawing.Rectangle]::new($offset[0],$offset[1],[int]$width,[int]$height)
    $content=$bitmap.Clone($rectangle,[Drawing.Imaging.PixelFormat]::Format24bppRgb)
    try{$content.Save((Join-Path $output ("native-"+$fixture.name+'.png')),[Drawing.Imaging.ImageFormat]::Png)}finally{$content.Dispose()}
}finally{$bitmap.Dispose()}
$report|Add-Member -NotePropertyName capture -NotePropertyValue @{
    client_size=@($client.right,$client.bottom);surface_offset=$offset;surface_size=@($width,$height)
    boundary='Complete color review surface; raw client retained; no application pixels masked or rescaled'
}
[IO.File]::WriteAllText($metadata,($report|ConvertTo-Json -Depth 80))
$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close()
if(!$review.WaitForExit(15000)){throw "Owned color review did not close: $($review.Id)"}
if($review.ExitCode -ne 0){throw "Color review failed: $($review.ExitCode)"}
[pscustomobject]@{fixture=$fixture.name;controls=6;scale=$report.scale;complete_surface='passed';exit_code=$review.ExitCode;scope='Production color controls; browser geometry and raster comparison still required'}|ConvertTo-Json
