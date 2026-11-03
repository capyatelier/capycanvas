$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
namespace CapyMenuInvokeTest {
 public class Pattern {
  public static string Outcome;public static bool Commit,Opened;public static int Calls;
  public void Invoke(){Calls++;Opened=Commit;if(Outcome=="retired")throw new System.Windows.Automation.ElementNotAvailableException();if(Outcome=="other")throw new InvalidOperationException("Unrelated menu invocation failure");}
 }
 public class Element{public Pattern GetCurrentPattern(object pattern){return new Pattern();}}
}
'@ -ReferencedAssemblies @('UIAutomationClient','UIAutomationTypes')
$tokens=$null;$errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile((Join-Path $PSScriptRoot '../scripts/open-application-menu.ps1'),[ref]$tokens,[ref]$errors)
if($errors.Count){throw($errors|Out-String)}
$invoke=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq 'Invoke-MenuButton'},$true)
if(!$invoke){throw 'Application menu invocation helper is missing'}
. ([scriptblock]::Create($invoke.Extent.Text))
function Read-MenuNodes([switch]$Popup){
 if([bool]$Popup -ne (!$script:expectedButton)){throw 'Menu acknowledgement used the wrong generic Flyout or MenuFlyout scope'}
 $script:reads++
 if([CapyMenuInvokeTest.Pattern]::Opened){[pscustomobject]@{Cached=[pscustomobject]@{IsOffscreen=$false;ControlType=$(if($script:buttonEntry){[System.Windows.Automation.ControlType]::Button}else{[System.Windows.Automation.ControlType]::MenuItem});AutomationId=$script:buttonEntry;ProcessId=$script:entryProcess}}}
}
foreach($case in @(
 @{outcome='success';commit=$true;error=$null;reads=0},
 @{outcome='retired';commit=$true;error=$null;reads=1},
 @{outcome='retired';commit=$false;error='ElementNotAvailableException';reads=1},
 @{outcome='other';commit=$false;error='InvalidOperationException';reads=0},
 @{outcome='retired';commit=$true;expected='header-overflow-item-99';button='header-overflow-item-99';error=$null;reads=1},
 @{outcome='retired';commit=$true;expected='header-overflow-item-99';button='header-overflow-item-17';error='ElementNotAvailableException';reads=1},
 @{outcome='retired';commit=$true;expected='header-overflow-item-99';error='ElementNotAvailableException';reads=1},
 @{outcome='retired';commit=$true;expected='header-overflow-item-99';button='header-overflow-item-99';foreign=$true;error='ElementNotAvailableException';reads=1}
)){
 $Root=[pscustomobject]@{Current=[pscustomobject]@{ProcessId=$PID}}
 $script:expectedButton=$case.expected;$script:entryProcess=if($case.foreign){$PID+1}else{$PID}
 $script:buttonEntry=$case.button
 [CapyMenuInvokeTest.Pattern]::Outcome=$case.outcome;[CapyMenuInvokeTest.Pattern]::Commit=$case.commit
 [CapyMenuInvokeTest.Pattern]::Opened=$false;[CapyMenuInvokeTest.Pattern]::Calls=0;$script:reads=0
 $watch=[Diagnostics.Stopwatch]::StartNew();$menuDeadline=0;$failure=$null
 try{Invoke-MenuButton ([CapyMenuInvokeTest.Element]::new()) $case.expected}catch{$failure=$_}
 if([CapyMenuInvokeTest.Pattern]::Calls -ne 1 -or $script:reads -ne $case.reads){throw 'Menu invocation repeated its action or changed its acknowledgement route'}
 if($null -eq $case.error){if($failure -or ![CapyMenuInvokeTest.Pattern]::Opened){throw 'Committed menu did not open'}}
 elseif(!$failure -or $failure.Exception.GetBaseException().GetType().Name -ne $case.error){throw 'Menu invocation lost its expected failure'}
}
Write-Output 'Application menu invocation passed: success, retired committed peer, unopened menu rejection and unrelated exception propagation; every action invoked once; retired header anchors require their exact expected Button, never another Button, foreign provider or a MenuItem; generic and windowed discovery stay separate.'
