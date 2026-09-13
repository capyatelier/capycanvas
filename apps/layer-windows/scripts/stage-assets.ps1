param([Parameter(Mandatory)][string]$Destination)
$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$web=Join-Path $repo 'apps/layer-web'
$palette=@{dark='#fafafb';light='#2e2e32'}
foreach($theme in @('dark','light')) {
    $icons=Join-Path $Destination "icons/$theme"
    [IO.Directory]::CreateDirectory($icons) | Out-Null
    foreach($file in Get-ChildItem -LiteralPath (Join-Path $web 'icons') -Filter '*.svg' -File) {
        $target=Join-Path $icons $file.Name
        $svg=[IO.File]::ReadAllText($file.FullName).Replace('currentColor',$palette[$theme])
        if(!(Test-Path -LiteralPath $target) -or [IO.File]::ReadAllText($target) -ne $svg) {
            [IO.File]::WriteAllText($target,$svg)
        }
    }
}
$previews=Join-Path $Destination 'brush-previews'
[IO.Directory]::CreateDirectory($previews) | Out-Null
Get-ChildItem -LiteralPath (Join-Path $web 'brush-previews') -Filter '*.png' -File |
    Copy-Item -Destination $previews -Force
$filters=Join-Path $Destination 'filters'
[IO.Directory]::CreateDirectory($filters)|Out-Null
foreach($file in Get-ChildItem -LiteralPath (Join-Path $repo 'assets/filters') -File){
    if($file.Name -eq 'manifest.json' -or $file.Extension -eq '.wgsl'){
        Copy-Item -LiteralPath $file.FullName -Destination (Join-Path $filters $file.Name) -Force
    }
}

# These four monochrome controls rotate and change ink on hover. Keep their
# shared SVG geometry as native vectors, so rotation never resamples a bitmap.
$colorIcons=Join-Path $Destination 'color-icons'
[IO.Directory]::CreateDirectory($colorIcons)|Out-Null
foreach($name in @('square','circle','triangle','swap')){
    [xml]$source=[IO.File]::ReadAllText((Join-Path $web "icons/layer-color-$name-symbolic.svg"))
    $nodes=@($source.DocumentElement.ChildNodes|Where-Object NodeType -eq Element)
    if($nodes.Count -ne 1){throw "Expected one shared color icon shape: $name"}
    $node=$nodes[0]
    if($node.GetAttribute('fill') -ne 'none' -or $node.GetAttribute('stroke') -ne 'currentColor' -or $node.HasAttribute('transform')){throw "Unsupported shared color icon paint or transform: $name"}
    $shape=switch($node.LocalName){
        path { '<Path Data="'+$node.GetAttribute('d')+'" />' }
        circle {
            $r=[double]::Parse($node.GetAttribute('r'),[Globalization.CultureInfo]::InvariantCulture)
            $cx=[double]::Parse($node.GetAttribute('cx'),[Globalization.CultureInfo]::InvariantCulture)
            $cy=[double]::Parse($node.GetAttribute('cy'),[Globalization.CultureInfo]::InvariantCulture)
            $f=[Globalization.CultureInfo]::InvariantCulture
            '<Path Data="M'+($cx-$r).ToString($f)+','+$cy.ToString($f)+' a'+$r.ToString($f)+','+$r.ToString($f)+' 0 1 0 '+(2*$r).ToString($f)+',0 a'+$r.ToString($f)+','+$r.ToString($f)+' 0 1 0 '+(-2*$r).ToString($f)+',0" />'
        }
        rect {
            $f=[Globalization.CultureInfo]::InvariantCulture
            $x=[double]::Parse($node.GetAttribute('x'),$f);$y=[double]::Parse($node.GetAttribute('y'),$f)
            $w=[double]::Parse($node.GetAttribute('width'),$f);$h=[double]::Parse($node.GetAttribute('height'),$f)
            $r=[double]::Parse($node.GetAttribute('rx'),$f)
            $d='M'+($x+$r).ToString($f)+','+$y.ToString($f)+' h'+($w-2*$r).ToString($f)
            $d+=' a'+$r.ToString($f)+','+$r.ToString($f)+' 0 0 1 '+$r.ToString($f)+','+$r.ToString($f)+' v'+($h-2*$r).ToString($f)
            $d+=' a'+$r.ToString($f)+','+$r.ToString($f)+' 0 0 1 '+(-$r).ToString($f)+','+$r.ToString($f)+' h'+(-$w+2*$r).ToString($f)
            $d+=' a'+$r.ToString($f)+','+$r.ToString($f)+' 0 0 1 '+(-$r).ToString($f)+','+(-$r).ToString($f)+' v'+(-$h+2*$r).ToString($f)
            $d+=' a'+$r.ToString($f)+','+$r.ToString($f)+' 0 0 1 '+$r.ToString($f)+','+(-$r).ToString($f)+' Z'
            '<Path Data="'+$d+'" />'
        }
        default { throw "Unsupported shared color icon element: $($node.LocalName)" }
    }
    $shape=$shape.Replace(' />',' StrokeThickness="'+$node.GetAttribute('stroke-width')+'" StrokeLineJoin="Round" StrokeStartLineCap="Round" StrokeEndLineCap="Round" />')
    $xaml='<Canvas xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" Width="16" Height="16">'+$shape+'</Canvas>'
    $target=Join-Path $colorIcons "$name.xaml"
    if(!(Test-Path -LiteralPath $target) -or [IO.File]::ReadAllText($target) -ne $xaml){[IO.File]::WriteAllText($target,$xaml)}
}
