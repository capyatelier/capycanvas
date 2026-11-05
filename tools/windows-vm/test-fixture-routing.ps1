$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'run-fixtures.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
foreach($name in 'Get-FixtureProcessArguments','Write-FixturePlan','Get-NextFixture','Select-Fixtures','Control-Fixture','Reviews','Get-FixtureTimeoutSeconds','Get-FixtureVariantArguments','Get-FixtureCompanionArguments','Write-FixtureResult','ConvertTo-FixtureCommandLine','Save-Evidence','Assert-FixtureFiles','Build-ControlFixture'){
    $function=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$false)
    . ([scriptblock]::Create($function.Extent.Text))
}
& {
    $scripts="C:\owned fixture's scripts";$TimeoutMinutes=7
    $arguments=@('-File',"C:\owned fixture's scripts\exercise-snap.ps1",'-Executable',"C:\owned app's directory\CapyCanvas.exe")
    foreach($fixture in @('drawers','color-picker','shortcuts','snapshot','Snap')){if(((Get-FixtureProcessArguments $fixture $arguments) -join [char]0) -cne ($arguments -join [char]0)){throw 'An ordinary fixture changed its process arguments'}}
    $routed=@(Get-FixtureProcessArguments 'snap' $arguments)
    if($routed.Count -ne 2 -or $routed[0] -ne '-Command'){throw 'Snap did not use a desktop helper command'}
    function Join-Path($Path,$ChildPath){'Capture-Desktop'}
    function Capture-Desktop($Desktop,$Script,$Parameters,$TimeoutSeconds){
        if($Desktop -cne 'Default' -or $Script -cne $arguments[1] -or $Parameters.Executable -cne $arguments[3] -or $Parameters.Count -ne 1 -or $TimeoutSeconds -ne 420){throw 'Snap desktop routing lost its desktop, script, arguments or timeout'}
    }
    $routed=@(Get-FixtureProcessArguments 'snap' $arguments)
    Invoke-Expression $routed[1]
    $launch=$ast.Find({param($node)$node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -eq '$arguments' -and $node.Right.Extent.Text -like '*Get-FixtureProcessArguments*'},$true)
    if(!$launch){throw 'Fixture launch bypasses desktop routing'}
}
& {
    $helper=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/test-desktop.ps1'),[ref]$tokens,[ref]$errors)
    if(@($errors).Count){throw ($errors|Out-String)}
    $bootstrap=$helper.Find({param($node)$node -is [Management.Automation.Language.StringConstantExpressionAst] -and $node.Value.Contains('$invocation=Import-Clixml')},$true)
    $child=[Management.Automation.Language.Parser]::ParseInput($bootstrap.Value,[ref]$tokens,[ref]$errors)
    if(@($errors).Count){throw ($errors|Out-String)}
    $context=@($child.EndBlock.Statements|Where-Object {$_.Extent.Text.Contains('$env:CAPY_FIXTURE_CONTEXT')})
    if($context.Count -ne 1){throw 'Desktop child must select one fixture context'}
    $contextCode=$context[0].Extent.Text.Replace('$PSScriptRoot',"'"+$PSScriptRoot.Replace("'","''")+"'")
    $prior=$env:CAPY_FIXTURE_CONTEXT
    try{
        $env:CAPY_FIXTURE_CONTEXT='C:\owned runner\snap-context.json'
        . ([scriptblock]::Create($contextCode))
        if($env:CAPY_FIXTURE_CONTEXT -cne 'C:\owned runner\snap-context.json'){throw 'Nested desktop lost the runner fixture context'}
        $env:CAPY_FIXTURE_CONTEXT=$null
        . ([scriptblock]::Create($contextCode))
        if($env:CAPY_FIXTURE_CONTEXT -cne (Join-Path $PSScriptRoot 'fixture.json')){throw 'A standalone desktop lost its private fixture context'}
    }finally{$env:CAPY_FIXTURE_CONTEXT=$prior}
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
    $directory=Join-Path ([IO.Path]::GetTempPath()) ('capy-fixture-evidence-'+[guid]::NewGuid().ToString('N'))
    $HardwareGpu=$false
    function Reviews{[pscustomobject]@{Id=42}}
    function Stop-Review{}
    function Get-ChildItem($LiteralPath,[switch]$File){
        $files=@(Microsoft.PowerShell.Management\Get-ChildItem -LiteralPath $LiteralPath -File:$File)
        if(!$publication.done){
            $publication.done=$true
            if($publication.mode -eq 'rename'){Move-Item -LiteralPath ($final+'.pending') -Destination $final -Force}
            if($publication.mode -eq 'missing-final'){Remove-Item -LiteralPath $final}
        }
        $files
    }
    try{
        foreach($mode in 'rename','unpublished','missing-final'){
            $trace=Join-Path $directory $mode
            [IO.Directory]::CreateDirectory($trace)|Out-Null
            $Executable=Join-Path $trace 'CapyCanvas.exe';$log=Join-Path $directory $mode
            @{process_id=41;trace_directory=$trace}|ConvertTo-Json|Set-Content -LiteralPath ($log+'-context.json')
            $expected=@{}
            foreach($owner in 41,42){
                foreach($prefix in 'ui-state','camera-state','presentation-state','search-state','windows','prediction','latency'){
                    $name=if($prefix -eq 'latency'){"$prefix-$owner.csv"}else{"$prefix-$owner-7.json"}
                    $expected[$name]=if($prefix -eq 'latency'){"frame,latency`n1,3"}else{@{complete=$true;name=$name}|ConvertTo-Json -Compress}
                    [IO.File]::WriteAllText((Join-Path $trace $name),$expected[$name])
                }
            }
            $final=Join-Path $trace 'presentation-state-41-7.json'
            [IO.File]::WriteAllText(($final+'.pending'),$(if($mode -eq 'rename'){$expected['presentation-state-41-7.json']}else{'{'}))
            if($mode -eq 'rename'){[IO.File]::WriteAllText($final,'{"complete":false}')}
            foreach($name in 'ui-state-410-7.json','latency-43.csv'){[IO.File]::WriteAllText((Join-Path $trace $name),'foreign owner')}
            $publication=@{mode=$mode;done=$false};$failure=$null
            try{Save-Evidence $log}catch{$failure=$_}
            if($mode -eq 'missing-final'){
                if(!$failure -or $failure.CategoryInfo.Category -ne 'ObjectNotFound' -or $failure.Exception.Message -notlike '*presentation-state-41-7.json*'){throw 'A disappearing final trace must fail evidence collection'}
                continue
            }
            if($failure){throw $failure}
            $saved=@(Microsoft.PowerShell.Management\Get-ChildItem -LiteralPath ($log+'-evidence') -File)
            if((($saved.Name|Sort-Object) -join ',') -cne (($expected.Keys|Sort-Object) -join ',')){throw 'Evidence must contain every committed trace for its recorded owners and no pending or foreign traces'}
            foreach($file in $saved){if([IO.File]::ReadAllText($file.FullName) -cne $expected[$file.Name]){throw 'Evidence lost a complete published trace or latency CSV'}}
        }
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
& {
    $localization=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/exercise-localization.ps1'),[ref]$tokens,[ref]$errors)
    if(@($errors).Count){throw ($errors|Out-String)}
    $aliases=@($localization.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.IfStatementAst] -and $_.Extent.Text -match '^if\(\$Theme.StartsWith\('})
    $partition=$localization.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Journey-Languages'},$false)
    $probe=[scriptblock]::Create($localization.ParamBlock.Extent.Text+"`n"+($aliases.Extent.Text -join "`n")+"`n"+$partition.Extent.Text+"`n"+'[pscustomobject]@{theme=$Theme;large=[bool]$LargeText;part=$JourneyPart;tags=@(Journey-Languages @(1..15) $JourneyPart)}')
    foreach($case in @(
        @{selectors=@('JourneyOnly','journey-2-dark','journey-3-dark','journey-4-dark');theme='dark';large=$false},
        @{selectors=@('journey-light','journey-2-light','journey-3-light','journey-4-light');theme='light';large=$false},
        @{selectors=@('journey-dark-large','journey-2-dark-large','journey-3-dark-large','journey-4-dark-large');theme='dark';large=$true},
        @{selectors=@('journey-light-large','journey-2-light-large','journey-3-light-large','journey-4-light-large');theme='light';large=$true}
    )){
        $parts=@(foreach($selector in $case.selectors){
            $arguments=@($edgeRuns[('localization:'+$selector)])
            if(!$arguments.Count){throw "Missing generated Journey selector: $selector"}
            $bound=@{Executable='C:\owned\CapyCanvas.exe'}
            for($i=0;$i -lt $arguments.Count;$i++){
                $name=$arguments[$i].TrimStart('-')
                if($name -eq 'Theme'){$i++;$bound[$name]=$arguments[$i]}else{$bound[$name]=$true}
            }
            & $probe @bound
        })
        $expected=@('1,2,3,4','5,6,7,8','9,10,11,12','13,14,15')
        for($i=0;$i -lt 4;$i++){
            if($parts[$i].part -ne $i+1 -or ($parts[$i].tags -join ',') -cne $expected[$i]){throw 'Generated Journey selectors repeated or lost a language quarter'}
            if($parts[$i].theme -cne $case.theme -or $parts[$i].large -ne $case.large){throw 'Journey partition changed theme or text scale'}
        }
        $tags=@($parts|ForEach-Object {$_.tags})
        if(($tags -join ',') -cne ((1..15) -join ',') -or @($tags|Select-Object -Unique).Count -ne 15){throw 'Generated Journey selectors did not cover every tag exactly once'}
    }
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
& {
    foreach($name in 'Get-FixtureTextScale','Restore-FixtureTextScale','Wait-FixtureTextScale'){
        $definition=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$false)
        if($definition){. ([scriptblock]::Create($definition.Extent.Text))}
    }
    Add-Type -TypeDefinition 'namespace CapyVm { public static class Native { public static int Notifications; public static void NotifyTextSize(){Notifications++;} } }'
    $loop=$ast.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.WhileStatementAst] -and $_.Extent.Text.Contains('Get-NextFixture')}
    $baseline=$ast.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -eq '$textScale'}
    $startup=@($baseline)+@($ast.EndBlock.Statements|Where-Object {$_.Extent.Text -eq 'Restore-FixtureTextScale $textScale'})
    $execute=[scriptblock]::Create(($startup.Extent.Text -join "`n")+"`n"+$loop.Extent.Text)
    $registry=[pscustomobject]@{}
    $registry|Add-Member ScriptMethod GetValueNames {if($script:scale.exists){'TextScaleFactor'}}
    $registry|Add-Member ScriptMethod GetValueKind {param($name)$script:scale.kind}
    $registry|Add-Member ScriptMethod GetValue {param($name,$default,$options)$script:scale.value}
    function Test-Path($LiteralPath,$Path){$true}
    function Get-Item($LiteralPath){$registry}
    function New-Item($Path,$Force){}
    function Set-ItemProperty($LiteralPath,$Name,$Type,$Value){
        $script:events.Add('restore')
        if($script:restoreFails){throw 'Registry restore refused'}
        $script:scale=[pscustomobject]@{exists=$true;kind=[string]$Type;value=$Value}
    }
    function Remove-ItemProperty($LiteralPath,$Name){
        $script:events.Add('restore')
        if($script:restoreFails){throw 'Registry restore refused'}
        $script:scale=[pscustomobject]@{exists=$false;kind=$null;value=$null};$script:pendingReads=1
    }
    function Get-NextFixture($Planned,$Queue,$Claimed){
        $script:events.Add('claim');$script:claims++
        if($script:claims -eq 1){'fixture'}
    }
    function Stop-Review{}
    function Control-Fixture{}
    function Get-FixtureCompanionArguments{}
    function Get-Process{[pscustomobject]@{Path='powershell.exe'}}
    function Start-Process{
        $script:events.Add('child')
        if($script:scale.exists -or $script:runtimeScale -ne 1.0){throw 'A child launched before default Windows text scale was acknowledged'}
        if($script:mode -ne 'unchanged'){$script:scale=[pscustomobject]@{exists=$true;kind='DWord';value=150};$script:runtimeScale=1.5}
        if($script:mode -eq 'error'){throw 'Child launch failed'}
        $child=[pscustomobject]@{Id=123;Handle=1;PriorityClass='Normal';ExitCode=$(if($script:mode -eq 'failure'){7}else{0})}
        $child|Add-Member ScriptMethod WaitForExit {param($timeout)$script:mode -notin @('timeout','screen-error')}
        $child
    }
    function taskkill.exe{$script:events.Add('kill');$script:readerKilled=$true}
    function Save-Screen{if($script:mode -eq 'screen-error'){throw 'Capture failed'}}
    function Read-FixtureTextScale([int]$Milliseconds){
        $script:reads++;$script:events.Add('read')
        if($script:readFails){
            if($script:readFails -eq 'startup' -and $script:reads -eq 1){return 1.5}
            if($script:readFails -eq 'setup' -and $script:reads -le 2){if($script:reads -eq 1){return 1.0}else{return 1.5}}
            if($script:readFails -eq 'restoration' -and $script:reads -le 2){return 1.0}
            throw 'UISettings reader timed out'
        }
        if($script:pendingReads -gt 0){$script:pendingReads--;return $script:runtimeScale}
        $script:runtimeScale=if($script:scale.exists){[double]$script:scale.value/100.0}else{1.0}
        $script:runtimeScale
    }
    function Start-Sleep{}
    function Save-Evidence{$script:events.Add('evidence')}
    function Get-Content{if($script:mode -eq 'failure'){'Fixture failed'}}
    function Write-FixtureResult($Path,$Json){$script:events.Add('result');$script:result=$Json|ConvertFrom-Json}
    $Output='C:\owned';$scripts='C:\owned';$editorExecutable='C:\owned\CapyCanvas.exe'
    $runs=@{fixture=@()};$queue=$null;$DebuggerPath=$null;$TimeoutMinutes=20
    foreach($leaked in 150,130){
        $script:scale=[pscustomobject]@{exists=$true;kind='DWord';value=$leaked};$script:runtimeScale=$leaked/100.0
        $script:mode='unchanged';$script:restoreFails=$false;$script:readFails=$false;$script:pendingReads=0
        $script:events=[Collections.Generic.List[string]]::new();$script:claims=0;$script:reads=0;$script:result=$null
        $failure=$null;try{& $execute}catch{$failure=$_.Exception.Message}
        if($failure -or !$script:result -or $script:result.exit -ne 0 -or $script:scale.exists -or $script:runtimeScale -ne 1.0){throw 'Runner did not restore default Windows text scale before its first child'}
        if($script:events.IndexOf('restore') -ge $script:events.IndexOf('claim') -or $script:events.IndexOf('read') -ge $script:events.IndexOf('claim')){throw 'Runner default text-scale setup occurred after its first queue claim'}
    }
    foreach($mode in 'timeout','screen-error','failure','error','success','unchanged'){
        foreach($restoreFails in @($false,$true)){
            if($restoreFails -and $mode -notin @('timeout','success','failure')){continue}
            $script:scale=[pscustomobject]@{exists=$false;kind=$null;value=$null};$script:runtimeScale=1.0
            $script:mode=$mode;$script:restoreFails=$restoreFails;$script:pendingReads=0
            $script:events=[Collections.Generic.List[string]]::new();$script:claims=0;$script:result=$null
            $notifications=[CapyVm.Native]::Notifications;$failure=$null;$script:reads=0;$script:readFails=$false
            try{& $execute}catch{$failure=$_.Exception.Message}
            $childExit=switch($mode){'timeout'{'timeout'} 'failure'{7} {$_ -in @('error','screen-error')}{'runner'} default{0}}
            $expectedExit=if($restoreFails){'cleanup'}else{$childExit}
            if(!$script:result -or $script:result.exit -cne $expectedExit){throw 'Text-scale cleanup replaced the original fixture exit'}
            $expectedMessage=switch($mode){'failure'{'Fixture failed'} 'error'{'Child launch failed'} 'screen-error'{'Capture failed'} default{''}}
            if($restoreFails){$expectedMessage=(@($expectedMessage,"Text-scale cleanup failed (fixture exit ${childExit}): Registry restore refused")|Where-Object {$_}) -join ' '}
            if($script:result.message -cne $expectedMessage){throw 'Text-scale cleanup replaced the original fixture message'}
            if($restoreFails){
                if(!$failure -or $script:claims -ne 1){throw 'Failed text-scale cleanup claimed another fixture or lost its result'}
                if($script:events[-1] -ne 'result'){throw 'Failed text-scale cleanup did not write its fixture result before stopping'}
                continue
            }
            if($failure){throw $failure}
            $changed=$mode -ne 'unchanged'
            if($script:reads -ne $(if($changed){4}else{3})){throw 'The next fixture started before default UISettings text scale was acknowledged'}
            if($script:scale.exists -or $script:runtimeScale -ne 1.0){throw 'Forced fixture termination leaked Windows text scale into the next fixture'}
            if([CapyVm.Native]::Notifications -ne $notifications+[int]$changed){throw 'Text-scale notification did not match an actual restoration'}
            $restore=$script:events.IndexOf('restore');$result=$script:events.IndexOf('result')
            if($changed -and ($restore -le $script:events.IndexOf('evidence') -or $restore -ge $result)){throw 'Text scale must restore after evidence and before the fixture result'}
            if($mode -in @('timeout','screen-error') -and ($script:events.IndexOf('kill') -lt 0 -or $script:events.IndexOf('kill') -ge $restore)){throw 'Text scale restored before forced child termination'}
            if($script:claims -ne 2 -or $script:events[-1] -ne 'claim'){throw 'The next fixture was claimed before its predecessor completed cleanup'}
        }
    }
    if(@($baseline).Count -ne 1 -or $startup.Count -ne 2){throw 'Runner default text-scale setup was not performed once outside the child loop'}
    foreach($phase in 'startup','setup','restoration'){
        $script:scale=[pscustomobject]@{exists=$false;kind=$null;value=$null};$script:runtimeScale=1.0
        $script:mode='success';$script:restoreFails=$false;$script:pendingReads=0;$script:readFails=$phase
        $script:events=[Collections.Generic.List[string]]::new();$script:claims=0;$script:reads=0;$script:result=$null
        $failure=$null;try{& $execute}catch{$failure=$_.Exception.Message}
        if($phase -eq 'startup'){
            if(!$failure -or $script:claims -ne 0 -or $script:events.Contains('child')){throw 'Unacknowledged default text scale claimed or launched the first fixture'}
        }elseif($phase -eq 'setup'){
            if($script:events.Contains('child') -or !$script:result -or $script:result.exit -eq 0 -or $script:result.message -notlike 'Fixture setup did not acknowledge saved Windows text scale*'){throw 'An unacknowledged startup text scale launched a fixture or lost its named setup failure'}
        }elseif(!$failure -or $script:result.exit -ne 'cleanup' -or $script:claims -ne 1 -or $script:result.message -notlike '*Fixture restoration did not acknowledge saved Windows text scale*'){throw 'An unacknowledged runtime restoration passed or claimed another fixture'}
    }
    & {
        $definition=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Read-FixtureTextScale'},$false)
        if(!$definition){throw 'The parent text-scale reader is not bounded'}
        $read=[scriptblock]::Create($definition.Body.Extent.Text.Trim().Substring(1,$definition.Body.Extent.Text.Trim().Length-2))
        function Start-Process{
            $reader=[pscustomobject]@{Id=124}
            $reader|Add-Member ScriptProperty Handle {$script:readerHandle=$true;[IntPtr]1}
            $reader|Add-Member ScriptProperty ExitCode {if($script:readerWaitHadHandle){0}}
            $reader|Add-Member ScriptMethod WaitForExit {param($milliseconds)$script:readerBound=$milliseconds;$script:readerWaitHadHandle=$script:readerHandle;$script:readerCompletes}
            $reader|Add-Member ScriptMethod Dispose {$script:readerDisposed=$true}
            $reader
        }
        function Get-Content{'1.3'}
        foreach($completes in @($true,$false)){
            $script:readerCompletes=$completes;$script:readerKilled=$false;$script:readerDisposed=$false;$script:readerBound=0;$script:readerHandle=$false;$script:readerWaitHadHandle=$false
            $Milliseconds=23;$failure=$null;$value=$null
            try{$value=& $read}catch{$failure=$_.Exception.Message}
            if($script:readerBound -ne 23 -or !$script:readerDisposed){throw 'UISettings readback escaped its remaining deadline or leaked its process handle'}
            if(!$script:readerWaitHadHandle){throw 'Quick-exit UISettings reader lost its retained process handle'}
            if($completes){if($failure -or $value -ne 1.3 -or $script:readerKilled){throw 'Completed UISettings readback lost its exact double'}}
            elseif(!$script:readerKilled -or $failure -ne 'UISettings text-scale reader exceeded its acknowledgement deadline'){throw 'A blocked UISettings reader was not terminated with a named deadline failure'}
        }
    }
}
'Fixture selection and control entry-point routing passed'
