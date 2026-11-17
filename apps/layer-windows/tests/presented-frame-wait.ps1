$ErrorActionPreference='Stop'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $repo 'apps/layer-windows/scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw 'CapyUia does not parse'}
foreach($name in 'Read-Snapshot','Read-CanvasProgress','Test-CanvasProgress','Wait-Canvas','Wait-PresentedFrame','Wait-StablePixels'){
    $helper=@($ast.FindAll({param($node)$node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq $name},$true))
    if($helper.Count -ne 1){throw "Missing $name"}
    . ([scriptblock]::Create($helper[0].Extent.Text))
}
$out=Join-Path $repo ('artifacts/windows/presented-frame-'+[Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory $out|Out-Null
try{
    $review=[pscustomobject]@{Id=$PID}
    $script:frameTestStatePath=Join-Path $out "ui-state-$PID-7.json"
    function State-File{$script:frameTestStatePath}
    function Write-Trace([string]$Kind,$Value){
        if($Kind -eq 'ui-state' -and !$Value.model.state.document_file){$Value.model=@{state=@{document_file=@{epoch=0;revision=0}}}}
        if($Kind -eq 'presentation-state' -and !$Value.ContainsKey('render_progress')){$Value.render_progress=@{owner=19;epoch=0;revision=0;content_owner=19;content_epoch=0;content_revision=0;settled=$true;outstanding_work=$false}}
        $Value|ConvertTo-Json -Depth 8 -Compress|Set-Content -LiteralPath (Join-Path $out "$Kind-$PID-7.json")
    }
    function Wait-CapyCondition([scriptblock]$Condition,[string]$Message,[int]$Seconds=8,[switch]$Closing,[scriptblock]$ResetBound,[switch]$Unscaled,[scriptblock]$TimeoutMessage){
        for($poll=0;$poll -lt 4;$poll++){if($ResetBound){$null=& $ResetBound};if(& $Condition){return};$script:polls++;if($script:advance){& $script:advance}}
        if($TimeoutMessage){$Message=& $TimeoutMessage}
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
    Write-Trace 'ui-state' @{process_id=$PID;window_id=7;presented=11;presentation_epoch=42;model=@{state=@{document_file=@{epoch=0;revision=1}}}}
    foreach($case in 'pending-work','not-settled','undisplayed-revision','foreign-content-owner','foreign-content-epoch','missing-progress'){
        $progress=@{owner=19;epoch=0;revision=1;content_owner=19;content_epoch=0;content_revision=1;settled=$true;outstanding_work=$false}
        switch($case){
            'pending-work'{$progress.outstanding_work=$true}
            'not-settled'{$progress.settled=$false}
            'undisplayed-revision'{$progress.content_revision=0}
            'foreign-content-owner'{$progress.content_owner=20}
            'foreign-content-epoch'{$progress.content_epoch=1}
            'missing-progress'{$progress=$null}
        }
        Write-Trace 'presentation-state' @{process_id=$PID;window_id=7;presentation_epoch=43;required=12;displayed=12;render_progress=$progress}
        $script:sampled=$false;$failure=$null
        try{Wait-StablePixels {$script:sampled=$true;'pixels'} -Idle|Out-Null}catch{$failure=$_}
        if(!$failure -or $failure.Exception.Message -cne 'The canvas did not settle on screen' -or $script:sampled){throw "Matching fences admitted $case before renderer idle"}
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
 function Import-ProgressHelper($Source,[string]$Name){
  $definition=@($Source.FindAll({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $Name},$true))
  if($definition.Count-ne1){throw "Missing progress helper: $Name"}
  [scriptblock]::Create($definition[0].Extent.Text.Replace('[Diagnostics.Stopwatch]','[CapyStrokeProgressClock]'))
 }
 foreach($name in 'Wait-CapyCondition','Wait-Until','Read-CanvasProgress','Test-CanvasProgress','Wait-Canvas','Wait-CanvasStroke','Wait-PresentedFrame','Wait-StablePixels'){
  . (Import-ProgressHelper $ast $name)
 }
 $progressDirectory=Join-Path $repo ('artifacts/windows/canvas-progress-'+[guid]::NewGuid().ToString('N'))
 New-Item -ItemType Directory $progressDirectory|Out-Null
 foreach($kind in 'ui-state','camera-state','presentation-state'){[IO.File]::WriteAllText((Join-Path $progressDirectory ($kind+'-7-11.json')),'mocked')}
 try{
  function Wait-Scale{$script:progressScale}
  function State-File{Join-Path $progressDirectory 'ui-state-7-11.json'}
  function Start-Sleep([int]$Milliseconds){
   if($Milliseconds-ne$script:progressCadence){throw 'Canvas progress changed the existing poll cadence'}
   [CapyStrokeProgressClock]::Seconds++
   if([CapyStrokeProgressClock]::Seconds-gt40){throw 'outer row timeout'}
  }
  function Reset-Progress([string]$Mode,[string]$Case){
   $script:progressMode=$Mode;$script:progressCase=$Case;$script:strokeInputs=0;$script:progressReads=0;$script:progressCadence=50;$script:progressScale=1
   [CapyStrokeProgressClock]::Seconds=0;[CapyStrokeProgressClock]::Restarts=0
  }
  function Model{
   $time=[CapyStrokeProgressClock]::Seconds
   if($script:progressMode-eq'stroke'){
    $committed=$script:strokeInputs-and($script:progressCase-notin@('heartbeat','outer')-and($script:progressCase-notin@('slow','paused')-or$time-ge25))
    return @{state=@{document_file=@{revision=[long][bool]$committed;epoch=$(if($script:strokeInputs-and$script:progressCase-eq'switched'){1}else{0})}};windows_tabs=@{selected=3}}
   }
   @{state=@{document_file=@{revision=$(if($script:progressCase-eq'never-idle'){9}elseif($script:progressCase-eq'owned-revision'){[long]$time}else{0});epoch=$(if($script:progressCase-eq'changing-document'){[long]$time}else{0});busy=($time-lt25)}};windows_tabs=@{selected=3}}
  }
  function Read-Snapshot([string]$Path){
   $time=[CapyStrokeProgressClock]::Seconds;$case=$script:progressCase
   if((Split-Path -Leaf $Path)-like'ui-state-*'){
    if($script:progressMode-ne'stroke'-and$case-eq'missing-ui'){return $null}
    return @{process_id=$(if($case-eq'foreign-ui'){8}else{7});window_id=$(if($case-eq'changing-window'){11+[long]$time}else{11});presented=1;presentation_epoch=1;model=(Model)}
   }
   if((Split-Path -Leaf $Path)-like'camera-state-*'){return @{process_id=7;window_id=11;presented=1;presentation_epoch=1;camera=@{}}}
   $script:progressReads++
   if($script:progressMode-eq'stroke'){
    $sent=$script:strokeInputs-gt0
    if($case-eq'missing-baseline'-or($sent-and$case-eq'missing')){return $null}
    $work=$sent-and($case-eq'outer'-or($case-in@('slow','paused')-and$time-lt30))
    $shown=$sent-and($case-eq'hardware'-or($case-in@('slow','paused')-and$time-ge31))
    $progress=@{owner=19;epoch=0;revision=(Model).state.document_file.revision;content_owner=19;content_epoch=0;content_revision=$(if($shown){1}else{0});settled=(!$work);outstanding_work=$work}
    if($sent-and$case-eq'unacknowledged'){$progress.content_revision=$null}
    if($sent-and$case-eq'malformed'){$progress.outstanding_work='true'}
    if($sent-and$case-eq'switched'){$progress.owner=20;$progress.epoch=1}
    return @{process_id=$(if($sent-and$case-eq'foreign'){8}else{7});window_id=11;displayed=0;presentation_epoch=$(if(!$sent){0}elseif($case-eq'paused'-and$time-lt31){1}else{[long]$time+1});render_progress=$progress}
   }
   if($case-eq'missing'-or($case-eq'gapped'-and$time%2-eq1)){return $null}
   $work=$case-in@('owned-work','paused-work','outer','present-work','pixels-work','pixels-composed')
   $work=$work-and($case-eq'outer'-or$time-lt$(if($script:progressMode-eq'pixels'){9}else{25}))
   if($case-eq'stopped-work'){$work=$time-lt4}
   $shown=if($case-eq'never-idle'){29663+[long]$time}elseif($case-in@('displayed','gapped','changing-owner','changing-document','changing-window','present-required-ahead')){[long]$time}else{0}
   $content=if($case-eq'never-idle'){9}elseif($case-in@('content','foreign-content-owner','foreign-content-epoch','pixels-content')){[long]$time}else{0}
   $progress=@{owner=$(if($case-eq'changing-owner'){19+[long]$time}else{19});epoch=(Model).state.document_file.epoch;revision=(Model).state.document_file.revision;content_owner=$(if($case-eq'foreign-content-owner'){20}else{19});content_epoch=$(if($case-eq'foreign-content-epoch'){1}else{0});content_revision=$content;settled=(!$work);outstanding_work=$work;busy=($case-eq'busy')}
   if($case-eq'wrong-document'){$progress.epoch=1}
   if($case-eq'malformed-owner'){$progress.owner='19'}
   if($case-eq'malformed-work'){$progress.outstanding_work='true'}
   if($case-eq'malformed-display'){$shown=[string]$time}
   if($script:progressMode-eq'present'){
    $progress.content_revision=if($case-eq'never-idle'){9}elseif($time-ge25){[long]$time}else{0}
    if($case-eq'never-idle'){}
    elseif($case-eq'present-display'){$shown=[long]$time}
    elseif($case-eq'present-catches-up'){$shown=if($time-ge4){25}else{[long]$time}}
    elseif($case-ne'present-required-ahead'){$shown=if($time-ge25){25}else{0}}
   }
   if($script:progressMode-eq'pixels'){$shown=25}
   @{process_id=$(if($case-eq'foreign-process'){8}else{7});window_id=$(if($case-eq'foreign-window'){12}elseif($case-eq'changing-window'){11+[long]$time}else{11});presentation_epoch=$(if($case-eq'paused-work'){1}else{[long]$time+1});required=$(if($case-eq'never-idle'){29666+[long]$time}elseif($case-eq'present-required-ahead'-and$time-lt25){[long]$time+1}else{25});displayed=$shown;render_progress=$progress}
  }
  $review=[pscustomobject]@{Id=7;HasExited=$false};$review|Add-Member ScriptMethod Refresh {}
  $script:CapyEach=$null;$script:CapyWaitSeconds=8
  if($env:CAPY_OLD_CANVAS_HELPER){
   $oldTokens=$null;$oldErrors=$null
   $oldAst=[Management.Automation.Language.Parser]::ParseFile($env:CAPY_OLD_CANVAS_HELPER,[ref]$oldTokens,[ref]$oldErrors)
   if($oldErrors.Count){throw 'OLD canvas helper does not parse'}
   if($env:CAPY_OLD_CANVAS_CASE-eq'never-idle'){
    foreach($name in 'Test-CanvasProgress','Wait-Canvas'){. (Import-ProgressHelper $oldAst $name)}
   }else{
   foreach($name in 'Wait-Until','Wait-PresentedFrame'){. (Import-ProgressHelper $oldAst $name)}
   Reset-Progress 'present' 'present-work'
   if($env:CAPY_OLD_CANVAS_CASE-eq'matched-fence-pending'){
    Reset-Progress 'pixels' 'pixels-work'
    Wait-PresentedFrame -Idle
    throw 'OLD accepted matching presentation fences while renderer work remained pending'
   }
   Wait-PresentedFrame -Idle
   throw 'OLD accepted slow owned work beyond its unchanged canvas-settle bound'
   }
  }
  $progressChecks=0
  Reset-Progress 'present' 'never-idle';$failure=$null
  try{Wait-PresentedFrame -Idle}catch{$failure=$_}
  if(!$failure-or$failure.Exception.Message-cne'Canvas kept presenting without work: required=29673 displayed=29670 revision=9'-or[CapyStrokeProgressClock]::Seconds-ne8){throw "Idle frames extended the unchanged canvas bound: $failure"}
  $progressChecks++
  Reset-Progress 'condition' 'displayed';$failure=$null
  try{Wait-Canvas {if([CapyStrokeProgressClock]::Seconds-ge2){throw 'Original condition failure'};$false} 'Original condition failure'}catch{$failure=$_}
  if(!$failure-or$failure.Exception.Message-cne'Original condition failure'-or[CapyStrokeProgressClock]::Seconds-ne2){throw "Idle diagnostics replaced the original condition failure: $failure"}
  $progressChecks++
  foreach($case in 'slow','hardware','heartbeat','paused','unacknowledged','missing','malformed','foreign','switched','outer','missing-baseline'){
   Reset-Progress 'stroke' $case;$failure=$null;$existing=@{checks=0}
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
   $progressChecks++
  }
  foreach($case in 'owned-work','paused-work','owned-revision','displayed','content','heartbeat','busy','stale-content','missing','missing-ui','foreign-process','foreign-ui','foreign-window','wrong-document','malformed-owner','malformed-work','malformed-display','foreign-content-owner','foreign-content-epoch','changing-owner','changing-document','changing-window','gapped','stopped-work','outer'){
   Reset-Progress 'condition' $case;$failure=$null
   try{Wait-Canvas {[CapyStrokeProgressClock]::Seconds-ge25-and$script:progressCase-ne'outer'} 'Controlled canvas condition did not finish'}catch{$failure=$_}
   if($case-in@('owned-work','paused-work','owned-revision','content')){
    if($failure-or[CapyStrokeProgressClock]::Seconds-ne25){throw "Owned canvas progress did not preserve the condition: $case $failure"}
   }elseif($case-eq'displayed'){
    if(!$failure-or$failure.Exception.Message-cne'Canvas kept presenting without work: required=25 displayed=7 revision=0'-or[CapyStrokeProgressClock]::Seconds-ne8){throw "Display-only frames reset the canvas bound: $failure"}
   }elseif($case-eq'outer'){
    if(!$failure-or$failure.Exception.Message-cne'outer row timeout'){throw 'Canvas progress swallowed the outer row timeout'}
   }else{
    $bound=if($case-eq'stopped-work'){12}else{8}
    if(!$failure-or$failure.Exception.Message-cne'Controlled canvas condition did not finish'-or[CapyStrokeProgressClock]::Seconds-gt$bound){throw "Unowned or idle canvas progress reset the bound: $case $failure"}
   }
   $progressChecks++
  }
  foreach($helper in 'condition','present'){
   foreach($unscaled in @($false,$true)){
    Reset-Progress $helper $(if($helper-eq'present'){'present-heartbeat'}else{'heartbeat'})
    $script:progressScale=3;$failure=$null;$message='Controlled five-second canvas endpoint did not finish'
    try{
     if($helper-eq'present'){Wait-PresentedFrame -Idle -Seconds 5 -Message $message -Unscaled:$unscaled}
     else{Wait-Canvas {$false} $message -Seconds 5 -Unscaled:$unscaled}
    }catch{$failure=$_}
    $expected=if($unscaled){5}else{15}
    if(!$failure-or$failure.Exception.Message-cne$message-or[CapyStrokeProgressClock]::Seconds-ne$expected){throw "Canvas endpoint changed its scale or message: $helper Unscaled=$unscaled $failure"}
    $progressChecks++
   }
  }
  Reset-Progress 'condition' 'owned-work';$script:progressScale=3;$failure=$null
  try{Wait-Canvas {[CapyStrokeProgressClock]::Seconds-ge25} 'Owned work did not finish' -Seconds 5 -Unscaled}catch{$failure=$_}
  if($failure-or[CapyStrokeProgressClock]::Seconds-ne25){throw "Unscaled canvas bound rejected owned progress: $failure"}
  $progressChecks++
  foreach($case in 'present-work','present-catches-up','present-display','present-required-ahead','present-heartbeat'){
   Reset-Progress 'present' $case;$failure=$null
   try{Wait-PresentedFrame -Idle}catch{$failure=$_}
   if($case-eq'present-heartbeat'){
    if(!$failure-or$failure.Exception.Message-cne'The canvas did not settle on screen'-or[CapyStrokeProgressClock]::Seconds-ne8){throw "Idle presentation accepted heartbeat without displayed pixels: $failure"}
   }elseif($case-in@('present-display','present-required-ahead')){
    if(!$failure-or$failure.Exception.Message-notlike'Canvas kept presenting without work: required=* displayed=* revision=0'-or[CapyStrokeProgressClock]::Seconds-ne8){throw "Display-only frames extended idle presentation: $case $failure"}
   }elseif($failure-or[CapyStrokeProgressClock]::Seconds-ne$(if($case-eq'present-catches-up'){4}else{25})){throw "Idle presentation lost its required/displayed join: $case $failure"}
   $progressChecks++
  }
  foreach($case in 'pixels-work','pixels-content','pixels-heartbeat','pixels-composed','pixels-already-stable'){
   Reset-Progress 'pixels' $case;$script:progressCadence=100;$script:pixelSamples=0;$failure=$null;$pixels=$null
   try{$pixels=Wait-StablePixels {$script:pixelSamples++;if($script:progressCase-eq'pixels-already-stable'-or[CapyStrokeProgressClock]::Seconds-ge9){'pixels'}else{[string][CapyStrokeProgressClock]::Seconds}} -Composed:($case-eq'pixels-composed')}catch{$failure=$_}
   if($case-in@('pixels-work','pixels-content','pixels-already-stable')){
    $expected=if($case-eq'pixels-already-stable'){3}else{12}
    if($failure-or$pixels-cne'pixels'-or[CapyStrokeProgressClock]::Seconds-ne$expected-or$script:pixelSamples-ne($expected+1)){throw "Pixel stability changed its three exact comparisons: $case $failure"}
   }elseif(!$failure-or$failure.Exception.Message-notlike'Canvas pixels did not settle for comparison*'-or[CapyStrokeProgressClock]::Seconds-ne5){throw "Pixel progress escaped the original five-second bound: $case $failure"}
   if($case-eq'pixels-composed'-and$script:progressReads){throw 'Composed captures started consulting canvas progress'}
   $progressChecks++
  }
  "Canvas progress: $progressChecks deterministic schedules passed"
 }finally{Remove-Item -LiteralPath $progressDirectory -Recurse -Force}
}
