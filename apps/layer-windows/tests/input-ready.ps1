param([string]$HelperPath=(Join-Path $PSScriptRoot '../scripts/InputReady.ps1'))
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;
namespace CapyInputReadyTest {
 public class Clock {
  public static long Milliseconds;public static int Restarts;long start;
  public static Clock StartNew(){return new Clock{start=Milliseconds};}
  public TimeSpan Elapsed{get{return TimeSpan.FromMilliseconds(Milliseconds-start);}}
  public void Restart(){start=Milliseconds;Restarts++;}
 }
 public static class Driver {
  public static int Downs;public static string NativeReceiver,Receiver;public static bool OwnedProcess;
  public static void Down(string device,int x,int y){if(!OwnedProcess)throw new Exception("Foreign input process");Receiver=NativeReceiver;Downs++;}
  public static void Up(){}
 }
}
'@
$tokens=$null;$errors=$null
$waitSource=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/CapyUia.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw ($errors|Out-String)}
foreach($name in 'Wait-CapyCondition','Wait-Until'){
 $wait=$waitSource.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name},$true)
 if($wait){. ([scriptblock]::Create($wait.Extent.Text.Replace('[Diagnostics.Stopwatch]','[CapyInputReadyTest.Clock]')))}
 elseif($name -ceq 'Wait-Until'){throw 'Missing actual input inactivity wait'}
}
. $HelperPath
$script:CapyWaitSeconds=1;$script:CapyEach=$null
function Wait-Scale{1}
function Start-Sleep([int]$Milliseconds){
 if($Milliseconds -ne 50){throw 'Input inactivity wait changed its original poll cadence'}
 [CapyInputReadyTest.Clock]::Milliseconds+=$Milliseconds
}
$review=[pscustomobject]@{Id=7;HasExited=$false};$review|Add-Member ScriptMethod Refresh {}
$script:inputArguments=[pscustomobject]@{Current=[pscustomobject]@{ProcessId=7}}
$script:inputArguments.Current|Add-Member ScriptProperty ItemStatus {Get-InputResponse}
$script:inputWriter=[pscustomobject]@{}
$script:inputWriter|Add-Member ScriptMethod SetValue {
 param($Json)
 if($script:inputCase.inAction){throw 'Readiness query split the original Down/Move/Up batch'}
 $script:inputCase.query=$Json|ConvertFrom-Json;$script:inputCase.writes++
 if($script:inputCase.query.request -notmatch '^[0-9a-f]{32}$'){throw 'Query transport omitted its fresh native nonce'}
 $script:inputCase.requests.Add($script:inputCase.query.request)
}
$script:inputInvoker=[pscustomobject]@{}
$script:inputInvoker|Add-Member ScriptMethod Invoke {
 if($script:inputCase.inAction){throw 'Query Invoke split the original input batch'}
 $script:inputCase.invokes++
}
$script:inputArguments|Add-Member ScriptMethod GetCurrentPattern {
 param($Pattern)
 if($Pattern -ne [System.Windows.Automation.ValuePattern]::Pattern){throw 'Query arguments lost ValuePattern transport'}
 $script:inputWriter
}
$script:inputCommand=[pscustomobject]@{Current=[pscustomobject]@{ProcessId=7}}
$script:inputCommand|Add-Member ScriptMethod GetCurrentPattern {
 param($Pattern)
 if($Pattern -ne [System.Windows.Automation.InvokePattern]::Pattern){throw 'Query command lost InvokePattern transport'}
 $script:inputInvoker
}
$root=[pscustomobject]@{Current=[pscustomobject]@{ProcessId=7;NativeWindowHandle=77}}
$root|Add-Member ScriptMethod FindFirst {
 param($Scope,$Condition)
 if($Scope -ne [System.Windows.Automation.TreeScope]::Descendants){throw 'Query escaped the owned drawing root'}
 if($script:inputCase.name -eq 'missing-transport' -or $script:inputCase.rediscoveryUnavailable){return $null}
 switch($Condition.Value){
  'test-input-query' {$script:inputArguments;return}
  'test-input-query-invoke' {$script:inputCommand;return}
  default {throw 'Query looked up an unexpected peer'}
 }
}
function Get-CapyInputParent($Element){$null}
function Reset-InputCase([string]$Name){
 [CapyInputReadyTest.Clock]::Milliseconds=0;[CapyInputReadyTest.Clock]::Restarts=0
 $script:inputCase=@{name=$Name;query=$null;writes=0;invokes=0;reads=0;actions=0;actionAt=$null;cleanups=0;inAction=$false;requests=[Collections.Generic.List[string]]::new();events=[Collections.Generic.List[string]]::new();target='target-control'}
 $script:CapyEach={if([CapyInputReadyTest.Clock]::Milliseconds -ge 5000){throw 'test schedule exceeded outer cap'}};$root.Current.ProcessId=7;$root.Current.NativeWindowHandle=77
 $script:inputArguments.Current.ProcessId=7;$script:inputCommand.Current.ProcessId=7
 if($Name -eq 'foreign-root'){$root.Current.ProcessId=8}
 if($Name -eq 'missing-window'){$root.Current.NativeWindowHandle=0}
 if($Name -eq 'foreign-peer'){$script:inputCommand.Current.ProcessId=8}
}
function Set-InputBatchResponse($Receipt,$Query){
 $state=$script:inputCase;if(!$state.contacts){return}
 if($Query.kind -eq 'pointer'){
  $slot=-1
  for($index=0;$index -lt $state.contacts.Count;$index++){if(($Query.screen -join ',') -ceq ($state.contacts[$index].Point -join ',')){$slot=$index;break}}
  if($slot -lt 0){throw 'Batch preflight changed its declared point'}
  $expected=$state.contacts[$slot].Target
  $Receipt.pointer.path=if($expected -is [string]){@($expected,'owned-root')}elseif($expected.id){@($expected.id,'owned-root')}else{@('','tool-bubble-desk')}
  $Receipt.pointer.record=if($null -ne $expected.record){$expected.record}else{42};$Receipt.pointer.part=if($expected.part){$expected.part}else{'face'}
  $Receipt.pointer.canvas=$expected -ceq 'drawing-canvas'
  if($state.name -eq 'batch-blocked' -and $slot -eq 1){$Receipt.pointer.path=@('other-control','owned-root')}
 }
 if(!$state.actions -or $Query.kind -ne 'receipts'){return}
 $rows=[Collections.Generic.List[object]]::new();$serial=10
 for($index=0;$index -lt $state.contacts.Count;$index++){
  if($index -eq 1 -and $state.name -in @('batch-missing-second','batch-reuse-one')){continue}
  $contact=$state.contacts[$index];$target=$contact.Target;$serial++
  $row=@{serial=$serial;device=$contact.Device;pointer_id=$(if($state.name -like '*double*' -or $state.name -eq 'batch-reuse-one'){41}else{41+$index});time=$(if($state.name -like '*double*' -or $state.name -eq 'batch-reuse-one'){100+100*$index}else{100});screen=@($contact.Point[0],$contact.Point[1]);path=$(if($target -is [string]){@($target,'owned-root')}elseif($target.id){@($target.id,'owned-root')}else{@('','tool-bubble-desk')});route='routed';admitted=$null;record=$(if($null -ne $target.record){$target.record}else{42});part=$(if($target.part){$target.part}else{'face'})}
  if($target -ceq 'drawing-canvas'){$row.route='independent';$row.admitted=$true}
  if($index -eq 1){
   if($state.name -eq 'batch-wrong-target'){$row.path=@('wrong-receiver','owned-root')}
   if($state.name -eq 'batch-wrong-device'){$row.device='pen'}
   if($state.name -eq 'batch-unknown-canvas'){$row.route='routed';$row.admitted=$null}
   if($state.name -eq 'batch-rejected-canvas'){$row.admitted=$false}
  }
  if($state.name -eq 'batch-duplicate-routes'){$generic=$row.Clone();$generic.route='routed';$generic.admitted=$null;$rows.Add($generic);$serial++;$row.serial=$serial}
  $rows.Add($row)
 }
 if($state.name -in @('batch-unexpected-point','batch-extra-contact')){
  $extra=$rows[0].Clone();$serial++;$extra.serial=$serial;$extra.pointer_id=99
  if($state.name -eq 'batch-unexpected-point'){$extra.screen=@(900,900)}
  $rows.Add($extra)
 }
 $Receipt.downs=@($rows.ToArray());$Receipt.down_serial=$serial
 if($state.name -eq 'batch-dropped'){$Receipt.dropped=1}
}
function Get-InputResponse{
 $state=$script:inputCase;$query=$state.query;if(!$query){throw 'Read preceded the actual query write'}
 if($state.invokes -ne $state.writes){throw 'Query response read preceded its actual Invoke'}
 $state.reads++;$time=[CapyInputReadyTest.Clock]::Milliseconds;$name=$state.name
 $path=@('text-leaf',$state.target,'owned-root');$focus=@($state.target,'dialog-root')
 if($name -in @('wrong-widget','heartbeat','key-order-heartbeat','arbitrary-progress-heartbeat','missing-down-heartbeat')){$path=@('other-control','owned-root')}
 if($name -eq 'delayed' -and $time -lt 400){$path=@('other-control','owned-root')}
 if($name -in @('progress','outer')){
  if($name -eq 'outer' -or $time -lt 2500){$path=@(('preparing-'+[Math]::Floor($time/200)),'owned-root')}
 }
 if($name -eq 'wrong-focus'){$focus=@('another-editor','dialog-root')}
 if($name -eq 'delayed-focus' -and $time -lt 400){$focus=@('another-editor','dialog-root')}
 if($name -in @('within','exact-parent')){$focus=@('nested-editor','dialog-root')}
 $pointer=@{path=$path;enabled=$true;loaded=$true;canvas=($state.target -eq 'drawing-canvas');record=42;part='face'}
 if($name -eq 'disabled'){$pointer.enabled=$false}
 if($name -eq 'unloaded'){$pointer.loaded=$false}
 if($name -eq 'wrong-record'){$pointer.record=43}
 if($name -eq 'wrong-part'){$pointer.part='ring'}
 if($name -eq 'malformed-pointer'){$pointer.canvas='true'}
 if($name -like 'semantic-*'){$pointer.path=@('','tool-bubble-desk')}
 if($name -eq 'semantic-wrong-record'){$pointer.record=43}
 if($name -eq 'semantic-wrong-part'){$pointer.part='bead'}
 $active=$name -notin @('inactive-pointer','inactive-keyboard')
 $foreground=if($name -in @('wrong-foreground','inactive-pointer','inactive-keyboard')){88}else{77}
 $receipt=@{request=$query.request;process_id=7;window_id=11;hwnd=77;foreground=$foreground;active=$active;point_hwnd=$(if($query.kind -eq 'pointer'){77}else{0});point_process=$(if($query.kind -ne 'pointer'){0}elseif($name -eq 'foreign-overlay'){8}else{7});down_serial=10;dropped=0;pointer=$pointer;focus=@{path=$focus};downs=@();rendering_serial=$time+1}
 if($state.actions -and $query.kind -eq 'receipts'){
  $entry=@{serial=11;device='mouse';pointer_id=41;time=100;screen=@(120,240);path=@($state.target,'owned-root');route='routed';admitted=$null;record=42;part='face'}
  if($name -eq 'wrong-down'){$entry.path=@('other-control','owned-root')}
  if($name -eq 'wrong-down-point'){$entry.screen=@(140,240)}
  if($name -eq 'wrong-device'){$entry.device='touch'}
  if($name -like 'semantic-*'){$entry.path=@('','tool-bubble-desk')}
  if($name -eq 'semantic-down-record'){$entry.record=43}
  if($name -eq 'semantic-down-part'){$entry.part='bead'}
  switch($name){
   'bad-admission-text'{$entry.admitted='false'}
   'bad-device'{$entry.device='trackpad'}
   'bad-route'{$entry.route='canvas'}
   'bad-serial'{$entry.serial='11'}
   'bad-pointer'{$entry.pointer_id=-1}
   'bad-time'{$entry.time='100'}
   'bad-path-scalar'{$entry.path='target-control'}
   'bad-path-element'{$entry.path=@('target-control',7)}
   'bad-screen-count'{$entry.screen=@(120)}
   'bad-screen-text'{$entry.screen=@('120',240)}
   'bad-screen-fraction'{$entry.screen=@(120.5,240)}
   'bad-screen-nonfinite'{$entry.screen=@([double]::NaN,240)}
   'bad-screen-overflow'{$entry.screen=@(2147483648,240)}
   'bad-record'{$entry.record='42'}
   'bad-part'{$entry.part=3}
  }
  if($state.target -eq 'drawing-canvas'){
   $entry.route=if($name -eq 'canvas-generic'){'routed'}elseif($name -eq 'canvas-xaml'){'xaml_canvas'}else{'independent'}
   $entry.admitted=if($name -in @('canvas-rejected','canvas-canceled')){$false}elseif($name -in @('canvas-unknown','canvas-generic')){$null}else{$true}
  }
  if($name -eq 'bad-admission-text'){$entry.admitted='false'}
  if($name -notin @('missing-down','missing-down-heartbeat','dropped')){$receipt.downs=@($entry);$receipt.down_serial=11}
  if($name -eq 'dropped'){$receipt.dropped=1}
  if($name -eq 'changed-window'){$receipt.window_id=12}
  if($name -eq 'bad-downs-scalar'){$receipt.downs=$entry}
 }
 if($name -eq 'missing-down-heartbeat' -and !$state.actions){$receipt.pointer.path=@('target-control','owned-root')}
 Set-InputBatchResponse $receipt $query
 $receipt.progress=@{pointer=$receipt.pointer;focus=$receipt.focus;active=$receipt.active;foreground=$receipt.foreground;downs=$receipt.downs}
 if($name -eq 'key-order-heartbeat'){$receipt.progress=if($state.reads%2){[ordered]@{downs=$receipt.downs;focus=$receipt.focus;pointer=$receipt.pointer;foreground=$receipt.foreground;active=$receipt.active}}else{[ordered]@{active=$receipt.active;foreground=$receipt.foreground;pointer=$receipt.pointer;focus=$receipt.focus;downs=$receipt.downs}}}
 if($name -eq 'arbitrary-progress-heartbeat'){$receipt.progress=@{request=$query.request;rendering_serial=$time}}
 if($name -eq 'malformed-focus'){$receipt.focus.path='target-control'}
 if($name -eq 'malformed-pointer-path'){$receipt.pointer.path='target-control'}
 if($name -eq 'malformed-pointer-record'){$receipt.pointer.record='42'}
 if($name -eq 'stale-nonce' -or ($name -eq 'late-nonce' -and $time -lt 200)){$receipt.request='stale-request'}
 if($name -eq 'foreign-process'){$receipt.process_id=8}
 if($name -eq 'stale-window'){$receipt.hwnd=78}
 if($name -eq 'malformed-number'){$receipt.foreground='77'}
 if($name -eq 'malformed-active'){$receipt.active='true'}
 if($name -eq 'missing-progress'){$receipt.progress=$null}
 if($name -eq 'malformed-progress'){$receipt.progress='heartbeat'}
 if($name -eq 'malformed-point-process'){$receipt.point_process='7'}
 if($name -eq 'malformed-point-window'){$receipt.point_hwnd='77'}
 if($name -eq 'query-error'){$receipt.error='unavailable test root'}
 if($name -eq 'malformed-json'){return '{'}
 $receipt|ConvertTo-Json -Depth 20 -Compress
}
function Invoke-OriginalPointerBatch{
 $state=$script:inputCase;$state.actions++;$state.actionAt=[CapyInputReadyTest.Clock]::Milliseconds;$state.inAction=$true
 try{
  $state.events.Add('Down');$state.events.Add('Move');$state.events.Add('Up')
  if($state.name -eq 'action-throws'){throw 'Original action failed'}
 }finally{$state.inAction=$false;$state.cleanups++}
}
function Invoke-OriginalKey{
 $script:inputCase.actions++;$script:inputCase.actionAt=[CapyInputReadyTest.Clock]::Milliseconds;$script:inputCase.events.Add('Key')
}
function Invoke-OriginalContactBatch($Contacts){
 $state=$script:inputCase;$state.actions++;$state.actionAt=[CapyInputReadyTest.Clock]::Milliseconds;$state.inAction=$true
 $queries=$state.writes
 try{
  if($queries -ne $Contacts.Count){throw 'Batch input began before every declared point was queried'}
  for($index=0;$index -lt $Contacts.Count;$index++){$state.events.Add('Down:'+($index+1));if($state.writes -ne $queries){throw 'Query split the original contact Downs'}}
  $state.events.Add('Move')
  for($index=$Contacts.Count-1;$index -ge 0;$index--){$state.events.Add('Up:'+($index+1))}
  if($state.name -eq 'batch-mutated-spec'){$Contacts[0].Point[0]=999;$Contacts[0].Target.part='changed-after-Down'}
  'original-batch-output'
 }finally{$state.inAction=$false;$state.cleanups++}
}
$checks=0
foreach($case in 'ready','delayed','inactive-pointer','late-nonce','progress','canvas-independent','canvas-xaml'){
 Reset-InputCase $case
 if($case -like 'canvas-*'){$script:inputCase.target='drawing-canvas'}
 Invoke-CapyPointerInput -Target $script:inputCase.target -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch}
 $state=$script:inputCase
 if($state.actions -ne 1 -or $state.cleanups -ne 1 -or ($state.events -join ',') -cne 'Down,Move,Up'){throw "Readiness changed or repeated the complete original batch: $case"}
 if($state.writes -ne $state.invokes){throw 'Readiness queries lost their Value/Invoke pair'}
 if(@($state.requests|Select-Object -Unique).Count -ne $state.writes){throw 'Readiness reused a query nonce'}
 if($case -eq 'delayed' -and $state.actionAt -ne 400){throw 'Delayed target admitted input before its native readiness'}
 if($case -eq 'late-nonce' -and ($state.actionAt -ne 200 -or $state.writes -ne 2)){throw 'Pending nonce was resent or admitted before its matching reply'}
 if($case -eq 'progress' -and ($state.actionAt -ne 2500 -or [CapyInputReadyTest.Clock]::Restarts -eq 0)){throw 'Meaningful native descriptor progress did not extend inactivity'}
 $checks++
}
foreach($case in 'wrong-widget','foreign-overlay','disabled','unloaded','stale-nonce','heartbeat','key-order-heartbeat','arbitrary-progress-heartbeat','wrong-record','wrong-part'){
 Reset-InputCase $case;$target=if($case -in @('wrong-record','wrong-part')){@{id='target-control';record=42;part='face'}}else{'target-control'}
 $failure=$null;try{Invoke-CapyPointerInput -Target $target -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch}}catch{$failure=$_.Exception.Message}
 if(!$failure -or !$failure.StartsWith('Native pointer target did not become input-ready:') -or $script:inputCase.actions -or [CapyInputReadyTest.Clock]::Milliseconds -ne 1000 -or [CapyInputReadyTest.Clock]::Restarts){throw "Wrong, foreign, stale or heartbeat target escaped inactivity: $case $failure"}
 if($case -eq 'wrong-widget' -and (!$failure.Contains('other-control') -or !$failure.Contains('point_process') -or !$failure.Contains('down_serial') -or $failure.Contains('"downs"') -or $failure.Contains('"progress"') -or $failure.Length -gt 4096 -or $script:inputCase.reads -ne 20)){throw 'Target timeout omitted bounded actual state or performed another read'}
 $checks++
}
foreach($case in 'missing-down','missing-down-heartbeat','wrong-device','canvas-generic','canvas-unknown'){
 Reset-InputCase $case;if($case -like 'canvas-*'){$script:inputCase.target='drawing-canvas'}
 $failure=$null;try{Invoke-CapyPointerInput -Target $script:inputCase.target -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch}}catch{$failure=$_.Exception.Message}
 if(!$failure -or !$failure.StartsWith('Native Down did not reach its intended target:') -or $script:inputCase.actions -ne 1 -or [CapyInputReadyTest.Clock]::Milliseconds -ne 1000 -or [CapyInputReadyTest.Clock]::Restarts){throw "Missing or unknown Down became admission or a replay: $case $failure"}
 $checks++
}
foreach($case in 'wrong-down','wrong-down-point','canvas-rejected','canvas-canceled','changed-window','dropped'){
 Reset-InputCase $case;if($case -like 'canvas-*'){$script:inputCase.target='drawing-canvas'}
 $failure=$null;try{Invoke-CapyPointerInput -Target $script:inputCase.target -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch}}catch{$failure=$_.Exception.Message}
 $prefix=switch($case){'changed-window'{'Input receiver window identity changed'}'dropped'{'Native Down receipt was lost'}{$_ -like 'canvas-*'}{'Canvas rejected its native Down:'}default{'Native Down reached another receiver:'}}
 if(!$failure -or !$failure.StartsWith($prefix) -or $script:inputCase.actions -ne 1 -or [CapyInputReadyTest.Clock]::Milliseconds){throw "Known wrong/rejected Down was not an immediate named failure: $case $failure"}
 $checks++
}
foreach($case in 'foreign-process','stale-window','malformed-number','malformed-active','malformed-pointer','missing-progress','query-error','malformed-json','missing-transport','foreign-root','missing-window','foreign-peer','malformed-point-process','malformed-point-window','malformed-progress','malformed-focus','malformed-pointer-path','malformed-pointer-record'){
 Reset-InputCase $case;$failure=$null;$caught=$null
 try{Invoke-CapyPointerInput -Target 'target-control' -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch}}catch{$caught=$_;$failure=$_.Exception.Message}
 $prefix=switch($case){
  {$_ -in @('foreign-process','stale-window')}{'Native input receipt belongs to another drawing'}
  'malformed-number'{'Native input receipt has an invalid foreground'}
  {$_ -in @('malformed-active','missing-progress','malformed-progress')}{'Native input receipt lacks its applied state'}
  'malformed-point-process'{'Native input receipt has an invalid point_process'}
  'malformed-point-window'{'Native input receipt has an invalid point_hwnd'}
  'malformed-pointer'{'Native pointer receipt has an invalid canvas'}
  'malformed-focus'{'Native input receipt has an invalid focus.path'}
  'malformed-pointer-path'{'Native input receipt has an invalid pointer.path'}
  'malformed-pointer-record'{'Native input receipt has an invalid pointer.record'}
  'query-error'{'Native input query refused the request:'}
  {$_ -in @('missing-transport','foreign-peer')}{'Owned native input query is unavailable'}
  'foreign-root'{'Input readiness requires the owned drawing root'}
  'missing-window'{'Input readiness requires an owned native window'}
 }
 if(!$failure -or $script:inputCase.actions -or [CapyInputReadyTest.Clock]::Milliseconds -or ($prefix -and !$failure.StartsWith($prefix)) -or ($case -eq 'malformed-json' -and $caught.FullyQualifiedErrorId -notmatch 'ConvertFromJson')){throw "Invalid transport/ownership/type admitted input or failed in its harness: $case $failure"}
 $checks++
}
foreach($case in 'ready','delayed-focus','within','exact-parent','wrong-focus','wrong-foreground','inactive-keyboard'){
 Reset-InputCase $case;$target=switch($case){'within'{@{id='dialog-root';within=$true}}'exact-parent'{'dialog-root'}default{'target-control'}}
 $failure=$null;try{Invoke-CapyKeyInput -Target $target -Action {Invoke-OriginalKey}}catch{$failure=$_.Exception.Message}
 $blocked=$case -in @('exact-parent','wrong-focus','wrong-foreground','inactive-keyboard')
 if($blocked){if(!$failure -or !$failure.StartsWith('Native keyboard target did not become input-ready:') -or $script:inputCase.actions -or [CapyInputReadyTest.Clock]::Milliseconds -ne 1000){throw "Keyboard admitted a wrong/broadened focus or inactive drawing: $case $failure"}}
 elseif($failure -or $script:inputCase.actions -ne 1 -or ($script:inputCase.events -join ',') -cne 'Key'){throw "Keyboard readiness changed the original key: $case $failure"}
 if($case -eq 'delayed-focus' -and $script:inputCase.actionAt -ne 400){throw 'Key preceded exact delayed focus'}
 if($case -eq 'wrong-focus' -and (!$failure.Contains('another-editor') -or $script:inputCase.reads -ne 20)){throw 'Keyboard timeout omitted its actual last focus or performed another query'}
 $checks++
}
Reset-InputCase 'missing-target';$failure=$null
try{Invoke-CapyPointerInput -Target '' -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch}}catch{$failure=$_.Exception.Message}
if($failure -cne 'Input site has no intended native target id' -or $script:inputCase.writes -or $script:inputCase.actions){throw 'A targetless input was inferred from the actual hit or owned root'};$checks++
Reset-InputCase 'outer';$script:CapyEach={if([CapyInputReadyTest.Clock]::Milliseconds -ge 3000){throw 'outer row timeout'}};$failure=$null
try{Invoke-CapyPointerInput -Target 'target-control' -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch}}catch{$failure=$_.Exception.Message}
if($failure -cne 'outer row timeout' -or $script:inputCase.actions -or [CapyInputReadyTest.Clock]::Restarts -eq 0){throw 'Native progress swallowed the independent outer row cap'};$checks++
Reset-InputCase 'action-throws';$failure=$null
try{Invoke-CapyPointerInput -Target 'target-control' -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch}}catch{$failure=$_.Exception.Message}
if($failure -cne 'Original action failed' -or $script:inputCase.actions -ne 1 -or $script:inputCase.cleanups -ne 1 -or $script:inputCase.writes -ne 1){throw 'A failed original action was replayed or queried before its cleanup'};$checks++
foreach($case in 'bad-admission-text','bad-device','bad-route','bad-serial','bad-pointer','bad-time','bad-path-scalar','bad-path-element','bad-screen-count','bad-screen-text','bad-screen-fraction','bad-screen-nonfinite','bad-screen-overflow','bad-record','bad-part','bad-downs-scalar'){
 Reset-InputCase $case;if($case -eq 'bad-admission-text'){$script:inputCase.target='drawing-canvas'};$failure=$null
 try{Invoke-CapyPointerInput -Target $script:inputCase.target -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch}}catch{$failure=$_.Exception.Message}
 $field=switch($case){'bad-admission-text'{'admitted'}'bad-device'{'device'}'bad-route'{'route'}'bad-serial'{'serial'}'bad-pointer'{'pointer_id'}'bad-time'{'time'}{$_ -like 'bad-path-*'}{'path'}{$_ -like 'bad-screen-*'}{'screen'}'bad-record'{'record'}'bad-part'{'part'}}
 $message=if($case -eq 'bad-downs-scalar'){'Native input receipt has an invalid downs'}else{'Native input receipt has an invalid downs.'+$field}
 if($failure -cne $message -or $script:inputCase.actions -ne 1 -or [CapyInputReadyTest.Clock]::Milliseconds){throw "Malformed retained native Down escaped schema validation: $case $failure"};$checks++
}
foreach($case in 'semantic-ready','semantic-wrong-record','semantic-wrong-part','semantic-down-record','semantic-down-part'){
 Reset-InputCase $case;$failure=$null
 try{Invoke-CapyPointerInput -Target @{record=42;part='face'} -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch}}catch{$failure=$_.Exception.Message}
 if($case -eq 'semantic-ready'){
  if($failure -or $script:inputCase.actions -ne 1){throw "Explicit native record/part target needed an invented ancestor ID: $failure"}
 }elseif($case -in @('semantic-wrong-record','semantic-wrong-part')){
  if(!$failure -or !$failure.StartsWith('Native pointer target did not become input-ready:') -or $script:inputCase.actions -or [CapyInputReadyTest.Clock]::Milliseconds -ne 1000){throw "Another semantic part admitted input: $case $failure"}
 }elseif(!$failure -or !$failure.StartsWith('Native Down reached another receiver:') -or $script:inputCase.actions -ne 1 -or [CapyInputReadyTest.Clock]::Milliseconds){throw "A Down for another semantic part became its receipt: $case $failure"}
 $checks++
}
foreach($targetCase in @(@{target=@{};error='Input site has no intended native target id or complete record/part'},@{target=@{record=42};error='Input site has no intended native target id or complete record/part'},@{target=@{part='face'};error='Input site has no intended native target id or complete record/part'},@{target=@{record='42';part='face'};error='Input site has an invalid native target record'},@{target=@{record=-1;part='face'};error='Input site has an invalid native target record'},@{target=@{record=42;part=3};error='Input site has an invalid native target part'},@{target=@{record=42;part=''};error='Input site has an invalid native target part'},@{target=@{id='target-control';within='true'};error='Input site has an invalid native target scope'})){
 Reset-InputCase 'invalid-target';$failure=$null
 try{Invoke-CapyPointerInput -Target $targetCase.target -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch}}catch{$failure=$_.Exception.Message}
 if($failure -cne $targetCase.error -or $script:inputCase.actions -or $script:inputCase.writes){throw "Incomplete semantic target was inferred from the actual hit: $failure"};$checks++
}
Reset-InputCase 'semantic-keyboard';$failure=$null
try{Invoke-CapyKeyInput -Target @{record=42;part='face'} -Action {Invoke-OriginalKey}}catch{$failure=$_.Exception.Message}
if($failure -cne 'Input site has no intended native target id' -or $script:inputCase.actions -or $script:inputCase.writes){throw 'Keyboard focus was broadened from its exact ID to a pointer semantic part'};$checks++
foreach($case in 'batch-two-touch','batch-double-tap','batch-semantic','batch-mutated-spec','batch-duplicate-routes','batch-missing-second','batch-reuse-one','batch-wrong-target','batch-wrong-device','batch-unknown-canvas','batch-rejected-canvas','batch-unexpected-point','batch-extra-contact','batch-dropped','batch-blocked'){
 Reset-InputCase $case
 $contacts=if($case -in @('batch-double-tap','batch-reuse-one')){@(@{Target='target-control';Point=@(120,240);Device='mouse'},@{Target='target-control';Point=@(120,240);Device='mouse'})}elseif($case -in @('batch-duplicate-routes','batch-unknown-canvas','batch-rejected-canvas')){@(@{Target='drawing-canvas';Point=@(120,240);Device='touch'},@{Target='drawing-canvas';Point=@(220,240);Device='touch'})}elseif($case -in @('batch-semantic','batch-mutated-spec')){@(@{Target=@{record=42;part='face'};Point=@(120,240);Device='touch'},@{Target=@{record=43;part='bead'};Point=@(220,240);Device='touch'})}else{@(@{Target='left-control';Point=@(120,240);Device='touch'},@{Target='right-control';Point=@(220,240);Device='touch'})}
 $script:inputCase.contacts=@(foreach($contact in $contacts){@{Target=$(if($contact.Target -is [string]){$contact.Target}else{$contact.Target.Clone()});Point=@($contact.Point[0],$contact.Point[1]);Device=$contact.Device}})
 $failure=$null;$output=$null
 try{$output=Invoke-CapyPointerInput -Contacts $contacts -Action {Invoke-OriginalContactBatch $contacts}}catch{$failure=$_.Exception.Message}
 $success=$case -in @('batch-two-touch','batch-double-tap','batch-semantic','batch-mutated-spec','batch-duplicate-routes')
 if($success){
  if($failure -or $output -cne 'original-batch-output' -or $script:inputCase.actions -ne 1 -or $script:inputCase.writes -ne 3 -or ($script:inputCase.events -join ',') -cne 'Down:1,Down:2,Move,Up:2,Up:1'){throw "Complete original contact batch lost its distinct receipts/output or atomic input: $case $failure"}
 }elseif($case -eq 'batch-blocked'){
  if(!$failure -or !$failure.StartsWith('Native pointer target did not become input-ready:') -or $script:inputCase.actions -or [CapyInputReadyTest.Clock]::Milliseconds -ne 1000){throw 'A batch began before its second intended target became ready'}
 }else{
  $expected=switch($case){
   {$_ -in @('batch-missing-second','batch-reuse-one','batch-unknown-canvas')}{'Native Down batch did not reach every declared contact'}
   'batch-wrong-target'{'Native Down reached another receiver:'}
   {$_ -in @('batch-wrong-device','batch-unexpected-point')}{'Native Down reached an undeclared batch point or device:'}
   'batch-extra-contact'{'Native Down batch contained more contacts than declared'}
   'batch-dropped'{'Native Down receipt was lost from its bounded journal'}
   'batch-rejected-canvas'{'Canvas rejected its native Down:'}
  }
  $time=if($case -in @('batch-missing-second','batch-reuse-one','batch-unknown-canvas')){1000}else{0}
  if(!$failure -or !$failure.StartsWith($expected) -or $script:inputCase.actions -ne 1 -or $script:inputCase.cleanups -ne 1 -or [CapyInputReadyTest.Clock]::Milliseconds -ne $time){throw "Incomplete, reused, wrong or dropped contact batch was accepted/replayed: $case $failure"}
 }
 $checks++
}
Reset-InputCase 'empty-batch';$failure=$null
try{Invoke-CapyPointerInput -Contacts @() -Action {Invoke-OriginalPointerBatch}}catch{$failure=$_.Exception.Message}
if($failure -cne 'Input batch has no declared contacts' -or $script:inputCase.actions -or $script:inputCase.writes){throw 'An empty contact declaration became inferred input'};$checks++
Reset-InputCase 'mixed-batch';$failure=$null
try{Invoke-CapyPointerInput -Target 'target-control' -Contacts @(@{Target='target-control';Point=@(120,240);Device='mouse'}) -Action {Invoke-OriginalPointerBatch}}catch{$failure=$_.Exception.Message}
if($failure -cne 'Input batch cannot mix Contacts with single-contact arguments' -or $script:inputCase.actions -or $script:inputCase.writes){throw 'An ambiguous contact declaration admitted input'};$checks++
& {
 function Wait-Until{throw 'Consumer local wait intercepted native readiness'}
 Reset-InputCase 'delayed'
 Invoke-CapyPointerInput -Target 'target-control' -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch}
 if($script:inputCase.actions -ne 1 -or $script:inputCase.actionAt -ne 400){throw 'Native input did not retain the actual common progress wait'}
}
$checks++
$point=@{x=17;y=29};$target=@{payload='original-target'};$device=@{payload='original-device'};$action=@{payload='original-action'}
Reset-InputCase 'caller-payload'
Invoke-CapyPointerInput -Target 'target-control' -Point @(120,240) -Device 'mouse' -Action {
 if($point.x -ne 17 -or $point.y -ne 29 -or $target.payload -cne 'original-target' -or $device.payload -cne 'original-device' -or $action.payload -cne 'original-action'){throw 'Pointer wrapper shadowed the original caller payload'}
 Invoke-OriginalPointerBatch
}
if($script:inputCase.actions -ne 1 -or ($script:inputCase.events -join ',') -cne 'Down,Move,Up'){throw 'Pointer public aliases changed or repeated the original action'};$checks++
Reset-InputCase 'caller-payload'
Invoke-CapyKeyInput -Target 'target-control' -Action {
 if($point.x -ne 17 -or $point.y -ne 29 -or $target.payload -cne 'original-target' -or $device.payload -cne 'original-device' -or $action.payload -cne 'original-action'){throw 'Key wrapper shadowed the original caller payload'}
 Invoke-OriginalKey
}
if($script:inputCase.actions -ne 1 -or ($script:inputCase.events -join ',') -cne 'Key'){throw 'Key public aliases changed or repeated the original action'};$checks++
"Actual native input handshake: $checks deterministic transport, target, focus, admission, progress and batch cases passed; no UI input."

