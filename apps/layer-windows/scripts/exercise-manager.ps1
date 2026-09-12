param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class CapyManagerKeys {
 [StructLayout(LayoutKind.Sequential)] public struct Keyboard {public ushort key,scan;public uint flags,time;public UIntPtr extra;}
 [StructLayout(LayoutKind.Explicit,Size=40)] public struct Input {[FieldOffset(0)]public uint type;[FieldOffset(8)]public Keyboard keyboard;}
 [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window,out uint process);
 [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,Input[] input,int size);
 public static void Key(uint process,ushort key) {
   uint owner;GetWindowThreadProcessId(GetForegroundWindow(),out owner);
   if(owner!=process)throw new Exception("Review does not own keyboard focus; no key sent.");
   var input=new[]{new Input{type=1,keyboard=new Keyboard{key=key}},new Input{type=1,keyboard=new Keyboard{key=key,flags=2}}};
   if(SendInput(2,input,40)!=2)throw new Exception("Windows rejected the review key.");
 }
}
'@
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/manager/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
    try{$s=Get-Content (Join-Path $directory 'ui-state.json') -Raw|ConvertFrom-Json;if($s.process_id -eq $review.Id -and $s.model.windows_isolated_settings){$script:lastModel=$s.model}}catch{}
    $script:lastModel
}
function Manager {(Model).windows_workspace_manager}
function Layout {(Model).state.workspace | ConvertTo-Json -Depth 80 -Compress}
function Wait-Until([scriptblock]$Predicate,[string]$Message,[int]$Seconds=5){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Predicate){return};$review.Refresh();if($review.HasExited){throw 'Manager review exited unexpectedly'};Start-Sleep -Milliseconds 50}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Value,[switch]$Name,$Within=$root,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Value)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $Within.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Control([string]$Value,[switch]$Name,$Within=$root,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Value -Name:$Name -Within $Within -Type $Type;$null -ne $hit.item} "Missing $Value"
    $hit.item
}
function Invoke([string]$Value,[switch]$Name,$Within=$root){
    (Control $Value -Name:$Name -Within $Within).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Edit([string]$Id,[string]$Value){(Control $Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)}
function Button([string]$Name){
    Control $Name -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button)
}
function Choose([string]$Name){(Button $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Settled {
    Wait-Until {$v=Manager;$null -ne $v -and !$v.loading -and !$v.busy} 'Manager did not settle'
    if((Manager).error){throw (Manager).error}
}
function Closed {
    Wait-Until {$null -eq (Manager) -and $null -eq (Find 'workspace-manager')} 'Manager did not close'
}
function Menu([string]$Name){
    Closed
    Invoke 'application-menu-window'
    (Control 'Workspaces' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Invoke $Name -Name
    Wait-Until {$null -ne (Find 'workspace-manager')} 'Native manager did not open'
    Settled
}
function Select-Row([string]$Id){
    $item=Control ('workspace-manager-row-'+$Id)
    $item.GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Manager).selected -eq $Id -and !(Manager).loading} 'Selected preview did not settle'
    if((Manager).error){throw (Manager).error}
    $item
}
function Row([string]$Title){(Manager).rows|Where-Object title -eq $Title|Select-Object -First 1}
function Capture([string]$Name){
    & (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))
}
function Launch([string]$Label){
    $script:lastModel=$null
    $script:stderr=Join-Path $run ($Label+'-stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/manager-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned manager review $($review.Id) ($Label)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Manager review did not start' 45
    $script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
}
function Close {
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
}
try {
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$null,'Process')}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    Launch 'initial'
    $original=(Model).windows_workspace.id
    $originalName=(Model).windows_workspace.name
    if($original -ne 'builtin:workspace:illustrator'){throw 'Fresh installations must open Illustrator'}
    $saved=Layout
    Capture 'illustrator'
    Invoke 'panel-tab-stats'
    Wait-Until {(Layout) -ne $saved} 'Fixture layout did not change'
    $oldSize=(Model).state.brush.diameter
    (Control 'Brush size slider' -Name -Type ([System.Windows.Automation.ControlType]::Slider)).GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(.61)
    Wait-Until {(Model).state.brush.diameter -ne $oldSize} 'First workspace tool edit did not apply'
    Wait-Until {!(Model).windows_workspace.dirty -and !(Model).windows_workspace.saving} 'Fixture edits did not autosave'
    $size=(Model).state.brush.diameter;$before=Layout
    Menu 'Manage Workspaces…'
    if((Manager).rows.Count -ne 3){throw 'Expected exactly three included workspaces'}
    if((Button 'Switch to Workspace').Current.IsEnabled){throw 'Current workspace switch must be disabled'}
    if((Manager).rows|Where-Object {$_.delete -or !$_.rename}){throw 'Included workspace options violate rename/delete policy'}
    $painter='builtin:workspace:painter'
    $item=Select-Row $painter
    if((Layout) -eq $before){throw 'Row selection did not preview Painter'}
    if((Model).state.brush.diameter -ne $size){throw 'Workspace preview changed tool settings'}
    $identity=$item.GetRuntimeId() -join ':'
    $item.SetFocus();[CapyManagerKeys]::Key([uint32]$review.Id,13)
    Start-Sleep -Milliseconds 250
    if(!(Manager)){throw 'Enter committed the preview'}
    if(((Control ('workspace-manager-row-'+$painter)).GetRuntimeId() -join ':') -ne $identity){throw 'Selection replaced the native row'}
    Capture 'workspace-preview'
    Choose 'Cancel';Closed
    if((Layout) -ne $before){throw 'Cancel did not restore the original layout'}
    Menu 'Manage Workspaces…'
    $null=Select-Row $painter
    Choose 'Switch to Workspace';Closed
    if((Model).windows_workspace.id -ne $painter){throw 'Explicit switch did not adopt Painter'}
    Capture 'painter'
    (Control 'workspace-switch-photographer').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq 'builtin:workspace:photographer'} 'Header did not switch to Photographer'
    Closed;Capture 'photographer'
    (Control 'workspace-switch-illustrator').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq $original} 'Header did not switch to Illustrator'
    Closed
    if((Layout) -ne $before -or (Model).state.brush.diameter -ne $size){throw 'Header switching reset saved workspace edits'}
    Menu 'Layout History…'
    if((Button 'Restore This Version').Current.IsEnabled){throw 'History enabled restoration of the current version'}
    $earlier=(Manager).rows|Where-Object {!$_.current}|Select-Object -First 1
    $item=Select-Row $earlier.id
    Capture 'history-preview'
    $item.SetFocus();[CapyManagerKeys]::Key([uint32]$review.Id,27)
    Closed
    if((Layout) -ne $before){throw 'Escape did not restore the original history preview'}
    Menu 'Layout History…'
    $null=Select-Row $earlier.id
    Choose 'Restore This Version';Closed
    if((Layout) -eq $before -or (Model).state.brush.diameter -ne $size){throw 'History did not apply only the selected arrangement'}
    Invoke 'panel-tab-stats'
    Menu 'Restore Starting Layout…'
    Choose ((Manager).prompt.confirm);Closed
    if((Layout) -ne $saved -or (Model).state.brush.diameter -ne $size){throw 'Restore Starting Layout did not preserve tool settings and restore the baseline'}
    Menu 'Manage Workspaces…'
    Capture 'workspaces'
    Invoke ('workspace-manager-options-'+$original);Invoke 'workspace-manager-rename'
    Wait-Until {(Manager).prompt.title -eq 'Rename'} 'Included workspace rename did not open'
    Edit 'workspace-manager-name' 'My Illustration'
    Choose 'Rename'
    Wait-Until {$null -eq (Manager).prompt -and $null -ne (Row 'My Illustration')} 'Included workspace rename did not update list'
    Choose 'Cancel';Closed
    if((Control 'workspace-switch-illustrator').Current.Name -ne 'My Illustration'){throw 'Header did not follow the renamed workspace identity'}
    Wait-Until {
        $menu=(Control 'application-menu-help').Current.BoundingRectangle
        $firstChoice=(Control 'workspace-switch-painter').Current.BoundingRectangle
        $menu.Right -le $firstChoice.Left
    } 'Renamed workspace header overlaps the application menus'
    Capture 'renamed-header'
    $originalName='My Illustration'
    Menu 'Manage Workspaces…'
    Invoke 'workspace-manager-create'
    Wait-Until {(Manager).prompt.title -eq 'New Workspace'} 'New Workspace prompt did not open'
    if((Manager).prompt.choices.Count -ne 0){throw 'New Workspace must ask only for a name'}
    Edit 'workspace-manager-name' 'Painting'
    Capture 'new-workspace'
    Choose 'Create and Switch';Closed
    if((Model).windows_workspace.name -ne 'Painting' -or (Model).state.brush.diameter -ne $size){throw 'Workspace creation did not preserve tool settings'}
    foreach($key in @('painter','illustrator','photographer')){
        if((Control ('workspace-switch-'+$key)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -ne [System.Windows.Automation.ToggleState]::Off){throw 'Custom workspace must leave all header choices off'}
    }
    (Control 'Brush size slider' -Name -Type ([System.Windows.Automation.ControlType]::Slider)).GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(.28)
    Wait-Until {(Model).state.brush.diameter -ne $size} 'Second workspace tool edit did not apply'
    $otherSize=(Model).state.brush.diameter
    Menu 'Manage Workspaces…'
    $painting=(Manager).selected
    $null=Select-Row $original
    if((Model).windows_workspace.name -ne 'Painting' -or (Model).state.brush.diameter -ne $otherSize){throw 'Selection switched before confirmation'}
    Choose 'Switch to Workspace';Closed
    if((Model).windows_workspace.name -ne $originalName -or (Model).state.brush.diameter -ne $size){throw 'Switch did not restore workspace tool settings'}
    Menu 'Manage Workspaces…'
    Invoke ('workspace-manager-options-'+$painting);Invoke 'workspace-manager-rename'
    Wait-Until {(Manager).prompt.title -eq 'Rename'} 'Rename prompt did not open'
    Edit 'workspace-manager-name' 'Inking'
    Choose 'Rename'
    Wait-Until {$null -eq (Manager).prompt -and $null -ne (Row 'Inking')} 'Rename did not update retained list'
    Invoke ('workspace-manager-options-'+$painting);Invoke 'workspace-manager-delete'
    Wait-Until {(Manager).prompt.title -eq 'Delete'} 'Delete prompt did not open'
    Choose 'Delete'
    Wait-Until {$null -eq (Manager).prompt -and $null -eq (Row 'Inking')} 'Delete did not remove the workspace'
    Choose 'Cancel';Closed
    $beforeReset=Layout
    Menu 'Reset All Brushes…';Capture 'reset-brushes'
    Choose 'Cancel';Closed
    if((Model).state.brush.diameter -ne $size){throw 'Cancelling brush reset changed settings'}
    Menu 'Reset All Brushes…'
    Choose 'Reset Brushes';Closed
    if((Layout) -ne $beforeReset -or (Model).state.brush.diameter -eq $size){throw 'Brush reset did not preserve the arrangement and reset settings'}
    $size=(Model).state.brush.diameter
    Menu 'Manage Workspaces…'
    $null=Select-Row $painter
    Close
    Launch 'restart'
    if((Layout) -ne $beforeReset){throw 'Closing during a preview persisted the temporary arrangement'}
    if((Model).windows_workspace.name -ne $originalName -or (Model).state.brush.diameter -ne $size){throw 'Restart did not restore the active workspace'}
    if((Control 'workspace-switch-illustrator').Current.Name -ne $originalName){throw 'Renamed header did not survive restart'}
    Menu 'Manage Workspaces…'
    if((Manager).rows.Count -ne 3){throw 'Restart did not preserve workspace deletion'}
    Choose 'Cancel';Closed
    Write-Output 'Manager actions and restart passed; checking the five-second final process exit.'
    Close
    [pscustomobject]@{included_workspaces='passed';header_switcher='passed';preview_cancel='passed';explicit_apply='passed';enter_previews_only='passed';retained_rows='passed';history_escape_restore='passed';name_only_create='passed';explicit_switch='passed';rename_delete='passed';reset_brushes='passed';restart='passed';zero_exit='passed';scope='native UI Automation and guarded OS keys; physical pointer and performance acceptance remain separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}
