$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'run-fixtures.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
foreach($name in 'Select-Fixtures','Control-Fixture','Reviews','Get-FixtureTimeoutSeconds','Get-FixtureVariantArguments','Get-FixtureCompanionArguments','Write-FixtureResult','ConvertTo-FixtureCommandLine','Assert-FixtureFiles','Build-ControlFixture'){
    $function=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$false)
    . ([scriptblock]::Create($function.Extent.Text))
}
foreach($ending in 'drag','cancel','escape'){if(((Get-FixtureVariantArguments 'switcher-drag' 'ContextEnding' $ending) -join ',') -ne "-ContextHoldOnly,-ContextEnding,$ending"){throw 'Context ending selection lost its required journey scope'}}
if(((Get-FixtureVariantArguments 'popup' 'Theme' 'light') -join ',') -ne '-Theme,light'){throw 'Ordinary fixture variant arguments changed'}
$available=@('header','header:catalog-only','header:catalog-only-light','popup','popup:light','effects')
if(((Select-Fixtures $available @()) -join ',') -ne ($available -join ',')){throw 'An empty fixture request must select every available selector'}
if(((Select-Fixtures $available $null) -join ',') -ne ($available -join ',')){throw 'A null fixture request must select every available selector'}
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
$argumentsRun=Join-Path ([IO.Path]::GetTempPath()) ('capy fixture arguments '+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($argumentsRun)|Out-Null
try{
    $probe=Join-Path $argumentsRun 'fixture child.ps1'
    $receipt=Join-Path $argumentsRun 'arguments.json'
    @'
param([string]$Executable,[string]$StateFile,[string]$DebuggerPath,[string]$DropFixture,[string]$Output,[switch]$Enabled)
@{Executable=$Executable;StateFile=$StateFile;DebuggerPath=$DebuggerPath;DropFixture=$DropFixture;Enabled=[bool]$Enabled}|ConvertTo-Json|Set-Content -LiteralPath $Output -Encoding UTF8
'@|Set-Content -LiteralPath $probe -Encoding UTF8
    $values=[ordered]@{Executable=('C:\owned app\'+[char]0x65e5+'\CapyCanvas.exe');StateFile='C:\owned state\ui.json';DebuggerPath='C:\owned tools\debugger"quoted.exe';DropFixture='C:\owned companion\'}
    $arguments=@('-NoProfile','-File',$probe)
    foreach($entry in $values.GetEnumerator()){$arguments+=@("-$($entry.Key)",$entry.Value)}
    $arguments+=@('-Output',$receipt,'-Enabled')
    $powershell=(Get-Process -Id $PID).Path
    $child=Start-Process $powershell -ArgumentList (ConvertTo-FixtureCommandLine $arguments) -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $argumentsRun 'stdout.log') -RedirectStandardError (Join-Path $argumentsRun 'stderr.log')
    $null=$child.Handle
    if(!$child.WaitForExit(30000)){Stop-Process -Id $child.Id;throw 'Fixture argument probe timed out'}
    if($child.ExitCode -ne 0 -or !(Test-Path -LiteralPath $receipt)){throw ('Fixture argument probe failed: '+(Get-Content (Join-Path $argumentsRun 'stderr.log') -Raw))}
    $actual=Get-Content -LiteralPath $receipt -Raw|ConvertFrom-Json
    foreach($entry in $values.GetEnumerator()){if($actual.($entry.Key) -cne $entry.Value){throw ('Fixture argument changed: '+$entry.Key+' expected '+$entry.Value+' received '+$actual.($entry.Key))}}
    if(!$actual.Enabled){throw 'Fixture switch did not bind'}
}finally{Remove-Item -LiteralPath $argumentsRun -Recurse -Force}
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
$cacheRun=Join-Path ([IO.Path]::GetTempPath()) ('capy-fixture-cache-'+[guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($cacheRun)|Out-Null
try{
    $Output=$cacheRun;$scripts=$cacheRun;$editorExecutable=Join-Path $cacheRun 'editor/CapyCanvas.exe'
    $directory=(Split-Path $editorExecutable)+'-Feedback'
    [IO.Directory]::CreateDirectory($directory)|Out-Null
    'throw "A shipped control fixture must never rebuild"'|Set-Content (Join-Path $scripts 'build.ps1')
    $files=@{}
    foreach($name in 'CapyCanvas.exe','layer_windows.dll','resources.pri'){
        $file=Join-Path $directory $name;$name|Set-Content -LiteralPath $file
        $files[$name]=(Get-FileHash -LiteralPath $file).Hash
    }
    $buildManifest=@{controls=@{Feedback=$files}}|ConvertTo-Json -Depth 5|ConvertFrom-Json
    $controlBuilds=@{};$RustProfile='dev-perf';$HardwareGpu=$false
    $first=Build-ControlFixture 'Feedback';$second=Build-ControlFixture 'Feedback'
    if($first -ne (Join-Path $directory 'CapyCanvas.exe') -or $second -ne $first){throw 'Shipped control fixture did not reuse its executable'}
    'stale'|Set-Content (Join-Path $directory 'resources.pri');$controlBuilds=@{};$rejected=$false
    try{Build-ControlFixture 'Feedback'}catch{if($_.Exception.Message -notlike 'Control fixture binary differs*'){throw};$rejected=$true}
    if(!$rejected){throw 'A stale shipped resource silently rebuilt or passed'}
    $priorityAssignments=@($ast.FindAll({param($node)$node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -like '*PriorityClass'},$true))
    if($priorityAssignments.Count -ne 1 -or $priorityAssignments[0].Left.Extent.Text -ne '$process.PriorityClass'){throw 'Only the fixture input driver process should receive High priority'}
    $all=@(& {
        $scripts=(Resolve-Path (Join-Path $PSScriptRoot '../../apps/layer-windows/scripts')).Path
        $runs=[ordered]@{shortcuts=$null};$debuggable=@()
        $discovery=$ast.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.ForEachStatementAst] -and $_.Extent.Text -like 'foreach ($script in Get-ChildItem*'}
        . ([scriptblock]::Create($discovery.Extent.Text))
        $runs.Keys
    })
    foreach($case in @(@{names=$null;controls='Feedback,LayerDrop,Popup'},@{names='navigator:default';controls=''},@{names='button-feedback:default,popup:default';controls='Feedback,Popup'},@{names='layer-pickup:default';controls='LayerDrop'})){
        $selection=@{};if($case.names){$selection.Name=$case.names}
        $json=& (Get-Process -Id $PID).Path -NoProfile -File (Join-Path $PSScriptRoot 'run-fixtures.ps1') -Executable 'C:\not-built\CapyCanvas.exe' -Output $cacheRun @selection -List
        if($LASTEXITCODE){throw 'Listing fixture controls required a build or desktop'}
        $plan=$json|ConvertFrom-Json
        if(($plan.controls -join ',') -ne $case.controls -or !$plan.runs.Count){throw 'Fixture plan selected unnecessary controls or lost its selectors'}
        if(!$case.names -and ($plan.runs -join ',') -ne ($all -join ',')){throw 'Listing without names must return every available selector'}
    }
    $process=[Diagnostics.Process]::GetCurrentProcess();$previous=$process.PriorityClass
    try{
        foreach($assignment in $priorityAssignments){. ([scriptblock]::Create($assignment.Extent.Text));$process.Refresh();if($process.PriorityClass -ne 'High'){throw 'A fixture input driver did not receive High priority'}}
    }finally{$process.PriorityClass=$previous}
}finally{Remove-Item -LiteralPath $cacheRun -Recurse -Force}
'Fixture selection and control entry-point routing passed'