& {
 function Get-CapyInputParent($Element){if($Element.PSObject.Properties.Name -contains 'Parent'){$Element.Parent}}
 $mainInputRoot=$root
 foreach($case in 'scoped-owned','scoped-foreign','scoped-orphan'){
  $root=$mainInputRoot;Reset-InputCase $case
  $bridge=[pscustomobject]@{Current=[pscustomobject]@{ProcessId=7;NativeWindowHandle=55};Parent=$mainInputRoot}
  if($case -eq 'scoped-foreign'){$bridge=[pscustomobject]@{Current=[pscustomobject]@{ProcessId=8;NativeWindowHandle=88};Parent=$mainInputRoot}}
  if($case -eq 'scoped-orphan'){$bridge=$null}
  $root=[pscustomobject]@{Current=[pscustomobject]@{ProcessId=7;NativeWindowHandle=0};Parent=$bridge}
  $scopedInputRoot=$root;$failure=$null
  try{Invoke-CapyKeyInput -Target 'target-control' -Action {Invoke-OriginalKey}}catch{$failure=$_.Exception.Message}
  if(![object]::ReferenceEquals($root,$scopedInputRoot)){throw 'Native query replaced the fixture scoped root'}
  if($case -eq 'scoped-owned'){
   if($failure -or $script:inputCase.actions -ne 1 -or $script:inputCase.writes -ne 1){throw "Owned query ancestry did not select the highest same-process native window: $failure"}
  }elseif($failure -cne 'Input readiness requires an owned native window' -or $script:inputCase.actions -or $script:inputCase.writes){throw "Query ancestry escaped its owned chain or acted without a window: $case $failure"}
 }
 $root=$mainInputRoot
 'Native input query scope: zero-HWND descendant resolves through its owned bridge to main HWND77; foreign and absent ancestry rejected before input.'
}

Reset-InputCase 'post-action-tree-change'
Invoke-CapyPointerInput -Target 'target-control' -Point @(120,240) -Device 'mouse' -Action {
 Invoke-OriginalPointerBatch
 $script:inputCase.rediscoveryUnavailable=$true
}
if($script:inputCase.actions -ne 1 -or $script:inputCase.writes -ne 2){throw 'Post-action receipt lost its admitted owned endpoint or repeated input'}
'Post-action receipt uses the exact pre-admitted owned query channel with a fresh nonce after the accessibility tree changes.'

Reset-InputCase 'foreign-admitted-channel'
$failure=$null
try{
 Invoke-CapyPointerInput -Target 'target-control' -Point @(120,240) -Device 'mouse' -Action {Invoke-OriginalPointerBatch;$review.Id=8}
}catch{$failure=$_.Exception.Message}finally{$review.Id=7}
if($failure -cne 'Input query channel belongs to another drawing' -or $script:inputCase.actions -ne 1 -or $script:inputCase.writes -ne 1){throw 'A retained input channel crossed its admitted process identity'}
'Admitted query channels remain tied to their original process.'
