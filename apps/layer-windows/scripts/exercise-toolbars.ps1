param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyToolbarKeys {
 [StructLayout(LayoutKind.Sequential)] public struct Keyboard {public ushort key,scan;public uint flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit,Size=40)] public struct Input {[FieldOffset(0)]public uint type;[FieldOffset(8)]public Keyboard keyboard;}
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
 public static void Context(uint process) {
   uint owner;GetWindowThreadProcessId(GetForegroundWindow(),out owner);
   if(owner!=process)throw new Exception("Review does not own keyboard focus; no keys sent.");
   var inputs=new[]{new Input{type=1,keyboard=new Keyboard{key=0x10}},new Input{type=1,keyboard=new Keyboard{key=0x79}},
     new Input{type=1,keyboard=new Keyboard{key=0x79,flags=2}},new Input{type=1,keyboard=new Keyboard{key=0x10,flags=2}}};
   if(SendInput(4,inputs,40)!=4)throw new Exception("Windows rejected context-menu keys.");
 }
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/toolbars/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
    try{$s=Get-Content (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json;if($s.process_id -eq $review.Id -and $s.model.windows_isolated_settings){$s.model}}catch{}
}
function Wait-Until([scriptblock]$Predicate,[string]$Message,[int]$Seconds=5){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Predicate){return};$review.Refresh();if($review.HasExited){throw 'Toolbar review exited unexpectedly'};Start-Sleep -Milliseconds 50}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Name,$Within=$root){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $Within.FindFirst([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new($property,$Value))
}
function Control([string]$Value,[switch]$Name,$Within=$root){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name -Within $Within;$null -ne $hit.item} "Missing $Value"
    $hit.item
}
function Invoke([string]$Value,[switch]$Name,$Within=$root){(Control $Value -Name:$Name -Within $Within).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function WindowCommand([string]$Id){
    Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Previous native menu remained visible'
    Start-Sleep -Milliseconds 250
    Invoke 'application-menu-window'
    if($Id -in @('new_toolbar','manage_toolbars')){
        (Control 'Quick Access Toolbars' -Name).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    }
    Invoke $Id
}
function Toolbar([string]$Id){(Model).panels|Where-Object id -eq $Id}
function ToolbarContext([string]$Id){
    Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Previous native menu remained visible'
    Start-Sleep -Milliseconds 250
    (Control $Id).SetFocus();[CapyToolbarKeys]::Context([uint32]$review.Id)
}
function Edit([string]$Id,[string]$Value){(Control $Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)}
function DialogButton([string]$Id,[string]$Name){
    $dialog=Control $Id;$found=@{item=$null}
    $condition=[System.Windows.Automation.AndCondition]::new(
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::NameProperty,$Name),
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Button))
    Wait-Until {$found.item=$dialog.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition);$null -ne $found.item} "Missing dialog button $Name"
    $found.item
}
function InvokeDialog([string]$Id,[string]$Name){(DialogButton $Id $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Capture([string]$Name){
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
}
try{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/toolbars-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned toolbar review $($review.Id)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $initial=@((Model).state.workspace.layout.panels).Count
    WindowCommand 'new_toolbar'
    $picker=Control 'tool-picker'
    Wait-Until {$null -ne (Model).picker} 'Shared picker did not open'
    if((DialogButton 'tool-picker' 'Create Toolbar').Current.IsEnabled){throw 'Empty selection enabled Create'}
    Wait-Until {!(Control 'Drawing canvas' -Name).Current.IsEnabled} 'Picker did not block painting'
    $checks=$picker.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::CheckBox))
    if($checks.Count -ge @((Model).picker.choices).Count){throw 'Tool picker eagerly created the whole tool catalog'}
    Edit 'tool-picker-search' 'pen'
    Wait-Until {(Model).picker.query -eq 'pen'} 'Shared picker did not finish search'
    Wait-Until {$visible=$picker.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::CheckBox));$visible.Count -eq @((Model).picker.choices).Count} 'Native picker did not settle its search results'
    $pen=Control 'picker-choice-command-pen'
    $identity=$pen.GetRuntimeId() -join ':'
    $pen.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).picker.selected_count -eq 1} 'Native checkbox did not select the shared tool'
    if(((Control 'picker-choice-command-pen').GetRuntimeId() -join ':') -ne $identity){throw 'Selection replaced its native row'}
    Edit 'toolbar-name' ' '
    Wait-Until {!(Model).picker.can_confirm} 'Invalid toolbar name was not rejected'
    Wait-Until {!(DialogButton 'tool-picker' 'Create Toolbar').Current.IsEnabled} 'Invalid name left native confirmation enabled'
    $nameValue=(Control 'toolbar-name').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    foreach($draft in @('W','Windows','Windows tools','Windows tools review')){$nameValue.SetValue($draft)}
    Wait-Until {(Model).picker.name -eq 'Windows tools review'} 'Rapid name edits did not reach shared state'
    if($nameValue.Current.Value -ne 'Windows tools review'){throw 'A delayed snapshot overwrote the name draft'}
    Edit 'tool-picker-search' 'unmatched review search'
    Wait-Until {(Model).picker.selected_count -eq 1 -and @((Model).picker.choices).Count -eq 0} 'Search lost the selected hidden tool'
    Edit 'tool-picker-search' 'pen'
    Wait-Until {(DialogButton 'tool-picker' 'Create Toolbar').Current.IsEnabled} 'Valid selection/name did not enable Create'
    Capture 'picker'
    InvokeDialog 'tool-picker' 'Create Toolbar'
    Wait-Until {$null -eq (Model).picker -and @((Model).state.workspace.layout.panels).Count -eq $initial+1} 'Create did not add one toolbar'
    Wait-Until {$null -eq (Find 'tool-picker') -and (Control 'Drawing canvas' -Name).Current.IsEnabled} 'Picker did not release its modal slot'
    $created=@((Model).panels|Where-Object title -eq 'Windows tools review')[0]
    if(!$created -or $created.id -notlike 'toolbar:*' -or @($created.tiles).Count -ne 1){throw 'Created toolbar does not match selection'}
    $toolbarId=$created.id
    if($created.tiles[0].control.command -ne 'pen'){throw 'Created toolbar contains the wrong control'}
    $gripId="ribbon-grip-$toolbarId"
    ToolbarContext $gripId;Invoke 'Rename Windows tools review toolbar…' -Name
    $null=Control 'toolbar-prompt'
    if((Model).toolbar_prompt.name -ne 'Windows tools review'){throw 'Rename prompt lost its source name'}
    Edit 'toolbar-name' 'Windows renamed review'
    Wait-Until {(Model).toolbar_prompt.name -eq 'Windows renamed review'} 'Rename draft did not reach Core'
    InvokeDialog 'toolbar-prompt' 'Rename'
    Wait-Until {$null -eq (Find 'toolbar-prompt') -and (Toolbar $toolbarId).title -eq 'Windows renamed review'} 'Rename did not update the toolbar'
    Wait-Until {(Control $gripId).Current.Name -eq 'Move Windows renamed review'} 'Native toolbar label did not update'
    WindowCommand 'undo_workspace'
    Wait-Until {(Toolbar $toolbarId).title -eq 'Windows tools review'} 'Undo did not restore toolbar name'
    WindowCommand 'redo_workspace'
    Wait-Until {(Toolbar $toolbarId).title -eq 'Windows renamed review'} 'Redo did not restore toolbar rename'
    WindowCommand 'undo_workspace'
    Wait-Until {(Toolbar $toolbarId).title -eq 'Windows tools review'} 'Second Undo did not restore toolbar name'
    ToolbarContext $gripId;Invoke 'Duplicate Windows tools review toolbar…' -Name
    $null=Control 'toolbar-prompt'
    Edit 'toolbar-name' 'Windows toolbar duplicate'
    Wait-Until {(Model).toolbar_prompt.name -eq 'Windows toolbar duplicate'} 'Duplicate draft did not reach Core'
    InvokeDialog 'toolbar-prompt' 'Duplicate'
    Wait-Until {$null -eq (Find 'toolbar-prompt') -and @((Model).state.workspace.layout.panels).Count -eq $initial+2} 'Duplicate did not add exactly one toolbar'
    $copy=@((Model).panels|Where-Object title -eq 'Windows toolbar duplicate')[0]
    if(!$copy -or $copy.id -eq $toolbarId -or @($copy.tiles).Count -ne 1 -or $copy.tiles[0].control.command -ne 'pen'){throw 'Duplicate did not preserve tools with independent identity'}
    WindowCommand 'undo_workspace'
    Wait-Until {@((Model).state.workspace.layout.panels).Count -eq $initial+1 -and $null -ne (Toolbar $toolbarId)} 'Undo duplicate changed the source toolbar'
    ToolbarContext $gripId;Invoke 'Add Tools…' -Name
    $null=Control 'tool-picker'
    if($null -ne (Find 'toolbar-name')){throw 'Insert picker exposed a toolbar rename field'}
    Edit 'tool-picker-search' 'pencil'
    Wait-Until {(Model).picker.query -eq 'pencil'} 'Insert picker did not search'
    (Control 'picker-choice-command-pencil').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).picker.selected_count -eq 1} 'Insert picker did not select a tool'
    InvokeDialog 'tool-picker' ((Model).picker.confirm_label)
    Wait-Until {$null -eq (Find 'tool-picker') -and @( (Toolbar $toolbarId).tiles).Count -eq 2} 'Insert picker did not add to the existing toolbar'
    if((Toolbar $toolbarId).tiles[1].control.command -ne 'pencil'){throw 'Insert picker added the wrong tool'}
    WindowCommand 'undo_workspace'
    Wait-Until {@((Toolbar $toolbarId).tiles).Count -eq 1} 'Undo did not remove the inserted tool'
    WindowCommand 'manage_toolbars'
    $manager=Control 'toolbar-manager'
    (Control "managed-toolbar-$toolbarId").GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).toolbar_manager.selected -eq $toolbarId} 'Native manager did not select the owned toolbar'
    InvokeDialog 'toolbar-manager' 'Delete Toolbar…'
    $prompt=Control 'toolbar-prompt'
    Wait-Until {(Model).toolbar_prompt.destructive -and (Model).toolbar_prompt.message -like '*Windows tools review*'} 'Delete prompt does not name the owned toolbar'
    InvokeDialog 'toolbar-prompt' 'Cancel'
    Wait-Until {$null -eq (Model).toolbar_prompt -and $null -ne (Find 'toolbar-manager')} 'Cancel did not return to manager'
    if(@((Model).state.workspace.layout.panels).Count -ne $initial+1){throw 'Cancel deleted the toolbar'}
    InvokeDialog 'toolbar-manager' 'Delete Toolbar…'
    Capture 'delete-prompt'
    InvokeDialog 'toolbar-prompt' 'Delete Toolbar'
    Wait-Until {$null -eq (Model).toolbar_prompt -and @((Model).state.workspace.layout.panels).Count -eq $initial} 'Delete did not remove exactly the owned toolbar'
    InvokeDialog 'toolbar-manager' 'Close'
    Wait-Until {$null -eq (Find 'toolbar-manager')} 'Toolbar manager did not close'
    WindowCommand 'undo_workspace'
    Wait-Until {@((Model).state.workspace.layout.panels).Count -eq $initial+1} 'Workspace Undo did not restore the deleted toolbar'
    WindowCommand 'redo_workspace'
    Wait-Until {@((Model).state.workspace.layout.panels).Count -eq $initial} 'Workspace Redo did not delete the toolbar again'
    WindowCommand 'new_toolbar'
    $null=Control 'tool-picker'
    InvokeDialog 'tool-picker' 'Cancel'
    Wait-Until {$null -eq (Model).picker -and $null -eq (Find 'tool-picker') -and (Control 'Drawing canvas' -Name).Current.IsEnabled} 'Cancel did not close the picker and release painting'
    Invoke 'Test stroke' -Name
    Wait-Until {(Model).state.document_file.modified} 'Controlled stroke did not dirty the review'
    WindowCommand 'new_toolbar'
    $null=Control 'tool-picker'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{virtualized_picker='passed';retained_selection='passed';name_validation='passed';create_toolbar='passed';rename_and_history='passed';duplicate_and_history='passed';insert_tools_and_history='passed';manager_selection='passed';delete_cancel_confirm='passed';workspace_history='passed';picker_cancel='passed';close_with_picker='passed';zero_exit='passed';scope='native toolbar dialogs and shared actions; panel expansion, physical input and presentation are separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
