param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][string]$StateFile)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -AssemblyName System.Windows.Forms,System.Drawing
Add-Type -Path (Join-Path $PSScriptRoot '../tests/HeldKeyWindow.cs') -ReferencedAssemblies @('System.dll','System.Core.dll','System.Windows.Forms.dll','System.Drawing.dll')
$review=Get-Process -Id $ProcessId
if($review.ProcessName -ne 'CapyCanvas'){throw 'Expected an owned CapyCanvas review'}
$CapyStateFile=$StateFile;$CapyTraceDirectory=Split-Path -Parent $StateFile
$native=@{window=$null}
Wait-Canvas {$native.window=Owned-DrawingWindow $review; $native.window -and (Input-Ready)} 'Isolated review did not become ready' 45
$root=$native.window.Root
function Key([int]$Code,[switch]$Ctrl,[switch]$Shift){
    $modifiers=[Collections.Generic.List[uint16]]::new()
    if($Ctrl){$modifiers.Add(0x11)};if($Shift){$modifiers.Add(0x10)}
    [CapyRowPointer]::Chord([uint32]$ProcessId,$modifiers.ToArray(),[uint16]$Code)
}
function Focus($Control){$Control.SetFocus();Wait-Until {$Control.Current.HasKeyboardFocus} 'Native control did not receive focus'}
function Click([string]$Id){
 $item=Control $Id;$b=$item.Current.BoundingRectangle
 [CapyRowPointer]::Down('mouse',[int]($b.X+$b.Width/2),[int]($b.Y+$b.Height/2));[CapyRowPointer]::Up()
 Wait-Until {$item.Current.HasKeyboardFocus} 'The clicked native button did not retain keyboard focus'
 $item
}
function Command([string]$Id){(Model).state.commands|Where-Object id -eq $Id}
function Value([string]$Id){((Model).state.tool_settings|Where-Object id -eq $Id).value}
function Tile([string]$Command){foreach($panel in (Model).panels){foreach($tile in $panel.tiles){if($tile.control.kind -eq 'command' -and $tile.control.command -eq $Command){return "tile-$($panel.id)-$($tile.id)"}}};throw "No toolbar tile for $Command"}
function Check-Undo([string]$Button){
 $item=Click $Button;$identity=$item.GetRuntimeId() -join ':'
 foreach($operation in @(@{key=0x5A;shift=$false;modified=$false},@{key=0x5A;shift=$true;modified=$true},@{key=0x5A;shift=$false;modified=$false},@{key=0x59;shift=$false;modified=$true})){
  $revision=(Model).state.document_file.revision
  Key $operation.key -Ctrl -Shift:$operation.shift
  Wait-Canvas {(Model).state.document_file.revision -gt $revision -and (Model).state.document_file.modified -eq $operation.modified} "Shortcut failed with focus on $Button"
  if(((Control $Button).GetRuntimeId() -join ':') -ne $identity -or !(Control $Button).Current.HasKeyboardFocus){throw 'Undo/Redo replaced the focused button or stole focus'}
 }
}
function Check-HeldFocus {
 [CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
 $window=$native.window.Handle;$canvas=Control 'drawing-canvas' -Arranged
 $baseline=Model
 if($baseline.state.layer_tools.tool -ne 'paint' -or $baseline.state.document_file.modified){throw 'Held-key focus acceptance requires a clean paint tool'}
 $preset=$baseline.state.brush.preset;$bounds=$canvas.Current.BoundingRectangle;$camera=$baseline.state.camera
 $x=[int]($bounds.X+($camera.work_area[0]+$camera.work_area[2]/2)*$bounds.Width/$camera.viewport[0])-24
 $y=[int]($bounds.Y+($camera.work_area[1]+$camera.work_area[3]/2)*$bounds.Height/$camera.viewport[1])
 if(!$bounds.Contains([double]$x,[double]$y) -or !$bounds.Contains([double]($x+48),[double]$y)){throw 'Held-key stroke exceeds the owned canvas'}
 $altHeld=$false;$penHeld=$false
 try{
  [CapyHeldKeyWindow]::Start($window,[uint32]$ProcessId,$env:CAPY_PRIVATE_DESKTOP)
  [CapyRowPointer]::SetForegroundWindow($window)|Out-Null;$canvas.SetFocus()
  Wait-Until {[CapyRowPointer]::GetForegroundWindow() -eq $window -and $canvas.Current.HasKeyboardFocus} 'Held-key canvas did not receive focus'
  [CapyRowPointer]::Hover($x,$y)
  [CapyRowPointer]::Hold(0x12,$true);$altHeld=$true
  [CapyRowPointer]::Hover(($x+1),$y)
  Wait-Until {(Model).state.layer_tools.tool -in @('pick_visible','pick_layer')} 'Alt did not start the eyedropper hold'
  [CapyHeldKeyWindow]::Check($window,[uint32]$ProcessId,$env:CAPY_PRIVATE_DESKTOP)
  [CapyRowPointer]::SetForegroundWindow([CapyHeldKeyWindow]::Window)|Out-Null
  Wait-Until {[CapyRowPointer]::GetForegroundWindow() -eq [CapyHeldKeyWindow]::Window} 'Owned-root peer did not receive focus'
  [CapyHeldKeyWindow]::Focus()
  Wait-Until {[System.Windows.Automation.AutomationElement]::FocusedElement.Current.ProcessId -eq $PID} 'Owned-root peer did not own keyboard focus'
  Wait-Until {$model=Model;$model.state.layer_tools.tool -eq 'paint' -and !@($model.state.commands|Where-Object {$_.id -eq 'eyedropper' -and $_.selected}).Count} 'Deactivation left the eyedropper hold active before Alt-up'
  [CapyHeldKeyWindow]::Check($window,[uint32]$ProcessId,$env:CAPY_PRIVATE_DESKTOP)
  [CapyRowPointer]::Hold(0x12,$false);$altHeld=$false
  [CapyRowPointer]::SetForegroundWindow($window)|Out-Null
  Wait-Until {[CapyRowPointer]::GetForegroundWindow() -eq $window} 'Held-key canvas did not reactivate'
  Wait-Until {$model=Model;$model.state.layer_tools.tool -eq 'paint' -and $model.state.brush.preset -eq $preset -and !@($model.state.commands|Where-Object {$_.id -eq 'eyedropper' -and $_.selected}).Count} 'Deactivation left the eyedropper hold active'
  $revision=(Model).state.document_file.revision
  [CapyRowPointer]::Down('pen',$x,$y);$penHeld=$true
  foreach($step in 1..8){[CapyRowPointer]::Move(($x+$step*6),$y);Start-Sleep -Milliseconds 8}
  [CapyRowPointer]::Up();$penHeld=$false
  Wait-Canvas {$model=Model;$model.state.document_file.revision -gt $revision -and $model.state.document_file.modified -and $model.state.layer_tools.tool -eq 'paint'} 'Pen after focus return did not paint with the restored tool'
  [CapyRowPointer]::Chord([uint32]$ProcessId,[uint16[]]@(0x11),0x5A)
  Wait-Canvas {!(Model).state.document_file.modified} 'Undo did not remove the focus-return pen stroke'
 }finally{
  try{
   [CapyRowPointer]::VerifyPrivateDesktop($env:CAPY_PRIVATE_DESKTOP)
   if($altHeld){[CapyRowPointer]::Hold(0x12,$false)}
   if($penHeld){[CapyRowPointer]::SetForegroundWindow($window)|Out-Null;Wait-Until {[CapyRowPointer]::GetForegroundWindow() -eq $window} 'Pen cleanup did not reacquire the owned window';[CapyRowPointer]::Up()}
  }finally{[CapyHeldKeyWindow]::Stop($env:CAPY_PRIVATE_DESKTOP)}
 }
}

[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
[CapyRowPointer]::SetForegroundWindow($native.window.Handle)|Out-Null
[CapyRowPointer]::Initialize([uint32]$ProcessId)
try{
 if((Model).state.document_file.modified){throw 'Use a clean isolated document for shortcut acceptance'}
 Invoke 'Test stroke' -Name;Wait-Canvas {(Model).state.document_file.modified} 'Test stroke did not finish'
 Check-Undo 'tool-group-0'
 $pen=Tile 'pen';Check-Undo $pen
 foreach($pair in @(@{key=0x42;tool='brush'},@{key=0x45;tool='eraser'},@{key=0x50;tool='pen'})){
  Focus (Control $pen);Key $pair.key
  Wait-Until {(Command $pair.tool).selected} 'Tool shortcut failed with toolbar button focus'
 }
 $revision=(Model).state.document_file.revision
 $field=Control 'tool-setting-flow';Focus $field
 $field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue('71')
 Key 0x5A -Ctrl;Key 0x45
 Wait-Until {$field.GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -match 'e'} 'Native text field did not receive the letter key'
 if((Model).state.document_file.revision -ne $revision -or !(Command 'pen').selected){throw 'Text editing invoked a canvas shortcut'}
 Key 0x1B
 $slider=Control 'tool-setting-flow-slider';Focus $slider
 $flow=Value 'flow';Key $(if($flow -gt .5){0x25}else{0x27})
 Wait-Until {[Math]::Abs((Value 'flow')-$flow) -gt .000001} 'Native slider arrow did not change its value'
 Key 0x5A -Ctrl;Key 0x45;Start-Sleep -Milliseconds 200
 if((Model).state.document_file.revision -ne $revision -or !(Command 'pen').selected){throw 'Native slider keys invoked a canvas shortcut'}
 foreach($activation in @(0x20,0x0D)){
  Focus (Control $pen);Key 0x45
  Wait-Until {(Command 'eraser').selected} 'Activation check did not select Eraser'
  Focus (Control $pen);Key $activation
  Wait-Until {(Command 'pen').selected} 'Space/Enter did not activate the focused native button'
 }
 $zen=(Command 'zen_mode').selected;$focused=[System.Windows.Automation.AutomationElement]::FocusedElement.GetRuntimeId() -join ':'
 Key 0x09
 Wait-Until {([System.Windows.Automation.AutomationElement]::FocusedElement.GetRuntimeId() -join ':') -ne $focused} 'Tab did not move native focus'
 if((Command 'zen_mode').selected -ne $zen){throw 'Native focus navigation toggled Zen'}
 & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Edit'
 Key 0x45;Start-Sleep -Milliseconds 200
 if((Model).state.document_file.revision -ne $revision -or !(Command 'pen').selected){throw 'Open menu keys reached the canvas'}
 # Compact menus have a submenu and an outer flyout; Escape unwinds each level.
 for($level=0;$level -lt 3;$level++){
  Key 0x1B;Start-Sleep -Milliseconds 150
  if($root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0){break}
 }
 Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Escape did not close the native menu'
 Key 0x5A -Ctrl
 Wait-Canvas {!(Model).state.document_file.modified} 'Undo after closing the native menu did not restore the clean drawing'
 Check-HeldFocus
 [CapyRowPointer]::Verify()
 @{button_shortcuts='Tool Set and toolbar Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y, retained native focus';tool_shortcuts='B/E/P from toolbar focus';native_controls='text editing, slider arrows, Space/Enter activation, Tab traversal, menu Escape';drawing='seed and focus-return strokes, clean after Undo';focus_loss='owned-root peer, Alt released away, restored pen paints';scope='owned native mouse and keyboard input; physical pen and performance remain separate'}|ConvertTo-Json
}finally{[CapyRowPointer]::Dispose()}
