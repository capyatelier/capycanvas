$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $repo 'apps/layer-windows/scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'CapyUia does not parse'}
foreach($name in 'Read-Snapshot','Wait-PresentedFrame','Wait-StablePixels'){
    $helper=@($ast.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true))
    if($helper.Count -ne 1){throw "Missing $name"}
    . ([scriptblock]::Create($helper[0].Extent.Text))
}
$out=Join-Path $repo ('artifacts/windows/presented-frame-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $out|Out-Null
try{
    $review=[pscustomobject]@{Id=$PID}
    $state=Join-Path $out "ui-state-$PID-7.json"
    function State-File{$state}
    function Write-Trace([string]$Kind,$Value){$Value|ConvertTo-Json -Compress|Set-Content -LiteralPath (Join-Path $out "$Kind-$PID-7.json")}
    function Wait-Until([scriptblock]$Condition,[string]$Message){
        for($poll=0;$poll -lt 4;$poll++){if(& $Condition){return};$script:polls++;if($script:advance){& $script:advance}}
        throw $Message
    }
    function Expect([string]$Name,[bool]$Refused){
        $script:polls=0;$failed=$false
        try{Wait-PresentedFrame}catch{if($_.Exception.Message -ne 'The display did not show the latest canvas frame'){throw};$failed=$true}
        if($failed -ne $Refused){throw "$Name was $(if($failed){'refused'}else{'accepted'})"}
        $script:checks++
    }
    $checks=0
    Write-Trace 'ui-state' @{process_id=$PID;window_id=7;presented=12;model=@{}}
    Write-Trace 'camera-state' @{process_id=$PID;window_id=7;presented=14;camera=@{}}
    $script:advance=$null
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;displayed=13}
    Expect 'a display behind the newer camera frame' $true
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;displayed=14}
    Expect 'the displayed camera frame' $false
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;displayed=11}
    $script:shown=11;$script:advance={$script:shown++;Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;displayed=$script:shown}}
    Expect 'a display that catches up' $false
    if($script:polls -ne 3){throw 'The wait accepted a frame before it was displayed'}
    $script:advance=$null
    Write-Trace 'presentation-state' @{process_id=($PID+1);window_id=7;displayed=99}
    Expect 'another process display' $true
    Remove-Item -LiteralPath (Join-Path $out "presentation-state-$PID-7.json")
    Expect 'a missing display acknowledgement' $true
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;displayed=$null}
    Expect 'an adapter without frame statistics' $false
    Remove-Item -LiteralPath (Join-Path $out "ui-state-$PID-7.json"),(Join-Path $out "camera-state-$PID-7.json")
    Expect 'an untraced review' $false
    Write-Trace 'ui-state' @{process_id=$PID;window_id=7;presented=20;model=@{}}
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;displayed=19}
    $sampled=$false
    try{Wait-StablePixels {$script:sampled=$true;'pixels'}|Out-Null}catch{if($_.Exception.Message -ne 'The display did not show the latest canvas frame'){throw}}
    if($sampled){throw 'Pixels were sampled before the latest frame was displayed'}
    $checks++
    Write-Trace 'ui-state' @{process_id=$PID;window_id=7;presented=11;presentation_epoch=42;model=@{}}
    Write-Trace 'camera-state' @{process_id=$PID;window_id=7;presented=11;presentation_epoch=42;camera=@{}}
    foreach($case in @(
        @{name='a retired no-op reservation';epoch=43;required=10;displayed=10;owner=$PID;refused=$false},
        @{name='an undisplayed required frame';epoch=43;required=11;displayed=10;owner=$PID;refused=$true},
        @{name='an older acknowledgement';epoch=41;required=11;displayed=11;owner=$PID;refused=$true},
        @{name='the displayed required frame';epoch=43;required=11;displayed=11;owner=$PID;refused=$false},
        @{name='a foreign acknowledgement';epoch=43;required=11;displayed=11;owner=($PID+1);refused=$true},
        @{name='unsupported display statistics';epoch=43;required=11;displayed=$null;owner=$PID;refused=$false}
    )){
        Write-Trace 'presentation-state' @{process_id=$case.owner;window_id=7;presentation_epoch=$case.epoch;required=$case.required;displayed=$case.displayed}
        Expect $case.name $case.refused
    }
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;presentation_epoch=43;required=11;displayed=10}
    $script:advance={Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;presentation_epoch=43;required=10;displayed=10}}
    Expect 'a required reservation that retires while waiting' $false
    if($script:polls -ne 1){throw 'The wait did not acknowledge the retired reservation at its first valid display'}
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;presentation_epoch=43;required=11;displayed=10}
    $script:required=11;$script:shown=10
    $script:advance={
        $script:required++;$script:shown++
        Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;presentation_epoch=43;required=$script:required;displayed=$script:shown}
    }
    Expect 'a requested frame followed by later submissions' $false
    if($script:polls -ne 1){throw 'The wait followed later submissions after its requested frame was displayed'}
    foreach($idle in @($false,$true)){
        Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;presentation_epoch=43;required=11;displayed=10}
        $script:polls=0
        $script:advance={
            Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;presentation_epoch=43;required=12;displayed=$(if($script:polls -eq 1){11}else{12})}
        }
        Wait-PresentedFrame -Idle:$idle
        $expected=if($idle){2}else{1}
        if($script:polls -ne $expected){throw "Idle=$idle returned at snapshot $($script:polls+1), expected $($expected+1)"}
        $checks++
    }
    Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;presentation_epoch=43;required=11;displayed=10}
    $script:polls=0;$script:samples=0
    function Start-Sleep([int]$Milliseconds){}
    $pixels=Wait-StablePixels {
        if($script:polls -lt 2){throw 'Idle pixels were sampled before the third snapshot'}
        $script:samples++;'pixels'
    } -Idle
    if($pixels -ne 'pixels' -or $script:samples -ne 4 -or $script:polls -ne 2){throw 'Idle pixel sampling did not follow the idle acknowledgement'}
    $checks++
    $script:advance=$null
    foreach($case in @(
        @{name='an older idle acknowledgement';epoch=41;required=12;displayed=12;owner=$PID},
        @{name='a foreign idle acknowledgement';epoch=43;required=12;displayed=12;owner=($PID+1)},
        @{name='missing idle statistics';epoch=43;required=12;displayed=$null;owner=$PID},
        @{name='a missing idle requirement';epoch=43;required=$null;displayed=12;owner=$PID},
        @{name='a missing idle epoch';epoch=$null;required=12;displayed=12;owner=$PID}
    )){
        Write-Trace 'presentation-state' @{process_id=$case.owner;window_id=7;presentation_epoch=$case.epoch;required=$case.required;displayed=$case.displayed}
        $failed=$false
        try{Wait-PresentedFrame -Idle}catch{if($_.Exception.Message -ne 'The canvas did not settle on screen'){throw};$failed=$true}
        if(!$failed){throw "Idle wait accepted $($case.name)"}
        $checks++
    }
    "Presented frame wait: $checks passed"
}finally{Remove-Item -LiteralPath $out -Recurse -Force}

