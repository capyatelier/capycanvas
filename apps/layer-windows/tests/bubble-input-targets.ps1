param([string]$Scripts=(Join-Path $PSScriptRoot '../scripts'))
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;using System.Collections.Generic;
namespace CapyNamedInputTest {
 public static class Driver {
  public static readonly List<string> Events=new List<string>();
  public static string Focused;public static bool Search,Mask;public static int Downs,Moves,Ups,X,Y;
  public static void Reset(){Events.Clear();Focused="layer-new";Search=Mask=false;Downs=Moves=Ups=X=Y=0;}
  public static bool SetForegroundWindow(IntPtr window){if(window!=new IntPtr(77))throw new Exception("Foreground owner changed");Events.Add("foreground");return true;}
  public static void Chord(uint process,ushort[] modifiers,ushort key){if(process!=7||modifiers.Length!=1||modifiers[0]!=17||key!=75)throw new Exception("Command input changed");Events.Add("CtrlK");Search=true;}
  public static void Key(uint process,ushort key){if(process!=7)throw new Exception("Key owner changed");if(key==13){if(Focused!="command-search")throw new Exception("Return preceded search focus");Events.Add("Return");Search=false;}else if(key==81){if(Focused!="drawing-canvas")throw new Exception("Mask key lost canvas focus");Events.Add("Q");Mask=true;}else throw new Exception("Key changed");}
  public static void Down(string device,int x,int y){Downs++;X=x;Y=y;Events.Add("Down:"+device);}
  public static void Move(int x,int y){Moves++;}
  public static void Up(){Ups++;Events.Add("Up");}
 }
}
'@
$tokens=$null;$errors=$null
$enclose=[Management.Automation.Language.Parser]::ParseFile((Join-Path $Scripts 'exercise-enclose-fill.ps1'),[ref]$tokens,[ref]$errors);if($errors.Count){throw($errors|Out-String)}
$compact=[Management.Automation.Language.Parser]::ParseFile((Join-Path $Scripts 'exercise-compact-color.ps1'),[ref]$tokens,[ref]$errors);if($errors.Count){throw($errors|Out-String)}
$bubbles=[Management.Automation.Language.Parser]::ParseFile((Join-Path $Scripts 'exercise-tool-bubbles.ps1'),[ref]$tokens,[ref]$errors);if($errors.Count){throw($errors|Out-String)}
$visual=[Management.Automation.Language.Parser]::ParseFile((Join-Path $Scripts 'BubbleVisualChecks.ps1'),[ref]$tokens,[ref]$errors);if($errors.Count){throw($errors|Out-String)}
& {
 foreach($name in 'Key','Chord','Command'){
  $actual=$enclose.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name},$true)
  if(!$actual){throw "Missing actual Enclose helper: $name"}
  . ([scriptblock]::Create($actual.Extent.Text.Replace('[CapyRowPointer]','[CapyNamedInputTest.Driver]')))
 }
 [CapyNamedInputTest.Driver]::Reset();$review=@{Id=7};$drawingWindow=[IntPtr]77;$state=@{queries=0;typed=0;waits=0}
 $writer=[pscustomobject]@{};$writer|Add-Member ScriptMethod SetValue {param($text)if($text -cne 'Fill'){throw 'Targeted search ValuePattern text changed'};$state.typed++}
 $entry=[pscustomobject]@{Current=@{IsOffscreen=$false}};$entry|Add-Member ScriptMethod GetCurrentPattern {param($pattern)if($pattern -ne [System.Windows.Automation.ValuePattern]::Pattern){throw 'Search editing pattern changed'};$writer}
 function Model{@{state=@{commands=@(@{id='fill';label='Fill'})}}}
 function Find([string]$Id){if(![CapyNamedInputTest.Driver]::Search){return};if($Id -ceq 'command-search'){$entry}elseif($Id -ceq 'command-result-0'){@{Current=@{Name='Fill'}}}else{throw 'Unexpected search lookup'}}
 function Wait-Until([scriptblock]$Condition,[string]$Message){$state.waits++;if(!(& $Condition)){throw $Message}}
 function Wait-CapyInputTarget($Target,[string]$Kind,$Point){
  if($Target -cne 'command-search' -or $Kind -cne 'keyboard' -or $null -ne $Point -or ![CapyNamedInputTest.Driver]::Search){throw 'Global command invented a canvas or another focus target'}
  $state.queries++;[CapyNamedInputTest.Driver]::Focused='command-search';[CapyNamedInputTest.Driver]::Events.Add('search-ready')
 }
 function Invoke-CapyKeyInput($Target,[scriptblock]$Action){$null=Wait-CapyInputTarget $Target 'keyboard' $null;& $Action}
 Command 'fill'
 if($state.typed -ne 1 -or $state.queries -ne 1 -or $state.waits -ne 3 -or ([CapyNamedInputTest.Driver]::Events -join ',') -cne 'foreground,CtrlK,search-ready,Return'){throw 'Enclose command changed raw global CtrlK, exact search admission or input count'}
}
& {
 $calls=@($compact.FindAll({param($n)$n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -ceq 'Invoke-CapyPointerInput'},$true))
 if($calls.Count -ne 3){throw 'Expected exactly the three approved compact-color hold acquisitions'}
 $point=@(101,202);$x=101;$y=202;$bounds=@{X=90;Y=190;Width=22;Height=24};$Slot='background'
 foreach($index in 0..2){
  [CapyNamedInputTest.Driver]::Reset();$Device=if($index -eq 0){'touch'}elseif($index -eq 1){'pen'}else{'mouse'};$trace=@{};$state=@{queries=0;delays=0}
  function Slot-Colors{'unchanged'}
  function Find($Id,[switch]$Visible){if($Id -cnotin @('color-swatch-edit','color-swatch-swap')){throw 'Hold assertion changed its control'};$null}
  function Start-Sleep([int]$Milliseconds){if($Milliseconds -ne 1100){throw 'Compact hold changed its original cadence'};$state.delays++}
  function Invoke-CapyPointerInput([Alias('Target')]$ContractTarget,[Alias('Point')][double[]]$ContractPoint,[Alias('Device')][string]$ContractDevice,[Alias('Action')][scriptblock]$ContractAction){
   $expected=if($index -eq 0){'color-background'}else{'color-foreground'}
   if($ContractTarget -cne $expected -or ($ContractPoint -join ',') -cne '101,202' -or $ContractDevice -cne $device -or [CapyNamedInputTest.Driver]::Downs){throw 'Compact hold queried another receiver, point or device'}
   $state.queries++;& $ContractAction
   if([CapyNamedInputTest.Driver]::Downs -ne 1 -or [CapyNamedInputTest.Driver]::Ups -ne 1){throw 'Compact hold receipt split its original lifetime'}
  }
  . ([scriptblock]::Create($calls[$index].Extent.Text.Replace('[CapyRowPointer]','[CapyNamedInputTest.Driver]')))
  if($state.queries -ne 1 -or $state.delays -ne 1 -or [CapyNamedInputTest.Driver]::X -ne 101 -or [CapyNamedInputTest.Driver]::Y -ne 202){throw 'Compact hold changed the frozen point or hold cardinality'}
 }
}
& {
 foreach($name in 'Relative','Drag'){
  $actual=$bubbles.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $name},$true)
  if(!$actual){throw "Missing actual title-bar input helper: $name"}
  . ([scriptblock]::Create($actual.Extent.Text.Replace('[CapyRowPointer]','[CapyNamedInputTest.Driver]')))
 }
 $loop=@($bubbles.FindAll({param($n)$n -is [Management.Automation.Language.ForEachStatementAst] -and $n.Extent.Text.Contains("'Title-bar drop did not put bubble back'")},$true))
 if($loop.Count -ne 1){throw 'Missing unique actual title-bar put-back loop'}
 [CapyNamedInputTest.Driver]::Reset();$width=1400;$scale=1.5;$Device='touch';$state=@{queries=0;delays=0}
 function Packets{@{view=@{id=27;kind='color';diameter=120}}}
 function Center([long]$Id){if($Id -ne 27){throw 'Color record changed'};@{x=1000;y=600}}
 function Screen([double]$X,[double]$Y){if($X -ne 700 -or $Y -ne 12){throw 'Title-bar put-back destination changed'};@{x=700;y=12}}
 function Packet([long]$Id){if($Id -ne 27 -or [CapyNamedInputTest.Driver]::Ups -ne 1){throw 'Put-back acknowledgement preceded release'}}
 function Cancel-Contact{throw 'Title-bar unexpectedly cancelled its original put-back contact'}
 function Wait-Until([scriptblock]$Condition,[string]$Message){if($Message -cne 'Title-bar drop did not put bubble back' -or !(& $Condition)){throw $Message}}
 function Start-Sleep([int]$Milliseconds){if($Milliseconds -ne 20){throw 'Title-bar drag cadence changed'};$state.delays++}
 function Invoke-CapyPointerInput([Alias('Target')]$ContractTarget,[Alias('Point')][double[]]$ContractPoint,[Alias('Device')][string]$ContractDevice,[Alias('Action')][scriptblock]$ContractAction){
  if($ContractTarget.record -ne 27 -or $ContractTarget.part -cne 'face' -or ($ContractPoint -join ',') -cne '914,600' -or $ContractDevice -cne 'touch' -or [CapyNamedInputTest.Driver]::Downs){throw 'Title-bar query lost the exact color27 face or original off-center point'}
  $state.queries++;& $ContractAction
  if([CapyNamedInputTest.Driver]::Downs -ne 1 -or [CapyNamedInputTest.Driver]::Ups -ne 1){throw 'Title-bar receipt changed the original contact lifetime'}
 }
 . ([scriptblock]::Create($loop[0].Extent.Text))
 if($state.queries -ne 1 -or $state.delays -ne 16 -or [CapyNamedInputTest.Driver]::Moves -ne 16 -or [CapyNamedInputTest.Driver]::X -ne 914 -or [CapyNamedInputTest.Driver]::Y -ne 600){throw 'Actual title-bar loop changed its input or lost exact admission'}
}
& {
 $retire=$visual.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Invoke-BubbleHdrRetirement'},$true)
 $switch=$retire.Find({param($n)$n -is [Management.Automation.Language.SwitchStatementAst] -and $n.Extent.Text.Contains("'Mask context did not retire HDR intensity'")},$true)
 $mask=@($switch.Clauses|Where-Object {$_.Item1.Value -ceq 'mask'})
 if($mask.Count -ne 1){throw 'Missing actual retirement mask-Q branch'}
 [CapyNamedInputTest.Driver]::Reset();$review=@{Id=7};$Id=27;$state=@{focuses=0;queries=0}
 $canvas=[pscustomobject]@{};$canvas|Add-Member ScriptMethod SetFocus {$state.focuses++;[CapyNamedInputTest.Driver]::Focused='drawing-canvas';[CapyNamedInputTest.Driver]::Events.Add('canvas-focus')}
 function Control([string]$Id){if($Id -cne 'drawing-canvas'){throw 'Mask setup focused another control'};$canvas}
 function Invoke-CapyKeyInput($Target,[scriptblock]$Action){if($Target -cne 'drawing-canvas' -or [CapyNamedInputTest.Driver]::Focused -cne 'drawing-canvas'){throw 'Mask query preceded its original canvas SetFocus'};$state.queries++;& $Action}
 function Model{@{state=@{layer_tools=@{quick_mask=[CapyNamedInputTest.Driver]::Mask}}}}
 function Packet([long]$Id){if($Id -ne 27){throw 'Mask retirement lost its original bubble'};@{color=$null}}
 function Find([string]$Id,[switch]$Visible){if($Id -cne 'bubble-intensity-27' -or !$Visible){throw 'Mask retirement changed its visibility assertion'};$null}
 function Wait-Until([scriptblock]$Condition,[string]$Message){if($Message -cne 'Mask context did not retire HDR intensity' -or !(& $Condition)){throw $Message}}
 . ([scriptblock]::Create((($mask[0].Item2.Statements|ForEach-Object {$_.Extent.Text}) -join "`n").Replace('[CapyRowPointer]','[CapyNamedInputTest.Driver]')))
 if($state.focuses -ne 1 -or $state.queries -ne 1 -or ([CapyNamedInputTest.Driver]::Events -join ',') -cne 'canvas-focus,Q'){throw 'Mask retirement changed its original focus action or Q cardinality'}
}
'Approved named-input contracts: Enclose search typing/Return, three compact-color hold batches, exact color27 put-back and mask-Q passed without UI input.'
