$ErrorActionPreference='Stop'
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot 'prepare.ps1'),[ref]$tokens,[ref]$errors)
if(@($errors).Count){throw ($errors|Out-String)}
$definition=$ast.Find({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Enable-FixtureJapaneseInput'},$true)
if(!$definition){throw 'VM preparation does not enable the fixture Japanese input profile'}
. ([scriptblock]::Create($definition.Extent.Text))
$tip='0411:{03B5835F-F03C-411B-9CE2-AA23E1171E36}{A76C93D9-5523-4E90-AAFA-4DB112F9AC76}'
function Language([string]$Tag,[string[]]$Tips){
    $values=[Collections.Generic.List[string]]::new();foreach($value in $Tips){$values.Add($value)}
    [pscustomobject]@{LanguageTag=$Tag;InputMethodTips=$values}
}
function Get-WindowsCapability([switch]$Online,[string]$Name){
    if(!$Online -or $Name -ne 'Language.Basic~~~ja-JP~0.0.1.0'){throw 'Unexpected capability lookup'}
    [pscustomobject]@{State=$script:capabilityState}
}
function Add-WindowsCapability([switch]$Online,[string]$Name){
    if(!$script:inTask){throw 'Basic installation did not run in the elevated interactive task'}
    if(!$Online -or $Name -ne 'Language.Basic~~~ja-JP~0.0.1.0'){throw 'Preparation installed an unrelated capability'}
    $script:installs++
    if($script:installFailure -eq 'throw'){throw 'Fixture installation failure'}
    if($script:installFailure -ne 'unchanged'){$script:capabilityState='Installed'}
}
function Get-Process($Name,[switch]$IncludeUserName,$Id,$ErrorAction){
    if($Id){if($Id -ne $PID){throw 'Queried an unrelated executable'};return [pscustomobject]@{Path='fixture-powershell.exe'}}
    if($Name -ne 'explorer' -or !$IncludeUserName){throw 'Unexpected desktop readiness lookup'}
    $script:explorerChecks++;[pscustomobject]@{UserName='fixture\other-user'}
    if($script:explorerChecks -ge $script:explorerAfter){[pscustomobject]@{UserName=('fixture\'+[Environment]::UserName)}}
}
function Get-Date {$script:clock}
function Start-Sleep([int]$Seconds){if($Seconds -ne 1){throw 'Unexpected readiness observation cadence'};$script:clock=$script:clock.AddSeconds($Seconds);$script:sleeps++}
function New-ScheduledTaskAction($Execute,$Argument){if(!$Execute -or $Argument -notmatch '-EncodedCommand ([A-Za-z0-9+/=]+)$'){throw 'Invalid capability action'};[pscustomobject]@{Execute=$Execute;Argument=$Argument}}
function New-ScheduledTaskPrincipal($UserId,$LogonType,$RunLevel){
    if($UserId -ne [Environment]::UserName -or $LogonType -ne 'Interactive' -or $RunLevel -ne 'Highest'){throw 'Capability task changed private user or elevation'}
    [pscustomobject]@{UserId=$UserId}
}
function New-ScheduledTaskSettingsSet([switch]$AllowStartIfOnBatteries,[switch]$DontStopIfGoingOnBatteries,$ExecutionTimeLimit){if($ExecutionTimeLimit -ne 0){throw 'Capability task acquired an installation deadline'};@{}}
function Register-ScheduledTask($TaskName,$Action,$Principal,$Settings){
    if($script:explorerChecks -lt $script:explorerAfter){throw 'Registered before the owned interactive desktop was ready'}
    if($TaskName -notmatch '^capycanvas-japanese-input-[a-f0-9]{32}$' -or $script:task){throw 'Capability task did not have a unique owned name'}
    $script:task=[pscustomobject]@{Name=$TaskName;Action=$Action;State='Ready';LastRunTime=[datetime]::MinValue;LastTaskResult=0};$script:registered++
}
function Start-ScheduledTask($TaskName){
    if($TaskName -ne $script:task.Name){throw 'Started an unrelated task'}
    $script:task.State='Running';$script:task.LastRunTime=[datetime]::MinValue.AddTicks(1);$script:inTask=$true
    try{$null=$script:task.Action.Argument -match '-EncodedCommand ([A-Za-z0-9+/=]+)$';$command=[Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($Matches[1]));& ([scriptblock]::Create($command))}catch{$script:task.LastTaskResult=1}finally{$script:inTask=$false;$script:task.State='Ready'}
}
function Get-ScheduledTaskInfo($TaskName){
    if($TaskName -ne $script:task.Name){throw 'Queried unrelated task info'}
    if($script:taskObservations -and $script:task.LastRunTime -gt [datetime]::MinValue){
        if($script:observationIndex -ge $script:taskObservations.Count){throw 'Read task info again after its terminal observation'}
        $observation=$script:taskObservations[$script:observationIndex];$script:observationIndex++
        return [pscustomobject]@{LastRunTime=$script:task.LastRunTime;LastTaskResult=$observation.result}
    }
    $script:task
}
function Get-ScheduledTask($TaskName,$ErrorAction){
    if($script:task -and $TaskName -ne $script:task.Name){throw 'Queried unrelated task'}
    if($script:taskObservations -and $script:observationIndex -gt 0){
        $state=$script:taskObservations[$script:observationIndex-1].state
        if($null -eq $state){return $null}
        return [pscustomobject]@{State=$state}
    }
    $script:task
}
function Unregister-ScheduledTask($TaskName,[switch]$Confirm){if($Confirm -or $TaskName -ne $script:task.Name -or $script:task.State -ne 'Ready'){throw 'Removed an unrelated or running task'};$script:task=$null;$script:removed++}
function Get-WinUserLanguageList {if($script:capabilityState -ne 'Installed'){throw 'User input profile touched before Basic installation'};,$script:languages}
function New-WinUserLanguageList([string]$Language){
    if($Language -ne 'ja-JP'){throw 'Preparation requested an unrelated user language'}
    $result=[Collections.Generic.List[object]]::new();$result.Add((Language 'ja-JP' @($script:newTip)));,$result
}
function Set-WinUserLanguageList($LanguageList,[switch]$Force){
    if(!$Force -or ![object]::ReferenceEquals($LanguageList,$script:languages)){throw 'Preparation replaced the current user language list'}
    $script:writes++
}
$script:installFailure='';$script:inTask=$false;$script:task=$null;$script:taskObservations=$null
foreach($script:explorerAfter in @(1,2)){foreach($initialCapability in @('Installed','NotPresent')){foreach($script:newTip in @('0411:00000411',$tip)){foreach($existingJapanese in @($false,$true)){
    $script:capabilityState=$initialCapability;$script:installs=0;$script:registered=0;$script:removed=0
    $script:explorerChecks=0;$script:clock=[datetime]'2026-01-01';$script:sleeps=0
    $script:languages=[Collections.Generic.List[object]]::new();$script:writes=0
    $english=Language 'en-US' @('0409:00000409');$german=Language 'de-DE' @('0407:00000407')
    $script:languages.Add($english)
    if($existingJapanese){$script:languages.Add((Language 'ja-JP' @('0411:00000411')))}
    $script:languages.Add($german)
    Enable-FixtureJapaneseInput
    $order=($script:languages|ForEach-Object LanguageTag)-join ','
    $expected=if($existingJapanese){'en-US,ja-JP,de-DE'}else{'en-US,de-DE,ja-JP'}
    $japanese=@($script:languages|Where-Object LanguageTag -eq 'ja-JP')[0]
    $originalTip=if($existingJapanese){'0411:00000411'}else{$script:newTip}
    if($order -ne $expected -or @($japanese.InputMethodTips|Where-Object {$_ -eq $tip}).Count -ne 1 -or $japanese.InputMethodTips -notcontains $originalTip){throw 'Japanese preparation reordered languages or replaced existing input methods'}
    if(![object]::ReferenceEquals($script:languages[0],$english) -or ($english.InputMethodTips -join ',') -ne '0409:00000409' -or ($german.InputMethodTips -join ',') -ne '0407:00000407'){throw 'Japanese preparation changed existing user languages'}
    if($script:writes -ne 1){throw 'Missing Japanese input profile was not persisted once'}
    Enable-FixtureJapaneseInput
    if($script:writes -ne 1 -or $script:languages.Count -ne 3){throw 'Japanese preparation was not idempotent'}
    $expectedInstalls=if($initialCapability -eq 'Installed'){0}else{1}
    if($script:installs -ne $expectedInstalls -or $script:capabilityState -ne 'Installed'){throw 'Basic capability installation was missing, repeated or unconditional'}
    if($script:registered -ne $expectedInstalls -or $script:removed -ne $expectedInstalls -or $script:task){throw 'Capability task was repeated, unconditional or left registered'}
    if($script:sleeps -ne $expectedInstalls*($script:explorerAfter-1)){throw 'Preparation ignored desktop readiness or waited with no missing capability'}
}}}}
foreach($script:installFailure in @('throw','unchanged')){
    $script:capabilityState='NotPresent';$script:installs=0;$script:registered=0;$script:removed=0;$script:writes=0;$failed=$false
    $script:explorerAfter=1;$script:explorerChecks=0;$script:clock=[datetime]'2026-01-01';$script:sleeps=0
    try{Enable-FixtureJapaneseInput}catch{$failed=$true}
    if(!$failed -or $script:installs -ne 1 -or $script:writes -ne 0){throw 'Failed Basic installation continued into user profile changes'}
    if($script:registered -ne 1 -or $script:removed -ne 1 -or $script:task){throw 'Failed capability task escaped owned cleanup'}
}
foreach($terminalResult in @(0,1)){
    $script:installFailure=if($terminalResult){'throw'}else{''}
    $script:capabilityState='NotPresent';$script:installs=0;$script:registered=0;$script:removed=0;$script:writes=0
    $script:explorerAfter=1;$script:explorerChecks=0;$script:clock=[datetime]'2026-01-01';$script:sleeps=0
    $script:languages=[Collections.Generic.List[object]]::new();$script:languages.Add((Language 'en-US' @('0409:00000409')))
    $script:observationIndex=0;$script:taskObservations=@(
        @{state=$null;result=267009},
        @{state='Ready';result=267009},
        @{state='Running';result=0},
        @{state='Ready';result=$terminalResult}
    )
    $failure=$null;try{Enable-FixtureJapaneseInput}catch{$failure=$_}
    if($script:observationIndex -ne 4 -or $script:sleeps -ne 3){throw "Preparation accepted an incomplete scheduler observation: $failure"}
    if($terminalResult){
        if(!$failure -or $failure.Exception.Message -notmatch 'failed: 1$' -or $script:writes -ne 0){throw 'Completed installation failure was not preserved'}
    }elseif($failure -or $script:writes -ne 1){throw "Completed installation did not enable its input profile: $failure"}
    if($script:registered -ne 1 -or $script:installs -ne 1 -or $script:removed -ne 1 -or $script:task){throw 'Scheduler observations repeated installation or escaped owned cleanup'}
    $script:taskObservations=$null
}
$script:installFailure='';$script:capabilityState='NotPresent';$script:installs=0;$script:registered=0;$script:writes=0
$script:explorerAfter=[int]::MaxValue;$script:explorerChecks=0;$script:clock=[datetime]'2026-01-01';$script:sleeps=0;$failed=$false
try{Enable-FixtureJapaneseInput}catch{$failed=$true}
if(!$failed -or $script:registered -ne 0 -or $script:installs -ne 0 -or $script:writes -ne 0 -or $script:sleeps -ne 120){throw 'Missing interactive desktop escaped its existing120s deadline or launched work'}
Write-Output 'VM preparation preserves user language order and existing tips while enabling Microsoft Japanese IME.'
