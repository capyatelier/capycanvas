param([Parameter(Mandatory)][string]$Executable,[Parameter(Mandatory)][string]$FixtureFile)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyNumberCapture {
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int left,top,right,bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
 [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window,out Rect rect);
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,ref Point point);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr window,IntPtr dc,uint flags);
}
'@
[CapyNumberCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$FixtureFile=(Resolve-Path -LiteralPath $FixtureFile).Path
$fixture=Get-Content -LiteralPath $FixtureFile -Raw|ConvertFrom-Json
if($fixture.schema -ne 1 -or $fixture.name -notin @('windows-light','windows-dark')){throw 'Expected the synthetic numeric fixture'}
$output=Split-Path -Parent $FixtureFile
$metadata=Join-Path $output ("native-"+$fixture.name+'.json')
$previousFixture=$env:CAPY_NUMBER_FIXTURE
try{
    $env:CAPY_NUMBER_FIXTURE=$FixtureFile
    $review=Start-Process -FilePath $Executable -WorkingDirectory (Split-Path -Parent $Executable) -WindowStyle Hidden -PassThru
}finally{$env:CAPY_NUMBER_FIXTURE=$previousFixture}
$null=$review.Handle
[IO.File]::WriteAllText((Join-Path $output 'review-process.txt'),[string]$review.Id)
function Wait-Until([scriptblock]$Condition,[string]$Message){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{
        if(& $Condition){return}
        $review.Refresh();if($review.HasExited){throw "Numeric fixture exited with code $($review.ExitCode): $Message"}
        Start-Sleep -Milliseconds 100
    }while($watch.Elapsed.TotalSeconds -lt 45)
    throw "$Message (owned process $($review.Id) retained for inspection)"
}
Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero} 'Numeric review window did not open'
$handle=$review.MainWindowHandle
Wait-Until {
    try{$script:report=Get-Content -LiteralPath $metadata -Raw|ConvertFrom-Json;$report.process_id -eq $review.Id}catch{$false}
} 'Numeric display, bounds and geometry did not settle'
$root=[System.Windows.Automation.AutomationElement]::FromHandle($handle)
$surface=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
    [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,'number-review-surface'))
if(!$surface){throw 'Numeric review surface is missing from accessibility'}
$bounds=$surface.Current.BoundingRectangle
$width=$fixture.width*$report.scale;$height=$fixture.height*$report.scale
if($bounds.Width -ne $width -or $bounds.Height -ne $height){throw "Incomplete numeric surface: $bounds"}
$client=[CapyNumberCapture+Rect]::new();$origin=[CapyNumberCapture+Point]::new()
if(![CapyNumberCapture]::GetClientRect($handle,[ref]$client) -or ![CapyNumberCapture]::ClientToScreen($handle,[ref]$origin)){throw 'Cannot measure review client'}
$offset=@([int]($bounds.X-$origin.x),[int]($bounds.Y-$origin.y))
$bitmap=[Drawing.Bitmap]::new($client.right,$client.bottom,[Drawing.Imaging.PixelFormat]::Format24bppRgb)
$graphics=[Drawing.Graphics]::FromImage($bitmap);$dc=$graphics.GetHdc()
try{if(![CapyNumberCapture]::PrintWindow($handle,$dc,3)){throw 'Numeric control capture failed'}}
finally{$graphics.ReleaseHdc($dc);$graphics.Dispose()}
try{
    $bitmap.Save((Join-Path $output ("client-"+$fixture.name+'.png')),[Drawing.Imaging.ImageFormat]::Png)
    $rectangle=[Drawing.Rectangle]::new($offset[0],$offset[1],[int]$width,[int]$height)
    $content=$bitmap.Clone($rectangle,[Drawing.Imaging.PixelFormat]::Format24bppRgb)
    try{$content.Save((Join-Path $output ("native-"+$fixture.name+'.png')),[Drawing.Imaging.ImageFormat]::Png)}finally{$content.Dispose()}
}finally{$bitmap.Dispose()}
$report|Add-Member -NotePropertyName capture -NotePropertyValue @{
    client_size=@($client.right,$client.bottom);surface_offset=$offset;surface_size=@($width,$height)
    boundary='Complete numeric review surface; raw client retained; no application pixels masked or rescaled'
}
[IO.File]::WriteAllText($metadata,($report|ConvertTo-Json -Depth 80))
$root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close()
if(!$review.WaitForExit(15000)){throw "Owned numeric review did not close: $($review.Id)"}
if($review.ExitCode -ne 0){throw "Numeric review failed: $($review.ExitCode)"}
[pscustomobject]@{fixture=$fixture.name;controls=30;scale=$report.scale;display_and_step_availability='passed';exit_code=$review.ExitCode;scope='Production numeric controls; browser geometry and raster comparison still required'}|ConvertTo-Json
