param([Parameter(Mandatory)][string]$Executable,[ValidateSet("F16","F32")][string]$Depth="F16")
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes,System.Drawing
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
function Assert-CanvasInk([string]$Label){
 $capture=Join-Path $run ($Label+'-canvas.png')
 & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output $capture -ClientOnly *> (Join-Path $run ($Label+'-canvas.json'))
 $bitmap=[Drawing.Bitmap]::new($capture)
 try {
  # This fixture uses an 1800x1300 window; these bounds lie inside the canvas,
  # away from panels, title controls, overlays and the pointer. Orange HDR ink
  # must survive presentation as well as the independently checked file exports.
  $ink=0
  for($y=[int]($bitmap.Height*.2);$y -lt [int]($bitmap.Height*.8);$y+=2){
   for($x=[int]($bitmap.Width*.28);$x -lt [int]($bitmap.Width*.7);$x+=2){
    $p=$bitmap.GetPixel($x,$y)
    if($p.R -gt $p.G+12 -and $p.G -gt $p.B+8){$ink++}
   }
  }
  if($ink -lt 20){throw "Visible HDR ink is missing after $Label ($ink samples)"}
 }finally{$bitmap.Dispose()}
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
 $hit=@{item=$null};Wait-Until {
  $hit.item=Find $Value -Name:$Name -Type $Type
  if(!$hit.item -and !$Name -and $Value.StartsWith('proof-')){
   $entry=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,$Value))
   if($entry){$scroll=$null;if($entry.TryGetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern,[ref]$scroll)){$scroll.ScrollIntoView()}else{$entry.SetFocus()}}
  }
  $null -ne $hit.item
 } "Missing HDR control: $Value";$hit.item
}
function Open-Drawings {
 $selector=Control 'drawing-selector'
 Wait-Until {$selector.Current.ItemStatus -eq 'Closed'} 'Previous drawing popup is still closing'
 Invoke 'drawing-selector'
 Wait-Until {$selector.Current.ItemStatus -eq 'Open' -and (Find 'drawing-list')} 'Drawing selector did not open'
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
function Sdr { ProofPanel;$null=Control 'proof-panel-exposure' }
function ProofPanel {
 if(Find 'panel-tab-proof'){Invoke 'panel-tab-proof'}
 $null=Control 'proof-panel-mode'
}
function Setup {
 ProofPanel;Invoke 'proof-panel-setup'
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
 Command 'export_document' 'File';Select-Choice 'export-format' $Format
 if($Format.StartsWith('HDR JPEG')){Select-Choice 'export-background' 'White'}
 Button 'Preview export'
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
 Button 'Test pen';Wait-Until {(Model).state.document_file.modified} 'Initial drawing did not become dirty' 60
 Command 'new_document' 'File';Set-Text 'document-width' '128';Set-Text 'document-height' '96'
 Select-Choice 'document-depth' $(if($Depth -eq 'F32'){'32-bit float HDR'}else{'16-bit float HDR'})
 Button 'Create';Idle
 Wait-Until {(Model).color_panel.hdr -and (Model).color_panel.document_depth -eq $Depth -and (Model).brush_ready -and (Model).windows_display.analysis.ready} 'HDR creation/analysis failed' 60
 if((Model).windows_display.format -ne 'Rgba16Float'){throw 'Expected native floating-point swap chain'}
 if(@((Model).windows_tabs.tabs).Count -ne 2 -or !(Model).windows_tabs.tabs[0].modified){throw 'New did not retain the dirty first drawing'}
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1800 -Height 1300
 Wait-Until {(Find 'drawing-tab-1') -and (Find 'drawing-tab-2') -and (Model).windows_tabs.available} 'Full native drawing strip did not appear'
 $tabWidget=(Control 'drawing-tab-1').GetRuntimeId() -join ':'
 foreach($device in @('mouse','touch','pen')){
  Write-Output "Native contact: $device at line $($MyInvocation.ScriptLineNumber)"
  $beforeOrder=((Model).windows_tabs.tabs.id -join ',')
  $from=(Control 'drawing-tab-1').Current.BoundingRectangle;$to=(Control 'drawing-tab-2').Current.BoundingRectangle
  $dpi=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));[CapyRowPointer]::Initialize([uint32]$review.Id)
  try {
   [CapyRowPointer]::Down($device,[int]($from.X+$from.Width/2),[int]($from.Y+$from.Height/2))
   [CapyRowPointer]::Move([int]($to.X+$to.Width-8),[int]($to.Y+$to.Height/2))
   Wait-Until {(Control 'drawing-tabs').Current.ItemStatus -eq 'Dragging'} 'Tab movement did not capture'
   $slide=@{value=$null}
   Wait-Until {$slide.value=(Control 'drawing-tab-slide').Current.ItemStatus|ConvertFrom-Json;$slide.value.attached -and $slide.value.offsets[1] -lt 0 -and $slide.value.offsets[0] -eq 0} 'Held tab did not slide its neighbor'
   if($device -eq 'mouse'){[CapyRowPointer]::Key([uint32]$review.Id,0x1B);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
   Wait-Until {(Control 'drawing-tabs').Current.ItemStatus -eq 'Ready' -and (Control 'drawing-tab-1').Current.BoundingRectangle.Width -gt 0} 'Tab cancellation retained capture'
   if(((Model).windows_tabs.tabs.id -join ',') -ne $beforeOrder){throw 'Tab cancellation changed order'}
   [CapyRowPointer]::Down($device,[int]($from.X+$from.Width/2),[int]($from.Y+$from.Height/2))
   [CapyRowPointer]::Move([int]($to.X+$to.Width-8),[int]($to.Y+$to.Height/2))
   [CapyRowPointer]::Move([int]($to.X+$to.Width-8),[int]($to.Y+$to.Height*3))
   Wait-Until {$slide.value=(Control 'drawing-tab-slide').Current.ItemStatus|ConvertFrom-Json;!$slide.value.attached -and $slide.value.offsets[1] -eq 0} 'Leaving the strip did not detach the tab'
   [CapyRowPointer]::Up()
   Wait-Until {(Control 'drawing-tabs').Current.ItemStatus -eq 'Ready'} 'Detached release retained capture'
   Start-Sleep -Milliseconds 300
   if(((Model).windows_tabs.tabs.id -join ',') -ne $beforeOrder){throw 'Releasing outside the strip changed order'}
   [CapyRowPointer]::Down($device,[int]($from.X+$from.Width/2),[int]($from.Y+$from.Height/2))
   [CapyRowPointer]::Move([int]($to.X+$to.Width-8),[int]($to.Y+$to.Height/2));[CapyRowPointer]::Up()
  }finally{[CapyRowPointer]::Dispose();[CapyRowPointer]::SetThreadDpiAwarenessContext($dpi)|Out-Null}
  Wait-Until {((Model).windows_tabs.tabs.id -join ',') -ne $beforeOrder} 'A tab did not reorder immediately after slop'
  if(((Control 'drawing-tab-1').GetRuntimeId() -join ':') -ne $tabWidget){throw 'Reorder rebuilt the native tab'}
  foreach($history in @('undo','redo','undo')){
   (Control 'drawing-tab-1').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x5D);Invoke ('drawing-order-'+$history)
   $expected=if($history -eq 'redo'){'2,1'}else{'1,2'}
   Wait-Until {((Model).windows_tabs.tabs.id -join ',') -eq $expected} 'Tab order history failed'
   Wait-Until {!(Find ('drawing-order-'+$history))} 'Tab order menu did not close'
  }
 }
 Invoke 'drawing-close-1';Button 'Cancel';Idle
 Wait-Until {(Model).windows_tabs.selected -eq 1 -and (Model).windows_tabs.available} 'Closing an inactive dirty tab did not activate and preserve it on Cancel' 60
 if(@((Model).windows_tabs.tabs).Count -ne 2){throw 'Close cancellation removed a drawing'}
 Command 'undo' 'Edit';Wait-Until {!(Model).state.document_file.modified} 'First drawing lost independent undo history'
 Command 'redo' 'Edit';Wait-Until {(Model).state.document_file.modified} 'First drawing lost independent redo history'
 Invoke 'drawing-tab-2';Wait-Until {(Model).windows_tabs.selected -eq 2 -and (Model).brush_ready -and (Model).color_panel.document_depth -eq $Depth -and (Model).windows_display.analysis.ready} 'Returning to HDR tab failed' 60
 if((Model).windows_tabs.parked_renderers -ne 0){throw 'Inactive drawing retained a renderer'}

 # Three drawings force a compact selector at this DPI; two still fit the strip.
 Command 'new_document' 'File';Set-Text 'document-width' '32';Set-Text 'document-height' '24';Button 'Create';Idle
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq 3 -and (Model).windows_tabs.available} 'Third drawing did not become available' 60
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 900 -Height 1100
 Wait-Until {(Find 'drawing-selector')} 'Compact drawing selector did not appear'
 foreach($device in @('mouse','touch','pen')){
  Write-Output "Native contact: $device at line $($MyInvocation.ScriptLineNumber)"
  Open-Drawings
  $grip=(Control 'drawing-grip-1').Current.BoundingRectangle;$target=(Control 'drawing-row-2').Current.BoundingRectangle
  $dpi=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));[CapyRowPointer]::Initialize([uint32]$review.Id)
  try {
   [CapyRowPointer]::Down($device,[int]($grip.X+$grip.Width/2),[int]($grip.Y+$grip.Height/2))
   [CapyRowPointer]::Move([int]($target.X+$target.Width/2),[int]($target.Y+$target.Height-4));[CapyRowPointer]::Up()
  }finally{[CapyRowPointer]::Dispose();[CapyRowPointer]::SetThreadDpiAwarenessContext($dpi)|Out-Null}
  Wait-Until {((Model).windows_tabs.tabs.id -join ',') -eq '2,1,3'} 'Selector grip did not reorder immediately'
  (Control 'drawing-row-1').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x5D);Invoke 'drawing-order-undo'
  Wait-Until {((Model).windows_tabs.tabs.id -join ',') -eq '1,2,3'} 'Selector order undo failed'
  Wait-Until {!(Find 'drawing-order-undo')} 'Tab order menu did not close'
  (Control 'drawing-row-1').SetFocus()
  [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
  Wait-Until {(Control 'drawing-selector').Current.ItemStatus -eq 'Closed' -and !(Find 'drawing-list')} 'Drawing selector did not dismiss'
 }
 Open-Drawings;(Control 'drawing-row-1').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {(Model).windows_tabs.selected -eq 1 -and (Model).windows_tabs.available} 'Selector keyboard activation failed' 60
 Open-Drawings;(Control 'drawing-row-2').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {(Model).windows_tabs.selected -eq 2 -and (Model).windows_tabs.available} 'Selector did not restore the HDR drawing' 60
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Resize -Width 1800 -Height 1300
 Wait-Until {(Find 'drawing-close-3')} 'Full strip did not return'
 Invoke 'drawing-close-3'
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq 2 -and (Model).windows_tabs.selected -eq 2 -and (Model).windows_tabs.available} 'Closing the clean selector fixture drawing failed' 60
 $display=(Model).windows_display
 Button 'Test SDR output';Wait-Until {(Model).windows_display.headroom -eq 1} 'Synthetic SDR fallback did not apply'
 $before=(Model).state.document_file|ConvertTo-Json -Compress
 Sdr
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 (Control 'proof-panel-exposure').SetFocus();Set-Text 'proof-panel-exposure' '-25';[CapyRowPointer]::Key([uint32]$review.Id,0x1B);Idle
 if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $before){throw 'Cancelled SDR field edited the drawing'}
 (Control 'proof-panel-exposure').SetFocus();Set-Text 'proof-panel-exposure' '-25';[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {(Model).windows_proof_form.rendition.exposure -eq -1} 'Saved SDR appearance was lost'
 Command 'undo' 'Edit';Wait-Until {(Model).windows_proof_form.rendition.exposure -eq 0} 'SDR appearance undo failed'
 Command 'redo' 'Edit';Wait-Until {(Model).windows_proof_form.rendition.exposure -eq -1} 'SDR appearance redo failed'
 Invoke 'panel-tab-color'
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 (Control 'color-readout').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x5D)
 Invoke 'edit-color';Select-Choice 'precise-color-model' 'Linear RGB'
 Set-Text 'precise-color-0' '1';Set-Text 'precise-color-1' '0.5';Set-Text 'precise-color-2' '0.25';Set-Text 'precise-color-3' '100'
 Set-Text 'precise-color-intensity' 'not a number';Invoke 'precise-color-apply'
 Wait-Until {!(Control 'precise-color-apply').Current.IsEnabled} 'Invalid HDR intensity was accepted'
 Set-Text 'precise-color-intensity' '2';Invoke 'precise-color-apply'
 Wait-Until {(Model).color_panel.intensity -eq 2 -and (Model).color_panel.definition.rgba[0] -gt 1} 'HDR numeric paint was not applied'
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 $revision=(Model).state.document_file.revision
 Button 'Test pen';Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).windows_display.analysis.ready} 'HDR painting analysis failed' 60
 # Exercise the retained native Proof controls through the shared Window menu.
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
 $windowMenu=(Model).application_menus|Where-Object id -eq 'window'
 $proofEntry=$null
 foreach($section in $windowMenu.model.sections){foreach($item in $section){if($item.action.action.type -eq 'set_panel_visible' -and $item.action.action.panel -eq 'proof'){$proofEntry=$item}}}
 if(!$proofEntry){throw 'Windows has no shared Proof panel menu'}
 if($proofEntry.selected){
  [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
  Invoke 'panel-tab-proof'
 } else {
  $proofMenuItem=Control $proofEntry.label -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
  $previousDpi=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
  [CapyRowPointer]::Initialize([uint32]$review.Id)
  try {
   $box=$proofMenuItem.Current.BoundingRectangle
   [CapyRowPointer]::Down('mouse',[int]($box.X+$box.Width/2),[int]($box.Y+$box.Height/2));[CapyRowPointer]::Up()
  } finally {[CapyRowPointer]::Dispose();[CapyRowPointer]::SetThreadDpiAwarenessContext($previousDpi)|Out-Null}
 }
 $null=Control 'proof-panel-exposure'
 $field=Control 'proof-panel-exposure';$fieldId=$field.GetRuntimeId() -join ':'
 [CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)|Out-Null
 $field.SetFocus();Set-Text 'proof-panel-exposure' 'invalid';[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 if((Model).windows_proof_form.rendition.exposure -ne -1){throw 'Invalid Proof field changed the recipe'}
 [CapyRowPointer]::Key([uint32]$review.Id,0x1B)
 $field.SetFocus();Set-Text 'proof-panel-exposure' '-10';[CapyRowPointer]::Key([uint32]$review.Id,0x0D)
 Wait-Until {[Math]::Abs((Model).windows_proof_form.rendition.exposure+0.4) -lt 0.0001} 'Proof numeric commit did not apply'
 if(((Control 'proof-panel-exposure').GetRuntimeId() -join ':') -ne $fieldId){throw 'Proof value update rebuilt its native editor'}
 Command 'undo' 'Edit';Wait-Until {(Model).windows_proof_form.rendition.exposure -eq -1} 'Proof field Undo failed'
 Command 'redo' 'Edit';Wait-Until {[Math]::Abs((Model).windows_proof_form.rendition.exposure+0.4) -lt 0.0001} 'Proof field Redo failed'
 Command 'undo' 'Edit';Wait-Until {(Model).windows_proof_form.rendition.exposure -eq -1} 'Proof field final Undo failed'
 $proofHistory=(Model).state.document_file|ConvertTo-Json -Compress
 Select-Choice 'proof-panel-mode' 'SDR';Wait-Until {(Model).state.preview_sdr} 'Proof panel SDR mode failed'
 Select-Choice 'proof-panel-mode' 'Off';Wait-Until {!(Model).state.preview_sdr} 'Proof panel Off failed'
 if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $proofHistory){throw 'Proof panel modes edited history'}
 # Real native capture with injected mouse/touch/pen; each edit has one history step.
 $dialBefore=(Model).windows_proof_form.rendition|ConvertTo-Json -Compress
 foreach($device in @('mouse','touch','pen')){
  Write-Output "Native contact: $device at line $($MyInvocation.ScriptLineNumber)"
  $dial=Control 'proof-dial';$dial.SetFocus();Start-Sleep -Milliseconds 150;$box=$dial.Current.BoundingRectangle
  $x=[int]($box.X+$box.Width/2);$y=[int]($box.Y+$box.Height/2)
  $dpi=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));[CapyRowPointer]::Initialize([uint32]$review.Id)
  try {
   [CapyRowPointer]::Down($device,$x,$y);[CapyRowPointer]::Move($x+25,$y-15)
   Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -ne $dialBefore} 'Dial drag did not preview'
   if($device -eq 'mouse'){[CapyRowPointer]::Key([uint32]$review.Id,0x1B);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}
   Wait-Until {$m=Model;!$m.state.sdr_appearance_preview -and (($m.windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialBefore)} 'Dial cancellation did not restore the recipe'
   [CapyRowPointer]::Down($device,$x,$y);[CapyRowPointer]::Move($x+25,$y-15);[CapyRowPointer]::Up()
   Wait-Until {$m=Model;!$m.state.sdr_appearance_preview -and (($m.windows_proof_form.rendition|ConvertTo-Json -Compress) -ne $dialBefore)} 'Dial release did not commit'
  } finally {[CapyRowPointer]::Dispose();[CapyRowPointer]::SetThreadDpiAwarenessContext($dpi)|Out-Null}
  $dialAfter=(Model).windows_proof_form.rendition|ConvertTo-Json -Compress
  Command 'undo' 'Edit';Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialBefore} 'Dial did not undo in one step'
  Command 'redo' 'Edit';Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialAfter} 'Dial redo failed'
  Command 'undo' 'Edit';Wait-Until {((Model).windows_proof_form.rendition|ConvertTo-Json -Compress) -eq $dialBefore} 'Dial final undo failed'
 }
 $plain=Delivery 'SDR.png' 'PNG'
 $pq=Delivery 'HDR.png' 'HDR PNG · BT.2020 PQ'
 $exr=Delivery 'HDR.exr' 'OpenEXR · 32-bit float'
 $jpeg=Delivery 'gain-map.jpg' 'HDR JPEG · gain map'
 $avif=Delivery 'gain-map.avif' 'HDR AVIF · gain map'
 $null=Delivery 'gain-map-clipped.jpg' 'HDR JPEG · clip to gain-map range'
 $null=Delivery 'gain-map-clipped.avif' 'HDR AVIF · clip to gain-map range'
 Command 'undo' 'Edit'
 if((Delivery 'undo-paint.exr' 'OpenEXR · 32-bit float') -eq $exr){throw 'Float painting undo did not remove the stroke'}
 Command 'redo' 'Edit'
 if((Delivery 'redo-paint.exr' 'OpenEXR · 32-bit float') -ne $exr){throw 'Float painting redo did not restore exact export'}

 $file=(Model).state.document_file|ConvertTo-Json -Compress
 Button 'Test HDR output';Wait-Until {(Model).windows_display.headroom -eq 5 -and (Model).state.hdr_display_available} 'Synthetic HDR switch did not apply'
 Select-Choice 'proof-panel-mode' 'SDR';Wait-Until {(Model).state.preview_sdr} 'SDR preview did not enable'
 Select-Choice 'proof-panel-mode' 'Off';Button 'Test SDR output';Wait-Until {!(Model).state.hdr_display_available} 'Synthetic return to SDR failed'
 if(((Model).state.document_file|ConvertTo-Json -Compress) -ne $file){throw 'Display switching modified artwork/history'}
 if((Delivery 'SDR-after-switch.png' 'PNG') -ne $plain){throw 'Display switching changed SDR export'}
 Setup;Button 'Apply';Idle;Wait-Until {(Model).windows_proof.bytes -gt 0} 'HDR print proof failed' 60
 if((Delivery 'proof-on.png' 'PNG') -ne $plain){throw 'Proof changed SDR export'}
 Command 'soft_proof'
 Command 'save_document_as' 'File';Picker 'Save As';$master=Join-Path $run 'HDR 日本語.capy';Path-In-Picker $master;Idle
 Write-Output 'HDR device recovery and master reopen'
 $generation=(Model).windows_gpu_generation
 Button 'Test GPU loss'
 Wait-Until {(Model).windows_gpu_generation -gt $generation -and (Model).brush_ready -and (Model).windows_display.analysis.ready} 'HDR device recovery failed' 60
 if((Delivery 'recovered.exr' 'OpenEXR · 32-bit float') -ne $exr){throw 'GPU replacement changed float export'}
 Command 'open_document' 'File';Picker 'Open';Path-In-Picker $master;Idle
 Wait-Until {(Model).windows_display.analysis.ready} 'Reopened HDR analysis failed' 60
 if((Model).color_panel.document_depth -ne $Depth -or (Model).state.soft_proof){throw 'Reopen lost precision or retained proof viewing'}
 if((Delivery 'reopened.exr' 'OpenEXR · 32-bit float') -ne $exr){throw 'Reopen changed float export'}
 Assert-CanvasInk 'reopened-master'
 Sdr;if((Model).windows_proof_form.rendition.exposure -ne -1){throw 'Reopen lost saved SDR appearance'};Idle
 Command 'export_document' 'File';Button 'Cancel';Idle
 Write-Output 'HDR photo reopen and presentation'
 $import=Join-Path $run $(if($Depth -eq 'F32'){'HDR.exr'}else{'HDR.png'})
 Command 'open_document' 'File';Picker 'Open';Path-In-Picker $import;Idle
 Wait-Until {(Model).color_panel.document_depth -eq $Depth -and (Model).brush_ready -and (Model).windows_display.analysis.ready} 'Exported HDR photo did not reopen as HDR' 60
 foreach($name in @('gain-map.jpg','gain-map.avif')){
  Command 'open_document' 'File';Picker 'Open';Path-In-Picker (Join-Path $run $name);Idle
  Wait-Until {(Model).color_panel.hdr -and (Model).brush_ready -and (Model).windows_display.analysis.ready} 'Gain-map photo did not reopen as HDR' 60
  Assert-CanvasInk $name
 }
 & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'hdr.png') -ClientOnly *> (Join-Path $run 'hdr.json')
 $count=@((Model).windows_tabs.tabs).Count
 $review.CloseMainWindow()|Out-Null;Button 'Cancel';Idle
 if(@((Model).windows_tabs.tabs).Count -ne $count){throw 'Window close cancellation lost drawings'}
 Command 'close_document' 'File';Button 'Discard Changes';Idle
 Wait-Until {@((Model).windows_tabs.tabs).Count -eq $count-1 -and (Model).windows_tabs.available} 'Close drawing did not retain the other tabs' 60
 & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved -StateDirectory $directory
 if((Get-Item (Join-Path $run 'stderr.log')).Length){throw 'HDR stderr requires inspection'}
 [pscustomobject]@{depth=$Depth;creation='passed';sdr_appearance_cancel_history='passed';painting='passed';painting_history='passed';numeric_hdr_color='passed';hdr_photo_open='passed';reopened_canvas_presentation='passed';pq_exr_sdr_exports='passed';gainmap_exports_and_open='passed';proof_panel_numeric_modes_history='passed';proof_dial_mouse_touch_pen_cancel_history='passed';drawing_tabs_reorder_history_close_cancel='passed';synthetic_display_switching='passed';proof_export_separation='passed';device_recovery='passed';save_reopen='passed';physical_display=$display;scope='Native UIA/D3D12 functional checks. Injected display reports do not qualify physical HDR, mixed-monitor behavior, or performance.'}|ConvertTo-Json -Depth 8|Tee-Object -FilePath (Join-Path $run 'results.json')
}catch{
 if($review -and !$review.HasExited){try{& (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run 'failure.png') -ClientOnly *> (Join-Path $run 'failure.json')}catch{}}
 [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
 foreach($name in $names){if($null -eq $previous[$name]){Remove-Item -LiteralPath ("Env:"+$name) -ErrorAction SilentlyContinue}else{[Environment]::SetEnvironmentVariable($name,$previous[$name])}}
}
