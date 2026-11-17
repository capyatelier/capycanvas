$ErrorActionPreference='Stop'
Add-Type -TypeDefinition @'
using System;
using System.Management.Automation.Runspaces;
public static class CapyClipboardOwnerTest {
 public static Runspace Owner;
 public static string Text;
 public static void SetText(string value){Owner=Runspace.DefaultRunspace;Text=value;}
 public static string GetText(){return Text;}
 public static void Copy(){
  if(Owner!=null && Owner.RunspaceStateInfo.State!=RunspaceState.Closed)
   throw new InvalidOperationException("EmptyClipboard blocked on the fixture's non-pumping owner");
  Text="#CA4B35";
 }
}
public static class CapyRowPointer {public static void Chord(uint id,ushort[] modifiers,int key){}}
public static class ClipboardLock {
 public static int Attempts,Rejects,Closes;public static bool Held,AlwaysBusy;
 public static void Reset(int rejects,bool alwaysBusy){Attempts=Closes=0;Rejects=rejects;Held=false;AlwaysBusy=alwaysBusy;}
 public static IntPtr GetOpenClipboardWindow(){return IntPtr.Zero;}
 public static bool OpenClipboard(IntPtr owner){
  if(owner!=IntPtr.Zero||Held)throw new InvalidOperationException("Unexpected clipboard acquisition");
  Attempts++;if(AlwaysBusy||Attempts<=Rejects)return false;Held=true;return true;
 }
 public static bool CloseClipboard(){if(!Held)throw new InvalidOperationException("Closing an unowned clipboard");Held=false;Closes++;return true;}
}
'@ -ReferencedAssemblies ([Management.Automation.PowerShell].Assembly.Location)
function Load-Function([string]$Path,[string]$Name){
 $tokens=$null;$errors=$null
 $ast=[Management.Automation.Language.Parser]::ParseFile($Path,[ref]$tokens,[ref]$errors)
 if($errors.Count){throw "Parse failed: $Path"}
 $node=$ast.Find({param($n)$n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -eq $Name}.GetNewClosure(),$true)
 if(!$node){throw "Missing function: $Name"}
 $node.Extent.Text.Replace('[Windows.Forms.Clipboard]','[CapyClipboardOwnerTest]')
}
$scripts=Join-Path $PSScriptRoot '../scripts'
foreach($name in 'Sta','Wait-Scale','Wait-CapyCondition','Wait-Until'){. ([scriptblock]::Create((Load-Function (Join-Path $scripts 'CapyUia.ps1') $name)))}
foreach($name in 'Paste','Clipboard-Text','Set-ClipboardText','Clipboard-Closed'){
 . ([scriptblock]::Create((Load-Function (Join-Path $scripts 'exercise-color-editor.ps1') $name)))
}
function Set-Clipboard([string]$Value){[CapyClipboardOwnerTest]::SetText($Value)}
function Get-Clipboard([switch]$Raw){[CapyClipboardOwnerTest]::GetText()}
function Control([string]$Id){New-Object PSObject|Add-Member ScriptMethod SetFocus {} -PassThru}
$review=[pscustomobject]@{Id=1}
foreach($text in 'rgb(202 75 53)',"native clipboard sentinel`r`nsecond line"){
 Paste $text
 if((Clipboard-Text) -cne $text){throw 'Clipboard text changed during transfer'}
 [CapyClipboardOwnerTest]::Copy()
 if((Clipboard-Text) -cne '#CA4B35'){throw 'App clipboard copy was not read'}
}
Write-Output 'Clipboard owner lifetime passed'

& {
 . ([scriptblock]::Create((Load-Function (Join-Path $scripts 'exercise-color-editor.ps1') 'Busy-Clipboard')))
 function Wait-Until([scriptblock]$Condition,[string]$Message){for($poll=0;$poll -lt 3;$poll++){if(& $Condition){return}};throw $Message}
 foreach($case in @(@{name='transient';rejects=2;busy=$false;throws=$false},@{name='ready';rejects=0;busy=$false;throws=$false},@{name='permanent';rejects=0;busy=$true;throws=$false},@{name='action-throws';rejects=1;busy=$false;throws=$true})){
  [ClipboardLock]::Reset($case.rejects,$case.busy);$actions=@{count=0};$failure=$null
  try{
   Busy-Clipboard {
    if(![ClipboardLock]::Held -or [ClipboardLock]::GetOpenClipboardWindow() -ne [IntPtr]::Zero){throw 'Busy action ran without its actual anonymous clipboard lock'}
    $actions.count++;if($case.throws){throw 'Clipboard refusal action failed'}
   }
  }catch{$failure=$_.Exception.Message}
  if($case.busy){
   if($failure -cne 'The fixture could not hold the clipboard open' -or $actions.count -ne 0 -or [ClipboardLock]::Attempts -ne 3 -or [ClipboardLock]::Closes -ne 0){throw 'Permanent anonymous contention bypassed the bound or reached the refusal action'}
  }else{
   $expected=if($case.throws){'Clipboard refusal action failed'}else{$null}
   if($failure -cne $expected -or $actions.count -ne 1 -or [ClipboardLock]::Attempts -ne $case.rejects+1 -or [ClipboardLock]::Closes -ne 1){throw "Actual Busy-Clipboard did not acquire once before one action and release it: $($case.name): $failure"}
  }
  if([ClipboardLock]::Held){throw 'Busy-Clipboard retained its anonymous lock after completion'}
 }
 'Actual Busy-Clipboard owns the anonymous lock before one action, bounds contention and releases it after an action failure.'
}
