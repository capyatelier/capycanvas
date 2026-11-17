$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'session-identity.ps1')
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'run-fixtures.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
foreach($name in 'Get-FixtureGraphicsFailure','Write-FixtureSessionRecord','Write-FixtureGraphicsWithdrawal','Wait-FixtureGraphicsEvidence','Get-FixtureProcessArguments','Write-FixturePlan','Get-NextFixture','Select-Fixtures','Control-Fixture','Reviews','Get-FixtureTimeoutSeconds','Get-FixtureVariantArguments','Get-FixtureCompanionArguments','Write-FixtureResult','ConvertTo-FixtureCommandLine','Save-Evidence','Assert-FixtureFiles','Build-ControlFixture'){
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
    foreach($value in 'MILERR_NO_HARDWARE_DEVICE','0x8898008D','2291662989','-2003304307'){
        if((Get-FixtureGraphicsFailure $value '') -cne 'MILERR_NO_HARDWARE_DEVICE' -or (Get-FixtureGraphicsFailure 1 ("Owned review exited with code ${value}: Review did not start")) -cne 'MILERR_NO_HARDWARE_DEVICE'){throw "Graphics withdrawal missed $value in an exit or nested fixture error"}
    }
    foreach($message in 'DXGI_ERROR_DEVICE_REMOVED','MILERR_NO_HARDWARE_DEVICE -2003304307','0x887A0006 2289696775'){if(Get-FixtureGraphicsFailure 0 $message){throw 'A passing row logged a handled graphics failure and was withdrawn'}}
    foreach($message in 'DeviceLost','Unknown GPU failure','122916629890','0x8898008D0','NOT_DXGI_ERROR_DEVICE_RESET','MILERR_NO_HARDWARE_DEVICE_EXTRA'){
        if(Get-FixtureGraphicsFailure 1 $message){throw "Graphics withdrawal classified an unrelated error: $message"}
    }
    if((Get-FixtureGraphicsFailure 1 'failure 0x008898008d') -cne 'MILERR_NO_HARDWARE_DEVICE'){throw 'Padded lowercase HRESULT was not classified'}
    $directory=Join-Path ([IO.Path]::GetTempPath()) ('capy-graphics-withdrawal-'+[Guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($directory)|Out-Null
    $TimeoutMinutes=20;$HardwareGpu=$true
    function Get-CapySessionIdentity{@{account='HOST\capy04';session_id=42;logon_sid='S-1-5-5-2-3';boot_utc='2026-11-11T09:00:00.0000000Z'}}
    try{
        Write-FixturePlan (Join-Path $directory 'plan.json') @('first')
        $result=@{name='first';exit='evidence';message='Owned review exited with code -2003304307: Review did not start';infrastructure_cause='MILERR_NO_HARDWARE_DEVICE'}
        $identity=Get-CapySessionIdentity
        Write-FixtureGraphicsWithdrawal $directory $result $identity
        $request=[IO.File]::ReadAllText((Join-Path $directory 'graphics-withdrawal.json'))|ConvertFrom-Json
        if($request.name -cne $result.name -or $request.exit -cne $result.exit -or $request.error -cne $result.message -or $request.cause -cne $result.infrastructure_cause){throw 'Atomic graphics withdrawal lost the original fixture result'}
        if($request.account -cne 'HOST\capy04' -or $request.session_id -ne 42 -or $request.logon_sid -cne 'S-1-5-5-2-3' -or $request.boot_utc -cne '2026-11-11T09:00:00.0000000Z' -or !$request.time_utc){throw 'Graphics withdrawal lost its worker boot/account/logon identity'}
        if(Test-Path -LiteralPath (Join-Path $directory 'graphics-withdrawal.json.tmp')){throw 'Graphics withdrawal left an unpublished temporary request'}
        Write-FixtureSessionRecord $directory 'graphics-identity.json' $identity
        $header=[IO.File]::ReadAllText((Join-Path $directory 'graphics-identity.json'))|ConvertFrom-Json
        if($header.account -cne $identity.account -or $header.session_id -ne $identity.session_id -or $header.logon_sid -cne $identity.logon_sid -or $header.boot_utc -cne $identity.boot_utc){throw 'Shared worker header lost the identity reused for withdrawal'}
        foreach($entry in $identity.GetEnumerator()){if($entry.Key -in 'name','exit','cause','error','time_utc'){throw 'Withdrawal mutated the cached worker identity'}}
        $startup=@($ast.EndBlock.Statements|Where-Object {$_.Extent.Text.StartsWith('$fixtureSessionIdentity=') -or $_.Extent.Text.StartsWith('if($fixtureSessionIdentity)')})
        if($startup.Count -ne 2 -or @($startup|Where-Object {$_.Extent.Text.Contains('Get-CapySessionIdentity')}).Count -ne 1){throw 'Shared identity is not captured and published once outside the fixture loop'}
    }finally{Remove-Item -LiteralPath $directory -Recurse -Force}
}
& {
    $definition=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-FixtureGraphicsEvidence'},$false)
    . ([scriptblock]::Create($definition.Extent.Text.Replace('[Diagnostics.Stopwatch]::StartNew()','$script:watch')))
    function Test-Path($LiteralPath){$script:acknowledged}
    function Start-Sleep($Milliseconds){
        if($Milliseconds -lt 0 -or $Milliseconds -gt 1000){throw 'Graphics evidence wait exceeded its bounded polling interval'}
        $script:watch.ElapsedMilliseconds=360000
    }
    function Write-FixtureSessionRecord($Directory,$Name,$Record){
        if($Name -cne 'graphics-evidence-wait.json'){throw 'Graphics barrier lost its wait receipt'}
        $script:waitReceipt=$Record
    }
    foreach($acknowledged in @($true,$false)){
        $script:acknowledged=$acknowledged;$script:watch=[pscustomobject]@{ElapsedMilliseconds=0};$script:waitReceipt=$null
        Wait-FixtureGraphicsEvidence 'C:\owned'
        if(!$script:waitReceipt.started_utc -or !$script:waitReceipt.completed_utc -or $script:waitReceipt.timeout_seconds -ne 360 -or $script:waitReceipt.timeout -ne !$acknowledged){throw 'Graphics evidence barrier lost its bounded acknowledgement or timeout outcome'}
        if(!$acknowledged -and $script:watch.ElapsedMilliseconds -ne 360000){throw 'Missing graphics evidence acknowledgement did not reach the bounded timeout'}
    }
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
    $directory=Join-Path ([IO.Path]::GetTempPath()) ('capy-fixture-source-'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($directory)|Out-Null
    $path=Join-Path $directory 'plan.json';$temporary=$path+'.tmp';$TimeoutMinutes=20;$HardwareGpu=$false
    $old=$null;$source=$null;$oldReader=$null;$sourceReader=$null;$fresh=$null;$freshReader=$null
    try{
        Write-FixturePlan $path @('previous-'+('x'*200))
        $before=[IO.File]::ReadAllBytes($path)
        $share=[IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete
        $old=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,$share)
        $oldReader=[IO.BinaryReader]::new($old);$prefix=$oldReader.ReadBytes([int]($before.Length/2))
        [IO.File]::WriteAllText($temporary,'seed',[Text.UTF8Encoding]::new($false))
        $source=[IO.File]::Open($temporary,[IO.FileMode]::Open,[IO.FileAccess]::Read,$share)
        Write-FixturePlan $path @('next')
        $retained=[Text.Encoding]::UTF8.GetString([byte[]]($prefix+$oldReader.ReadBytes($before.Length)))|ConvertFrom-Json
        if($retained.runs[0] -cne ('previous-'+('x'*200))){throw 'Publication changed a partially consumed old plan reader'}
        $null=$source.Seek(0,[IO.SeekOrigin]::Begin);$sourceReader=[IO.StreamReader]::new($source);$published=$sourceReader.ReadToEnd()
        $fresh=[IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,$share);$freshReader=[IO.StreamReader]::new($fresh);$current=$freshReader.ReadToEnd()
        if($current -cne $published){throw 'Fresh and retained source readers did not receive the same complete plan'}
        $plan=$current|ConvertFrom-Json
        if(($plan.runs -join ',') -cne 'next' -or $plan.timeout_minutes -ne 20 -or $plan.hardware_gpu){throw 'Publication lost the complete next plan'}
        if(Test-Path -LiteralPath $temporary){throw 'Publication retained the temporary plan pathname'}
    }finally{
        if($freshReader){$freshReader.Dispose()};if($fresh){$fresh.Dispose()};if($sourceReader){$sourceReader.Dispose()};if($source){$source.Dispose()};if($oldReader){$oldReader.Dispose()};if($old){$old.Dispose()}
        Remove-Item -LiteralPath $directory -Recurse -Force
    }
}
& {
    $directory=Join-Path ([IO.Path]::GetTempPath()) ('capy-fixture-evidence-'+[guid]::NewGuid().ToString('N'))
    $HardwareGpu=$false
    function Reviews{$process=[pscustomobject]@{Id=42};$process|Add-Member ScriptMethod Dispose {};$process}
    function Stop-Review{$script:evidenceStops++}
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
        foreach($mode in 'rename','unpublished','preserve','missing-final'){
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
            $script:evidenceStops=0
            try{Save-Evidence $log -PreserveReviews:($mode -eq 'preserve')}catch{$failure=$_}
            if($script:evidenceStops -ne $(if($mode -in 'preserve','missing-final'){0}else{1})){throw 'Evidence stopped a preserved review or failed ordinary review cleanup'}
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
    $statements=@($ast.EndBlock.Statements)
    $discovery=$statements|Where-Object {$_ -is [Management.Automation.Language.ForEachStatementAst] -and $_.Extent.Text -like 'foreach ($script in Get-ChildItem*'}
    $hardware=$statements|Where-Object {$_ -is [Management.Automation.Language.ForEachStatementAst] -and $_.Variable.VariablePath.UserPath -eq 'hardwareSelector'}
    $first=[array]::IndexOf($statements,$discovery);$last=[array]::IndexOf($statements,$hardware)
    if($first -lt 0 -or $last -le $first){throw 'Missing actual fixture discovery/registration boundary'}
    . ([scriptblock]::Create(($statements[$first..($last-1)]|ForEach-Object {$_.Extent.Text}) -join "`n"))
    $runs
}
& {
    $expected=@('tool-bubbles:hdr-dark','tool-bubbles:hdr-light')
    $leaves=@($edgeRuns.Keys|Where-Object {$_ -in $expected})
    if($leaves.Count -ne 2 -or $edgeRuns.Contains('tool-bubbles:HdrOnly')){throw 'HDR matrix must split the two full themes without an aggregate duplicate'}
    $selected=@(Select-Fixtures @($edgeRuns.Keys) @('tool-bubbles:HdrOnly'))
    $deduplicated=@(Select-Fixtures @($edgeRuns.Keys) (@('tool-bubbles:HdrOnly')+$expected))
    if(($selected -join ',') -cne ($expected -join ',') -or ($selected -join ',') -cne ($deduplicated -join ',')){throw 'HDR legacy selector lost or duplicated a theme'}
    $hdrAst=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/exercise-tool-bubbles.ps1'),[ref]$tokens,[ref]$errors)
    if(@($errors).Count){throw ($errors|Out-String)}
    $prefix=@();foreach($statement in $hdrAst.EndBlock.Statements){if($statement.Extent.Text.StartsWith('. (Join-Path')){break};$prefix+=$statement.Extent.Text}
    $probe=[scriptblock]::Create($hdrAst.ParamBlock.Extent.Text+"`n"+'$PSCommandPath="Invoke-HdrTheme"'+"`n"+($prefix -join "`n")+"`n"+'[pscustomobject]@{variant=$Variant;hdr=[bool]$HdrOnly;preview=[bool]$HdrPreviewOnly;retirement=[bool]$HdrRetirementOnly;contacts=[bool]$HdrContactsOnly;depth=$HdrDepth}')
    function Invoke-HdrTheme($Executable,$Variant,[switch]$HdrOnly,[switch]$HdrRetirementOnly,[switch]$HdrPreviewOnly,[switch]$HdrContactsOnly,$HdrDepth){
        [pscustomobject]@{variant=$Variant;hdr=[bool]$HdrOnly;preview=[bool]$HdrPreviewOnly;retirement=[bool]$HdrRetirementOnly;contacts=[bool]$HdrContactsOnly;depth=$HdrDepth}
    }
    $direct=@(& $probe -Executable 'C:\owned\CapyCanvas.exe' -HdrOnly)
    $split=@(foreach($name in $expected){
        $arguments=@($edgeRuns[$name].arguments)
        if(($arguments -join ',') -cne ('-Variant,'+($name -replace '^tool-bubbles:',''))){throw 'HDR leaf lost its exact theme argument'}
        & $probe -Executable 'C:\owned\CapyCanvas.exe' -Variant $arguments[1]
    })
    if($direct.Count -ne 2 -or $split.Count -ne 2 -or ($direct|ConvertTo-Json -Compress) -cne ($split|ConvertTo-Json -Compress)){throw 'HDR leaves changed the direct HdrOnly theme/check/depth union'}
    $selection=@($ast.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -in '$selected','$planned'})
    $list=$ast.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.IfStatementAst] -and $_.Clauses[0].Item1.Extent.Text -eq '$List'}
    $listBody=@($list.Clauses[0].Item2.Statements|Where-Object {$_ -isnot [Management.Automation.Language.ExitStatementAst]})
    $listCode=(($selection+$listBody)|ForEach-Object {$_.Extent.Text}) -join "`n"
    foreach($request in @($null,'tool-bubbles:HdrOnly')){
        $plan=& {$runs=$edgeRuns;$Name=$request;. ([scriptblock]::Create($listCode))}|ConvertFrom-Json
        $names=@($plan.runs|Where-Object {$_ -in $expected})
        if($names.Count -ne 2 -or @($plan.runs|Where-Object {$_ -ceq 'tool-bubbles:HdrOnly'}).Count){throw 'Actual List lost a full HDR leaf or retained the aggregate'}
        if($request -and @($plan.runs).Count -ne 2){throw 'Legacy HDR selector expanded beyond the two full themes'}
    }
}
foreach($case in @(@{option='CanvasResetOnly';journey='Check-CanvasReset'},@{option='CancelOnly';journey='Check-NativeEdgeCancel'})){
    $journey=$edgeAst.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $case.journey},$false)
    $defaults=$edgeAst.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.IfStatementAst] -and $_.Extent.Text.Contains("PSBoundParameters.ContainsKey('Device')")}
    $probe=[scriptblock]::Create($edgeAst.ParamBlock.Extent.Text+"`n"+$defaults.Extent.Text+"`n"+$journey.Body.EndBlock.Statements[0].Extent.Text+"`n"+'$Device')
    $arguments=@($edgeRuns[('edge-reach:'+$case.option)].arguments)
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
    $scenarios=@('photo','photo-open','photo-collapsed','paint');$themes=@('dark','light')
    $leaves=@($edgeRuns.Keys|Where-Object {$_ -like 'edge-reach:sweep-*'})
    if($leaves.Count -ne 8){throw 'Sweep matrix must contain exactly eight scenario/theme leaves'}
    $expected=@(foreach($scenario in $scenarios){foreach($theme in $themes){"edge-reach:sweep-$scenario-$theme"}})
    if((($leaves|Sort-Object) -join ',') -cne (($expected|Sort-Object) -join ',')){throw 'Sweep matrix changed its exact scenario/theme pairs'}
    $scoped=@($edgeRuns.Keys|Where-Object {'-SweepOnly' -in $edgeRuns[$_].arguments -or '-SweepScenario' -in $edgeRuns[$_].arguments})
    if($scoped.Count -ne 8 -or (($scoped|Sort-Object) -join ',') -cne (($expected|Sort-Object) -join ',')){throw 'Sweep matrix retained an aggregate or raw scenario duplicate'}
    foreach($name in @('edge-reach:SweepOnly')+@($scenarios|ForEach-Object {"edge-reach:$_"})){if($edgeRuns.Contains($name)){throw 'Sweep matrix retained a generic aggregate or raw scenario leaf'}}
    foreach($scenario in $scenarios){foreach($theme in $themes){
        $arguments=@($edgeRuns["edge-reach:sweep-$scenario-$theme"].arguments)
        if(($arguments -join ',') -cne "-SweepOnly,-SweepScenario,$scenario,-Theme,$theme"){throw 'Sweep leaf lost its exact scenario/theme arguments or changed device routing'}
    }}
    $selection=@($ast.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -in '$selected','$planned'})
    $list=@($ast.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.IfStatementAst] -and $_.Clauses[0].Item1.Extent.Text -eq '$List'})
    if($selection.Count -ne 2 -or $list.Count -ne 1){throw 'Missing actual fixture selection/List path'}
    $listBody=@($list[0].Clauses[0].Item2.Statements|Where-Object {$_ -isnot [Management.Automation.Language.ExitStatementAst]})
    $listCode=(($selection+$listBody)|ForEach-Object {$_.Extent.Text}) -join "`n"
    foreach($request in @($null,'edge-reach:SweepOnly','edge-reach:SweepOnly,edge-reach:sweep-photo-dark,edge-reach:SweepOnly')){
        $plan=& {$runs=$edgeRuns;$Name=$request;$List=$true;. ([scriptblock]::Create($listCode))}|ConvertFrom-Json
        $plannedLeaves=@($plan.runs|Where-Object {$_ -like 'edge-reach:sweep-*'})
        if($plannedLeaves.Count -ne 8 -or (($plannedLeaves|Sort-Object) -join ',') -cne (($expected|Sort-Object) -join ',')){throw 'Actual List plan lost or duplicated a sweep scenario/theme pair'}
        if($request -and @($plan.runs).Count -ne 8){throw 'Legacy SweepOnly alias expanded beyond the exact eight leaves'}
        foreach($name in $expected){if($null -eq $plan.requirements.$name){throw 'Actual List omitted a sweep requirement entry'}}
    }
    $selected=@(Select-Fixtures @($edgeRuns.Keys) @('edge-reach:SweepOnly'))
    $deduplicated=@(Select-Fixtures @($edgeRuns.Keys) (@('edge-reach:SweepOnly')+$expected+@('edge-reach:SweepOnly')))
    if(($selected -join ',') -cne ($deduplicated -join ',')){throw 'Legacy SweepOnly alias and explicit leaves did not deduplicate in inventory order'}
    $scenarioParameter=@($edgeAst.ParamBlock.Parameters|Where-Object {$_.Name.VariablePath.UserPath -eq 'SweepScenario'})
    $choices=@($scenarioParameter.Attributes|Where-Object {$_.TypeName.Name -eq 'ValidateSet'}|ForEach-Object {$_.PositionalArguments.Value})
    if($scenarioParameter.Count -ne 1 -or ($choices -join ',') -cne ($scenarios -join ',')){throw 'SweepScenario must retain its four actual validated choices'}
    $normalizers=@($edgeAst.EndBlock.Statements|Where-Object {
        $_ -is [Management.Automation.Language.IfStatementAst] -and @($_.FindAll({param($n)$n -is [Management.Automation.Language.VariableExpressionAst] -and $n.VariablePath.UserPath -in 'SweepScenario','Device'},$true)).Count
    })
    $sweep=$edgeAst.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Check-ReachSweep'},$false)
    $scenarioInit=@($sweep.Body.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -eq '$scenarios'})
    $themeInit=@($edgeAst.FindAll({param($n)$n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$themes'},$true))
    if($scenarioInit.Count -ne 1 -or $themeInit.Count -ne 1){throw 'Missing actual sweep scenario/theme initializer'}
    $probeStatements=@($normalizers)+@($scenarioInit)+@($themeInit)
    $probe=[scriptblock]::Create($edgeAst.ParamBlock.Extent.Text+"`n"+(($probeStatements|ForEach-Object {$_.Extent.Text}) -join "`n")+"`n"+'[pscustomobject]@{sweep=[bool]$SweepOnly;device=$Device;scenarios=@($scenarios);themes=@($themes)}')
    $direct=& $probe -Executable 'C:\owned\CapyCanvas.exe' -SweepOnly
    if(!$direct.sweep -or $direct.device -cne 'mouse' -or ($direct.scenarios -join ',') -cne ($scenarios -join ',') -or ($direct.themes -join ',') -cne ($themes -join ',')){throw 'Direct SweepOnly lost its original four scenarios/two-theme order or device default'}
    foreach($scenario in $scenarios){
        $implied=& $probe -Executable 'C:\owned\CapyCanvas.exe' -SweepScenario $scenario
        if(!$implied.sweep -or ($implied.scenarios -join ',') -cne $scenario -or ($implied.themes -join ',') -cne ($themes -join ',')){throw 'Binding SweepScenario did not imply one-scenario SweepOnly with original default themes'}
    }
    $rejected=$false;try{& $probe -Executable 'C:\owned\CapyCanvas.exe' -SweepScenario 'unknown'|Out-Null}catch{$rejected=$true}
    if(!$rejected){throw 'SweepScenario accepted an unregistered scenario'}
    $reach=$edgeAst.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Reach-Samples'},$false)
    if(!$reach){throw 'Missing actual Reach-Samples helper'}
    . ([scriptblock]::Create($reach.Extent.Text))
    function Layout{$coverageLayout}
    $layouts=@{};$index=0
    foreach($scenario in $scenarios){foreach($theme in $themes){
        $offset=$index*3;$left=if($index -eq 0){18}else{42};$id=100+$index*10
        $layouts["$theme/$scenario"]=[pscustomobject]@{viewport=@((300+$offset),200);dividers=@(
            [pscustomobject]@{id=$id;axis='horizontal';owner_side='left';fixed=$false;bounds=@{x=$left;y=20;width=6;height=120}},
            [pscustomobject]@{id=($id+1);axis='horizontal';owner_side='right';fixed=$false;bounds=@{x=(200+$offset);y=20;width=6;height=72}},
            [pscustomobject]@{id=($id+2);axis='vertical';owner_side='top';fixed=$false;bounds=@{x=(12+$offset);y=40;width=120;height=6}},
            [pscustomobject]@{id=($id+3);axis='horizontal';owner_side='left';fixed=$true;bounds=@{x=80;y=0;width=6;height=100}},
            [pscustomobject]@{id=($id+4);axis='vertical';owner_side=$null;fixed=$false;bounds=@{x=0;y=80;width=120;height=6}}
        )};$index++
    }}
    $legacy=@{};$legacyUnion=[Collections.Generic.List[string]]::new()
    foreach($theme in $direct.themes){foreach($scenario in $direct.scenarios){
        $key="$theme/$scenario";$coverageLayout=$layouts[$key]
        $legacy[$key]=@(Reach-Samples|ForEach-Object {@($_.divider,$_.axis,$_.depth,$_.x,$_.y,$_.dx,$_.dy) -join '|'})
        if($legacy[$key].Count -ne $(if($key -eq 'dark/photo'){21}else{27})){throw 'Synthetic sweep layout lost viewport-edge, axis, fixed or owner filtering coverage'}
        foreach($tuple in $legacy[$key]){$legacyUnion.Add($key+'|'+$tuple)}
    }}
    $actualUnion=[Collections.Generic.List[string]]::new();$visited=[Collections.Generic.List[string]]::new()
    foreach($name in $selected){
        $bound=@{Executable='C:\owned\CapyCanvas.exe'};$arguments=@($edgeRuns[$name].arguments)
        for($i=0;$i -lt $arguments.Count;$i++){
            $parameter=$arguments[$i].TrimStart('-')
            if($i+1 -lt $arguments.Count -and !$arguments[$i+1].StartsWith('-')){$i++;$bound[$parameter]=$arguments[$i]}else{$bound[$parameter]=$true}
        }
        $case=& $probe @bound
        if(!$case.sweep -or $case.device -cne 'mouse' -or $case.scenarios.Count -ne 1 -or $case.themes.Count -ne 1){throw 'Registered sweep arguments did not bind exactly one original case'}
        foreach($theme in $case.themes){foreach($scenario in $case.scenarios){
            $key="$theme/$scenario"
            if(!$legacy.ContainsKey($key) -or $visited.Contains($key)){throw 'Registered sweep path repeated or invented a scenario/theme case'}
            $visited.Add($key);$coverageLayout=$layouts[$key]
            $actual=@(Reach-Samples|ForEach-Object {@($_.divider,$_.axis,$_.depth,$_.x,$_.y,$_.dx,$_.dy) -join '|'})
            if($actual.Count -ne $legacy[$key].Count -or ($actual -join "`n") -cne ($legacy[$key] -join "`n")){throw 'Registered sweep changed sample tuple order or multiplicity within a case'}
            foreach($tuple in $actual){$actualUnion.Add($key+'|'+$tuple)}
        }}
    }
    if($visited.Count -ne 8 -or $actualUnion.Count -ne $legacyUnion.Count -or (($actualUnion|Sort-Object) -join "`n") -cne (($legacyUnion|Sort-Object) -join "`n")){throw 'Sweep leaf union lost or repeated original theme/scenario sample identities'}
    if(@($actualUnion|Select-Object -Unique).Count -ne $actualUnion.Count){throw 'Sweep leaf union duplicated a generated sample identity within one case'}
    'Sweep leaves preserve exact eight-case routing, List/alias selection, manual defaults and actual generated sample coverage.'
}
& {
    $popupPath=Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/exercise-popup.ps1'
    $popup=[Management.Automation.Language.Parser]::ParseFile($popupPath,[ref]$tokens,[ref]$errors)
    if(@($errors).Count){throw ($errors|Out-String)}
    $groups=@('controls','text-layout');$dark=@($groups|ForEach-Object {'popup:'+$_});$light=@($dark|ForEach-Object {$_+'-light'});$leaves=$dark+$light
    $actual=@($edgeRuns.Keys|Where-Object {($_ -split ':')[0] -eq 'popup'})
    if(($actual -join ',') -cne ($leaves -join ',')){throw 'Popup matrix must contain only its exact four group/theme leaves'}
    foreach($group in $groups){
        if(($edgeRuns[('popup:'+$group)].arguments -join ',') -cne "-Group,$group" -or ($edgeRuns[('popup:'+$group+'-light')].arguments -join ',') -cne "-Group,$group,-Theme,light"){throw 'Popup leaf changed its group/theme arguments'}
    }
    $deduplicated=@(Select-Fixtures @($edgeRuns.Keys) (@('popup:default','popup:light')+$leaves+@('popup:default')))
    if(($deduplicated -join ',') -cne ($leaves -join ',')){throw 'Popup parent aliases and explicit leaves repeated or lost a group/theme pair'}
    $expected=@($popup.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -eq '$expected'})
    $aggregate=@($popup.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.IfStatementAst] -and $_.Clauses[0].Item1.Extent.Text -eq '!$Group'})
    $driver=@($popup.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.TryStatementAst]})
    if($expected.Count -ne 1 -or $aggregate.Count -ne 1 -or $driver.Count -ne 1){throw 'Popup split lost its actual group counts, direct invocation or child lifecycle'}
    $validation=@($driver[0].Body.Statements|Where-Object {$_ -is [Management.Automation.Language.IfStatementAst] -and $_.Extent.Text -like '*$result.exit_code*'})
    if($validation.Count -ne 1){throw 'Popup child lost its native result validation'}
    $probe=[scriptblock]::Create($popup.ParamBlock.Extent.Text+"`n"+$expected[0].Extent.Text+"`n"+'[pscustomobject]@{group=$Group;theme=$Theme;counts=$expected}')
    $counts=@{controls=@(19,0);'text-layout'=@(7,16)};$popupTotal=0;$imeTotal=0
    foreach($group in $groups){
        $bound=& $probe -Executable 'C:\owned\Popup.exe' -Group $group
        if(($bound.counts -join ',') -cne ($counts[$group] -join ',')){throw 'Popup group no longer requires its exact original assertion counts'}
        $popupTotal+=$bound.counts[0];$imeTotal+=$bound.counts[1]
        foreach($fault in 'none','group','popup','ime','exit'){
            $result=[pscustomobject]@{group=$group;passed=$counts[$group][0];ime_passed=$counts[$group][1];exit_code=0};$expected=$counts[$group];$Group=$group;$run='owned-test'
            switch($fault){'group'{$result.group='wrong'} 'popup'{$result.passed--} 'ime'{$result.ime_passed--} 'exit'{$result.exit_code=1}}
            $failed=$false;try{. ([scriptblock]::Create($validation[0].Extent.Text))}catch{$failed=$true}
            if($failed -ne ($fault -ne 'none')){throw 'Popup native result accepted another group or incomplete counts, or rejected its exact result'}
        }
    }
    if($popupTotal -ne 26 -or $imeTotal -ne 16){throw 'Popup split lost the 26 popup and 16 IME assertion union'}
    $manual=Join-Path ([IO.Path]::GetTempPath()) ('capy-popup-groups-'+[guid]::NewGuid().ToString('N')+'.ps1')
    try{
        'param([string]$Executable,[string]$Theme,[string]$Group);[pscustomobject]@{executable=$Executable;theme=$Theme;group=$Group}'|Set-Content -LiteralPath $manual
        $dispatch=@($aggregate[0].Clauses[0].Item2.Statements|Where-Object {$_ -isnot [Management.Automation.Language.ReturnStatementAst]})
        foreach($theme in 'dark','light'){
            $command=($dispatch.Extent.Text -join "`n").Replace('$PSCommandPath',("'"+$manual.Replace("'","''")+"'"))
            $visited=@(& {$Executable='C:\owned\Popup.exe';$Theme=$theme;. ([scriptblock]::Create($command))})
            if(($visited.group -join ',') -cne ($groups -join ',') -or @($visited|Where-Object {$_.theme -cne $theme -or $_.executable -cne 'C:\owned\Popup.exe'}).Count){throw 'Direct Popup invocation must visit each original group exactly once with its chosen theme and executable'}
        }
    }finally{Remove-Item -LiteralPath $manual}
    . (Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/CapyEnvironment.ps1')
    $before=@($popup.EndBlock.Statements|Where-Object {$_ -is [Management.Automation.Language.AssignmentStatementAst] -and $_.Left.Extent.Text -in '$beforeTrace','$beforePath','$beforeTheme','$beforeGroup','$process'})
    if($before.Count -ne 5 -or @($driver[0].Finally.Statements|Where-Object {$_.Extent.Text -like "Restore-CapyEnvironmentValue 'CAPY_POPUP_GROUP'*"}).Count -ne 1){throw 'Popup child must save and restore its group environment through the existing helper'}
    & {
        . ([scriptblock]::Create(($before.Extent.Text -join "`n")))
        try{$env:CAPY_TRACE_UI='child';$env:CAPY_POPUP_FIXTURE='child';$env:CAPY_POPUP_THEME='child';$env:CAPY_POPUP_GROUP='child'}
        finally{. ([scriptblock]::Create(($driver[0].Finally.Statements.Extent.Text -join "`n")))}
        foreach($entry in @(@('CAPY_TRACE_UI',$beforeTrace),@('CAPY_POPUP_FIXTURE',$beforePath),@('CAPY_POPUP_THEME',$beforeTheme),@('CAPY_POPUP_GROUP',$beforeGroup))){
            if([Environment]::GetEnvironmentVariable($entry[0]) -cne $entry[1]){throw 'Popup child environment escaped its finally restoration'}
        }
    }
    'Popup leaves preserve exact counts, aliases, direct invocation and environment restoration.'
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
            $arguments=@($edgeRuns[('localization:'+$selector)].arguments)
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
if(($edgeRuns['edge-reach:pen'].arguments -join ',') -ne '-Device,pen' -or ($edgeRuns['edge-reach:ColumnResetOnly'].arguments -join ',') -ne '-ColumnResetOnly'){throw 'Native edge routing changed unrelated variants'}
$smoke=@(
    'activation','canvas-editing:smoke','canvas-editing:smoke-light','clipboard',
    'color-editor:light','column-stacks:touch','documents:new-drawing','documents:native-writeback-light',
    'drawers','drawers:light','editor','effects:light',
    'header','header:light','image-rows','layer-pickup',
    'layers','layers:PaperLight','localization-smoke:light','navigator',
    'navigator:light','persistence','popup:controls','popup:text-layout',
    'popup:controls-light','popup:text-layout-light',
    'saved-data:light','scopes','selection:light','startup-close',
    'switcher-drag:mouse','switcher','tab-drag','tab-drag:light',
    'tool-bubbles:light-pen','toolbar-components','window','window:light',
    'workspace-layout','workspace-layout:light','workspace-pickup','zoom-readout:touch'
)
$smokePlan=@(Select-Fixtures @($edgeRuns.Keys) @('smoke'))
if($smokePlan.Count -ne 42 -or ($smokePlan -join ',') -ne ($smoke -join ',')){throw 'Smoke must select its exact forty-two inventory entries'}
if(((Select-Fixtures @($edgeRuns.Keys) @('smoke','popup:default')) -join ',') -ne ($smoke -join ',')){throw 'An explicit smoke entry must not duplicate or expand variants'}
$smokeExtended=@($edgeRuns.Keys|Where-Object {$_ -in $smoke -or $_ -eq 'navigator:PixelsOnly'})
if(((Select-Fixtures @($edgeRuns.Keys) @('smoke','navigator:PixelsOnly')) -join ',') -ne ($smokeExtended -join ',')){throw 'Smoke must preserve an explicitly requested extra variant'}
$popupDark=@('popup:controls','popup:text-layout');$popupLight=@($popupDark|ForEach-Object {$_+'-light'});$popupLeaves=$popupDark+$popupLight
$available=@('header','header:catalog-only','header:catalog-only-light')+$popupLeaves+@('effects')
if(((Select-Fixtures $available @()) -join ',') -ne ($available -join ',')){throw 'An empty fixture request must select every available selector'}
if(((Select-Fixtures $available $null) -join ',') -ne ($available -join ',')){throw 'A null fixture request must select every available selector'}
if(((Select-Fixtures $available @('header-settings')) -join ',') -ne 'header:catalog-only,header:catalog-only-light'){throw 'Header settings must exercise both catalog themes'}
if(((Select-Fixtures $available @('popup:default')) -join ',') -cne ($popupDark -join ',')){throw 'Popup default alias lost its two dark leaves'}
if(((Select-Fixtures $available @('popup:light')) -join ',') -cne ($popupLight -join ',')){throw 'Popup light alias lost its two light leaves'}
if(((Select-Fixtures $available @('popup')) -join ',') -cne ($popupLeaves -join ',')){throw 'Popup family selection lost its four group/theme leaves'}
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
$script:ownershipSnapshotQueries=0
function Get-CimInstance{
    [CmdletBinding()]param($ClassName,$Filter,$Property)
    if($ClassName -cne 'Win32_Process' -or $Filter -or ($Property -join ',') -cne 'ProcessId,SessionId,CreationDate'){throw 'Initial ownership snapshot changed its identity fields or repeated a live match'}
    $script:ownershipSnapshotQueries++
    foreach($id in 41,42){
        $born=$runnerStarted.AddSeconds($(if($id -eq 41){1}else{2})).ToUniversalTime()
        [pscustomobject]@{ProcessId=$id;SessionId=[Diagnostics.Process]::GetCurrentProcess().SessionId;CreationDate=[DateTime]::new($born.Ticks-($born.Ticks%10L),[DateTimeKind]::Utc)}
    }
}
function Get-Process([int]$Id,[string]$ErrorAction){
    $script:queried.Add($Id)
    $process=[pscustomobject]@{Id=$Id;SessionId=[Diagnostics.Process]::GetCurrentProcess().SessionId;Handle=1;HasExited=$false;StartTime=$runnerStarted.AddSeconds($(if($Id -eq 41){1}else{2}))}
    $process|Add-Member NoteProperty Path $(if($Id -eq 41){$editorExecutable}else{'C:\other\CapyCanvas.exe'})
    $process|Add-Member ScriptMethod Dispose {}
    $process
}
try{
    $resultsFile=Join-Path $Output 'results.jsonl'
    $source=[IO.File]::ReadAllText((Join-Path $PSScriptRoot 'windows-vm.py'))
    $runner=[regex]::Match($source,'(?s)FIXTURE_RUNNER = r"""(?<script>.*?)"""')
    $command=[regex]::Match($source,'(?s)def fixture_status_command\(output\):\s+return \(r"""(?<prefix>.*?)""" \+ FIXTURE_RUNNER \+ r"""(?<suffix>.*?)"""\)\.replace\("\{output\}", output\)')
    if(!$runner.Success -or !$command.Success){throw 'Cannot extract the actual fixture progress command'}
    $commandText=$command.Groups['prefix'].Value+$runner.Groups['script'].Value+$command.Groups['suffix'].Value
    $status=[scriptblock]::Create($commandText.Replace('{output}',$Output))
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
    $script="[Console]::OutputEncoding=[Text.Encoding]::GetEncoding(437);`n"+$commandText.Replace('{output}',$Output)
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
    if(@(Reviews).Count -or $queried.Count -or $ownershipSnapshotQueries){throw 'Missing ownership receipt searched for processes or fetched a snapshot'}
    @{process_id=41;process_start_utc=$runnerStarted.AddSeconds(1).ToUniversalTime().ToString('o');windows=@(@{process_id=42;process_start_utc=$runnerStarted.AddSeconds(1).ToUniversalTime().ToString('o')},@{process_id=41;process_start_utc=$runnerStarted.AddSeconds(1).ToUniversalTime().ToString('o')})}|ConvertTo-Json|Set-Content (Join-Path $Output 'owned-context.json')
    $owned=@(Reviews)
    if($owned.Count -ne 1 -or $owned[0].Id -ne 41 -or (($queried|Sort-Object) -join ',') -ne '41' -or $ownershipSnapshotQueries -ne 1){throw 'Cleanup escaped its one atomic snapshot, opened a reused PID or lost its exact pinned owner'}
}finally{Remove-Item Function:\Get-Process;Remove-Item Function:\Get-CimInstance;Remove-Item -LiteralPath $Output -Recurse -Force}
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
    $all=@('shortcuts')+@($edgeRuns.Keys)
    foreach($case in @(@{names=$null;controls='Feedback,LayerDrop,Popup'},@{names='navigator:default';controls='';runs=@('navigator')},@{names='popup:default';controls='Popup';runs=$popupDark},@{names='popup:light';controls='Popup';runs=$popupLight},@{names='popup';controls='Popup';runs=$popupLeaves},@{names='button-feedback:default,popup:default';controls='Feedback,Popup'},@{names='layer-pickup:default';controls='LayerDrop'},@{names='smoke';controls='LayerDrop,Popup';runs=$smoke},@{names='smoke,popup:default';controls='LayerDrop,Popup';runs=$smoke},@{names='smoke,navigator:PixelsOnly';controls='LayerDrop,Popup';runs=$smokeExtended})){
        $selection=@{};if($case.names){$selection.Name=$case.names}
        $json=& (Get-Process -Id $PID).Path -NoProfile -File (Join-Path $PSScriptRoot 'run-fixtures.ps1') -Executable 'C:\not-built\CapyCanvas.exe' -Output $cacheRun @selection -List
        if($LASTEXITCODE){throw 'Listing fixture controls required a build or desktop'}
        $plan=$json|ConvertFrom-Json
        if(($plan.controls -join ',') -ne $case.controls -or !$plan.runs.Count){throw "Fixture plan selected unnecessary controls or lost its selectors: requested=$($case.names); controls=$($plan.controls -join ','); expected=$($case.controls); runs=$($plan.runs.Count)"}
        if($case.runs -and ($plan.runs.Count -ne $case.runs.Count -or ($plan.runs -join ',') -ne ($case.runs -join ','))){throw 'Listed smoke selectors differ from the exact requested inventory'}
        if(!$case.names -and ($plan.runs -join ',') -ne ($all -join ',')){throw 'Listing without names must return every available selector'}
        if(@($plan.requirements.PSObject.Properties).Count -ne $plan.runs.Count){throw 'Fixture requirements lost selected rows'}
        foreach($name in $plan.runs){
            $requirement=$plan.requirements.$name
            if($requirement.hardware_gpu -isnot [bool] -or $null -eq $requirement.controls){throw 'A fixture did not declare a Boolean hardware requirement with its control inventory'}
        }
        if($case.names -like 'smoke*'){
            $hardware=@($plan.requirements.PSObject.Properties|Where-Object {$_.Value.hardware_gpu}|ForEach-Object {$_.Name}|Sort-Object)
            if(($hardware -join ',') -ne 'drawers,drawers:light'){throw 'Smoke must require exactly its two approved hardware rows'}
        }
        if(!$case.names){
            if(@($plan.requirements.PSObject.Properties|Where-Object {$_.Value.hardware_gpu}).Count -ne 24){throw 'Hardware registry membership differs from the approved inventory'}
            foreach($family in 'lifecycle','multiwindow'){
                $marked=@($plan.requirements.PSObject.Properties|Where-Object {($_.Name -split ':')[0] -eq $family -and $_.Value.hardware_gpu}|ForEach-Object {$_.Name})
                if(($marked -join ',') -cne "${family}:RecoverGpu"){throw "Only the bounded device-loss hook row requires hardware in ${family}"}
            }
            if(!$plan.requirements.drawers.hardware_gpu -or !$plan.requirements.'drawers:light'.hardware_gpu -or !$plan.requirements.'toolbars:light'.hardware_gpu -or $plan.requirements.toolbars.hardware_gpu -or $plan.requirements.'toolbars:dark'.hardware_gpu -or !$plan.requirements.'tool-variations:sketch'.hardware_gpu -or !$plan.requirements.'tool-variations:sketch-light'.hardware_gpu -or $plan.requirements.'localization:journey-dark-large'.hardware_gpu -or $plan.requirements.navigator.hardware_gpu){throw 'Hardware requirements did not preserve exact row membership and CPU defaults'}
        }
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
    $loop=$ast.Find({param($node)$node -is [Management.Automation.Language.WhileStatementAst] -and $node.Condition.Extent.Text.Contains('Get-NextFixture')},$false)
    if(!$loop){throw 'The actual fixture queue loop is missing'}
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
    function Remove-FixtureJapaneseInput{}
    function Wait-FixtureReviews{}
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
    $runs=@{fixture=@{arguments=@();hardware_gpu=$false}};$queue=$null;$DebuggerPath=$null;$TimeoutMinutes=20
    & {
        $definition=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-FixtureReviews'},$false)
        if($definition){. ([scriptblock]::Create($definition.Extent.Text))}
        function Get-NextFixture($Planned,$Queue,$Claimed){
            $script:events.Add('claim');$script:claims++
            if($script:claims -eq 1){'fixture'}elseif($script:claims -eq 2){$script:mode='unchanged';'fixture'}
        }
        function Reviews($ContextPath){
            if($ContextPath -cne (Join-Path $Output 'fixture-context.json')){throw 'Timeout cleanup read another row context'}
            $script:review
        }
        function Write-FixtureResult($Path,$Json){$script:events.Add('result');$script:rowResults.Add(($Json|ConvertFrom-Json))}
        foreach($lingers in @($true,$false)){
            $script:scale=[pscustomobject]@{exists=$false;kind=$null;value=$null};$script:runtimeScale=1.0
            $script:mode='timeout';$script:restoreFails=$false;$script:readFails=$false;$script:pendingReads=0
            $script:events=[Collections.Generic.List[string]]::new();$script:claims=0;$script:reads=0
            $script:rowResults=[Collections.Generic.List[object]]::new();$script:lingers=$lingers
            $script:review=[pscustomobject]@{Id=41}
            $script:review|Add-Member ScriptMethod WaitForExit {param($milliseconds)if($milliseconds -lt 0 -or $milliseconds -gt 30000){throw 'Unbounded review exit wait'};$script:events.Add('review-wait');!$script:lingers}
            $script:review|Add-Member ScriptMethod Dispose {$script:events.Add('review-dispose')}
            $failure=$null;try{& $execute}catch{$failure=$_.Exception.Message}
            if($lingers -and ($script:claims -ne 1 -or @($script:events|Where-Object {$_ -eq 'child'}).Count -ne 1)){throw 'A timed-out row lingering review allowed the next selector to start'}
            if(!$script:rowResults.Count -or $script:rowResults[0].exit -cne 'timeout'){throw 'Review exit cleanup lost the first-attempt timeout'}
            if(!$script:events.Contains('review-dispose') -or $script:events.IndexOf('review-wait') -lt $script:events.IndexOf('kill')){throw 'Review exit wait did not follow the timeout kill and release its handle'}
            if($lingers){
                if($failure -notlike 'Session retired after timeout:*' -or $script:rowResults.Count -ne 1 -or $script:rowResults[0].message -notlike '*Session retired after timeout:*'){throw 'Lingering review did not write its timeout result and retirement reason before stopping'}
                if($script:events[-1] -ne 'result'){throw 'Retired session touched the queue after writing its result'}
            }elseif($failure -or $script:claims -ne 3 -or $script:rowResults.Count -ne 2 -or $script:rowResults[1].exit -ne 0){throw 'An exited review did not let the next selector run once'}
        }
    }
    & {
        function Get-NextFixture($Planned,$Queue,$Claimed){$script:events.Add('claim');$script:claims++;if($script:claims -le 2){'fixture'}}
        function Get-Content{if($script:graphicsText){$script:graphicsText}}
        function Start-Process{
            $script:events.Add('child')
            $child=[pscustomobject]@{Id=123;Handle=1;PriorityClass='Normal';ExitCode=$script:graphicsExit}
            $child|Add-Member ScriptMethod WaitForExit {param($timeout)$true};$child
        }
        function Save-Evidence($Log,$Renders,[switch]$PreserveReviews){
            $script:events.Add('evidence');$script:preserved=$PreserveReviews.IsPresent
            if($script:evidenceFails){throw 'Hardware fixture runtime scale differs from the round display'}
        }
        function Write-FixtureResult($Path,$Json){$script:events.Add('result');$script:rowResults.Add(($Json|ConvertFrom-Json))}
        function Write-FixtureGraphicsWithdrawal($Directory,$Result,$Identity){$script:events.Add('withdrawal');$script:withdrawal=$Result}
        function Wait-FixtureGraphicsEvidence($Directory){$script:events.Add('barrier')}
        foreach($case in @(
            @{exit=-2003304307;text='';cause='MILERR_NO_HARDWARE_DEVICE'},
            @{exit=-2005270523;text='';cause=$null},
            @{exit=-2005270522;text='';cause=$null},
            @{exit=-2005270521;text='';cause=$null},
            @{exit=1;text='Owned review exited with code 2289696773: failed';cause=$null},
            @{exit=1;text='DXGI_ERROR_DEVICE_RESET';cause=$null},
            @{exit=1;text='Error 0x887A0006';cause=$null},
            @{exit=1;text='ordinary assertion';cause=$null},
            @{exit=0;text='Handled DXGI_ERROR_DEVICE_REMOVED and MILERR_NO_HARDWARE_DEVICE';cause=$null}
        )){
          foreach($shared in @($false,$true)){
            foreach($evidenceFails in @($false,$true)){
                $SharedSession=$shared;$script:graphicsExit=$case.exit;$script:graphicsText=$case.text;$script:evidenceFails=$evidenceFails
                $script:scale=[pscustomobject]@{exists=$false;kind=$null;value=$null};$script:runtimeScale=1.0
                $script:mode='unchanged';$script:restoreFails=$false;$script:readFails=$false;$script:pendingReads=0
                $script:events=[Collections.Generic.List[string]]::new();$script:claims=0;$script:reads=0
                $script:rowResults=[Collections.Generic.List[object]]::new();$script:withdrawal=$null;$script:preserved=$false
                & $execute
                $withdraw=$shared -and $case.cause
                if($script:claims -ne $(if($withdraw){1}else{3}) -or $script:rowResults.Count -ne $(if($withdraw){1}else{2})){throw "Graphics classification changed queue progress: exit=$($case.exit), message=$($case.text), shared=$shared, cause=$($case.cause), claims=$($script:claims), rows=$($script:rowResults.Count)"}
                $row=$script:rowResults[0]
                if($row.exit -cne $(if($evidenceFails -and !$case.cause){'evidence'}else{$case.exit}) -or $row.message -cne ($case.text+$(if($evidenceFails){' Hardware fixture runtime scale differs from the round display'}else{''}))){throw 'Graphics failure changed the original row exit or error evidence'}
                if($case.cause){
                    if(!$row.infrastructure_invalid -or $row.infrastructure_cause -cne $case.cause){throw 'Graphics failure lost its infrastructure-invalid result when evidence collection failed'}
                }elseif($row.infrastructure_invalid -or $row.infrastructure_cause){throw 'An ordinary assertion was marked infrastructure-invalid'}
                if([bool]$script:withdrawal -ne [bool]$withdraw -or $script:preserved -ne [bool]$withdraw){throw 'Withdrawal or live review preservation escaped the failing shared worker'}
                if($withdraw -and ((($script:events|Where-Object {$_ -ne 'read'}) -join ',') -cne 'claim,child,evidence,result,withdrawal,barrier' -or $script:withdrawal.name -cne $row.name)){throw 'Withdrawal did not follow exactly one result before the next claim'}
            }
          }
        }
        foreach($case in @(
            @('DXGI_ERROR_DEVICE_REMOVED','0x887A0005','2289696773','-2005270523'),
            @('DXGI_ERROR_DEVICE_HUNG','0x887A0006','2289696774','-2005270522'),
            @('DXGI_ERROR_DEVICE_RESET','0x887A0007','2289696775','-2005270521')
        )){
            foreach($value in $case){
                if((Get-FixtureGraphicsFailure $value '') -or (Get-FixtureGraphicsFailure 1 ("Owned review exited with code ${value}: failed"))){throw "Product graphics failure was classified as infrastructure: $value"}
            }
        }
        $tail=@($ast.EndBlock.Statements|Where-Object {$_.Extent.StartOffset -gt $loop.Extent.EndOffset})
        function Stop-Review{$script:events.Add('stop')}
        function Set-Content($Path,$Value){$script:events.Add('complete')}
        foreach($bad in @($false,$true)){
            $SharedSession=$true;$graphicsCause=if($bad){'MILERR_NO_HARDWARE_DEVICE'}else{$null};$script:events=[Collections.Generic.List[string]]::new()
            . ([scriptblock]::Create(($tail.Extent.Text -join "`n")))
            if(($script:events -join ',') -cne 'stop,complete'){throw 'Withdrawal lost cleanup after its evidence barrier or lost its completion marker'}
        }
    }
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
            $childExit=switch($mode){{$_ -in @('timeout','screen-error')}{'timeout'} 'failure'{7} 'error'{'runner'} default{0}}
            $expectedExit=if($restoreFails -and $childExit -cne 'timeout'){'cleanup'}else{$childExit}
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
    & {
        $script:scale=[pscustomobject]@{exists=$false;kind=$null;value=$null};$script:runtimeScale=1.0
        $script:mode='unchanged';$script:restoreFails=$false;$script:readFails=$false;$script:pendingReads=0
        $script:events=[Collections.Generic.List[string]]::new();$script:claims=0;$script:reads=0
        $script:rowResults=[Collections.Generic.List[object]]::new();$script:scaleEvidence=[Collections.Generic.List[object]]::new()
        $runs=@{first=@{arguments=@();hardware_gpu=$true};second=@{arguments=@();hardware_gpu=$true}}
        $HardwareGpu=$true;$buildManifest=@{fixture_scale=1.5};$Executable=$editorExecutable
        $definition=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Save-Evidence'},$false)
        . ([scriptblock]::Create($definition.Extent.Text))
        function Control-Fixture{}
        function Assert-HardwareTraces{}
        function Get-NextFixture($Planned,$Queue,$Claimed){
            $script:claims++
            if($script:claims -le 2){@('first','second')[$script:claims-1]}
        }
        function Reviews{}
        function Get-ChildItem($LiteralPath,$Filter,[switch]$File,[switch]$Recurse){
            if($Filter){[pscustomobject]@{Name='ui-state-123.json';FullName='ui-state-123.json'}}
        }
        function Get-Content($LiteralPath,[switch]$Raw){
            if($LiteralPath -like '*-context.json'){'{"process_id":123}'}
            elseif($LiteralPath -eq 'ui-state-123.json'){
                @{process_id=123;window_id=1;model=@{windows_adapter='Fixture GPU (DiscreteGpu)';layout_input=@{cache=@{context=@{scale=$(if(!$script:scaleEvidence.Count){1.25}else{1.5})}}}}}|ConvertTo-Json -Depth 8 -Compress
            }
        }
        function Set-Content($LiteralPath,[Parameter(ValueFromPipeline)]$Value){
            process{if($LiteralPath -like '*-display.json'){$script:scaleEvidence.Add(@{path=$LiteralPath;receipt=($Value|ConvertFrom-Json)})}}
        }
        function Write-FixtureResult($Path,$Json){$script:rowResults.Add(($Json|ConvertFrom-Json))}
        & $execute
        if($script:rowResults.Count -ne 2 -or $script:rowResults[0].name -ne 'first' -or $script:rowResults[0].exit -ne 'evidence' -or $script:rowResults[0].message.Trim() -ne 'Hardware fixture runtime scale differs from the round display'){
            throw 'A hardware scale mismatch lost its failing fixture result'
        }
        if($script:rowResults[1].name -ne 'second' -or $script:rowResults[1].exit -ne 0 -or $script:claims -ne 3 -or $script:scaleEvidence.Count -ne 2){throw 'A hardware scale mismatch stopped the fixture queue or skipped scale evidence'}
        if($script:scaleEvidence[0].path -notlike '*first-display.json' -or $script:scaleEvidence[0].receipt.context_scales[0].scale -ne 1.25 -or $script:scaleEvidence[1].receipt.context_scales[0].scale -ne 1.5){throw 'Hardware scale evidence did not retain each row actual scale'}
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

& {
    Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;using Microsoft.Win32;
namespace CapyLocalDumpsTest {
 public class Key:IDisposable {
  public Dictionary<string,object> Values=new Dictionary<string,object>();public Dictionary<string,RegistryValueKind> Kinds=new Dictionary<string,RegistryValueKind>();public bool FailSet;public int FailAfter;public int Children;
  public int ValueCount{get{return Values.Count;}}public int SubKeyCount{get{return Children;}}
  public string[] GetValueNames(){var a=new string[Values.Count];Values.Keys.CopyTo(a,0);return a;}
  public RegistryValueKind GetValueKind(string n){return Kinds[n];}
  public object GetValue(string n,object d,RegistryValueOptions o){return Values.ContainsKey(n)?Values[n]:d;}
  public void SetValue(string n,object v,RegistryValueKind k){if(FailSet||(FailAfter>0&&--FailAfter==0))throw new InvalidOperationException("Registry write failed");Values[n]=v;Kinds[n]=k;}
  public void DeleteValue(string n,bool fail){Values.Remove(n);Kinds.Remove(n);}
  public void Dispose(){}
 }
 public class Registry {
  public Dictionary<string,Key> Keys=new Dictionary<string,Key>();public string Root="SOFTWARE\\Microsoft\\Windows\\Windows Error Reporting\\LocalDumps";public bool PartialArm;
  public Key OpenSubKey(string p){return Keys.ContainsKey(p)?Keys[p]:null;}public Key OpenSubKey(string p,bool w){return OpenSubKey(p);}
  public Key CreateSubKey(string p){if(!Keys.ContainsKey(Root))Keys[Root]=new Key();if(!Keys.ContainsKey(p)){Keys[p]=new Key();if(p!=Root)Keys[Root].Children++;}if(PartialArm)Keys[p].FailAfter=2;return Keys[p];}
  public void DeleteSubKey(string p,bool fail){if(Keys.ContainsKey(p)){if(Keys[p].ValueCount!=0||Keys[p].SubKeyCount!=0)throw new InvalidOperationException("Nonempty key deletion");Keys.Remove(p);if(p!=Root)Keys[Root].Children--;}}
 }
}
'@
    foreach($name in 'Get-FixtureLocalDumps','Enable-FixtureLocalDumps','Restore-FixtureLocalDumps'){
        $node=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$false)
        if(!$node){throw "Missing LocalDumps helper: $name"}
        . ([scriptblock]::Create($node.Extent.Text.Replace('[Microsoft.Win32.Registry]::LocalMachine','$script:registry')))
    }
    $loop=$ast.Find({param($node)$node -is [Management.Automation.Language.WhileStatementAst] -and $node.Condition.Extent.Text.Contains('Get-NextFixture')},$false)
    if(!$loop -or $loop.Parent -isnot [Management.Automation.Language.NamedBlockAst]){throw 'Private fixture loop unexpectedly owns privileged WER lifecycle'}
    $temporary=Join-Path ([IO.Path]::GetTempPath()) ('capy-local-dumps-'+[Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory $temporary|Out-Null
    try{
        $cases=0
        foreach($kind in @('absent','parent-only','empty','String','ExpandString','DWord','QWord','Binary','MultiString','None')){
            foreach($mode in @('success','imported-snapshot','partial-arm','restore-fault','concurrent-value','concurrent-child')){
                $script:registry=[CapyLocalDumpsTest.Registry]::new();$path=$script:registry.Root+'\CapyCanvas.exe'
                if($kind -eq 'parent-only'){$null=$script:registry.CreateSubKey($script:registry.Root)}
                if($kind -notin @('absent','parent-only')){
                    $key=$script:registry.CreateSubKey($path)
                    if($kind -ne 'empty'){
                        $key.SetValue('Unrelated','keep',[Microsoft.Win32.RegistryValueKind]::String)
                        $value=switch($kind){'DWord'{[int]-7};'QWord'{[long]9876543210};'Binary'{[byte[]]@(0,255,13)};'None'{[byte[]]@(11,12)};'MultiString'{[string[]]@('one','two')};default{'%UNEXPANDED%\original'}}
                        if($kind -in @('Binary','None')){$value=[byte[]]$value}elseif($kind -eq 'MultiString'){$value=[string[]]$value}
                        foreach($name in @('DumpFolder','DumpCount','DumpType')){$key.SetValue($name,$value,[Enum]::Parse([Microsoft.Win32.RegistryValueKind],$kind))}
                    }
                }
                $dumpState=Get-FixtureLocalDumps;$before=$dumpState|ConvertTo-Json -Depth 12 -Compress
                $Output=Join-Path $temporary ($kind+'-'+$mode);New-Item -ItemType Directory $Output|Out-Null
                if($mode -eq 'partial-arm'){$script:registry.PartialArm=$true}
                $armError=$null;try{Enable-FixtureLocalDumps $dumpState}catch{$armError=$_}
                if($mode -eq 'partial-arm'){if(!$armError){throw 'Partial write did not exercise helper failure'}}elseif($armError){throw $armError}
                $armed=$script:registry.OpenSubKey($path)
                if(!$armError){
                    if($armed.GetValue('DumpFolder',$null,0) -ne (Join-Path $Output 'wer/dumps') -or $armed.GetValue('DumpCount',$null,0) -ne 3 -or $armed.GetValue('DumpType',$null,0) -ne 2){throw 'Wrong WER arming'}
                    if($armed.GetValueKind('DumpFolder') -ne [Microsoft.Win32.RegistryValueKind]::ExpandString){throw 'Wrong dump folder registry type'}
                }
                if($mode -eq 'imported-snapshot'){$dumpState=Import-Clixml -LiteralPath (Join-Path $Output 'wer/registry-before.clixml')}
                if($mode -eq 'concurrent-child'){$armed.Children++}
                if($mode -in @('concurrent-value','concurrent-child')){$armed.SetValue('Concurrent','keep',[Microsoft.Win32.RegistryValueKind]::String)}
                $fault=$mode -eq 'restore-fault' -and $kind -notin @('absent','parent-only','empty')
                if($fault){$armed.FailSet=$true}
                $restoreError=$null;try{Restore-FixtureLocalDumps $dumpState}catch{$restoreError=$_}
                if($fault){if(!$restoreError){throw 'Typed helper hid restoration failure'}}
                elseif($mode -in @('concurrent-value','concurrent-child')){
                    $key=$script:registry.OpenSubKey($path)
                    if(!$key -or $key.Values['Concurrent'] -ne 'keep' -or ($mode -eq 'concurrent-child' -and $key.SubKeyCount -ne 1)){throw 'Cleanup removed an unowned value or child'}
                    if(((Get-FixtureLocalDumps).values|ConvertTo-Json -Depth 12 -Compress) -cne ($dumpState.values|ConvertTo-Json -Depth 12 -Compress)){throw 'Owned typed values did not restore beside concurrent data'}
                }elseif($restoreError -or ((Get-FixtureLocalDumps)|ConvertTo-Json -Depth 12 -Compress) -cne $before){throw "Typed registry snapshot did not restore: $kind $mode"}
                if(!(Test-Path (Join-Path $Output 'wer/registry-before.clixml'))){throw 'Original typed registry evidence was not exported'}
                $cases++
            }
        }
        "Per-app LocalDumps actual typed helpers passed $cases cases"
    }finally{Remove-Item -LiteralPath $temporary -Recurse -Force}
}
& {
    $assignment=$ast.Find({param($node)$node -is [Management.Automation.Language.AssignmentStatementAst] -and $node.Left.Extent.Text -eq '$env:CAPY_WAIT_SCALE'},$false)
    if(!$assignment){throw 'The fixture runner does not choose a wait scale'}
    foreach($case in @(@($true,$false,$null),@($true,$true,'3'),@($false,$false,'3'))){
        $HardwareGpu,$SharedSession,$expected=$case
        $actual=& ([scriptblock]::Create($assignment.Right.Extent.Text))
        if($actual -cne $expected){throw "Wait scale for hardware=$HardwareGpu session=${SharedSession}: $actual"}
    }
    'Shared sessions keep the WARP wait scale; exclusive hardware keeps the hardware bound'
}
& {
    $definition=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Save-Evidence'},$false)
    . ([scriptblock]::Create($definition.Extent.Text))
    function Stop-Review{}
    function Reviews{}
    $HardwareGpu=$true;$buildManifest=@{fixture_scale=1.5}
    $directory=Join-Path ([IO.Path]::GetTempPath()) ('capy-feedback-evidence-'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($directory)|Out-Null
    try{
        $Executable=Join-Path $directory 'CapyCanvas.exe';$log=Join-Path $directory 'row'
        foreach($kind in 'Feedback','Popup','LayerDrop'){$controlFixture=$kind;Save-Evidence $log}
        @{process_id=7;trace_directory=$directory}|ConvertTo-Json|Set-Content -LiteralPath ($log+'-context.json')
        foreach($kind in 'Feedback','Popup','LayerDrop'){$controlFixture=$kind;Save-Evidence $log}
        $controlFixture=$null;$failure=$null
        try{Save-Evidence $log}catch{$failure=$_.Exception.Message}
        if($failure -ne 'Hardware fixture runtime scale differs from the round display'){throw "A rendered row without scale evidence passed: $failure"}
        Remove-Item -LiteralPath ($log+'-context.json')
        $controlFixture=$null;$failure=$null
        try{Save-Evidence $log}catch{$failure=$_.Exception.Message}
        if($failure -notlike 'Missing fixture context*'){throw 'A rendered row without its context passed'}
        Save-Evidence $log $false
        $trace=Join-Path $directory 'closed-window';[IO.Directory]::CreateDirectory($trace)|Out-Null
        [IO.File]::WriteAllText((Join-Path $trace 'ui-state-90-3.json'),'{"process_id":90}')
        @{windows=@(@{process_id=90;trace_directory=$trace})}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath ($log+'-context.json')
        function Assert-HardwareTraces{}
        $buildManifest=@{}
        Save-Evidence $log
        if(!(Test-Path -LiteralPath ($log+'-evidence\ui-state-90-3.json'))){throw 'Evidence lost the trace of a recorded window whose app already exited'}
    }finally{Remove-Item -LiteralPath $directory -Recurse -Force}
    'Control and registered unrendered rows skip app adapter evidence; rendered rows require context and scale and keep traces of exited windows'
}

& {
    $directory=Join-Path ([IO.Path]::GetTempPath()) ('capy-desktop-evidence-'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($directory)|Out-Null
    $desktopRoot=Join-Path $directory 'test-desktop';$trace=Join-Path $directory 'trace'
    [IO.Directory]::CreateDirectory($trace)|Out-Null
    $HardwareGpu=$false;$controlFixture=$null;$Executable=Join-Path $trace 'CapyCanvas.exe'
    $nativeExit=$global:LASTEXITCODE;$robocopy=Join-Path $env:WINDIR 'System32/robocopy.exe'
    function Reviews{}
    function Stop-Review{}
    function Join-Path($Path,$ChildPath){
        if($ChildPath -eq '../../artifacts/windows/test-desktop'){if($Path -and $Path -ne $PSScriptRoot){throw 'Desktop evidence root escaped the fixture runner'};return $desktopRoot}
        Microsoft.PowerShell.Management\Join-Path $Path $ChildPath
    }
    function robocopy.exe {
        $script:copies.Add([string]$args[0])
        if($script:desktopCase -eq 'copy-error'){$global:LASTEXITCODE=8;return}
        & $robocopy @args|Out-Null
    }
    try{
        $recorded=Join-Path $desktopRoot ([guid]::NewGuid().ToString('N'))
        $unrecorded=Join-Path $desktopRoot ([guid]::NewGuid().ToString('N'))
        foreach($folder in @($recorded,$unrecorded)){
            [IO.Directory]::CreateDirectory($folder)|Out-Null
            foreach($name in 'failure.txt','transcript.log','invocation.clixml','result.json'){[IO.File]::WriteAllText((Join-Path $folder $name),($folder+':'+$name))}
        }
        foreach($case in 'recorded','without-context','rendered-without-context','unrecorded','foreign','traversal','malformed-id','copy-error'){
            $HardwareGpu=$case -eq 'rendered-without-context'
            $script:desktopCase=$case;$script:copies=[Collections.Generic.List[string]]::new()
            $log=Join-Path $directory $case;$evidence=$log+'-evidence'
            $path=switch($case){
                'foreign'{Join-Path $directory ([guid]::NewGuid().ToString('N'))}
                'traversal'{Join-Path $desktopRoot ('../'+[guid]::NewGuid().ToString('N'))}
                'malformed-id'{Join-Path $desktopRoot 'not-a-run'}
                default{$recorded}
            }
            if($case -notin @('without-context','rendered-without-context')){@{process_id=41;trace_directory=$trace}|ConvertTo-Json|Set-Content -LiteralPath ($log+'-context.json')}
            if($case -eq 'unrecorded'){"Unrelated path: $unrecorded"|Set-Content -LiteralPath ($log+'.log')}
            else{@("Private desktop artifacts: $path","Private desktop artifacts: $path","Unrelated path: $unrecorded")|Set-Content -LiteralPath ($log+'.log')}
            $failure=$null;try{Save-Evidence $log}catch{$failure=$_.Exception.Message}
            if($case -in @('foreign','traversal','malformed-id')){
                if($failure -notlike 'Invalid private desktop evidence path:*' -or $script:copies.Count){throw 'An unowned private desktop path reached evidence copying'}
            }elseif($case -eq 'copy-error'){
                if($failure -notlike 'Could not preserve private desktop evidence:*' -or $script:copies.Count -ne 1){throw 'Private desktop evidence copy failure was lost or retried'}
            }elseif($case -eq 'unrecorded'){
                if($failure -or $script:copies.Count -or (Test-Path (Join-Path $evidence 'test-desktop'))){throw 'Evidence searched for an unrecorded private desktop run'}
            }else{
                $saved=Join-Path $evidence ('test-desktop/'+(Split-Path -Leaf $recorded))
                if(($case -eq 'rendered-without-context' -and $failure -notlike 'Missing fixture context:*') -or ($case -ne 'rendered-without-context' -and $failure) -or !(Test-Path -LiteralPath (Join-Path $saved 'failure.txt'))){throw "The recorded private desktop failure and transcript were not retrieved: $case ($failure)"}
                if($script:copies.Count -ne 1 -or $script:copies[0] -cne $recorded){throw 'Desktop evidence copied more than the exact recorded run once'}
                foreach($name in 'failure.txt','transcript.log','invocation.clixml','result.json'){
                    if([IO.File]::ReadAllText((Join-Path $saved $name)) -cne [IO.File]::ReadAllText((Join-Path $recorded $name))){throw 'Private desktop evidence changed or omitted an owned artifact'}
                }
                if(@(Get-ChildItem -LiteralPath (Join-Path $evidence 'test-desktop') -Directory).Count -ne 1){throw 'Private desktop evidence included another run'}
            }
        }
    }finally{$global:LASTEXITCODE=$nativeExit;Remove-Item -LiteralPath $directory -Recurse -Force}
    'Evidence retrieves only the exact recorded private desktop run, including failures without an app context'
}

& {
    foreach($name in 'Reviews','Stop-Review','Wait-FixtureReviews'){
        $node=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$false)
        . ([scriptblock]::Create($node.Extent.Text))
    }
    $script:snapSession=[Diagnostics.Process]::GetCurrentProcess().SessionId
    $script:snapStart=[DateTime]::Parse('2026-11-09T00:16:40.0111228Z',[Globalization.CultureInfo]::InvariantCulture,[Globalization.DateTimeStyles]::RoundtripKind)
    $script:snapCimStart=[DateTime]::Parse('2026-11-09T00:16:40.0111220Z',[Globalization.CultureInfo]::InvariantCulture,[Globalization.DateTimeStyles]::RoundtripKind)
    function New-SnapshotRow($Id,$Session=$script:snapSession,$Birth=$script:snapCimStart){
        $row=[pscustomobject]@{ProcessId=$Id;SessionId=$Session;CreationDate=$Birth;Snapshot=$true}
        $row|Add-Member ScriptProperty Handle {$script:snapMetadataHandles++;throw 'Snapshot metadata was opened'}
        $row|Add-Member ScriptMethod WaitForExit {param($milliseconds)$script:snapMetadataWaits++;throw 'Snapshot metadata was waited'}
        $row
    }
    function Get-Item($LiteralPath){if($LiteralPath -cne 'row-context.json'){throw 'Timeout cleanup escaped its recorded row'};[pscustomobject]@{FullName=$LiteralPath}}
    function Get-ChildItem{
        $(if($script:snapCase -eq 'bad-json'){'bad-context.json';'live-context.json'}else{'row-context.json'})|ForEach-Object {[pscustomobject]@{FullName=$_}}
    }
    function Get-Content($LiteralPath){
        if($script:snapCase -eq 'bad-json'){
            if($LiteralPath -ne 'live-context.json'){return '{'}
            return (@{process_id=42;process_start_utc=$script:snapStart.ToString('o')}|ConvertTo-Json)
        }
        if($script:snapCase -eq 'empty'){return '{"windows":[]}'}
        $context=@{process_id=41;process_start_utc=$script:snapStart.ToString('o');windows=@(@{process_id=42;process_start_utc=$script:snapStart.ToString('o')})}
        if($script:snapCase -eq 'missing-birth'){$context.Remove('process_start_utc')}
        if($script:snapCase -eq 'malformed-birth'){$context.process_start_utc='not-a-date'}
        if($script:snapCase -like 'close-*'){
            $close=@{process_id=41;process_start_utc=$script:snapStart.ToString('o');window_only=($script:snapCase -eq 'close-window');result=$(if($script:snapCase -eq 'close-failed'){'fail'}else{'pass'})}
            if($script:snapCase -eq 'close-other-birth'){$close.process_start_utc=$script:snapStart.AddTicks(-1).ToString('o')}
            if($script:snapCase -eq 'close-missing-birth'){$close.Remove('process_start_utc')}
            $context.closes=@($close)
        }
        $context|ConvertTo-Json -Depth 5
    }
    function Get-CimInstance{
        [CmdletBinding()]param($ClassName,$Filter,$Property)
        if($ClassName -cne 'Win32_Process' -or ($Property -join ',') -cne 'ProcessId,SessionId,CreationDate'){throw 'Review snapshot changed its atomic identity fields'}
        if($Filter){
            if($Filter -cne 'ProcessId=41'){throw 'Fresh snapshot escaped the failed owner PID'}
            $script:snapFresh++
            switch($script:snapCase){
                {$_ -in 'snapshot-open-exit','pin-start-exit'}{return}
                'fresh-foreign'{New-SnapshotRow 41 ($script:snapSession+1);return}
                'fresh-birth'{New-SnapshotRow 41 $script:snapSession ($script:snapCimStart.AddTicks(10));return}
                'fresh-session-unknown'{New-SnapshotRow 41 $null;return}
                'fresh-birth-unknown'{New-SnapshotRow 41 $script:snapSession $null;return}
                'fresh-query-error'{throw 'Fresh CIM snapshot failed'}
            }
            New-SnapshotRow 41;return
        }
        $script:snapInitial++
        if($script:snapCase -eq 'snapshot-query-error'){throw 'Initial CIM snapshot failed'}
        if($script:snapCase -eq 'snapshot-pid-invalid'){New-SnapshotRow 'invalid';return}
        if($script:snapCase -ne 'gone-before-snapshot'){
            switch($script:snapCase){
                'foreign-reuse'{New-SnapshotRow 41 ($script:snapSession+1) $null}
                'birth-reuse'{New-SnapshotRow 41 $script:snapSession ($script:snapCimStart.AddTicks(10))}
                'initial-session-unknown'{New-SnapshotRow 41 $null}
                'initial-birth-unknown'{New-SnapshotRow 41 $script:snapSession $null}
                'bad-precision'{New-SnapshotRow 41 $script:snapSession ($script:snapCimStart.AddTicks(1))}
                default{New-SnapshotRow 41}
            }
        }
        New-SnapshotRow 42
        New-SnapshotRow 99 ($script:snapSession+1) $null
    }
    function Get-Process{
        [CmdletBinding()]param([int]$Id)
        if($Id -notin 41,42){throw 'Review lookup escaped its recorded owner PIDs'}
        $script:snapQueries[$Id]++
        if($Id -eq 41 -and $script:snapCase -eq 'process-query-error'){throw 'Recorded process lookup denied'}
        $tag="$Id/$($script:snapQueries[$Id])"
        $process=[pscustomobject]@{Id=$Id;Tag=$tag;Pinned=$false;Snapshot=$false}
        $process|Add-Member ScriptProperty SessionId {
            $script:snapProcessSessions++
            if($this.Id -eq 41 -and ($script:snapCase -eq 'gone-before-snapshot' -or ($script:snapCase -eq 'snapshot-open-exit' -and $script:snapQueries[41] -gt 1))){return $null}
            if($this.Id -eq 41 -and $script:snapCase -eq 'foreign-reuse'){return $script:snapSession+1}
            $script:snapSession
        }
        $process|Add-Member ScriptProperty HasExited {$script:snapProcessExits++;!$script:snapLive[[int]$this.Id]}
        $process|Add-Member ScriptProperty Handle {
            $script:snapHandleReads[[int]$this.Id]++
            if($this.Id -eq 41 -and $script:snapCase -in 'snapshot-open-exit','live-denied','fresh-foreign','fresh-birth','fresh-session-unknown','fresh-birth-unknown','fresh-query-error'){
                if($script:snapCase -eq 'snapshot-open-exit'){$script:snapLive[41]=$false}
                return $null
            }
            $this.Pinned=$true;[IntPtr]1
        }
        $process|Add-Member ScriptProperty StartTime {
            $script:snapBirthReads[[int]$this.Id]++
            if(!$this.Pinned){throw 'Process creation time was read without its pinned handle'}
            if($this.Id -eq 41 -and $script:snapCase -in 'pin-start-unavailable','pin-start-exit'){return $null}
            if($this.Id -eq 41 -and $script:snapCase -eq 'same-us-alias'){return $script:snapStart.AddTicks(1)}
            if($this.Id -eq 41 -and $script:snapCase -eq 'birth-reuse'){return $script:snapStart.AddTicks(10)}
            $script:snapStart
        }
        $process|Add-Member ScriptMethod WaitForExit {
            param($milliseconds)
            $script:snapWaits.Add([int]$this.Id)
            if($milliseconds -lt 0 -or $milliseconds -gt 30000 -or !$this.Pinned -or $this.Id -notin $script:snapTargets){throw 'Review wait escaped its pinned exact identity or 30-second budget'}
            if($script:snapAction -eq 'stop' -and ($script:snapStops -join ',') -cne ($script:snapTargets -join ',')){throw 'Review wait preceded an attempt to stop every verified owner'}
            if($this.Id -eq 41 -and $script:snapCase -in 'stop-error','wait-lingering'){return $false}
            $script:snapLive[[int]$this.Id]=$false;$true
        }
        $process|Add-Member ScriptMethod Dispose {$script:snapDisposed.Add($this.Tag);if($this.Id -eq 41 -and $script:snapCase -eq 'dispose-error'){throw 'Owner41 disposal failed'}}
        $script:snapReturned.Add($tag);$script:snapProcesses[$Id]=$process;$process
    }
    function Stop-Process{
        [CmdletBinding()]param([Parameter(ValueFromPipeline)]$InputObject,[switch]$Force)
        process{
            if($InputObject.Snapshot){$script:snapMetadataStops++;throw 'Snapshot metadata was stopped'}
            $script:snapStops.Add([int]$InputObject.Id)
            if(!$Force -or !$InputObject.Pinned -or $InputObject.Id -notin $script:snapTargets -or ![object]::ReferenceEquals($InputObject,$script:snapProcesses[[int]$InputObject.Id])){throw 'Stop escaped a verified pinned process identity'}
            if($InputObject.Id -eq 41 -and $script:snapCase -eq 'stop-error'){throw 'Owner41 stop denied'}
            $script:snapLive[[int]$InputObject.Id]=$false
        }
    }
    function Test-SnapshotCase($Action,$Case){
        $script:snapAction=$Action;$script:snapCase=$Case
        $globalFault=$Case -in 'snapshot-query-error','snapshot-pid-invalid' -or ($Action -eq 'wait' -and $Case -in 'bad-json','empty')
        $fault=$Case -in 'live-denied','initial-session-unknown','initial-birth-unknown','bad-precision','fresh-session-unknown','fresh-birth-unknown','fresh-query-error','process-query-error','pin-start-unavailable','same-us-alias','missing-birth','malformed-birth','bad-json','snapshot-query-error','snapshot-pid-invalid','stop-error','dispose-error','wait-lingering','empty'
        $open41=$Case -notin 'gone-before-snapshot','foreign-reuse','birth-reuse','initial-session-unknown','initial-birth-unknown','bad-precision','missing-birth','malformed-birth','close-exact','bad-json','snapshot-query-error','snapshot-pid-invalid','empty'
        $fresh=$Case -in 'live-denied','fresh-foreign','fresh-birth','fresh-session-unknown','fresh-birth-unknown','fresh-query-error','process-query-error','pin-start-unavailable','pin-start-exit','same-us-alias'
        $kernelExitCheck=$Case -eq 'snapshot-open-exit' -or ($fresh -and $Case -ne 'process-query-error')
        $matches41=$Case -in 'live-match','close-other-birth','close-window','close-failed','close-missing-birth','stop-error','dispose-error','wait-lingering'
        $script:snapTargets=if($globalFault -or ($Action -eq 'wait' -and $fault -and $Case -ne 'wait-lingering')){@()}elseif($matches41){@(41,42)}else{@(42)}
        $script:snapInitial=0;$script:snapFresh=0;$script:snapQueries=@{41=0;42=0};$script:snapHandleReads=@{41=0;42=0};$script:snapBirthReads=@{41=0;42=0};$script:snapLive=@{41=($Case -ne 'gone-before-snapshot');42=$true};$script:snapProcesses=@{}
        $script:snapProcessSessions=0;$script:snapProcessExits=0;$script:snapMetadataHandles=0;$script:snapMetadataWaits=0;$script:snapMetadataStops=0
        $script:snapReturned=[Collections.Generic.List[string]]::new();$script:snapDisposed=[Collections.Generic.List[string]]::new();$script:snapStops=[Collections.Generic.List[int]]::new();$script:snapWaits=[Collections.Generic.List[int]]::new()
        $failure=$null;try{if($Action -eq 'stop'){Stop-Review}else{Wait-FixtureReviews 'row'}}catch{$failure=$_}
        if(!$fault -and $failure){throw "Snapshot case $Case ($Action): $($failure.Exception.Message)"}
        if($fault -and !$failure){throw "An unknown or denied snapshot identity was accepted: $Case ($Action)"}
        if($script:snapMetadataHandles -or $script:snapMetadataWaits -or $script:snapMetadataStops -or $script:snapProcessSessions -or $script:snapProcessExits -ne [int]$kernelExitCheck){throw "Snapshot cleanup escaped its catch-only kernel exit check or acted on metadata: $Case ($Action)"}
        if((($script:snapReturned|Sort-Object) -join ',') -cne (($script:snapDisposed|Sort-Object) -join ',')){throw "Snapshot cleanup did not dispose every returned process exactly once: $Case ($Action)"}
        $initial=if($Action -eq 'wait' -and $Case -in 'bad-json','empty'){0}else{1}
        $q42=if($globalFault -or ($Action -eq 'wait' -and $fault -and $Case -ne 'wait-lingering')){0}else{1}
        if($script:snapInitial -ne $initial -or $script:snapFresh -ne [int]$fresh -or $script:snapQueries[41] -ne [int]$open41 -or $script:snapQueries[42] -ne $q42){throw "Snapshot cleanup fetched or opened more than its declared observations: $Case ($Action)"}
        if(($script:snapStops -join ',') -cne $(if($Action -eq 'stop'){$script:snapTargets -join ','}else{''})){throw "Snapshot cleanup stopped an unknown identity or lost a matched owner: $Case ($Action)"}
        $waits=if($Case -eq 'wait-lingering'){'41'}else{$script:snapTargets -join ','}
        if(($script:snapWaits -join ',') -cne $waits){throw "Snapshot cleanup waited on an unknown identity or lost a bounded wait: $Case ($Action)"}
        if($Case -eq 'same-us-alias' -and ($script:snapStops.Contains(41) -or $script:snapWaits.Contains(41))){throw 'A different 100ns creation time was authorized by a coarse CIM alias'}
        if($Case -eq 'live-denied' -and $failure.Exception.Message -cne 'Fixture review 41 process handle is unavailable'){throw 'The same live inaccessible identity lost its original open failure'}
        if($Case -eq 'missing-birth' -and $failure.Exception.Message -cne 'Fixture review 41 has no recorded start time'){throw 'A missing recorded birth was recovered from a current PID'}
        if($Case -eq 'stop-error' -and (!$failure.Exception.Message.Contains('Owner41 stop denied') -or !$failure.Exception.Message.Contains('did not exit within 30 seconds'))){throw 'Attempt-all cleanup lost a stop or bounded-wait error'}
        if($Case -eq 'wait-lingering' -and $failure.Exception.Message -cne 'Fixture review 41 did not exit within 30 seconds'){throw 'A lingering pinned owner lost its bounded timeout failure'}
    }
    $exitFailures=[Collections.Generic.List[string]]::new()
    foreach($case in 'gone-before-snapshot','snapshot-open-exit'){try{Test-SnapshotCase 'stop' $case}catch{$exitFailures.Add($_.Exception.Message)}}
    if($exitFailures.Count){throw ($exitFailures -join '; ')}
    foreach($case in 'foreign-reuse','birth-reuse','live-match','live-denied','initial-session-unknown','initial-birth-unknown','bad-precision','fresh-foreign','fresh-birth','fresh-session-unknown','fresh-birth-unknown','fresh-query-error','process-query-error','pin-start-unavailable','pin-start-exit','same-us-alias','missing-birth','malformed-birth','bad-json','snapshot-query-error','snapshot-pid-invalid','close-exact','close-other-birth','close-window','close-failed','close-missing-birth','stop-error','dispose-error'){Test-SnapshotCase 'stop' $case}
    foreach($case in 'gone-before-snapshot','snapshot-open-exit','foreign-reuse','birth-reuse','live-match','live-denied','same-us-alias','missing-birth','malformed-birth','bad-json','empty','wait-lingering'){Test-SnapshotCase 'wait' $case}
    'Atomic snapshots classify owners before one pinned handle; exact creation, all-owner cleanup and bounded row-only waits remain strict'
}

& {
    foreach($name in 'Reviews','Stop-Review','Wait-FixtureReviews'){
        $node=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$false)
        . ([scriptblock]::Create($node.Extent.Text))
    }
    $helper=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw 'Fixture context helper did not parse'}
    foreach($name in 'Read-Snapshot','Read-CapyFixtureContext','Update-CapyFixtureContext','Record-CapyFixtureClose'){
        $node=$helper.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$false)
        if(!$node){throw "Missing actual fixture context helper: $name"}
        . ([scriptblock]::Create($node.Extent.Text))
    }
    Add-Type -TypeDefinition @'
using System;using System.ComponentModel;using System.Runtime.InteropServices;using System.Security.AccessControl;
public static class CapyReviewOwnershipTest {
 [StructLayout(LayoutKind.Sequential)] struct Luid {public uint Low;public int High;}
 [StructLayout(LayoutKind.Sequential)] struct Privilege {public Luid Id;public uint Attributes;}
 [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool OpenProcessToken(IntPtr process,uint access,out IntPtr token);
 [DllImport("advapi32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern bool LookupPrivilegeValue(string system,string name,out Luid id);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool CreateRestrictedToken(IntPtr token,uint flags,uint disableCount,IntPtr disable,uint deleteCount,ref Privilege delete,uint restrictCount,IntPtr restrict,out IntPtr result);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool ImpersonateLoggedOnUser(IntPtr token);
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool RevertToSelf();
 [DllImport("advapi32.dll",SetLastError=true)] static extern bool SetKernelObjectSecurity(IntPtr process,uint information,byte[] descriptor);
 public static void Restrict(IntPtr process,uint access){
  var descriptor=new RawSecurityDescriptor("D:(A;;0x"+access.ToString("x")+";;;WD)");var bytes=new byte[descriptor.BinaryLength];descriptor.GetBinaryForm(bytes,0);
  if(!SetKernelObjectSecurity(process,4,bytes))throw new Win32Exception(Marshal.GetLastWin32Error());
 }
 public static IntPtr WithoutDebug(){
  IntPtr original,restricted;var privilege=new Privilege();
  if(!OpenProcessToken(GetCurrentProcess(),10,out original))throw new Win32Exception(Marshal.GetLastWin32Error());
  try{if(!LookupPrivilegeValue(null,"SeDebugPrivilege",out privilege.Id))throw new Win32Exception(Marshal.GetLastWin32Error());
   if(!CreateRestrictedToken(original,0,0,IntPtr.Zero,1,ref privilege,0,IntPtr.Zero,out restricted))throw new Win32Exception(Marshal.GetLastWin32Error());
  }finally{CloseHandle(original);}
  if(!ImpersonateLoggedOnUser(restricted)){var error=Marshal.GetLastWin32Error();CloseHandle(restricted);throw new Win32Exception(error);}
  return restricted;
 }
 public static void Restore(IntPtr token){try{if(!RevertToSelf())throw new Win32Exception(Marshal.GetLastWin32Error());}finally{CloseHandle(token);}}
}
'@
    $directory=Join-Path ([IO.Path]::GetTempPath()) ('capy-review-close-'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($directory)|Out-Null
    $Output=$directory;$log=Join-Path $directory 'row';$priorContext=$env:CAPY_FIXTURE_CONTEXT
    $owned=[Collections.Generic.List[object]]::new();$engine=(Get-Process -Id $PID).Path
    function New-OwnershipChild {
        $info=[Diagnostics.ProcessStartInfo]::new();$info.FileName=$engine
        $info.Arguments='-NoProfile -NonInteractive -Command "[Console]::In.ReadLine()|Out-Null"'
        $info.UseShellExecute=$false;$info.CreateNoWindow=$true;$info.RedirectStandardInput=$true;$info.RedirectStandardOutput=$true;$info.RedirectStandardError=$true
        $child=[Diagnostics.Process]::Start($info);$null=$child.Handle
        $entry=@{process=$child;id=$child.Id;start=$child.StartTime.ToUniversalTime()};$owned.Add($entry);$entry
    }
    function Finish-OwnershipChild($Entry){
        if(!$Entry.process.HasExited){$Entry.process.StandardInput.WriteLine('exit');$Entry.process.StandardInput.Flush();if(!$Entry.process.WaitForExit(30000)){throw 'The owned process identity child did not exit'}}
    }
    try{
        & {
            $entry=New-OwnershipChild;$subject=[Diagnostics.Process]::GetProcessById($entry.id)
            try{
                @{process_id=$entry.id;process_start_utc=$entry.start.ToString('o');windows=@();closes=@()}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath ($log+'-context.json')
                Finish-OwnershipChild $entry
                $script:realSnapshotCalls=0;$script:realOpenCalls=0
                function Get-CimInstance{
                    [CmdletBinding()]param($ClassName,$Filter,$Property)
                    $script:realSnapshotCalls++
                    CimCmdlets\Get-CimInstance Win32_Process -Filter ("ProcessId="+$entry.id) -Property ProcessId,SessionId,CreationDate -ErrorAction Stop
                }
                function Get-Process([int]$Id){if($Id -ne $entry.id){throw 'Retired child lookup escaped its exact PID'};$script:realOpenCalls++;$subject}
                if(@(Reviews ($log+'-context.json')).Count -or $script:realSnapshotCalls -ne 1 -or $script:realOpenCalls){throw 'A real retired ID-only subject was opened after its absent atomic snapshot'}
                if(!$entry.process.HasExited -or $entry.process.ExitCode -ne 0){throw 'The real retired child did not finish normally'}
            }finally{$subject.Dispose()}
        }
        & {
            $entry=New-OwnershipChild;$subject=Microsoft.PowerShell.Management\Get-Process -Id $entry.id -ErrorAction Stop
            try{
                if($subject.SessionId -ne [Diagnostics.Process]::GetCurrentProcess().SessionId){throw 'The real open-race subject lost its original session'}
                @{process_id=$entry.id;process_start_utc=$entry.start.ToString('o');windows=@();closes=@()}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath ($log+'-context.json')
                $script:realSnapshotCalls=0;$script:realOpenCalls=0
                $script:realOpenRaceRows=@(CimCmdlets\Get-CimInstance Win32_Process -Filter ("ProcessId="+$entry.id) -Property ProcessId,SessionId,CreationDate -ErrorAction Stop)
                if($script:realOpenRaceRows.Count -ne 1){throw 'The real live child was absent before the open race'}
                function Get-CimInstance{
                    [CmdletBinding()]param($ClassName,$Filter,$Property)
                    $script:realSnapshotCalls++
                    if($script:realSnapshotCalls -eq 1){Finish-OwnershipChild $entry}
                    $script:realOpenRaceRows
                }
                function Get-Process([int]$Id){if($Id -ne $entry.id){throw 'Open-race lookup escaped its exact child'};$script:realOpenCalls++;$subject}
                if(@(Reviews ($log+'-context.json')).Count -or $script:realSnapshotCalls -ne 1 -or $script:realOpenCalls -ne 1){throw 'A real kernel-confirmed retired subject required fresh CIM absence or remained owned'}
                if(!$entry.process.HasExited -or $entry.process.ExitCode -ne 0){throw 'The real open-race child did not finish normally'}
            }finally{$subject.Dispose()}
        }
        & {
            function Get-CimInstance{
                [CmdletBinding()]param($ClassName,$Filter,$Property)
                if($Filter -and $Filter -cne ("ProcessId="+$entry.id)){throw 'The native rights fixture snapshot escaped its child'}
                $script:ownershipRow
            }
        foreach($case in 'closed','newer-owner','window-only','failed-close','missing-close-start','query-denied'){
            $entry=New-OwnershipChild;$old=$entry.start.AddSeconds(-1)
            $ownerStart=if($case -eq 'closed'){$old}else{$entry.start}
            $closeStart=if($case -in 'closed','newer-owner'){$old}else{$entry.start}
            $close=@{process_id=$entry.id;process_start_utc=$closeStart.ToString('o');window_only=($case -eq 'window-only');result=$(if($case -in 'failed-close','query-denied'){'fail'}else{'pass'})}
            if($case -eq 'missing-close-start'){$close.Remove('process_start_utc')}
            @{process_id=$entry.id;process_start_utc=$ownerStart.ToString('o');windows=@(@{process_id=$entry.id;process_start_utc=$ownerStart.ToString('o')});closes=@($close)}|ConvertTo-Json -Depth 6|Set-Content -LiteralPath ($log+'-context.json')
            $script:ownershipRow=@(CimCmdlets\Get-CimInstance Win32_Process -Filter ("ProcessId="+$entry.id) -Property ProcessId,SessionId,CreationDate -ErrorAction Stop)
            if($script:ownershipRow.Count -ne 1 -or $script:ownershipRow[0].CreationDate -isnot [DateTime] -or $script:ownershipRow[0].CreationDate.ToUniversalTime().Ticks -ne $entry.start.Ticks-($entry.start.Ticks%10L)){throw 'The native live child lost its exact microsecond atomic identity'}
            [CapyReviewOwnershipTest]::Restrict($entry.process.Handle,$(if($case -eq 'query-denied'){0x100000}else{0x101400}))
            $token=[CapyReviewOwnershipTest]::WithoutDebug();$failure=$null
            try{
                try{
                    $snapshot=Microsoft.PowerShell.Management\Get-Process -Id $entry.id -ErrorAction Stop
                    try{
                        if($snapshot.SessionId -ne [Diagnostics.Process]::GetCurrentProcess().SessionId){throw 'Restricted process snapshot lost the child session without opening a handle'}
                        if($case -eq 'query-denied'){
                            $deniedBirth=$null;try{$null=$snapshot.get_StartTime()}catch{$deniedBirth=$_.Exception.GetBaseException()}
                            if($deniedBirth -isnot [ComponentModel.Win32Exception] -or $deniedBirth.NativeErrorCode -ne 5){throw 'The sync-only child no longer denies process-time queries'}
                        }elseif($case -ne 'closed'){
                            $denied=$null;try{$null=$snapshot.get_Handle()}catch{$denied=$_.Exception.GetBaseException()}
                            if($denied -isnot [ComponentModel.Win32Exception] -or $denied.NativeErrorCode -ne 5){throw 'The live query-readable fixture did not deny full handle access'}
                            $readableStart=$snapshot.StartTime;$readableExit=$snapshot.HasExited
                            if($null -eq $readableStart -or $readableStart.ToUniversalTime() -ne $entry.start -or $readableExit -isnot [bool] -or $readableExit){throw 'The live denied-ALL fixture lost its readable birth or exit state'}
                        }
                    }finally{$snapshot.Dispose()}
                    if($case -eq 'closed'){
                        if(@(Reviews ($log+'-context.json')).Count){throw 'A terminal closed identity remained owned'}
                        Wait-FixtureReviews $log
                    }
                    Stop-Review
                }catch{$failure=$_}
            }finally{[CapyReviewOwnershipTest]::Restore($token)}
            if($entry.process.HasExited){throw 'Cleanup stopped a newer process with a recycled recorded PID'}
            if($case -eq 'closed'){
                if($failure){throw $failure}
            }elseif(!$failure -or $failure.Exception.Message -notlike 'Fixture review * process handle is unavailable'){throw "An unreadable pending owner lost its identity failure: $case ($failure)"}
            Finish-OwnershipChild $entry
        }
        }
        $entry=New-OwnershipChild;$env:CAPY_FIXTURE_CONTEXT=$log+'-context.json'
        @{windows=@();closes=@()}|ConvertTo-Json|Set-Content -LiteralPath $env:CAPY_FIXTURE_CONTEXT
        Finish-OwnershipChild $entry
        Record-CapyFixtureClose $entry.id 7 $false -ProcessStartUtc $entry.start
        $close=(Read-CapyFixtureContext).closes[0]
        if($close.process_id -ne $entry.id -or ([DateTime]$close.process_start_utc).ToUniversalTime() -ne $entry.start -or $close.window_only -ne $false -or $close.result -cne 'pass'){throw 'Terminal close receipt lost the original process creation identity'}
        & {
            $Output=Join-Path $directory 'natural-exit';[IO.Directory]::CreateDirectory($Output)|Out-Null
            $entry=New-OwnershipChild
            @{process_id=$entry.id;process_start_utc=$entry.start.ToString('o')}|ConvertTo-Json|Set-Content -LiteralPath (Join-Path $Output 'row-context.json')
            $node=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Reviews'},$false)
            . ([scriptblock]::Create($node.Extent.Text.Replace('function Reviews(','function Find-NaturalReview(')))
            $node=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Stop-Review'},$false)
            . ([scriptblock]::Create($node.Extent.Text))
            function Reviews([string]$ContextPath,[Collections.Generic.List[object]]$Failures){
                $matched=@(Find-NaturalReview -ContextPath $ContextPath -Failures $Failures)
                if($matched.Count -ne 1 -or $matched[0].Id -ne $entry.id){throw 'Natural exit control lost its exact matched child'}
                Finish-OwnershipChild $entry
                $matched
            }
            Stop-Review
            if(!$entry.process.HasExited -or $entry.process.ExitCode -ne 0){throw 'Natural exit of a retained owner became a stop fault'}
        }
        foreach($file in 'exercise-window.ps1','exercise-image-rows.ps1'){
            $caller=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot ('../../apps/layer-windows/scripts/'+$file)),[ref]$tokens,[ref]$errors)
            if($errors.Count){throw 'Close identity caller did not parse'}
            $capture=@($caller.FindAll({param($n)$n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -eq '$processStart'},$true))
            $closes=@($caller.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Record-CapyFixtureClose'},$true))
            $begin=if($file -eq 'exercise-window.ps1'){$caller.Find({param($n)$n -is [Management.Automation.Language.InvokeMemberExpressionAst] -and $n.Member.Value -eq 'PostMessage'},$true)}else{$caller.Find({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Invoke-Id' -and $n.Extent.Text -eq "Invoke-Id 'close_document'"},$true)}
            if($capture.Count -ne 1 -or !$begin -or $capture[0].Extent.EndOffset -ge $begin.Extent.StartOffset -or $closes.Count -ne $(if($file -eq 'exercise-window.ps1'){2}else{1})){throw 'Close creation identity was not captured before closing began'}
            foreach($close in $closes){if($close.Extent.Text -notlike '*-ProcessStartUtc $processStart*'){throw 'A close consumer omitted its original process creation identity'}}
        }
    }finally{
        $env:CAPY_FIXTURE_CONTEXT=$priorContext
        foreach($entry in $owned){try{if(!$entry.process.HasExited){$entry.process.Kill();$null=$entry.process.WaitForExit(5000)}}finally{$entry.process.Dispose()}}
        Remove-Item -LiteralPath $directory -Recurse -Force
    }
    'Terminal close receipts discharge only their exact creation identities; unreadable pending owners fail closed'
}

& {
    $HardwareGpu=$true;$controlFixture=$null;$buildManifest=@{fixture_scale=1.5}
    function Stop-Review{}
    function Reviews{}
    function Assert-HardwareTraces{}
    $directory=Join-Path ([IO.Path]::GetTempPath()) ('capy-editor-scale-'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($directory)|Out-Null
    try{
        $Executable=Join-Path $directory 'CapyCanvas.exe';$log=Join-Path $directory 'row'
        @{process_id=7;trace_directory=$directory;windows=@(@{process_id=8;trace_directory=$directory})}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath ($log+'-context.json')
        @{process_id=8;window_id=2;model=@{}}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $directory 'ui-state-8-2.json')
        $cases=@(
            @{name='valid';layout=@{cache=@{context=@{scale=1.5}}};scale=1.5;passes=$true},
            @{name='wrong';layout=@{cache=@{context=@{scale=1.0}}};scale=1.0},
            @{name='null';layout=@{cache=@{context=@{scale=$null}}};scale=$null},
            @{name='missing-scale';layout=@{cache=@{context=@{}}};scale=$null},
            @{name='missing-context';layout=@{cache=@{}};scale=$null;unmeasured=$true},
            @{name='missing-cache';layout=@{};scale=$null;unmeasured=$true},
            @{name='malformed-scale';layout=@{cache=@{context=@{scale='not-a-scale'}}};scale='not-a-scale'},
            @{name='malformed-context';layout=@{cache=@{context='not-a-context'}};scale=$null}
        )
        foreach($case in $cases){
            $scale=$case.scale
            @{process_id=7;window_id=1;model=@{layout_input=$case.layout}}|ConvertTo-Json -Depth 7|Set-Content -LiteralPath (Join-Path $directory 'ui-state-7-1.json')
            $failure=$null
            try{Save-Evidence $log}catch{$failure=$_.Exception.Message}
            if($case.passes){
                if($failure){throw "A helper snapshot without layout_input rejected the editor scale: $failure"}
            }elseif($failure -ne 'Hardware fixture runtime scale differs from the round display'){throw "An editor with incorrect or absent scale passed: $($case.name)"}
            $display=Get-Content -LiteralPath ($log+'-display.json') -Raw|ConvertFrom-Json
            $expectedCount=if($case.unmeasured){0}else{1}
            if(@($display.context_scales).Count -ne $expectedCount -or ($expectedCount -and ($display.context_scales[0].process_id -ne 7 -or $display.context_scales[0].window_id -ne 1 -or $display.context_scales[0].scale -ne $scale))){throw 'Editor scale evidence included a helper or lost editor identity or scale'}
        }
        @{process_id=7;window_id=1;model=@{layout_input=@{cache=@{context=@{scale=1.5}}}}}|ConvertTo-Json -Depth 7|Set-Content -LiteralPath (Join-Path $directory 'ui-state-7-1.json')
        @{process_id=9;window_id=1;model=@{layout_input=@{cache=@{context=$null}}}}|ConvertTo-Json -Depth 7|Set-Content -LiteralPath (Join-Path $directory 'ui-state-9-1.json')
        @{process_id=7;trace_directory=$directory;windows=@(@{process_id=8;trace_directory=$directory},@{process_id=9;trace_directory=$directory})}|ConvertTo-Json -Depth 4|Set-Content -LiteralPath ($log+'-context.json')
        $failure=$null
        try{Save-Evidence $log}catch{$failure=$_.Exception.Message}
        if($failure){throw "A valid laid-out editor plus a never-laid-out window rejected the editor scale: $failure"}
        $display=Get-Content -LiteralPath ($log+'-display.json') -Raw|ConvertFrom-Json
        if(@($display.context_scales).Count -ne 1 -or $display.context_scales[0].process_id -ne 7 -or $display.context_scales[0].scale -ne 1.5){throw 'Scale evidence included a null-context window or lost the laid-out editor'}
        Remove-Item -LiteralPath (Join-Path $directory 'ui-state-7-1.json'),(Join-Path $directory 'row-evidence/ui-state-7-1.json')
        $failure=$null
        try{Save-Evidence $log}catch{$failure=$_.Exception.Message}
        if($failure -ne 'Hardware fixture runtime scale differs from the round display'){throw 'A helper-only row supplied editor scale evidence'}
    }finally{Remove-Item -LiteralPath $directory -Recurse -Force}
}

& {
    $fixture=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/exercise-multiwindow.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    $node=$fixture.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Wait-Reopened'},$false)
    if(!$node){throw 'Missing actual multiwindow reopen path'}
    . ([scriptblock]::Create($node.Extent.Text))
    $helper=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../../apps/layer-windows/scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
    if($errors.Count){throw ($errors|Out-String)}
    foreach($name in 'Read-Snapshot','Read-CapyFixtureContext','Update-CapyFixtureContext','Exit-CapyEnvironment'){
        $node=$helper.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$false)
        if(!$node){throw "Missing actual fixture context helper: $name"}
        . ([scriptblock]::Create($node.Extent.Text))
    }
    $directory=Join-Path ([IO.Path]::GetTempPath()) ('capy-reopened-identity-'+[guid]::NewGuid().ToString('N'))
    [IO.Directory]::CreateDirectory($directory)|Out-Null
    $prior=$env:CAPY_FIXTURE_CONTEXT;$priorEnvironment=$script:CapyEnvironment;$run=$directory;$script:CapyEnvironment=@{CAPY_STORAGE_DIR=$null}
    function Restore-CapyEnvironmentValue($Name,$Value){if($Name -cne 'CAPY_STORAGE_DIR' -or $null -ne $Value){throw 'Unexpected reopen environment restoration'}}
    function Windows{$reopenedWindows}
    function Model($Window){@{windows_recovery=@{busy=$false;restoring=$false};state=@{document_file=@{modified=$true}}}}
    function Review-Ready($Model,$Window){$true}
    function Wait-Until($Condition,$Message,$Seconds){if($Seconds -ne 60 -or !(& $Condition)){throw $Message}}
    function Wait-Canvas($Condition,$Message,$Seconds){Wait-Until @PSBoundParameters}
    function Register-CapyFixtureWindow($Process,$WindowHandle,$TraceDirectory){
        if($Process.HasExited -or $TraceDirectory -cne $directory -or $WindowHandle -notin @($reopenedWindows.hwnd)){throw 'Reopened census did not receive its live owned window'}
        $entry=@{process_id=$Process.Id;process_start_utc=$Process.StartTime.ToUniversalTime().ToString('o');hwnd=$WindowHandle}
        Update-CapyFixtureContext {param($context);$context.windows=@($context.windows)+@($entry)}
    }
    try{
        $env:CAPY_FIXTURE_CONTEXT=Join-Path $directory 'row-context.json'
        Update-CapyFixtureContext {param($context);$context.windows=@(@{process_id=31;process_start_utc='2026-11-15T10:00:00.0000000Z';hwnd=310})}
        foreach($generation in 1,2){
            $review=[pscustomobject]@{Id=70+$generation;HasExited=$false;StartTime=[DateTime]::Parse('2026-11-15T10:01:00.1234567Z').AddMinutes($generation)}
            $reopenedWindows=@(foreach($index in 1..(3-$generation)){@{id=$index;hwnd=$review.Id*10+$index}})
            $expectedBirth=$review.StartTime.ToUniversalTime().ToString('o')
            Wait-Reopened (3-$generation) 'Reopened drawing did not become ready'
            $review.HasExited=$true
        }
        Exit-CapyEnvironment
        $context=Read-CapyFixtureContext
        if($context.process_id -ne 72 -or $context.process_start_utc -cne $expectedBirth){throw 'Final reopened review lost its recorded start time'}
        $actual=@($context.windows|Where-Object process_id -eq 72)
        if($actual.Count -ne 1 -or $actual[0].hwnd -ne 721 -or $actual[0].process_start_utc -cne $expectedBirth -or @($context.windows|Where-Object process_id -eq 71).Count -ne 2 -or @($context.windows|Where-Object process_id -eq 31).Count -ne 1){throw 'Reopened census lost an actual window or an earlier review identity'}
    }finally{if($null -eq $prior){Remove-Item Env:CAPY_FIXTURE_CONTEXT -ErrorAction SilentlyContinue}else{$env:CAPY_FIXTURE_CONTEXT=$prior};$script:CapyEnvironment=$priorEnvironment;Remove-Item -LiteralPath $directory -Recurse -Force}
    'Every reopened review records its live window identity before the final clean exit'
}

& (Join-Path $PSScriptRoot 'test-session-graphics.ps1')
