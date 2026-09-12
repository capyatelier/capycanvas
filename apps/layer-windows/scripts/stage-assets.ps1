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
