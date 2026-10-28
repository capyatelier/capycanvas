$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'UI Automation helpers do not parse'}
foreach($name in 'Wait-PresentedFrame','Wait-StablePixels'){
    $helper=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true)
    if(!$helper){throw "Pixel stability helper is missing: $name"}
    . ([scriptblock]::Create($helper.Extent.Text))
}
function Capture-CallerPixels{
    $sample=[pscustomobject]@{kind='column';x=1640;y=116;pixels='owned-pixels'}
    Wait-StablePixels {"$($sample.kind):$($sample.x),$($sample.y):$($sample.pixels)"}
}
if((Capture-CallerPixels) -cne 'column:1640,116:owned-pixels'){throw 'Pixel stability helper shadowed the caller sample'}
'Pixel stability caller sample: passed'
$slow=@{captures=0}
$result=Wait-StablePixels -Composed {$slow.captures++;Start-Sleep -Milliseconds 1750;'owned-composed-pixels'}
if($result -cne 'owned-composed-pixels' -or $slow.captures -ne 4){throw 'Slow composed capture did not establish three matching comparisons'}
$changing=@{captures=0};$refused=$false
try{Wait-StablePixels -Composed {$changing.captures++;"frame:$($changing.captures)"}|Out-Null}
catch{if(!$_.Exception.Message.StartsWith('Canvas pixels did not settle for comparison')){throw};$refused=$true}
if(!$refused){throw 'Changing composed pixels were accepted as stable'}
'Slow and changing composed captures: passed'
