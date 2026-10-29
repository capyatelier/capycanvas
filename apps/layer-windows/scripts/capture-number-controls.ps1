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
function Measure-RenderedInk([Drawing.Bitmap]$Bitmap,[int[]]$Region,[Drawing.Color]$Background,[Drawing.Color]$Foreground=[Drawing.Color]::Empty){
    $left,$top,$width,$height=$Region;$right=$left+$width;$bottom=$top+$height
    if($left-lt0-or$top-lt0-or$right-gt$Bitmap.Width-or$bottom-gt$Bitmap.Height-or$width-le0-or$height-le0){throw 'Invalid rendered ink pixel region'}
    $contrast=[Math]::Pow($Foreground.R-$Background.R,2)+[Math]::Pow($Foreground.G-$Background.G,2)+[Math]::Pow($Foreground.B-$Background.B,2)
    if(!$Foreground.IsEmpty -and !$contrast){throw 'Rendered caption has no foreground contrast'}
    $ink=0;$minX=$right;$minY=$bottom;$maxX=-1;$maxY=-1
    for($y=$top;$y-lt$bottom;$y++){for($x=$left;$x-lt$right;$x++){
        $pixel=$Bitmap.GetPixel($x,$y)
        $distance=[Math]::Pow($pixel.R-$Foreground.R,2)+[Math]::Pow($pixel.G-$Foreground.G,2)+[Math]::Pow($pixel.B-$Foreground.B,2)
        $matches=if($Foreground.IsEmpty){$pixel.R-ne$Background.R-or$pixel.G-ne$Background.G-or$pixel.B-ne$Background.B}else{$distance-le$contrast/16}
        if($matches){$ink++;$minX=[Math]::Min($minX,$x);$minY=[Math]::Min($minY,$y);$maxX=[Math]::Max($maxX,$x);$maxY=[Math]::Max($maxY,$y)}
    }}
    [ordered]@{ink_pixels=$ink;region=$Region;ink_bounds=$(if($ink){@($minX,$minY,($maxX-$minX+1),($maxY-$minY+1))}else{@()})}
}
function Assert-NumberValueInk([Drawing.Bitmap]$Bitmap,$Report,[int[]]$Offset){
    $background=[Drawing.ColorTranslator]::FromHtml($Report.palette.input)
    $records=@(foreach($part in $Report.frames.PSObject.Properties|Where-Object Name -like '*:entry'){
        $frame=$part.Value;$padding=[double]$Report.panel_metrics.field_padding;$scale=[double]$Report.scale
        $inset=($frame.height-20)/2
        $left=[int][Math]::Ceiling($Offset[0]+($frame.x+$padding)*$scale)
        $right=[int][Math]::Floor($Offset[0]+($frame.x+$frame.width-$padding)*$scale)
        $top=[int][Math]::Ceiling($Offset[1]+($frame.y+$inset)*$scale)
        $bottom=[int][Math]::Floor($Offset[1]+($frame.y+$frame.height-$inset)*$scale)
        $record=Measure-RenderedInk $Bitmap @($left,$top,($right-$left),($bottom-$top)) $background
        if(!$record.ink_pixels){throw "Rendered numeric value is blank: $($part.Name)"}
        $record['part']=$part.Name;$record
    })
    if(!$records.Count){throw 'Numeric review contains no rendered spin value regions'}
    $records
}
function Assert-ControlCaptionInk([string]$Image,[double[]]$ControlBounds,[double[]]$CaptionBounds,[string]$Foreground,[string]$Output,[double]$Scale=1){
    if($Scale -le 0){throw 'Invalid caption capture scale'}
    $bitmap=[Drawing.Bitmap]::new($Image)
    try{
        $left=[int][Math]::Ceiling($CaptionBounds[0]);$right=[int][Math]::Floor($CaptionBounds[0]+$CaptionBounds[2])
        $top=[int][Math]::Ceiling($ControlBounds[1]+1);$bottom=[int][Math]::Floor($ControlBounds[1]+$ControlBounds[3]-1)
        $background=$bitmap.GetPixel([int](($left+$right)/2),$top)
        $record=Measure-RenderedInk $bitmap @($left,$top,($right-$left),($bottom-$top)) $background ([Drawing.ColorTranslator]::FromHtml($Foreground))
        if(!$record.ink_pixels){throw 'Rendered control caption is blank'}
        $record['control_bounds']=$ControlBounds;$record['caption_bounds']=$CaptionBounds;$record['foreground']=$Foreground;$record['minimum_foreground_coverage']=.75
        $record['scale']=$Scale;$record['control_height_dip']=$ControlBounds[3]/$Scale;$record['control_height_physical_pixels']=$ControlBounds[3]
        $record['top_gap']=$record.ink_bounds[1]-$ControlBounds[1]
        $record['bottom_gap']=$ControlBounds[1]+$ControlBounds[3]-$record.ink_bounds[1]-$record.ink_bounds[3]
        $record['gap_delta']=[Math]::Abs($record.top_gap-$record.bottom_gap);$record['tolerance_physical_pixels']=.5
        $centeredTop=$ControlBounds[1]+($ControlBounds[3]-$record.ink_bounds[3])/2
        $record['minimum_raster_gap_delta']=2*[Math]::Abs($centeredTop-[Math]::Round($centeredTop))
        $record['excess_gap_delta']=$record.gap_delta-$record.minimum_raster_gap_delta
        $record['raster_alignment_epsilon']=1e-6
        $record|ConvertTo-Json -Depth 6|Set-Content ($Output+'-ink.json')
        $crop=[Drawing.Rectangle]::FromLTRB([int][Math]::Floor($ControlBounds[0]),[int][Math]::Floor($ControlBounds[1]),[int][Math]::Ceiling($ControlBounds[0]+$ControlBounds[2]),[int][Math]::Ceiling($ControlBounds[1]+$ControlBounds[3]))
        $raw=$bitmap.Clone($crop,[Drawing.Imaging.PixelFormat]::Format24bppRgb)
        try{
            $raw.Save(($Output+'-raw.png'),[Drawing.Imaging.ImageFormat]::Png)
            $zoom=[Drawing.Bitmap]::new($raw.Width*4,$raw.Height*4);$graphics=[Drawing.Graphics]::FromImage($zoom)
            try{$graphics.InterpolationMode=[Drawing.Drawing2D.InterpolationMode]::NearestNeighbor;$graphics.PixelOffsetMode=[Drawing.Drawing2D.PixelOffsetMode]::Half;$graphics.DrawImage($raw,[Drawing.Rectangle]::new(0,0,$zoom.Width,$zoom.Height));$zoom.Save(($Output+'-4x.png'),[Drawing.Imaging.ImageFormat]::Png)}finally{$graphics.Dispose();$zoom.Dispose()}
        }finally{$raw.Dispose()}
        if($record.gap_delta -gt .5 -and $record.excess_gap_delta -gt $record.raster_alignment_epsilon){throw "Control caption ink gaps exceed the nearest raster center by $($record.excess_gap_delta) physical pixels"}
        $record
    }finally{$bitmap.Dispose()}
}
function Save-ControlCaptionInk($Control,[IntPtr]$Window,[string]$Image,[string]$Foreground,[string]$Output){
    $caption=@($Control.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Text))|Where-Object {$_.Current.Name -ceq $Control.Current.Name -and !$_.Current.IsOffscreen -and !$_.Current.BoundingRectangle.IsEmpty})
    if($caption.Count -ne 1){throw "Control has no unique arranged caption: $($Control.Current.Name)"}
    $origin=[CapyRowPointer+Point]::new()
    if(![CapyRowPointer]::ClientToScreen($Window,[ref]$origin)){throw 'Caption capture client origin is unavailable'}
    $button=$Control.Current.BoundingRectangle;$label=$caption[0].Current.BoundingRectangle
    Assert-ControlCaptionInk $Image @(($button.X-$origin.x),($button.Y-$origin.y),$button.Width,$button.Height) @(($label.X-$origin.x),($label.Y-$origin.y),$label.Width,$label.Height) $Foreground $Output ([CapyRowPointer]::GetDpiForWindow($Window)/96.)
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
