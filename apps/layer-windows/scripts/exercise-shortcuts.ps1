param([Parameter(Mandatory)][int]$ProcessId,[Parameter(Mandatory)][string]$StateFile)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -TypeDefinition @"
using System;using System.Collections.Generic;using System.Runtime.InteropServices;
public static class CapyShortcutKeys {
 [StructLayout(LayoutKind.Sequential)] public struct Keyboard {public ushort key,scan;public uint flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit,Size=40)] public struct Input {[FieldOffset(0)]public uint type;[FieldOffset(8)]public Keyboard keyboard;}
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint process);
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] inputs,int size);
 public static void Key(uint process,ushort key,bool control,bool shift){
  uint owner;GetWindowThreadProcessId(GetForegroundWindow(),out owner);if(owner!=process)throw new Exception("Review does not own keyboard focus; no keys sent.");
  var keys=new List<ushort>();if(control)keys.Add(0x11);if(shift)keys.Add(0x10);keys.Add(key);
  var inputs=new List<Input>();foreach(var k in keys)inputs.Add(new Input{type=1,keyboard=new Keyboard{key=k}});
  keys.Reverse();foreach(var k in keys)inputs.Add(new Input{type=1,keyboard=new Keyboard{key=k,flags=2}});
  if(SendInput((uint)inputs.Count,inputs.ToArray(),40)!=(uint)inputs.Count){
   var releases=inputs.GetRange(keys.Count,keys.Count);SendInput((uint)releases.Count,releases.ToArray(),40);throw new Exception("Windows rejected shortcut input.");
  }
 }
}
"@
$app=Get-Process -Id $ProcessId
if($app.ProcessName -ne 'CapyCanvas'){throw 'Expected an owned CapyCanvas review'}
function Model {try{$s=Get-Content -LiteralPath $StateFile -Raw|ConvertFrom-Json;if($s.process_id -eq $ProcessId -and $s.model.windows_isolated_settings){$s.model}}catch{}}
function Wait-Until([scriptblock]$Test,[string]$Message,[int]$Seconds=8){
 $w=[Diagnostics.Stopwatch]::StartNew();do{if(& $Test){return};$app.Refresh();if($app.HasExited){throw 'Owned shortcut review exited'};Start-Sleep -Milliseconds 75}while($w.Elapsed.TotalSeconds -lt $Seconds);throw $Message
}
Wait-Until {$app.Refresh();$app.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_filter_load.phase -eq 'ready'} 'Isolated review did not become ready' 45
$root=[System.Windows.Automation.AutomationElement]::FromHandle($app.MainWindowHandle)
function Find([string]$Value,[switch]$Name){
 $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
 $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new($property,$Value))
}
function Control([string]$Value,[switch]$Name){$hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name;$null -ne $hit.item} "Missing control: $Value";$hit.item}
function Invoke([string]$Name){(Control $Name -Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Key([int]$Code,[switch]$Ctrl,[switch]$Shift){[CapyShortcutKeys]::Key([uint32]$ProcessId,[uint16]$Code,[bool]$Ctrl,[bool]$Shift)}
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
  Wait-Until {(Model).state.document_file.revision -gt $revision -and (Model).state.document_file.modified -eq $operation.modified} "Shortcut failed with focus on $Button"
  if(((Control $Button).GetRuntimeId() -join ':') -ne $identity -or !(Control $Button).Current.HasKeyboardFocus){throw 'Undo/Redo replaced the focused button or stole focus'}
 }
}
[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))|Out-Null
[CapyRowPointer]::SetForegroundWindow($app.MainWindowHandle)|Out-Null
[CapyRowPointer]::Initialize([uint32]$ProcessId)
try{
 if((Model).state.document_file.modified){throw 'Use a clean isolated document for shortcut acceptance'}
 Invoke 'Test stroke';Wait-Until {(Model).state.document_file.modified} 'Test stroke did not finish'
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
  $step=Control 'tool-setting-flow-increase'
  if(!$step.Current.IsEnabled){$step=Control 'tool-setting-flow-decrease'}
  Focus $step;$flow=Value 'flow';Key $activation
  Wait-Until {[Math]::Abs((Value 'flow')-$flow) -gt .000001} 'Space/Enter did not activate the focused native button'
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
 Wait-Until {!(Model).state.document_file.modified} 'Undo after closing the native menu did not restore the clean drawing'
 [CapyRowPointer]::Verify()
 @{button_shortcuts='Tool Set and toolbar Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y, retained native focus';tool_shortcuts='B/E/P from toolbar focus';native_controls='text editing, slider arrows, Space/Enter activation, Tab traversal, menu Escape';drawing='one seed stroke, clean after final Undo';scope='owned native mouse and keyboard input; physical pen and performance remain separate'}|ConvertTo-Json
}finally{[CapyRowPointer]::Dispose()}
