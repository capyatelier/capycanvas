$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
namespace CapyDocumentInvokeTest {
 public class Pattern {
  public static string Outcome;public static bool Commit,Acknowledged;public static int Calls;
  public void Invoke(){Calls++;Acknowledged=Commit;if(Outcome=="retired")throw new System.Windows.Automation.ElementNotAvailableException();if(Outcome=="other")throw new InvalidOperationException("Unrelated invocation failure");}
 }
 public class State{public bool IsEnabled=true;}
 public class Element{public State Current=new State();public Pattern GetCurrentPattern(object pattern){return new Pattern();}}
 public static class Pointer{public static int Initialized,Disposed;public static void Initialize(uint pid){Initialized++;}public static void Hover(int x,int y){}public static void Dispose(){Disposed++;}}
}
'@ -ReferencedAssemblies @('UIAutomationClient','UIAutomationTypes')
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/exercise-documents.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw($errors|Out-String)}
$press=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Press'},$true)
if(!$press){throw 'Document Press helper is missing'}
. ([scriptblock]::Create($press.Extent.Text))
function Find{[CapyDocumentInvokeTest.Element]::new()}
function Wait-Until([scriptblock]$Condition,[string]$Message){$script:waits++;if(& $Condition){return};throw $Message}
foreach($case in @(
 @{outcome='success';commit=$true;error=$null},
 @{outcome='retired';commit=$true;error=$null},
 @{outcome='retired';commit=$false;error='Shared acknowledgement missing'},
 @{outcome='other';commit=$false;error='Unrelated invocation failure'}
)){
 [CapyDocumentInvokeTest.Pattern]::Outcome=$case.outcome;[CapyDocumentInvokeTest.Pattern]::Commit=$case.commit
 [CapyDocumentInvokeTest.Pattern]::Acknowledged=$false;[CapyDocumentInvokeTest.Pattern]::Calls=0;$script:waits=0
 $failure=$null
 try{Press 'new-drawing-paper-use' {[CapyDocumentInvokeTest.Pattern]::Acknowledged} 'Shared acknowledgement missing'}catch{$failure=$_}
 if([CapyDocumentInvokeTest.Pattern]::Calls -ne 1){throw 'Document action was repeated'}
 if($null -eq $case.error){if($failure -or $script:waits -ne 2){throw 'Committed document action did not await its acknowledgement'}}
 elseif(!$failure -or !$failure.ToString().Contains($case.error)){throw 'Document invocation lost its expected failure'}
}
Write-Output 'Document invocation passed: ordinary acknowledgement, retired committed peer, missing commit rejection and unrelated exception propagation; every action invoked once.'

& {
 foreach($name in @('Writeback-Projection','Writeback-Geometry','Writeback-Checked')){
  $function=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $name},$true)
  if(!$function){throw "Missing actual document function: $name"}
  . ([scriptblock]::Create($function.Extent.Text.Replace('[CapyRowPointer]','[CapyDocumentInvokeTest.Pointer]')))
 }
 function Foreground{}
 function Model{[pscustomobject]@{state=[pscustomobject]@{camera=$script:testCamera}}}
 function Control([string]$Id,[switch]$Arranged){if($Id -cne 'drawing-canvas' -or !$Arranged){throw 'Unexpected projection control'};[pscustomobject]@{Current=[pscustomobject]@{BoundingRectangle=$script:testBounds}}}
 $review=[pscustomobject]@{Id=33};$drawing=[pscustomobject]@{Handle=[IntPtr]22}
 foreach($case in @(
  @{name='revision';change={$script:testCamera.revision++};error=$null},
  @{name='bounds';change={$script:testBounds.X++};error='Native writeback owned projection changed during sampling or input'},
  @{name='zoom';change={$script:testCamera.zoom+=.1};error='Native writeback owned projection changed during sampling or input'},
  @{name='translation';change={$script:testCamera.translation[0]++};error='Native writeback owned projection changed during sampling or input'},
  @{name='viewport';change={$script:testCamera.viewport[0]++};error='Native writeback owned projection changed during sampling or input'},
  @{name='work area';change={$script:testCamera.work_area[0]++};error='Native writeback owned projection changed during sampling or input'},
  @{name='rotation';change={$script:testCamera.rotation=.1};error='Native writeback requires an unrotated canvas projection'},
  @{name='flip';change={$script:testCamera.flipped[0]=$true};error='Native writeback requires an unrotated canvas projection'},
  @{name='owner';change={$root.Current.ProcessId=44};error='Native writeback projection does not belong to the owned drawing'},
  @{name='window';change={$root.Current.NativeWindowHandle=23};error='Native writeback projection does not belong to the owned drawing'},
  @{name='action error';change={throw 'Original pixel action failure'};error='Original pixel action failure'}
 )){
  $script:testCamera='{"revision":9,"zoom":1,"rotation":0,"flipped":[false,false],"translation":[10,20],"viewport":[100,200],"work_area":[0,0,100,200]}'|ConvertFrom-Json
  $script:testBounds=[pscustomobject]@{X=1;Y=2;Width=100;Height=200}
  $root=[pscustomobject]@{Current=[pscustomobject]@{ProcessId=33;NativeWindowHandle=22;BoundingRectangle=$script:testBounds}}
  $script:writebackProjection=Writeback-Geometry (Writeback-Projection)|ConvertTo-Json -Depth 8 -Compress
  [CapyDocumentInvokeTest.Pointer]::Initialized=0;[CapyDocumentInvokeTest.Pointer]::Disposed=0;$script:actions=0;$failure=$null;$result=$null
  try{$result=Writeback-Checked {param($p)$script:actions++;& $case.change;'unchanged-pixels'}}catch{$failure=$_}
  if($script:actions -ne 1 -or [CapyDocumentInvokeTest.Pointer]::Initialized -ne 1 -or [CapyDocumentInvokeTest.Pointer]::Disposed -ne 1){throw "Projection action or pointer cleanup repeated: $($case.name)"}
  if($null -eq $case.error){if($failure -or $result -cne 'unchanged-pixels'){throw 'A revision-only camera update rejected unchanged writeback pixels'}}
  elseif(!$failure -or $failure.Exception.Message -cne $case.error){throw "Wrong projection rejection: $($case.name): $failure"}
 }
 'Document writeback projection: revision-only acknowledgement accepted; changed geometry/ownership, rotation/flips and original action errors rejected; every action and cleanup once.'
}
