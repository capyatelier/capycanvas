$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'run-fixtures.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
foreach($name in 'Select-Fixtures','Control-Fixture','Reviews','Get-FixtureTimeoutSeconds','Get-FixtureVariantArguments','Get-FixtureCompanionArguments','Write-FixtureResult'){
    $function=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$false)
    . ([scriptblock]::Create($function.Extent.Text))
}
foreach($ending in 'drag','cancel','escape'){if(((Get-FixtureVariantArguments 'switcher-drag' 'ContextEnding' $ending) -join ',') -ne "-ContextHoldOnly,-ContextEnding,$ending"){throw 'Context ending selection lost its required journey scope'}}
if(((Get-FixtureVariantArguments 'popup' 'Theme' 'light') -join ',') -ne '-Theme,light'){throw 'Ordinary fixture variant arguments changed'}
$available=@('header','header:catalog-only','header:catalog-only-light','popup','popup:light','effects')
if(((Select-Fixtures $available @('header-settings')) -join ',') -ne 'header:catalog-only,header:catalog-only-light'){throw 'Header settings must exercise both catalog themes'}
if(((Select-Fixtures $available @('popup:default')) -join ',') -ne 'popup'){throw 'Default selection expanded variants'}
if(((Select-Fixtures $available @('popup')) -join ',') -ne 'popup,popup:light'){throw 'Fixture selection lost theme variants'}
foreach($name in 'button-feedback','sdk-feedback','scroll-indicators','navigator-allocation'){if((Control-Fixture $name) -ne 'Feedback'){throw "Wrong feedback entry point: $name"}}
if((Control-Fixture 'popup') -ne 'Popup' -or $null -ne (Control-Fixture 'effects') -or $null -ne (Control-Fixture 'compact-color')){throw 'Control routing changed the editor entry point'}
if((Control-Fixture 'layer-pickup' @('-PendingDropOnly')) -ne 'LayerDrop' -or $null -ne (Control-Fixture 'layer-pickup') -or $null -ne (Control-Fixture 'layer-pickup' @('-ClippedEdgesOnly'))){throw 'Layer drop overlap must use its separate control executable'}
$companion={param($name) if($name -ne 'LayerDrop'){throw 'Unexpected companion fixture'};'C:\owned\LayerDrop.exe'}
if(((Get-FixtureCompanionArguments 'layer-pickup' @('-Theme','light') $companion) -join ',') -ne '-DropFixture,C:\owned\LayerDrop.exe'){throw 'Layer pickup lost its deterministic final-drop companion'}
foreach($case in @(@{fixture='layer-pickup';arguments=@('-PendingDropOnly')},@{fixture='layer-pickup';arguments=@('-ClippedEdgesOnly')},@{fixture='effects';arguments=@()})){
    if(@(Get-FixtureCompanionArguments $case.fixture $case.arguments {throw 'Unneeded companion build'}).Count){throw 'Companion fixture changed an unrelated journey'}
}
$rejected=$false
try{Select-Fixtures $available @('unknown')}catch{$rejected=$true}
if(!$rejected){throw 'Unknown fixture silently skipped'}
if((Get-FixtureTimeoutSeconds 47 20) -ne 62040){throw 'Matrix timeout lost per-fixture budgets'}
if((Get-FixtureTimeoutSeconds 1 20) -ne 1320){throw 'Single fixture timeout changed'}
if((Get-FixtureTimeoutSeconds 1000 1440) -ne 86400){throw 'Matrix timeout exceeded private desktop upper bound'}
$Output=Join-Path ([IO.Path]::GetTempPath()) ('capy-fixture-owners-'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($Output)|Out-Null
$runnerStarted=Get-Date;$editorExecutable='C:\owned\CapyCanvas.exe';$controlBuilds=@{}
$script:queried=[Collections.Generic.List[int]]::new()
function Get-Process([int]$Id,[string]$ErrorAction){
    $script:queried.Add($Id)
    [pscustomobject]@{Id=$Id;Path=$(if($Id -eq 41){$editorExecutable}else{'C:\other\CapyCanvas.exe'});StartTime=$runnerStarted.AddSeconds(1)}
}
try{
    $resultsFile=Join-Path $Output 'results.jsonl'
    $snapshot=[IO.File]::Open($resultsFile,[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    try{
        Write-FixtureResult $resultsFile '{"name":"first","exit":0}'
        Write-FixtureResult $resultsFile '{"name":"second","exit":1}'
        $reader=[IO.StreamReader]::new($snapshot,[Text.Encoding]::UTF8)
        try{$rows=@($reader.ReadToEnd().TrimEnd().Split([char]10)|ForEach-Object {$_|ConvertFrom-Json})}finally{$reader.Dispose()}
        if($rows.Count -ne 2 -or $rows[0].name -ne 'first' -or $rows[0].exit -ne 0 -or $rows[1].name -ne 'second' -or $rows[1].exit -ne 1){throw 'Live result snapshots lost or changed an appended fixture result'}
    }finally{$snapshot.Dispose()}
    if(@(Reviews).Count -or $queried.Count){throw 'Missing ownership receipt searched for processes'}
    @{process_id=41;windows=@(@{process_id=42},@{process_id=41})}|ConvertTo-Json|Set-Content (Join-Path $Output 'owned-context.json')
    $owned=@(Reviews)
    if($owned.Count -ne 1 -or $owned[0].Id -ne 41 -or ($queried -join ',') -ne '41,42'){throw 'Cleanup escaped recorded owners or accepted another executable'}
}finally{Remove-Item Function:\Get-Process;Remove-Item -LiteralPath $Output -Recurse -Force}
'Fixture selection and control entry-point routing passed'
