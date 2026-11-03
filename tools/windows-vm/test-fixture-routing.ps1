$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'run-fixtures.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
foreach($name in 'Write-FixturePlan','Get-NextFixture','Select-Fixtures','Control-Fixture','Reviews','Get-FixtureTimeoutSeconds','Get-FixtureVariantArguments','Get-FixtureCompanionArguments','Write-FixtureResult','ConvertTo-FixtureCommandLine','Assert-FixtureFiles','Build-ControlFixture'){
    $function=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$false)
    . ([scriptblock]::Create($function.Extent.Text))
}
& {
    $directory=Join-Path ([IO.Path]::GetTempPath()) ('capy-fixture-plan-'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($directory)|Out-Null
    $path=Join-Path $directory 'plan.json';$TimeoutMinutes=20;$HardwareGpu=$false
    try{
        Write-FixturePlan $path @()
        $empty=[IO.File]::ReadAllText($path)|ConvertFrom-Json
        if($null -eq $empty.runs -or $empty.runs.Count -ne 0){throw 'An idle queue runner must publish an empty plan array'}
        Write-FixturePlan $path @('previous-'+('x'*200))
        $before=[IO.File]::ReadAllBytes($path)
        $stream=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
        $reader=[IO.BinaryReader]::new($stream)
        try{
            $prefix=$reader.ReadBytes([int]($before.Length/2))
            Write-FixturePlan $path @('next')
            $observed=[Text.Encoding]::UTF8.GetString([byte[]]($prefix+$reader.ReadBytes($before.Length)))|ConvertFrom-Json
            if($observed.runs[0] -cne ('previous-'+('x'*200))){throw 'Plan replacement changed a partially consumed reader'}
        }finally{$reader.Dispose();$stream.Dispose()}
        $current=[IO.File]::ReadAllText($path)|ConvertFrom-Json
        if(($current.runs -join ',') -ne 'next' -or $current.timeout_minutes -ne 20 -or $current.hardware_gpu){throw 'New plan readers did not receive the complete replacement'}
    }finally{Remove-Item -LiteralPath $directory -Recurse -Force}
}
& {
    $planned=@('popup','header:pen','effects:light')
    $claimed=[Collections.Generic.List[string]]::new()
    $actual=@(while($null -ne ($name=Get-NextFixture $planned $null $claimed)){$name})
    if(($actual -join ',') -ne ($planned -join ',')){throw 'Default fixture iteration changed selection or order'}
    $claimed=[Collections.Generic.List[string]]::new()
    $queue=[pscustomobject]@{url='http://10.0.2.2:1234/r1';token='test-secret'}
    $script:responses=[Collections.Generic.Queue[object]]::new()
    foreach($name in @('effects:light','popup',$null)){$script:responses.Enqueue([pscustomobject]@{selector=$name})}
    $script:requests=0
    function Invoke-RestMethod($Method,$Uri,$Headers,$TimeoutSec){
        if($Method -ne 'Post' -or $Uri -ne $queue.url -or $Headers.Authorization -ne 'Bearer test-secret'){throw 'Queue request lost its endpoint or authentication'}
        $script:requests++
        $script:responses.Dequeue()
    }
    $actual=@(while($null -ne ($name=Get-NextFixture $planned $queue $claimed)){$name})
    if(($actual -join ',') -ne 'effects:light,popup' -or $script:requests -ne 3){throw 'Queue was prefetched, reordered or retried'}
    foreach($bad in @('popup','unknown')){
        $script:responses.Enqueue([pscustomobject]@{selector=$bad})
        $failure=$null;try{Get-NextFixture $planned $queue $claimed|Out-Null}catch{$failure=$_.Exception.Message}
        if($failure -ne 'The fixture queue returned an unknown or repeated selector.'){throw 'Invalid queue assignment was accepted'}
    }
    $before=$script:requests
    $failure=$null;try{Get-NextFixture $planned $queue $claimed|Out-Null}catch{$failure=$_.Exception.Message}
    if($failure -ne 'The fixture queue request failed.' -or $script:requests -ne $before+1){throw 'Failed queue request was replayed or leaked transport details'}
}
foreach($ending in 'drag','cancel','escape'){if(((Get-FixtureVariantArguments 'switcher-drag' 'ContextEnding' $ending) -join ',') -ne "-ContextHoldOnly,-ContextEnding,$ending"){throw 'Context ending selection lost its required journey scope'}}
if(((Get-FixtureVariantArguments 'popup' 'Theme' 'light') -join ',') -ne '-Theme,light'){throw 'Ordinary fixture variant arguments changed'}
$edgeSource=Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/exercise-edge-reach.ps1'
$edgeAst=[Management.Automation.Language.Parser]::ParseFile($edgeSource,[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$edgeRuns=& {
    $scripts=(Resolve-Path (Join-Path $PSScriptRoot '../../apps/layer-windows/scripts')).Path
    $runs=[ordered]@{};$debuggable=@()
    $discovery=$ast.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.ForEachStatementAst] -and $_.Extent.Text -like 'foreach ($script in Get-ChildItem*'}
    . ([scriptblock]::Create($discovery.Extent.Text))
    $runs
}
foreach($case in @(@{option='CanvasResetOnly';journey='Check-CanvasReset'},@{option='CancelOnly';journey='Check-NativeEdgeCancel'})){
    $journey=$edgeAst.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $case.journey},$false)
    $defaults=$edgeAst.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.IfStatementAst] -and $_.Extent.Text.Contains("PSBoundParameters.ContainsKey('Device')")}
    $probe=[scriptblock]::Create($edgeAst.ParamBlock.Extent.Text+"`n"+$defaults.Extent.Text+"`n"+$journey.Body.EndBlock.Statements[0].Extent.Text+"`n"+'$Device')
    $arguments=@($edgeRuns[('edge-reach:'+$case.option)])
    if(($arguments -join ',') -ne ('-'+$case.option)){throw 'Generated native edge variants must leave the script device default intact'}
    $bound=@{Executable='C:\owned\CapyCanvas.exe'}
    for($i=0;$i -lt $arguments.Count;$i++){
        $name=$arguments[$i].TrimStart('-')
        if($name -eq 'Device'){$i++;$bound[$name]=$arguments[$i]}else{$bound[$name]=$true}
    }
    if((& $probe @bound) -ne 'touch'){throw 'Generated native edge variant lost its required touch device'}
    $bound.Device='touch';if((& $probe @bound) -ne 'touch'){throw 'Explicit touch input changed'}
    $bound.Device='mouse';$failure=$null
    try{& $probe @bound|Out-Null}catch{$failure=$_.Exception.Message}
    if(!$failure -or $failure -notlike ($case.option+' requires *')){throw 'Explicit mouse must retain the native edge guard'}
    if($case.option -eq 'CancelOnly'){$bound.Device='pen';if((& $probe @bound) -ne 'pen'){throw 'Explicit pen cancellation changed'}}
}
if(($edgeRuns['edge-reach:pen'] -join ',') -ne '-Device,pen' -or ($edgeRuns['edge-reach:ColumnResetOnly'] -join ',') -ne '-ColumnResetOnly'){throw 'Native edge routing changed unrelated variants'}
$smoke=@(
    'activation','canvas-editing:smoke','canvas-editing:smoke-light','clipboard',
    'color-editor:light','column-stacks:touch','documents:new-drawing','documents:native-writeback-light',
    'drawers','drawers:light','editor','effects:light',
    'header','header:light','image-rows','layer-pickup',
    'layers','layers:PaperLight','localization-smoke:light','navigator',
    'navigator:light','persistence','popup','popup:light',
    'saved-data:light','scopes','selection:light','startup-close',
    'switcher-drag:mouse','switcher','tab-drag','tab-drag:light',
    'tool-bubbles:light-pen','toolbar-components','window','window:light',
    'workspace-layout','workspace-layout:light','workspace-pickup','zoom-readout:touch'
)
$smokePlan=@(Select-Fixtures @($edgeRuns.Keys) @('smoke'))
if($smokePlan.Count -ne 40 -or ($smokePlan -join ',') -ne ($smoke -join ',')){throw 'Smoke must select its exact forty inventory entries'}
if(((Select-Fixtures @($edgeRuns.Keys) @('smoke','popup:default')) -join ',') -ne ($smoke -join ',')){throw 'An explicit smoke entry must not duplicate or expand variants'}
$smokeExtended=@($edgeRuns.Keys|Where-Object {$_ -in $smoke -or $_ -eq 'navigator:PixelsOnly'})
if(((Select-Fixtures @($edgeRuns.Keys) @('smoke','navigator:PixelsOnly')) -join ',') -ne ($smokeExtended -join ',')){throw 'Smoke must preserve an explicitly requested extra variant'}
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
    $source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'windows-vm.py'))
    $command=[regex]::Match($source,'(?s)def fixture_status_command\(output\):\s+return r"""(?<script>.*?)"""\.replace\("\{output\}", output\)')
    if(!$command.Success){throw 'Cannot extract the actual fixture progress command'}
    $status=[scriptblock]::Create($command.Groups['script'].Value.Replace('{output}',$Output))
    $writer=[IO.File]::Open($resultsFile,[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::Write,([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    try{
        Write-FixtureResult $resultsFile '{"name":"first","exit":0}'
        $first=& $status|ConvertFrom-Json
        if(($first.log|ConvertFrom-Json).name -ne 'first'){throw 'The live monitor did not read the first appended fixture result'}
        Write-FixtureResult $resultsFile '{"name":"second","exit":1}'
        $last=& $status|ConvertFrom-Json
        $rows=@($last.log.TrimEnd().Split([char]10)|ForEach-Object {$_|ConvertFrom-Json})
        if($rows.Count -ne 2 -or $rows[0].name -ne 'first' -or $rows[0].exit -ne 0 -or $rows[1].name -ne 'second' -or $rows[1].exit -ne 1){throw 'Live result snapshots lost or changed an appended fixture result'}
    }finally{$writer.Dispose()}
    $message='Result '+[char]0xec+[char]0x65e5
    Write-FixtureResult $resultsFile (@{name='unicode';exit=1;message=$message}|ConvertTo-Json -Compress)
    $script="[Console]::OutputEncoding=[Text.Encoding]::GetEncoding(437);`n"+$command.Groups['script'].Value.Replace('{output}',$Output)
    $start=[Diagnostics.ProcessStartInfo]::new()
    $start.FileName=$powershell
    $start.Arguments=ConvertTo-FixtureCommandLine @('-NoProfile','-NonInteractive','-EncodedCommand',[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script)))
    $start.UseShellExecute=$false;$start.CreateNoWindow=$true
    $start.RedirectStandardOutput=$true;$start.RedirectStandardError=$true
    $child=[Diagnostics.Process]::Start($start)
    $bytes=[IO.MemoryStream]::new()
    try{
        $copy=$child.StandardOutput.BaseStream.CopyToAsync($bytes)
        $errorText=$child.StandardError.ReadToEndAsync()
        if(!$child.WaitForExit(30000)){$child.Kill();throw 'Fixture status encoding probe timed out'}
        $null=$copy.GetAwaiter().GetResult()
        if($child.ExitCode -ne 0){throw ('Fixture status encoding probe failed: '+$errorText.GetAwaiter().GetResult())}
        $progress=[Text.UTF8Encoding]::new($false,$true).GetString($bytes.ToArray())|ConvertFrom-Json
        $result=@($progress.log.TrimEnd().Split([char]10)|ForEach-Object {$_|ConvertFrom-Json})[-1]
        if($result.name -cne 'unicode' -or $result.message -cne $message){throw 'Fixture status changed Unicode result text through the legacy console'}
    }finally{$bytes.Dispose();$child.Dispose()}
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
    foreach($case in @(@{names=$null;controls='Feedback,LayerDrop,Popup'},@{names='navigator:default';controls=''},@{names='button-feedback:default,popup:default';controls='Feedback,Popup'},@{names='layer-pickup:default';controls='LayerDrop'},@{names='smoke';controls='LayerDrop,Popup';runs=$smoke},@{names='smoke,popup:default';controls='LayerDrop,Popup';runs=$smoke},@{names='smoke,navigator:PixelsOnly';controls='LayerDrop,Popup';runs=$smokeExtended})){
        $selection=@{};if($case.names){$selection.Name=$case.names}
        $json=& (Get-Process -Id $PID).Path -NoProfile -File (Join-Path $PSScriptRoot 'run-fixtures.ps1') -Executable 'C:\not-built\CapyCanvas.exe' -Output $cacheRun @selection -List
        if($LASTEXITCODE){throw 'Listing fixture controls required a build or desktop'}
        $plan=$json|ConvertFrom-Json
        if(($plan.controls -join ',') -ne $case.controls -or !$plan.runs.Count){throw 'Fixture plan selected unnecessary controls or lost its selectors'}
        if($case.runs -and ($plan.runs.Count -ne $case.runs.Count -or ($plan.runs -join ',') -ne ($case.runs -join ','))){throw 'Listed smoke selectors differ from the exact requested inventory'}
        if(!$case.names -and ($plan.runs -join ',') -ne ($all -join ',')){throw 'Listing without names must return every available selector'}
    }
    $process=[Diagnostics.Process]::GetCurrentProcess();$previous=$process.PriorityClass
    try{
        foreach($assignment in $priorityAssignments){. ([scriptblock]::Create($assignment.Extent.Text));$process.Refresh();if($process.PriorityClass -ne 'High'){throw 'A fixture input driver did not receive High priority'}}
    }finally{$process.PriorityClass=$previous}
}finally{Remove-Item -LiteralPath $cacheRun -Recurse -Force}
'Fixture selection and control entry-point routing passed'
