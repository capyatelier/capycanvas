param([Parameter(Mandatory)][string]$Executable,[ValidateSet("F16","F32")][string]$Depth="F16")
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class CapyHdrPicker {
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
$run=Join-Path $repo ('artifacts/windows/hdr-ui/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory((Join-Path $run 'profile'))|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE','CAPY_TEST_HDR')
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
 do {if(& $Condition){return};$review.Refresh();if($review.HasExited){throw "HDR review exited: $Message"};Start-Sleep -Milliseconds 65}while($watch.Elapsed.TotalSeconds -lt $Seconds)
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
 $hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name -Type $Type;$null -ne $hit.item} "Missing HDR control: $Value";$hit.item
}
function Invoke([string]$Value,[switch]$Name){
 $item=Control $Value -Name:$Name
 Wait-Until {$item.Current.IsEnabled} "Control disabled: $Value"
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
function Idle {Wait-Until {$m=Model;$m -and !$m.windows_document -and !$m.state.document_file.busy -and !@($m.state.requests).Count} 'HDR/document operation did not finish' 60}
function Sdr {
 Command 'sdr_rendition'
 Wait-Until {(Model).windows_document.kind -eq 'sdr' -and (Find 'sdr-exposure')} 'SDR Appearance did not open'
}
function Setup {
 Command 'soft_proof_setup'
 Wait-Until {(Model).windows_document.kind -eq 'proof' -and (Find 'proof-profile')} 'Proof Setup did not open'
}
function Picker([string]$Name){
 $script:picker=Control $Name -Name -Type ([System.Windows.Automation.ControlType]::Window)
 if($picker.Current.ClassName -ne '#32770' -or $picker.Current.ProcessId -ne $review.Id){throw 'Picker is outside the owned HDR review'}
}
function Picker-Button([string]$Id){
 $item=$picker.FindFirst([System.Windows.Automation.TreeScope]::Children,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Id))
 $handle=[IntPtr]$item.Current.NativeWindowHandle;$owner=[uint32]0
 [CapyHdrPicker]::GetWindowThreadProcessId($handle,[ref]$owner)|Out-Null
 if($owner -ne $review.Id){throw 'Picker button ownership changed'}
 if(![CapyHdrPicker]::PostMessage($handle,245,[UIntPtr]::Zero,[IntPtr]::Zero)){throw 'Picker button failed'}
}
function Path-In-Picker([string]$Path){
 if(!(Split-Path -Parent $Path).Equals($run,[StringComparison]::OrdinalIgnoreCase)){throw 'HDR files must stay in the test directory'}
 $entry=$null
 foreach($id in @('1001','1148')){
  $entry=$picker.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.AndCondition]::new(
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$id),
   [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ClassNameProperty,'Edit')))
  if($entry){break}
 }
 if(!$entry -or $entry.Current.ProcessId -ne $review.Id){throw 'Picker filename ownership changed'}
 [CapyHdrPicker]::Type([IntPtr]$entry.Current.NativeWindowHandle,$Path);Picker-Button '1'
}
function Set-Text([string]$Id,[string]$Text){
 $control=Control $Id
 $pattern=$null
 if($control.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)){$pattern.SetValue($Text);return}
 $entry=$control.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Edit))
 if(!$entry -or $entry.Current.ProcessId -ne $review.Id){throw "No owned numeric editor: $Id"}
 $entry.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Text)
}
function Delivery([string]$Name,[string]$Format){
 Command 'export_document' 'File';Select-Choice 'export-format' $Format;Button 'Preview export'
 Wait-Until {(Model).windows_document.stage -eq 'preview'} 'HDR export preparation failed' 60
 Button 'Export…';Picker 'Save As';$path=Join-Path $run $Name;Path-In-Picker $path;Idle
 Wait-Until {Test-Path -LiteralPath $path} 'HDR export missing'
 (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
}
try {
 $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1';$env:CAPY_TEST_HDR='1'
 Remove-Item Env:CAPY_PRESENT_PROBE -ErrorAction SilentlyContinue
 $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $run 'stderr.log')
 $null=$review.Handle;Write-Output "HDR review $($review.Id), $Depth, $run"
 Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready} 'HDR app did not start' 60
 $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
 Command 'new_document' 'File';Set-Text 'document-width' '128';Set-Text 'document-height' '96'
 Select-Choice 'document-depth' $(if($Depth -eq 'F32'){'32-bit float HDR'}else{'16-bit float HDR'})
 Button 'Create';Idle
 Wait-Until {(Model).color_panel.hdr -and (Model).color_panel.document_depth -eq $Depth -and (Model).brush_ready -and (Model).windows_display.analysis.ready} 'HDR creation/analysis failed' 60
 if((Model).windows_display.format -ne 'Rgba16Float'){throw 'Expected native floating-point swap chain'}
 $display=(Model).windows_display
 Button 'Test SDR output';Wait-Until {(Model).windows_display.headroom -eq 1} 'Synthetic SDR fallback did not apply'
 $before=(Model).state.document_file|ConvertTo-Json -Compress
 Sdr;Set-Text 'sdr-exposure' '-1';Button 'Cancel';Idle
 if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $before){throw 'Cancelled SDR appearance edited the drawing'}
 Sdr;Set-Text 'sdr-exposure' '-1';Button 'Save appearance';Idle
 Sdr
 if((Model).windows_document.details.rendition.exposure -ne -1){throw 'Saved SDR appearance was lost'}
 Button 'Cancel';Idle
 Command 'undo' 'Edit';Sdr
 if((Model).windows_document.details.rendition.exposure -ne 0){throw 'SDR appearance undo failed'}
 Button 'Cancel';Idle;Command 'redo' 'Edit'
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 (Control 'color-readout').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x5D)
 Invoke 'edit-color-palettes';Select-Choice 'precise-color-model' 'Linear RGB'
 Set-Text 'precise-color-0' '1';Set-Text 'precise-color-1' '0.5';Set-Text 'precise-color-2' '0.25';Set-Text 'precise-color-3' '100'
 Set-Text 'precise-color-intensity' 'not a number';Invoke 'precise-color-apply'
 Wait-Until {!(Control 'precise-color-apply').Current.IsEnabled} 'Invalid HDR intensity was accepted'
 Set-Text 'precise-color-intensity' '2';Invoke 'precise-color-apply'
 Wait-Until {(Model).color_panel.intensity -eq 2 -and (Model).color_panel.definition.rgba[0] -gt 1} 'HDR numeric paint was not applied'
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 $revision=(Model).state.document_file.revision
 Button 'Test pen';Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).windows_display.analysis.ready} 'HDR painting analysis failed' 60
 $plain=Delivery 'SDR.png' 'PNG'
 $pq=Delivery 'HDR.png' 'HDR PNG · BT.2020 PQ'
 $exr=Delivery 'HDR.exr' 'OpenEXR · 32-bit float'
 Command 'undo' 'Edit'
 if((Delivery 'undo-paint.exr' 'OpenEXR · 32-bit float') -eq $exr){throw 'Float painting undo did not remove the stroke'}
 Command 'redo' 'Edit'
 if((Delivery 'redo-paint.exr' 'OpenEXR · 32-bit float') -ne $exr){throw 'Float painting redo did not restore exact export'}

 $file=(Model).state.document_file|ConvertTo-Json -Compress
 Button 'Test HDR output';Wait-Until {(Model).windows_display.headroom -eq 5 -and (Model).state.hdr_display_available} 'Synthetic HDR switch did not apply'
 Command 'preview_sdr';Wait-Until {(Model).state.preview_sdr} 'SDR preview did not enable'
 Command 'preview_sdr';Button 'Test SDR output';Wait-Until {!(Model).state.hdr_display_available} 'Synthetic return to SDR failed'
 if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $file){throw 'Display switching modified artwork/history'}
 if((Delivery 'SDR-after-switch.png' 'PNG') -ne $plain){throw 'Display switching changed SDR export'}
 Setup;Button 'Apply';Idle;Wait-Until {(Model).windows_proof.bytes -gt 0} 'HDR print proof failed' 60
 if((Delivery 'proof-on.png' 'PNG') -ne $plain){throw 'Proof changed SDR export'}
 Command 'soft_proof'
 Command 'save_document_as' 'File';Picker 'Save As';$master=Join-Path $run 'HDR 日本語.capy';Path-In-Picker $master;Idle
 $generation=(Model).windows_gpu_generation
 Button 'Test GPU loss'
 Wait-Until {(Model).windows_gpu_generation -gt $generation -and (Model).brush_ready -and (Model).windows_display.analysis.ready} 'HDR device recovery failed' 60
 if((Delivery 'recovered.exr' 'OpenEXR · 32-bit float') -ne $exr){throw 'GPU replacement changed float export'}
 Command 'open_document' 'File';Picker 'Open';Path-In-Picker $master;Idle
 Wait-Until {(Model).windows_display.analysis.ready} 'Reopened HDR analysis failed' 60
 if((Model).color_panel.document_depth -ne $Depth -or (Model).state.soft_proof){throw 'Reopen lost precision or retained proof viewing'}
 if((Delivery 'reopened.exr' 'OpenEXR · 32-bit float') -ne $exr){throw 'Reopen changed float export'}
 Sdr;if((Model).windows_document.details.rendition.exposure -ne -1){throw 'Reopen lost saved SDR appearance'};Button 'Cancel';Idle
 Command 'export_document' 'File';Button 'Cancel';Idle
 $import=Join-Path $run $(if($Depth -eq 'F32'){'HDR.exr'}else{'HDR.png'})
 Command 'open_document' 'File';Picker 'Open';Path-In-Picker $import;Idle
 Wait-Until {(Model).color_panel.document_depth -eq $Depth -and (Model).brush_ready -and (Model).windows_display.analysis.ready} 'Exported HDR photo did not reopen as HDR' 60
 & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'hdr.png') -ClientOnly *> (Join-Path $run 'hdr.json')
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $directory
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'HDR stderr requires inspection'}
 [pscustomobject]@{depth=$Depth;creation='passed';sdr_appearance_cancel_history='passed';painting='passed';painting_history='passed';numeric_hdr_color='passed';hdr_photo_open='passed';pq_exr_sdr_exports='passed';synthetic_display_switching='passed';proof_export_separation='passed';device_recovery='passed';save_reopen='passed';physical_display=$display;scope='Native UIA/D3D12 functional checks. Injected display reports do not qualify physical HDR, mixed-monitor behavior, or performance.'}|ConvertTo-Json -Depth 8|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{
 if($review -and !$review.HasExited){try{& (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'failure.png') -ClientOnly *> (Join-Path $run 'failure.json')}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
 foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ("Env:"+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name])}}
}
