param([Parameter(Mandatory)][string]$Destination,[string]$Icon)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName PresentationCore,WindowsBase
if([Threading.Thread]::CurrentThread.GetApartmentState() -ne 'STA'){throw 'Generate package logos from an STA PowerShell session.'}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
[xml]$svg=Get-Content (Join-Path $repo 'apps/layer-web/icons/layer-zen-looking-up-symbolic.svg') -Raw
$paths=@($svg.DocumentElement.ChildNodes|Where-Object NodeType -eq Element)
if($paths.Count -ne 1 -or $paths[0].LocalName -ne 'path' -or $paths[0].GetAttribute('fill') -ne 'currentColor' -or $paths[0].GetAttribute('fill-rule') -ne 'evenodd'){throw 'Expected the shared symbolic capybara mark.'}
$view=@($svg.DocumentElement.GetAttribute('viewBox').Split(' ')|ForEach-Object {[double]::Parse($_,[Globalization.CultureInfo]::InvariantCulture)})
if($view.Count -ne 4 -or $view[2] -ne $view[3]){throw 'Expected a square brand viewBox.'}
[IO.Directory]::CreateDirectory($Destination)|Out-Null
function Render([int]$size){
    # Match the shared GTK app icon's background, padding and symbolic ink.
    $visual=[Windows.Media.DrawingVisual]::new();$draw=$visual.RenderOpen()
    try{
        $background=[Windows.Media.BrushConverter]::new().ConvertFromString('#767676')
        $ink=[Windows.Media.BrushConverter]::new().ConvertFromString('#f6f5f4')
        $draw.DrawRoundedRectangle($background,$null,[Windows.Rect]::new(0,0,$size,$size),$size*.15,$size*.15)
        $geometry=[Windows.Media.Geometry]::Parse('F0 '+$paths[0].GetAttribute('d')).Clone()
        $scale=$size*(440.0/512.0)/$view[2]
        $inset=$size*(36.0/512.0)
        $geometry.Transform=[Windows.Media.MatrixTransform]::new($scale,0,0,$scale,$inset-$view[0]*$scale,$inset-$view[1]*$scale)
        $draw.DrawGeometry($ink,$null,$geometry)
    }finally{$draw.Close()}
    $bitmap=[Windows.Media.Imaging.RenderTargetBitmap]::new($size,$size,96,96,[Windows.Media.PixelFormats]::Pbgra32)
    $bitmap.Render($visual)
    $encoder=[Windows.Media.Imaging.PngBitmapEncoder]::new()
    $encoder.Frames.Add([Windows.Media.Imaging.BitmapFrame]::Create($bitmap))
    $stream=[IO.MemoryStream]::new()
    try{$encoder.Save($stream);,$stream.ToArray()}finally{$stream.Dispose()}
}
foreach($size in @(44,50,150)){[IO.File]::WriteAllBytes((Join-Path $Destination ("Logo"+$size+'.png')),(Render $size))}
if($Icon){
    $frames=@(foreach($size in @(16,24,32,48,64,256)){,@($size,(Render $size))})
    $stream=[IO.File]::Create($Icon);$writer=[IO.BinaryWriter]::new($stream)
    try{
        $writer.Write([uint16]0);$writer.Write([uint16]1);$writer.Write([uint16]$frames.Count)
        $offset=6+16*$frames.Count
        foreach($frame in $frames){
            $writer.Write([byte]($frame[0]%256));$writer.Write([byte]($frame[0]%256));$writer.Write([byte]0);$writer.Write([byte]0)
            $writer.Write([uint16]1);$writer.Write([uint16]32);$writer.Write([uint32]$frame[1].Length);$writer.Write([uint32]$offset)
            $offset+=$frame[1].Length
        }
        foreach($frame in $frames){$writer.Write([byte[]]$frame[1])}
    }finally{$writer.Dispose();$stream.Dispose()}
}
