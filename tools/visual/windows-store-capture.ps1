param(
 [Parameter(Mandatory=$true)][string]$Recipe,
 [Parameter(Mandatory=$true)][string]$Executable,
 [Parameter(Mandatory=$true)][string]$Output,
 [Parameter(Mandatory=$true)][string[]]$Locales,
 [string[]]$Scenes=@('paint','sketch','photo'),
 [string[]]$Themes=@('light','dark')
)
$ErrorActionPreference='Stop'
$repo=[IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$Recipe=(Resolve-Path $Recipe).Path
$Executable=(Resolve-Path $Executable).Path
$Output=[IO.Path]::GetFullPath($Output)
if(Test-Path $Output){throw 'Use a new capture output directory'}
[IO.Directory]::CreateDirectory($Output)|Out-Null
foreach($locale in $Locales){foreach($scene in $Scenes){foreach($theme in $Themes){
 & (Join-Path $repo 'apps/layer-windows/scripts/test-desktop.ps1') `
  -Script (Join-Path $repo 'apps/layer-windows/scripts/capture-store.ps1') `
  -Parameters @{Recipe=$Recipe;Executable=$Executable;Output=$Output;Scene=$scene;Theme=$theme;Locale=$locale}
}}}
