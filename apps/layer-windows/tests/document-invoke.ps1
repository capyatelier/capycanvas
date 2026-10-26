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
