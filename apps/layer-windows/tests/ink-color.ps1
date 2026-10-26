$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-documents.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'Document fixture parse failed'}
$helper=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Ink-ColorMatches'},$true)
. ([scriptblock]::Create($helper.Extent.Text))
foreach($case in @(
    @{space='Srgb';encoded=@(0,0,0);linear=@(0,0,0)},
    @{space='Srgb';encoded=@(.5,.01,-.5);linear=@(.21404114048223255,.0007739938080495357,-.21404114048223255)},
    @{space='DisplayP3';encoded=@(.5,1,0);linear=@(.21404114048223255,1,0)},
    @{space='AdobeRgb';encoded=@(.5,-.5,1);linear=@(.21775552814439456,-.21775552814439456,1)},
    @{space='ProPhoto';encoded=@(.5,-.5,.02);linear=@(.2871745887492587,-.2871745887492587,.00125)}
)){
    $definition=[pscustomobject]@{space=$case.space;rgba=@($case.encoded)+@(1)}
    if(!(Ink-ColorMatches $definition $case.linear)){throw "Encoded $($case.space) ink did not match linear RGB"}
    $different=@($case.linear);$different[1]+=.001
    if(Ink-ColorMatches $definition $different){throw 'Mismatched ink passed'}
}
$definition=[pscustomobject]@{space='Srgb';rgba=@(.18974828,.22091636,.17184409,1);linear_rgb=@(.03,.04,.025)}
if(!(Ink-ColorMatches $definition @(.03,.04,.025))){throw 'Preserved linear ink did not match'}
$definition.rgba=@(0,0,0,1)
if(Ink-ColorMatches $definition @(0,0,0)){throw 'Encoded values replaced the authored linear ink'}
$definition.rgba[3]=.5
if(Ink-ColorMatches $definition @(.03,.04,.025)){throw 'Translucent ink passed'}
$definition.rgba[3]=1;$definition.linear_rgb=@(.03,[double]::NaN,.025)
if(Ink-ColorMatches $definition @(.03,.04,.025)){throw 'Nonfinite ink passed'}
Write-Output 'Encoded and preserved linear ink checks passed'
