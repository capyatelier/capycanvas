param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CapyProofPicker {
 [DllImport("user32.dll",CharSet=CharSet.Unicode,SetLastError=true)] static extern IntPtr SendMessageTimeout(IntPtr h,uint m,UIntPtr w,IntPtr l,uint f,uint t,out UIntPtr r);
 [DllImport("user32.dll",SetLastError=true)] public static extern bool PostMessage(IntPtr h,uint m,UIntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 public static void Type(IntPtr h,string text) {
  UIntPtr r;
  if(SendMessageTimeout(h,0xB1,UIntPtr.Zero,new IntPtr(-1),2,2000,out r)==IntPtr.Zero)throw new Exception("Cannot select picker text");
  if(SendMessageTimeout(h,0x303,UIntPtr.Zero,IntPtr.Zero,2,2000,out r)==IntPtr.Zero)throw new Exception("Cannot clear picker text");
  foreach(char c in text)if(SendMessageTimeout(h,0x102,new UIntPtr(c),IntPtr.Zero,2,2000,out r)==IntPtr.Zero)throw new Exception("Cannot type picker path");
 }
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/proof-ui/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory((Join-Path $run 'profile'))|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name)}
function Model {
 try {
  if(!$script:statePath){
   foreach($candidate in [IO.Directory]::EnumerateFiles($directory,("ui-state-"+$review.Id+"-*.json"))){
    if([IO.File]::GetLastWriteTimeUtc($candidate) -ge $review.StartTime.ToUniversalTime()){$script:statePath=$candidate;break}
   }
  }
  if(!$script:statePath){return}
  $snapshot=Get-Content -LiteralPath $script:statePath -Raw|ConvertFrom-Json
  if($snapshot.process_id -eq $review.Id -and $snapshot.model.windows_isolated_settings){return $snapshot.model}
 }catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=15){
 $watch=[Diagnostics.Stopwatch]::StartNew()
 do {if(& $Condition){return};$review.Refresh();if($review.HasExited){throw "Proof review exited: $Message"};Start-Sleep -Milliseconds 65}while($watch.Elapsed.TotalSeconds -lt $Seconds)
 throw $Message
}
function Find([string]$Value,[switch]$Name,$Type){
 $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
 $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
 foreach($item in $root.FindAll([System.Windows.Automation.TreeScope]::Descendants,$condition)){
  if(!$item.Current.IsOffscreen -and (!$Type -or $item.Current.ControlType -eq $Type)){return $item}
 }
}
function Control([string]$Value,[switch]$Name,$Type){
 $hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name -Type $Type;$null -ne $hit.item} "Missing proof control: $Value";$hit.item
}
function Invoke([string]$Value,[switch]$Name){
 $item=Control $Value -Name:$Name
 $pattern=$null
 if($item.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern,[ref]$pattern)){$pattern.Invoke()}
 else{$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
}
function Button([string]$Name){
 (Control $Name -Name -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Command([string]$Id,[string]$Menu='View'){
 Wait-Until {((Model).state.commands|Where-Object id -eq $Id).enabled} "Command disabled: $Id" 45
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name $Menu
 Invoke $Id
}
function Select-Choice([string]$Id,[string]$Name){
 $box=Control $Id
 $box.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
 $item=Control $Name -Name -Type ([System.Windows.Automation.ControlType]::ListItem)
 $item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
}
function Idle {Wait-Until {$m=Model;$m -and !$m.windows_document -and !$m.state.document_file.busy -and !@($m.state.requests).Count} 'Proof/document operation did not finish' 60}
function ProofPanel {
 if(Find 'panel-tab-proof'){Invoke 'panel-tab-proof'}
 Wait-Until {(Find 'proof-panel-mode')} 'Retained Proof panel did not open'
}
function Setup {
 ProofPanel;Invoke 'proof-panel-setup'
 Wait-Until {(Model).windows_document.kind -eq 'proof' -and (Find 'proof-profile')} 'Proof Setup did not open'
}
function Picker([string]$Name){
 $script:picker=Control $Name -Name -Type ([System.Windows.Automation.ControlType]::Window)
 if($picker.Current.ClassName -ne '#32770' -or $picker.Current.ProcessId -ne $review.Id){throw 'Picker is outside the owned proof review'}
}
function Picker-Button([string]$Id){
 $item=$picker.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))
 $handle=[IntPtr]$item.Current.NativeWindowHandle;$owner=[uint32]0
 [CapyProofPicker]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
 if($owner -ne $review.Id){throw 'Picker button ownership changed'}
 if(![CapyProofPicker]::PostMessage($handle,245,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'Picker button failed'}
}
function Path-In-Picker([string]$Path){
 if(!(Split-Path -Parent $Path).Equals($run,[StringComparison]::OrdinalIgnoreCase)){throw 'Proof files must stay in the test directory'}
 $entry=$null
 foreach($id in @('1001','1148')){
  $entry=$picker.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$id),
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit')))
  if($entry){break}
 }
 if(!$entry -or $entry.Current.ProcessId -ne $review.Id){throw 'Picker filename ownership changed'}
 [CapyProofPicker]::Type([IntPtr]$entry.Current.NativeWindowHandle,$Path);Picker-Button '1'
}
function Export([string]$Name){
 Command 'export_document' 'File';Button 'Preview export'
 Wait-Until {(Model).windows_document.stage -eq 'preview'} 'Export preparation did not finish' 60
 Button 'Export…';Picker 'Save As';$path=Join-Path $run $Name;Path-In-Picker $path;Idle
 Wait-Until {Test-Path -LiteralPath $path} 'Export file missing'
 (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
}
try {
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
 Remove-Item Env:CAPY_PRESENT_PROBE -ErrorAction SilentlyContinue
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle;Write-Output "Proof review $($review.Id), $run"
 Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Proof app did not start' 60
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 $initial=(Model).state.document_file|ConvertTo-Json -Compress
 Command 'soft_proof'
 Wait-Until {$null -ne (Find 'proof-panel-setup')} 'Proof command did not reveal the panel'
 Invoke 'proof-panel-setup'
 Wait-Until {$null -ne (Find 'proof-profile')} 'Print setup did not open'
 Button 'Cancel';Idle
 if((Model).state.soft_proof -or ((Model).state.document_file|ConvertTo-Json -Compress) -ne $initial){throw 'First-use cancellation edited the drawing'}
 Setup;Select-Choice 'proof-profile' 'Display P3';Select-Choice 'proof-intent' 'Absolute'
 if((Control 'proof-bpc').Current.IsEnabled){throw 'Absolute intent left BPC enabled'}
 Select-Choice 'proof-intent' 'Relative'
 if(!(Control 'proof-bpc').Current.IsEnabled){throw 'Relative intent disabled BPC'}
 Select-Choice 'proof-simulation' 'Paper & ink'
 Button 'Add Profile…';Picker 'Open';Picker-Button '2'
 Wait-Until {$null -ne (Find 'proof-profile')} 'Profile picker cancel did not restore setup'
 Button 'Manage Profiles…';Button 'Done'
 Wait-Until {$null -ne (Find 'proof-profile')} 'Library did not return to setup'
 Button 'Apply';Idle
 Wait-Until {(Model).state.soft_proof -and (Model).windows_proof.bytes -gt 0 -and (Model).windows_proof.text -eq 'Proof: Display P3'} 'Proof did not prepare and apply' 60
 Setup
 $settings=(Model).windows_document.details.settings
 if($settings.profile.name -ne 'Display P3' -or $settings.simulation -ne 'paper_and_ink' -or $settings.intent -ne 'RelativeColorimetric'){throw 'Profile/library navigation lost the draft'}
 Button 'Cancel';Idle
 $withProof=(Model).state.document_file|ConvertTo-Json -Compress
 Invoke 'proof-panel-gamut';Wait-Until {(Model).state.gamut_warning} 'Gamut warning failed'
 Command 'soft_proof';Wait-Until {!(Model).state.soft_proof -and !(Model).state.gamut_warning -and !(Model).windows_proof.text} 'Off did not clear proof status'
 if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $withProof){throw 'Viewing toggles modified the drawing'}
 Command 'undo' 'Edit';Idle
 if((Model).state.document_file.modified){throw 'One Undo did not restore the clean drawing'}
 Command 'redo' 'Edit';Idle;Command 'soft_proof'
 Wait-Until {(Model).windows_proof.bytes -gt 0 -and (Model).windows_proof.text -eq 'Proof: Display P3'} 'Redo proof view did not recover' 60
 Button 'Test stroke';Wait-Until {((Model).state.commands|Where-Object id -eq 'undo').enabled} 'Stroke did not commit'
 $proofHash=Export 'proof-on.png'
 Command 'soft_proof';$plainHash=Export 'proof-off.png'
 if($proofHash -ne $plainHash){throw 'Proof contaminated exported pixels'}
 Command 'undo' 'Edit';Command 'redo' 'Edit'
 if((Export 'redo.png') -ne $plainHash){throw 'Drawing Undo/Redo changed exact export pixels'}
 Command 'soft_proof';Wait-Until {(Model).windows_proof.bytes -gt 0} 'Proof not ready before GPU removal'
 $generation=(Model).windows_gpu_generation;$file=(Model).state.document_file|ConvertTo-Json -Compress
 Button 'Test GPU loss'
 Wait-Until {(Model).windows_gpu_generation -gt $generation -and (Model).brush_ready -and !(Model).windows_rendering_suspended} 'D3D12 recovery failed' 60
 Wait-Until {(Model).windows_proof.text -eq 'Proof: Display P3'} 'Proof did not survive GPU reconstruction'
 if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $file){throw 'GPU recovery changed proof or drawing history'}
 if((Export 'recovered.png') -ne $plainHash){throw 'GPU recovery changed drawing pixels'}
 Command 'save_document_as' 'File';Picker 'Save As';$master=Join-Path $run 'Proof 日本語.capy';Path-In-Picker $master;Idle
 Command 'open_document' 'File';Picker 'Open';Path-In-Picker $master;Idle
 if((Model).state.soft_proof -or (Model).state.gamut_warning){throw 'Reopen retained temporary view toggles'}
 Command 'soft_proof';Wait-Until {(Model).windows_proof.text -eq 'Proof: Display P3'} 'Saved recipe did not rebuild after reopen' 60
 Setup;Select-Choice 'proof-profile' 'sRGB';Button 'Apply';Idle
 Wait-Until {(Model).windows_proof.text -eq 'Proof: sRGB'} 'Replacement proof did not apply' 60
 Command 'undo' 'Edit';Wait-Until {(Model).windows_proof.text -eq 'Proof: Display P3'} 'Proof recipe Undo did not rebuild' 60
 Command 'redo' 'Edit';Wait-Until {(Model).windows_proof.text -eq 'Proof: sRGB'} 'Proof recipe Redo did not rebuild' 60
 & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'proof.png') -ClientOnly *> (Join-Path $run 'proof.json')
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $directory
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'Proof stderr requires inspection'}
 [pscustomobject]@{first_use_cancel='passed';shared_options='passed';picker_cancel_draft='passed';library_draft='passed';apply_view_warning='passed';proof_history='passed';artwork_history_pixels='passed';export_uncontaminated='passed';d3d12_device_recovery='passed';save_reopen='passed';scope='Native UIA and hardware D3D12 functional checks; no physical print or performance acceptance'}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{
 if($review -and !$review.HasExited){try{& (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'failure.png') -ClientOnly *> (Join-Path $run 'failure.json')}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
 foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name])}
}
