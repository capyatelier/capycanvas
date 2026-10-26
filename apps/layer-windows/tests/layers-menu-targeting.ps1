$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-layers.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Layers fixture does not parse'}
$helper=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Invoke'},$true)
. ([scriptblock]::Create($helper.Extent.Text))
$command=$ast.Find({param($node)$node -is [Management.Automation.Language.CommandAst] -and $node.GetCommandName() -eq 'Invoke' -and $node.CommandElements[1].Extent.Text -eq '$filter'},$true)
if(!$command){throw 'Missing filter menu action'}
$invokeFilter=[scriptblock]::Create($command.Extent.Text)
$script:invoked=@()
function New-ItemDouble([string]$Kind){
    $item=[pscustomobject]@{Kind=$Kind}
    $item|Add-Member ScriptMethod GetCurrentPattern {param($pattern)$this}
    $item|Add-Member ScriptMethod Invoke {$script:invoked+= $this.Kind}
    $item
}
$button=New-ItemDouble 'button'
$menu=New-ItemDouble 'menu'
function Find([string]$Value,[switch]$Name,$Type,[switch]$Visible){
    if($Type -eq [System.Windows.Automation.ControlType]::Button){return $button}
    if($Type -eq [System.Windows.Automation.ControlType]::MenuItem -and $script:menuExists -and (!$Visible -or !$script:menuHidden)){return $menu}
}
function Wait-Until([scriptblock]$Condition,[string]$Message){if(!(&$Condition)){throw $Message}}
foreach($filter in 'Exposure','Curves','Levels'){
    $script:menuExists=$true;$script:menuHidden=$false;$script:invoked=@()
    & $invokeFilter
    if($script:invoked.Count -ne 1 -or $script:invoked[0] -ne 'menu'){throw "$filter invoked a same-named toolbar button"}
    foreach($case in 'missing','hidden'){
        $script:menuExists=$case -ne 'missing';$script:menuHidden=$case -eq 'hidden';$script:invoked=@()
        $refused=$false
        try{& $invokeFilter}catch{if($_.Exception.Message -ne "Missing action: $filter"){throw};$refused=$true}
        if(!$refused -or $script:invoked.Count){throw "$filter fell back from a $case menu item"}
    }
}
'Layers filter menu targeting: 9 passed'

foreach($name in 'Stable-PreviewSamples','Stable-ThumbnailPixels'){
    $helper=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true)
    . ([scriptblock]::Create($helper.Extent.Text))
}
function Control([string]$Value,[switch]$Arranged){if(!$Arranged){throw 'Thumbnail must be arranged'};$script:resolves++;$menu}
function Model {$script:models++;[pscustomobject]@{state=[pscustomobject]@{theme='dark';layer_tools=[pscustomobject]@{editing_layer=[pscustomobject]@{mask_selected=$false}}}}}
function Preview-Samples([string]$Id,[string]$Save,$Image){
    if($Image -ne $menu){throw 'Thumbnail element was not reused'}
    if($Save){$script:saves++;if($script:changed){return '2'}}else{$script:samples++}
    '1'
}
function Thumbnail-Pixels([string]$Id,[string]$Save,$Image){Preview-Samples $Id -Save $Save -Image $Image}
function Wait-StablePixels([scriptblock]$Sample){
    $last=& $Sample
    foreach($i in 1..3){if((& $Sample) -ne $last){throw 'Unstable test samples'}}
    $last
}
foreach($name in 'Stable-PreviewSamples','Stable-ThumbnailPixels'){
    foreach($changed in $false,$true){
        $script:resolves=0;$script:models=0;$script:saves=0;$script:samples=0;$script:changed=$changed
        $refused=$false
        try{if($name -eq 'Stable-PreviewSamples'){Stable-PreviewSamples 'thumbnail' 'original'|Out-Null}else{Stable-ThumbnailPixels 'thumbnail'|Out-Null}}catch{
            if($_.Exception.Message -ne 'Saved thumbnail pixels changed after settling'){throw};$refused=$true
        }
        if($refused -ne $changed -or $script:resolves -ne 1 -or $script:samples -ne 4 -or $script:saves -ne 1 -or $script:models -gt 1){throw "$name did not isolate sampling from evidence or verify saved pixels"}
    }
}
'Layers thumbnail sampling and saved evidence: 4 passed'