& {
 Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
 Add-Type -TypeDefinition @'
using System;
public class CapyStrokeProgressClock {
 public static double Seconds; public static int Restarts;
 double start;
 public static CapyStrokeProgressClock StartNew(){return new CapyStrokeProgressClock{start=Seconds};}
 public TimeSpan Elapsed{get{return TimeSpan.FromSeconds(Seconds-start);}}
 public void Restart(){start=Seconds;Restarts++;}
}
'@
 foreach($name in 'Wait-Until','Wait-CanvasStroke'){
  $definition=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$true)
  if(!$definition){throw "Missing progress helper: $name"}
  . ([scriptblock]::Create($definition.Extent.Text.Replace('[Diagnostics.Stopwatch]','[CapyStrokeProgressClock]')))
 }
 function Wait-Scale{1}
 function State-File{Join-Path $repo 'ui-state-7-11.json'}
 function Start-Sleep([int]$Milliseconds){
  if($Milliseconds-ne50){throw 'Stroke progress changed the existing poll cadence'}
  [CapyStrokeProgressClock]::Seconds++
  if([CapyStrokeProgressClock]::Seconds-gt40){throw 'outer row timeout'}
 }
 function Model{
  $time=[CapyStrokeProgressClock]::Seconds
  $committed=$script:strokeInputs-and($script:strokeCase-notin@('heartbeat','outer')-and($script:strokeCase-notin@('slow','paused')-or$time-ge25))
  @{state=@{document_file=@{revision=[long][bool]$committed;epoch=$(if($script:strokeInputs-and$script:strokeCase-eq'switched'){1}else{0})}};windows_tabs=@{selected=3}}
 }
 function Read-Snapshot([string]$Path){
  if((Split-Path -Leaf $Path)-like'ui-state-*'){return @{process_id=7;window_id=11;model=(Model)}}
  $time=[CapyStrokeProgressClock]::Seconds;$sent=$script:strokeInputs-gt0
  if($script:strokeCase-eq'missing-baseline'-or($sent-and$script:strokeCase-eq'missing')){return $null}
  $work=$sent-and($script:strokeCase-eq'outer'-or($script:strokeCase-in@('slow','paused')-and$time-lt30))
  $shown=$sent-and($script:strokeCase-eq'hardware'-or($script:strokeCase-in@('slow','paused')-and$time-ge31))
  $progress=@{owner=19;epoch=0;revision=(Model).state.document_file.revision;content_owner=19;content_epoch=0;content_revision=$(if($shown){1}else{0});settled=(!$work);outstanding_work=$work}
  if($sent-and$script:strokeCase-eq'unacknowledged'){$progress.content_revision=$null}
  if($sent-and$script:strokeCase-eq'malformed'){$progress.outstanding_work='true'}
  if($sent-and$script:strokeCase-eq'switched'){$progress.owner=20;$progress.epoch=1}
  @{process_id=$(if($sent-and$script:strokeCase-eq'foreign'){8}else{7});window_id=11;presentation_epoch=$(if(!$sent){0}elseif($script:strokeCase-eq'paused'-and$time-lt31){1}else{[long]$time+1});render_progress=$progress}
 }
 $review=[pscustomobject]@{Id=7;HasExited=$false};$review|Add-Member ScriptMethod Refresh {}
 $script:CapyEach=$null;$script:CapyWaitSeconds=8
 foreach($case in 'slow','hardware','heartbeat','paused','unacknowledged','missing','malformed','foreign','switched','outer','missing-baseline'){
  $script:strokeCase=$case;$script:strokeInputs=0;$failure=$null;$existing=@{checks=0}
  [CapyStrokeProgressClock]::Seconds=0;[CapyStrokeProgressClock]::Restarts=0
  try{Wait-CanvasStroke {$script:strokeInputs++};Wait-Until {$existing.checks++;$true} 'original pixel assertion'}catch{$failure=$_}
  if($case-eq'missing-baseline'){
   if(!$failure-or$script:strokeInputs-ne0-or$existing.checks){throw 'A missing owned baseline admitted a stroke'}
  }elseif($script:strokeInputs-ne1){throw "Stroke progress repeated or omitted the original input: $case $failure"}
  elseif($case-in@('slow','paused','hardware')){
   if($failure-or$existing.checks-ne1){throw "A committed displayed stroke failed its progress gate: $case $failure"}
   if($case-in@('slow','paused')-and[CapyStrokeProgressClock]::Seconds-ne31){throw 'The existing pixel assertion started before committed pixels were displayed'}
   if($case-eq'hardware'-and[CapyStrokeProgressClock]::Seconds-ne0){throw 'An already-displayed hardware stroke acquired another wait'}
  }elseif($case-eq'switched'){
   if(!$failure-or$failure.Exception.Message-cne'The drawing changed during its stroke progress wait'-or$existing.checks){throw "A different document satisfied the stroke gate: $failure"}
  }elseif($case-eq'outer'){
   if(!$failure-or$failure.Exception.Message-cne'outer row timeout'-or$existing.checks){throw 'Stroke progress swallowed its outer row timeout'}
  }else{
   if(!$failure-or$failure.Exception.Message-cne'The stroke did not commit and reach the displayed canvas'-or$existing.checks-or[CapyStrokeProgressClock]::Seconds-gt9){throw "Idle, malformed or foreign progress escaped the unchanged bound: $case $failure"}
  }
 }
 'Stroke progress: slow GPU work, commit/display join, immediate hardware completion, idle heartbeat, paused polling during outstanding work, missing/malformed/foreign receipts, document changes and outer interruption passed.'
}
