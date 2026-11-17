param([string]$Scripts=(Join-Path $PSScriptRoot '../scripts'))
$ErrorActionPreference='Stop'
Add-Type -AssemblyName WindowsBase,UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;
public static class CapyInventoryDriver {
 public static readonly List<string> Events=new List<string>();public static int Downs,X,Y;public static bool Active;public static string Device;
 public static void Reset(){Events.Clear();Downs=0;Active=false;}
 public static void Down(string device,int x,int y){Device=device;X=x;Y=y;Downs++;Active=true;Events.Add("down:"+device+":"+x+":"+y);}
 public static void Move(int x,int y){Events.Add("move:"+x+":"+y);}
 public static void Up(){Up(false);}public static void Up(bool hover){Active=false;Events.Add("up:"+hover);}
 public static void Cancel(){Active=false;Events.Add("cancel");}
 public static void Hover(int x,int y){Events.Add("hover:"+x+":"+y);}
}
'@
function Read-InventoryAst([string]$Name){
 $tokens=$null;$errors=$null;$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $Scripts $Name),[ref]$tokens,[ref]$errors)
 if($errors.Count){throw ($errors|Out-String)}
 $ast
}
function Import-InventoryFunction($Ast,[string]$Name,[switch]$Optional){
 $node=$Ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $Name},$true)
 if(!$node){if(!$Optional){throw "Missing inventory function $Name"};return}
 $node.Extent.Text.Replace('[CapyRowPointer]','[CapyInventoryDriver]')
}
function Require-Inventory([bool]$Condition,[string]$Message){if(!$Condition){throw $Message}}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=1,[switch]$Closing,[scriptblock]$ResetBound){
 $script:inventory.waits++
 foreach($attempt in 1..3){if(& $Condition){return}}
 throw $Message
}
. (Join-Path $PSScriptRoot '../scripts/InputReady.ps1')
$edge=Read-InventoryAst 'exercise-edge-reach.ps1'
foreach($name in 'Contact-Receipt','Contact-Down','Contact'){$definition=Import-InventoryFunction $edge $name -Optional:($name -eq 'Contact-Down');if($definition){. ([scriptblock]::Create($definition))}}
$switcher=Read-InventoryAst 'exercise-switcher-drag.ps1'
foreach($name in 'Native-TapAt','Held-Button') {. ([scriptblock]::Create((Import-InventoryFunction $switcher $name)))}
$photo=Read-InventoryAst 'exercise-photo-tools.ps1'
. ([scriptblock]::Create((Import-InventoryFunction $photo 'Footprint-Corners')))
function Reset-Inventory([string]$Route='ui',[string]$Receipt='accepted',[bool]$Ready=$true){
 [CapyInventoryDriver]::Reset();$script:inventory=@{route=$Route;receipt=$Receipt;ready=$Ready;queries=0;reads=0;waits=0;invoked=[Collections.Generic.List[string]]::new()}
 $script:taps=[Collections.Generic.List[object]]::new()
}
$review=[pscustomobject]@{Id=7};$Device='touch';$scale=1;$run=$env:TEMP
$peer=[pscustomobject]@{Current=[pscustomobject]@{IsEnabled=$true;BoundingRectangle=[Windows.Rect]::new(100,200,40,40)}}
$peer|Add-Member ScriptMethod GetRuntimeId {,(7,11)}
function Control($Id,[switch]$Arranged){if($Id -cne 'workspace-manager-pin-7'){throw "Unexpected inventory control $Id"};$peer}
function Storage{[pscustomobject]@{id=7}}
function Screen($X,$Y){@{x=[int]$X;y=[int]$Y}}
function Input-Trace($X,$Y){@{hit=@(@{id=$(if($script:inventory.route -eq 'canvas'){'drawing-canvas'}else{'native-control'});process=7})}}
function Gesture{
 if($script:inventory.route -eq 'outside-ui'){return @{edge=$null;edge_contact=@{primary=$true;time=1;position=@(642,80)}}}
 $active=[CapyInventoryDriver]::Active;$canvas=$script:inventory.route -eq 'canvas'
 $position=if($script:inventory.route -eq 'wrong-point'){@(99,99)}else{@(120,220)}
 @{edge=$(if($active -and !$canvas){@{serial=3}}else{$null});edge_contact=$(if(!$canvas){@{primary=$true;time=$(if($active){2}else{1});position=$position}}else{$null})}
}
function Wait-CapyInputTarget($Target,[string]$Kind,[double[]]$Point){
 $script:inventory.queries++
 Require-Inventory ($Target -cin @('drawing-canvas','workspace-manager-pin-7')) 'Inventory boundary declared another target'
 if(!$script:inventory.ready){throw 'Native pointer target did not become input-ready'}
 @{window_id=77;down_serial=0;dropped=0;CapyNativeInputChannel=@{}}
}
function Start-CapyInputQuery($Kind,$Point,$Channel){@{kind=$Kind}}
function Read-CapyInputReceipt($Request){
 $script:inventory.reads++;$rows=@()
 if([CapyInventoryDriver]::Downs -and $script:inventory.receipt -ne 'missing'){
  $target=if($script:inventory.route -eq 'canvas'){'drawing-canvas'}else{'workspace-manager-pin-7'}
  if($script:inventory.receipt -eq 'wrong'){$target='wrong-receiver'}
  $rows=@(@{serial=1;device=[CapyInventoryDriver]::Device;screen=@([CapyInventoryDriver]::X,[CapyInventoryDriver]::Y);path=@($target);route=$(if($script:inventory.route -eq 'canvas'){'independent'}else{'routed'});admitted=($script:inventory.receipt -ne 'rejected')})
 }
 @{window_id=77;down_serial=[CapyInventoryDriver]::Downs;dropped=0;downs=$rows}
}
function Start-Sleep([int]$Milliseconds){[CapyInventoryDriver]::Events.Add('sleep:'+$Milliseconds)}
function Set-Content {param($LiteralPath,$Path,$Value);$input|Out-Null}
function Expect-InventoryFailure([scriptblock]$Action,[string]$Message){
 $failure=$null;try{& $Action|Out-Null}catch{$failure=$_.Exception.Message}
 if(!$failure){throw "Expected $Message but action completed with $([CapyInventoryDriver]::Downs) native Down(s)"}
 Require-Inventory ($failure.Contains($Message)) ("Wrong inventory failure: $failure; expected $Message")
}
$at=@{x=120;y=220}
Reset-Inventory 'outside-ui'
$outside=@{x=642;y=94}
try{$outsideDown=Contact-Down $outside $outside $false}finally{[CapyInventoryDriver]::Cancel()}
Require-Inventory ([CapyInventoryDriver]::Downs -eq 1 -and [CapyInventoryDriver]::X -eq 642 -and [CapyInventoryDriver]::Y -eq 94 -and $inventory.waits -eq 0 -and $inventory.queries -eq 0 -and $outsideDown.edge_contact.position[1] -eq 80) 'Outside-surface UI Down added a root acknowledgement instead of preserving the original Gesture sample'
foreach($receipt in 'accepted','missing','rejected'){
 Reset-Inventory 'canvas' $receipt
 try{
  if($receipt -eq 'accepted'){$null=Contact-Down $at $at $false;Require-Inventory ($inventory.reads -eq 1 -and [CapyInventoryDriver]::Downs -eq 1) 'Outside-surface canvas Down skipped its independent receipt'}
  else{Expect-InventoryFailure {Contact-Down $at $at $false} $(if($receipt -eq 'missing'){'Native Down did not reach its intended target'}else{'Canvas rejected its native Down'})}
 }finally{[CapyInventoryDriver]::Cancel()}
 Require-Inventory (![CapyInventoryDriver]::Active) 'Outside-surface canvas receipt failure skipped cleanup'
}
Reset-Inventory 'canvas'
$null=Contact $at
Require-Inventory ([CapyInventoryDriver]::Downs -eq 1 -and $inventory.reads -eq 1 -and ![CapyInventoryDriver]::Active) 'Independent canvas Down lacked its real admission receipt or cleanup'
foreach($receipt in 'missing','rejected'){
 Reset-Inventory 'canvas' $receipt
 Expect-InventoryFailure {Contact $at} $(if($receipt -eq 'missing'){'Native Down did not reach its intended target'}else{'Canvas rejected its native Down'})
 Require-Inventory (![CapyInventoryDriver]::Active) 'Failed canvas admission skipped Cancel'
}
Reset-Inventory
$null=Contact $at
Require-Inventory ([CapyInventoryDriver]::Downs -eq 1 -and $inventory.queries -eq 0 -and ![CapyInventoryDriver]::Active) 'Routed edge Down lost its existing gesture authority'
Reset-Inventory 'wrong-point'
Expect-InventoryFailure {Contact $at} 'Native contact Down was not acknowledged at the requested point'
$pinFunction=$switcher.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Pin-Buttons'},$true)
$pinStatement=@($pinFunction.Body.FindAll({param($n)$n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Right.Find({param($c)$c -is [Management.Automation.Language.CommandAst] -and $c.GetCommandName() -ceq 'Native-TapAt'},$true)},$true))
Require-Inventory ($pinStatement.Count -eq 1) 'Pin contact has no unique actual invocation'
$id=7
Reset-Inventory 'ui' 'accepted' $false
Expect-InventoryFailure {. ([scriptblock]::Create($pinStatement[0].Extent.Text))} 'Native pointer target did not become input-ready'
Require-Inventory ([CapyInventoryDriver]::Downs -eq 0) 'Unready pin still received Down'
Reset-Inventory 'ui' 'wrong'
Expect-InventoryFailure {. ([scriptblock]::Create($pinStatement[0].Extent.Text))} 'Native Down reached another receiver'
Reset-Inventory
. ([scriptblock]::Create($pinStatement[0].Extent.Text))
Require-Inventory (([CapyInventoryDriver]::Events -join '|') -ceq 'down:touch:120:220|sleep:35|move:121:221|up:False') 'Pin contact changed its drift, cadence or release'
$heldFunction=$switcher.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Held-Rename'},$true)
$heldCall=@($heldFunction.Body.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -ceq 'Held-Button'},$true))[0]
$pin=$peer;$active=7
Reset-Inventory 'ui' 'accepted' $false
Expect-InventoryFailure {. ([scriptblock]::Create($heldCall.Extent.Text))} 'Native pointer target did not become input-ready'
Require-Inventory ([CapyInventoryDriver]::Downs -eq 0 -and ([CapyInventoryDriver]::Events -join '|') -ceq 'cancel') 'Held pin acquisition failure lost unconditional cleanup'
Reset-Inventory 'ui' 'wrong'
Expect-InventoryFailure {. ([scriptblock]::Create($heldCall.Extent.Text))} 'Native Down reached another receiver'
Require-Inventory (([CapyInventoryDriver]::Events|Select-Object -Last 1) -ceq 'cancel') 'Wrong held-pin receiver skipped cleanup'
Reset-Inventory
. ([scriptblock]::Create($heldCall.Extent.Text))
Require-Inventory (([CapyInventoryDriver]::Events -join '|') -ceq 'down:touch:120:220|sleep:200|up:False|cancel') 'Held pin contact changed cadence, release or cleanup'
Reset-Inventory 'ui' 'accepted' $false
Held-Button $peer 'rename-held-apply'
Require-Inventory ($inventory.queries -eq 0 -and [CapyInventoryDriver]::Downs -eq 1) 'Anonymous Apply acquired an invented target'
function Invoke-Id([string]$Id){
 $script:inventory.invoked.Add($Id);$index=[int]($Id.Split('-')[-1])
 $script:anchor=@(foreach($i in 0..8){@{selected=(!$script:inventory.noAnchorSelection -and $i -eq $index)}})
}
function Model{@{state=@{tool_extra=@(@{Choice=@{id='transform-reference';items=$script:anchor}})}}}
function Value([string]$Id){if($Id -eq 'transform_x'){1}else{2}}
Reset-Inventory
$corners=Footprint-Corners
Require-Inventory (($inventory.invoked -join '|') -ceq 'tool-choice-transform-reference-0|tool-choice-transform-reference-2|tool-choice-transform-reference-8|tool-choice-transform-reference-6' -and $inventory.waits -eq 4 -and $corners.Count -eq 4 -and [CapyInventoryDriver]::Downs -eq 0) 'Photo anchors lost their exact shared acknowledgement or invented physical input'
Reset-Inventory
$inventory.noAnchorSelection=$true
Expect-InventoryFailure {Footprint-Corners} 'The native position anchor did not select'
'Input inventory: canvas/routed edge, pin and UIA anchor cases passed'
