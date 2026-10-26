$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$path=Join-Path $PSScriptRoot '../scripts/exercise-canvas-editing.ps1'
$ast=[Management.Automation.Language.Parser]::ParseFile($path,[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$function=@($ast.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Assert-TranslatedSamples'},$true))
if($function.Count -ne 1){throw 'Missing actual translation pixel assertion'}
. ([scriptblock]::Create($function[0].Extent.Text))

function Patch([int]$Level,[int]$Grain){
    $bytes=[byte[]]::new(16*16*3)
    for($i=0;$i -lt $bytes.Length;$i++){$bytes[$i]=$Level+($i*7+$Grain)%5-2}
    $hash=[Security.Cryptography.SHA256]::Create()
    try{[BitConverter]::ToString($hash.ComputeHash($bytes)).Replace('-','')}finally{$hash.Dispose()}
}
$paper=@((Patch 240 1),(Patch 240 17),(Patch 240 33))
$ink=@((Patch 46 1),(Patch 46 17),(Patch 46 33))
if($ink[0] -eq $ink[1]){throw 'The location-varying ink test setup is invalid'}
$blank=$paper -join ':'
$before=@($ink[0],$paper[1],$ink[2]) -join ':'
$after=@($paper[0],$ink[1],$ink[2]) -join ':'
Assert-TranslatedSamples $blank $before $after
$checks=1
function Reject([string]$Name,[string]$Blank,[string]$Before,[string]$After){
    $rejected=$false
    try{Assert-TranslatedSamples $Blank $Before $After}catch{$rejected=$true}
    if(!$rejected){throw "Translation assertion accepted $Name"}
    $script:checks++
}
Reject 'uncleared source' $blank $before (@($ink[0],$ink[1],$ink[2]) -join ':')
Reject 'blank destination' $blank $before (@($paper[0],$paper[1],$ink[2]) -join ':')
Reject 'corrupted unselected ink' $blank $before (@($paper[0],$ink[1],$paper[2]) -join ':')
Reject 'empty source baseline' $blank (@($paper[0],$paper[1],$ink[2]) -join ':') $after
Reject 'occupied destination baseline' $blank (@($ink[0],$ink[1],$ink[2]) -join ':') $after
Reject 'empty unselected baseline' $blank (@($ink[0],$paper[1],$paper[2]) -join ':') $after
Reject 'missing source sample' ($paper[0..1] -join ':') $before $after
Reject 'missing baseline sample' $blank '' $after
Reject 'missing translated sample' $blank $before ($paper[0]+':'+$ink[1])
Reject 'empty translated patch' $blank $before ($paper[0]+'::'+$ink[2])
Write-Output "Canvas editing translation pixels passed $checks cases using the actual assertion: location-varying grain, source/destination/unselected corruption and nonvacuous baseline guards; no UI or GPU."
