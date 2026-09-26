param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true
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
function Manager {(Model).windows_workspace_manager}
function Layout {(Model).state.workspace | ConvertTo-Json -Depth 80 -Compress}
function HeaderChoice([string]$Id){
    $choice=(Model).windows_workspace.switcher_display|Where-Object id -eq $Id
    if(!$choice){throw "Workspace $Id is missing from the header model"}
    $found=@{item=$null};Wait-Until {$found.item=Find ('workspace-switch-'+$choice.key);$found.item -or (Find 'header-workspace-menu')} "Missing workspace-switch-$($choice.key)"
    if($found.item){return $found.item}
    Invoke 'header-workspace-menu'
    Control $choice.name -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
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
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
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
function Launch([string]$Label){
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
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    Launch 'initial'
    $original=(Model).windows_workspace.id
    $originalName=(Model).windows_workspace.name
    if($original -ne 'builtin:workspace:illustrator'){throw 'Fresh installations must open Paint'}
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
    if((Manager).rows|Where-Object {$_.delete -or $_.rename}){throw 'Included workspace options violate rename/delete policy'}
    $painter='builtin:workspace:painter'
    $item=Select-Row $painter
    if((Layout) -eq $before){throw 'Row selection did not preview Sketch'}
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
    if((Model).windows_workspace.id -ne $painter){throw 'Explicit switch did not adopt Sketch'}
    Capture 'painter'
    (HeaderChoice 'builtin:workspace:photographer').GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq 'builtin:workspace:photographer'} 'Header did not switch to Photo'
    Closed;Capture 'photographer'
    (HeaderChoice $original).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq $original} 'Header did not switch to Paint'
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
    Wait-Until {(Layout) -ne $saved -and !(Model).windows_workspace.dirty -and !(Model).windows_workspace.saving} 'Reset fixture arrangement did not settle and save'
    $beforeStarting=Layout
    Menu 'Restore Starting Layout…'
    Wait-Until {(Layout) -eq $saved} 'Starting layout was not previewed before confirmation'
    if((Model).state.brush.diameter -ne $size){throw 'Starting-layout preview changed brush settings'}
    Capture 'starting-layout-preview'
    Choose 'Cancel';Closed
    if((Layout) -ne $beforeStarting){throw 'Cancelling starting-layout preview changed the arrangement'}
    Menu 'Restore Starting Layout…'
    Wait-Until {(Layout) -eq $saved} 'Reopening did not preview the starting layout'
    Choose ((Manager).prompt.confirm);Closed
    if((Layout) -ne $saved -or (Model).state.brush.diameter -ne $size){throw 'Restore Starting Layout did not preserve tool settings and restore the baseline'}
    foreach($step in @(@{id='undo_workspace';layout=$beforeStarting},@{id='redo_workspace';layout=$saved})){
        Wait-Until {@((Model).state.commands|Where-Object {$_.id -eq $step.id -and $_.enabled}).Count -eq 1 -and !(Model).windows_workspace.busy} 'Reset history action did not become available'
        & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
        Invoke $step.id
        Wait-Until {(Layout) -eq $step.layout} 'Starting-layout restore was not one history step'
    }
    Menu 'Manage Workspaces…'
    Capture 'workspaces'
    Invoke ('workspace-manager-options-'+$original)
    $null=Control 'workspace-manager-show'
    foreach($id in @('workspace-manager-rename','workspace-manager-delete')){
        $action=Find $id
        if($action -and !$action.Current.IsOffscreen){throw 'Included workspace exposes a protected rename/delete action'}
    }
    [CapyManagerKeys]::Key([uint32]$review.Id,27)
    Wait-Until {$null -eq (Find 'workspace-manager-show')} 'Included workspace menu did not close'
    Choose 'Cancel';Closed
    if((HeaderChoice $original).Current.Name -ne $originalName){throw 'Included workspace name changed'}
    Menu 'Manage Workspaces…'
    Invoke 'workspace-manager-create'
    Wait-Until {(Manager).prompt.title -eq 'New Workspace'} 'New Workspace prompt did not open'
    if((Manager).prompt.choices.Count -ne 0){throw 'New Workspace must ask only for a name'}
    Edit 'workspace-manager-name' 'Painting'
    Capture 'new-workspace'
    Choose 'Create and Switch';Closed
    if((Model).windows_workspace.name -ne 'Painting' -or (Model).state.brush.diameter -ne $size){throw 'Workspace creation did not preserve tool settings'}
    foreach($id in @($painter,$original,'builtin:workspace:photographer')){
        if((HeaderChoice $id).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -ne [System.Windows.Automation.ToggleState]::Off){throw 'Custom workspace must leave all header choices off'}
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
    Wait-Until {(HeaderChoice $painting).Current.Name -eq 'Inking'} 'Header did not follow the renamed custom workspace'
    Wait-Until {
        $menu=(& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Help' -Inspect).Current.BoundingRectangle
        $firstChoice=(HeaderChoice $painter).Current.BoundingRectangle
        $menu.Right -le $firstChoice.Left
    } 'Renamed workspace header overlaps the application menus'
    Capture 'renamed-header'
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
    if((HeaderChoice $original).Current.Name -ne $originalName){throw 'Included workspace header name changed after restart'}
    Menu 'Manage Workspaces…'
    if((Manager).rows.Count -ne 3){throw 'Restart did not preserve workspace deletion'}
    Choose 'Cancel';Closed
    Write-Output 'Manager actions and restart passed; checking the five-second final process exit.'
    Close
    [pscustomobject]@{included_workspaces='passed';header_switcher='passed';preview_cancel='passed';explicit_apply='passed';enter_previews_only='passed';retained_rows='passed';history_escape_restore='passed';starting_layout_preview='passed';starting_layout_undo_redo='passed';name_only_create='passed';explicit_switch='passed';rename_delete='passed';reset_brushes='passed';restart='passed';zero_exit='passed';scope='native UI Automation and guarded OS keys; physical pointer and performance acceptance remain separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    Exit-CapyEnvironment
}
