param([Parameter(Mandatory)][string]$Executable,[Parameter(Mandatory)][string]$FixtureFile)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyWaitSeconds=45
Add-Type -AssemblyName System.Drawing
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
function Assert-NumberValueInk([Drawing.Bitmap]$Bitmap,$Report,[int[]]$Offset){
    $background=[Drawing.ColorTranslator]::FromHtml($Report.palette.input)
    $records=@(foreach($part in $Report.frames.PSObject.Properties|Where-Object Name -like '*:entry'){
        $frame=$part.Value;$padding=[double]$Report.panel_metrics.field_padding;$scale=[double]$Report.scale
        $inset=($frame.height-20)/2
        $left=[int][Math]::Ceiling($Offset[0]+($frame.x+$padding)*$scale)
        $right=[int][Math]::Floor($Offset[0]+($frame.x+$frame.width-$padding)*$scale)
        $top=[int][Math]::Ceiling($Offset[1]+($frame.y+$inset)*$scale)
        $bottom=[int][Math]::Floor($Offset[1]+($frame.y+$frame.height-$inset)*$scale)
        if($left-lt0-or$top-lt0-or$right-gt$Bitmap.Width-or$bottom-gt$Bitmap.Height-or$left-ge$right-or$top-ge$bottom){throw "Invalid numeric value pixel region: $($part.Name)"}
        $ink=0
        for($y=$top;$y-lt$bottom;$y++){for($x=$left;$x-lt$right;$x++){
            $pixel=$Bitmap.GetPixel($x,$y)
            if($pixel.R-ne$background.R-or$pixel.G-ne$background.G-or$pixel.B-ne$background.B){$ink++}
        }}
        if(!$ink){throw "Rendered numeric value is blank: $($part.Name)"}
        [ordered]@{part=$part.Name;ink_pixels=$ink;region=@($left,$top,($right-$left),($bottom-$top))}
    })
    if(!$records.Count){throw 'Numeric review contains no rendered spin value regions'}
    $records
}
[CapyNumberCapture]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$FixtureFile=(Resolve-Path -LiteralPath $FixtureFile).Path
$fixture=Read-Snapshot $FixtureFile
if($fixture.schema -ne 1 -or $fixture.name -notin @('windows-light','windows-dark')){throw 'Expected the synthetic numeric fixture'}
if(!$fixture.panel_metrics -or @($fixture.rows|Where-Object{
    !$_.labels -or $_.labels.edit -isnot [string] -or $_.labels.decrease -isnot [string] -or $_.labels.increase -isnot [string] -or
    !$_.maximum -or $_.maximum.text -isnot [string]
}).Count){throw 'Numeric fixture lacks shared labels, maximum formatting or panel metrics; generate fresh inputs'}
$output=Split-Path -Parent $FixtureFile
$metadata=Join-Path $output ("native-"+$fixture.name+'.json')
$previousFixture=$env:CAPY_NUMBER_FIXTURE
try{
    $env:CAPY_NUMBER_FIXTURE=$FixtureFile
    $review=Start-Process -FilePath $Executable -WorkingDirectory (Split-Path -Parent $Executable) -WindowStyle Hidden -PassThru
}finally{$env:CAPY_NUMBER_FIXTURE=$previousFixture}
$null=$review.Handle
[IO.File]::WriteAllText((Join-Path $output 'review-process.txt'),[string]$review.Id)
Wait-Until {@(Owned-ProcessWindows $review).Count -gt 0} 'Numeric review window did not open'
Wait-Until {
    try{
        $script:report=Read-Snapshot $metadata
        if($report.process_id -ne $review.Id){return $false}
        $window=Owned-ProcessSurface $review 'number-review-surface'
        if(!$window){return $false}
        $script:handle=$window.Handle;$script:root=$window.Root;$script:surface=$window.Surface
        $true
    }catch{$false}
} 'Numeric display, bounds and geometry did not settle'
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
    $report|Add-Member -NotePropertyName rendered_value_ink -NotePropertyValue @(Assert-NumberValueInk $bitmap $report $offset)
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
