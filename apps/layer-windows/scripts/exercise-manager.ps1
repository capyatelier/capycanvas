param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light','both','workflow-light','pins-light')][string]$Theme='dark',[switch]$WorkflowOnly,[switch]$PinsOnly)
$ErrorActionPreference='Stop'
if($Theme -eq 'workflow-light'){$Theme='light';$WorkflowOnly=$true}
if($Theme -eq 'pins-light'){$Theme='light';$PinsOnly=$true}
if($WorkflowOnly -and $PinsOnly){throw 'Choose workspace workflows or pins'}
if($Theme -eq 'both'){foreach($choice in @('dark','light')){& $PSCommandPath -Executable $Executable -Theme $choice -WorkflowOnly:$WorkflowOnly -PinsOnly:$PinsOnly};return}
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
$CapyCacheModel=$true
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/manager/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Manager {$w=(Model).windows_workspace;if($w -and ($null -ne $w.page -or $null -ne $w.prompt)){$w}}
function Layout {(Model).state.workspace | ConvertTo-Json -Depth 80 -Compress}
function HeaderChoice([string]$Id){
    $choice=(Model).windows_workspace.switcher_display|Where-Object id -eq $Id
    if(!$choice){throw "Workspace $Id is missing from the header model"}
    $found=@{item=$null};Wait-Until {$found.item=Find ('workspace-switch-'+$choice.id);$found.item -or (Find 'header-workspace-menu')} "Missing workspace-switch-$($choice.id)"
    if($found.item){return $found.item}
    Invoke 'header-workspace-menu'
    Control $choice.title -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
}
function Edit([string]$Id,[string]$Value){(Control $Id).GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue($Value)}
function Button([string]$Name){
    Control $Name -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button)
}
function Choose([string]$Name){Wait-Until {(Button $Name).Current.IsEnabled} "Manager action remained disabled: $Name";(Button $Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
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
    [CapyWindowApi]::ShowWindow($review.MainWindowHandle,3)|Out-Null
    $owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$null -ne $owned.window} 'Owned drawing window is unavailable'
    $script:root=$owned.window.Root
}
function Close {
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
}
function Check-Pins([string]$WorkspaceId,[string]$WorkspaceName){
    if([CapyRowPointer]::Initialized){[CapyRowPointer]::Dispose()}
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    $transparency=(Model).state.settings.transparency
    foreach($glass in @(@{name='off';index=0},@{name='low';index=1})){
        Set-CapyTransparency $glass.name
        Menu 'Manage Workspaces…'
        $row=Control ('workspace-manager-row-'+$WorkspaceId) -Arranged
        $rect=$row.Current.BoundingRectangle
        [CapyRowPointer]::Hover([int]($rect.Left+$rect.Width*.5),[int]($rect.Top+$rect.Height*.5))
        Capture ("manager-hover-$Theme-glass-"+$glass.name) -WithModel -Composed
        $preview=@((Manager).rows|Where-Object id -ne $WorkspaceId)[0]
        $selected=Select-Row $preview.id
        $selectedBounds=$selected.Current.BoundingRectangle
        $details=@{item=$null}
        Wait-Until {
            $details.item=@((Control 'workspace-manager').FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::Text))|Where-Object {$_.Current.Name -eq $preview.title -and !$_.Current.IsOffscreen -and $_.Current.BoundingRectangle.Left -ge $selectedBounds.Right - 1})|Select-Object -First 1
            $null -ne $details.item
        } 'Workspace details title is not arranged to the right of the list'
        $detailsBounds=$details.item.Current.BoundingRectangle
        $dialogBounds=(Control 'workspace-manager').Current.BoundingRectangle
        $primaryBounds=(Button (Manager).primary).Current.BoundingRectangle
        [ordered]@{row=$selectedBounds;details_title=$detailsBounds;primary=$primaryBounds;overlay=$dialogBounds;dpi=[CapyRowPointer]::GetDpiForWindow([IntPtr]$root.Current.NativeWindowHandle)}|ConvertTo-Json -Depth 5|Set-Content (Join-Path $run ("manager-split-$Theme-glass-"+$glass.name+'.json'))
        Capture ("manager-selected-$Theme-glass-"+$glass.name) -WithModel -Composed
        Choose 'Cancel';Closed
    }
    Set-CapyTransparency $transparency
    Menu 'Manage Workspaces…'
    Capture 'workspaces'
    $pinId='workspace-manager-pin-'+$WorkspaceId
    $pinBefore=[bool](@((Manager).rows|Where-Object id -eq $WorkspaceId)[0].pin.selected)
    $pinLayout=Layout;$pinWorkspace=(Model).windows_workspace.id
    foreach($expected in @((-not $pinBefore),$pinBefore)){
        (Control $pinId -Arranged).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
        Wait-Until {$row=@((Manager).rows|Where-Object id -eq $WorkspaceId)[0];$row.pin.selected -eq $expected -and !(Manager).switcher_busy -and ((Control $pinId).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq $(if($expected){[System.Windows.Automation.ToggleState]::On}else{[System.Windows.Automation.ToggleState]::Off}))} 'Workspace pin did not acknowledge its shared visibility state'
    }
    $pinPattern=(Control $pinId -Arranged).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern)
    $script:pinRapid=[ordered]@{before_shared=$pinBefore;before_native=[string]$pinPattern.Current.ToggleState;revision_before=(Manager).switcher_revision;frequency=[Diagnostics.Stopwatch]::Frequency;started_ticks=$null;first_returned_ticks=$null;first_native=$null;second_started_ticks=$null;second_returned_ticks=$null;second_native=$null;milliseconds=$null;revision_after=$null;persisted_after_restart=$null}
    try{
        $pinRapid.started_ticks=[Diagnostics.Stopwatch]::GetTimestamp()
        $pinPattern.Toggle();$pinRapid.first_returned_ticks=[Diagnostics.Stopwatch]::GetTimestamp();$pinRapid.first_native=[string]$pinPattern.Current.ToggleState
        $pinRapid.second_started_ticks=[Diagnostics.Stopwatch]::GetTimestamp()
        $pinPattern.Toggle();$pinRapid.second_returned_ticks=[Diagnostics.Stopwatch]::GetTimestamp();$pinRapid.second_native=[string]$pinPattern.Current.ToggleState
        $pinRapid.milliseconds=1000.*($pinRapid.second_returned_ticks-$pinRapid.started_ticks)/$pinRapid.frequency
        if($pinRapid.milliseconds -ge 150){throw 'Native pin activations exceeded 150 ms; rapid coverage is unverified'}
        if($pinRapid.first_native -ne $(if($pinBefore){'Off'}else{'On'}) -or $pinRapid.second_native -ne $pinRapid.before_native){throw 'Rapid native pin activations did not toggle and restore the checked state'}
        Wait-Until {$state=Manager;$row=@($state.rows|Where-Object id -eq $WorkspaceId)[0];$pinRapid.revision_after=$state.switcher_revision;$row.pin.selected -eq $pinBefore -and !$state.switcher_busy -and [string](Control $pinId).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq $pinRapid.before_native} 'Rapid pin activations did not settle at the original shared and native state'
    }finally{$pinRapid|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $run 'pin-rapid.json')}
    if((Layout) -ne $pinLayout -or (Model).windows_workspace.id -ne $pinWorkspace){throw 'Pinning changed workspace selection or layout'}
    if(Find ('workspace-manager-options-'+$WorkspaceId) -Visible){throw 'Included workspace still has a row options button'}
    Invoke-Id ('workspace-manager-grip-'+$WorkspaceId)
    $null=Control 'workspace-manager-move-up'
    foreach($id in @('workspace-manager-rename','workspace-manager-delete')){
        $action=Find $id
        if($action -and !$action.Current.IsOffscreen){throw 'Included workspace exposes a protected rename/delete action'}
    }
    [CapyRowPointer]::Key([uint32]$review.Id,27)
    Wait-Until {$anchor=Find 'workspace-manager-move-up';!$anchor -or $anchor.Current.IsOffscreen} 'Included workspace menu did not close'
    Choose 'Cancel';Closed
    if((HeaderChoice $WorkspaceId).Current.Name -ne $WorkspaceName){throw 'Included workspace name changed'}
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    [IO.File]::WriteAllText((Settings-File),(@{language=@{Explicit='en'};theme=$Theme}|ConvertTo-Json -Depth 4))
    Launch 'initial'
    $original=(Model).windows_workspace.id
    $originalName=(Model).windows_workspace.name
    if($original -ne 'builtin:workspace:illustrator'){throw 'Fresh installations must open Paint'}
    $saved=Layout;$size=(Model).state.brush.diameter;$painter='builtin:workspace:painter'
    if($PinsOnly){
        Check-Pins $original $originalName
        Close
        Launch 'pins-restart'
        if((Model).windows_workspace.id -ne $original -or (Layout) -cne $saved){throw 'Restart after rapid pinning changed the workspace or layout'}
        Menu 'Manage Workspaces…'
        Wait-Until {$row=@((Manager).rows|Where-Object id -eq $original)[0];$row.pin.selected -eq $pinRapid.before_shared -and !(Manager).switcher_busy -and [string](Control ('workspace-manager-pin-'+$original)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq $pinRapid.before_native} 'Restart did not retain the rapid pin result'
        $pinRapid.persisted_after_restart='passed';$pinRapid|ConvertTo-Json -Depth 4|Set-Content -LiteralPath (Join-Path $run 'pin-rapid.json')
        Choose 'Cancel';Closed;Close
        [pscustomobject]@{theme=$Theme;pins_only=$true;native_pin_toggle='passed';native_pin_rapid='passed';rapid_milliseconds=$pinRapid.milliseconds;rapid_restart='passed';workspace_selection_and_layout='unchanged';protected_included_actions='passed';zero_exit='passed';evidence=$run}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
        return
    }
    if(!$WorkflowOnly){
    Capture 'illustrator'
    Invoke 'panel-tab-stats'
    Wait-Until {(Layout) -ne $saved} 'Fixture layout did not change'
    $oldSize=(Model).state.brush.diameter
    (Control 'tool-setting-size-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(.61)
    Wait-Until {(Model).state.brush.diameter -ne $oldSize} 'First workspace tool edit did not apply'
    Wait-Until {!(Model).windows_workspace.dirty -and !(Model).windows_workspace.saving} 'Fixture edits did not autosave'
    $size=(Model).state.brush.diameter;$before=Layout
    Menu 'Manage Workspaces…'
    if((Manager).rows.Count -ne 3){throw 'Expected exactly three included workspaces'}
    if((Button 'Switch to Workspace').Current.IsEnabled){throw 'Current workspace switch must be disabled'}
    if((Manager).rows|Where-Object {$_.actions|Where-Object {$_.enabled -and $_.action.type -in 'rename','delete'}}){throw 'Included workspace options violate rename/delete policy'}
    $painter='builtin:workspace:painter'
    $item=Select-Row $painter
    if((Layout) -eq $before){throw 'Row selection did not preview Sketch'}
    if((Model).state.brush.diameter -ne $size){throw 'Workspace preview changed tool settings'}
    $identity=$item.GetRuntimeId() -join ':'
    $item.SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,13)
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
    $item.SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,27)
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
    Check-Pins $original $originalName
    }
    Menu 'Manage Workspaces…'
    Invoke 'workspace-manager-create'
    Wait-Until {(Manager).prompt.title -eq 'New Workspace'} 'New Workspace prompt did not open'
    if((Manager).prompt.choices.Count -ne 0){throw 'New Workspace must ask only for a name'}
    Edit 'workspace-manager-name' ''
    Wait-Until {!(Button 'Create and Switch').Current.IsEnabled} 'Blank workspace name left confirmation enabled'
    Edit 'workspace-manager-name' 'Painting'
    Wait-Until {(Button 'Create and Switch').Current.IsEnabled} 'Valid workspace name did not enable confirmation'
    Capture 'new-workspace'
    Choose 'Create and Switch';Settled
    Wait-Until {$null -eq (Manager).prompt -and (Manager).page -eq 'workspaces'} 'Creation closed the manager'
    Choose 'Cancel';Closed
    if((Model).windows_workspace.name -ne 'Painting' -or (Model).state.brush.diameter -ne $size){throw 'Workspace creation did not preserve tool settings'}
    foreach($id in @($painter,$original,'builtin:workspace:photographer')){
        if((HeaderChoice $id).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -ne [System.Windows.Automation.ToggleState]::Off){throw 'Custom workspace must leave all header choices off'}
    }
    $painting=(Model).windows_workspace.id
    if(!$WorkflowOnly){
    (Control 'tool-setting-size-slider').GetCurrentPattern([System.Windows.Automation.RangeValuePattern]::Pattern).SetValue(.28)
    Wait-Until {(Model).state.brush.diameter -ne $size} 'Second workspace tool edit did not apply'
    $otherSize=(Model).state.brush.diameter
    Menu 'Manage Workspaces…'
    $painting=(Manager).selected
    $null=Select-Row $original
    if((Model).windows_workspace.name -ne 'Painting' -or (Model).state.brush.diameter -ne $otherSize){throw 'Selection switched before confirmation'}
    Choose 'Switch to Workspace';Closed
    if((Model).windows_workspace.name -ne $originalName -or (Model).state.brush.diameter -ne $otherSize){throw 'Switching workspaces changed the shared brush settings'}
    $size=$otherSize
    }
    $listWorkspaces=@()
    foreach($index in 1..4){
        Menu 'Manage Workspaces…';Invoke 'workspace-manager-create'
        Wait-Until {(Manager).prompt.title -eq 'New Workspace'} 'Long-list fixture naming form did not open'
        Edit 'workspace-manager-name' ("Workspace $index")
        Wait-Until {(Button 'Create and Switch').Current.IsEnabled} 'Long-list fixture valid name remained disabled'
        Choose 'Create and Switch';Settled
        Wait-Until {$null -eq (Manager).prompt -and (Manager).page -eq 'workspaces'} 'Creation closed the manager'
        Choose 'Cancel';Closed
        $listWorkspaces+=@{id=(Model).windows_workspace.id;name="Workspace $index"}
    }
    (HeaderChoice $original).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).windows_workspace.id -eq $original} 'Long-list fixture did not restore the original workspace'
    Menu 'Manage Workspaces…'
    Wait-Until {@((Manager).rows).Count -gt 7} 'Larger workspace list did not retain every row'
    if(Find 'workspace-manager-search' -Visible){throw 'Workspace manager still exposes Search'}
    foreach($workspace in $listWorkspaces){
        $null=Select-Row $workspace.id
        Invoke 'workspace-manager-remove'
        Wait-Until {(Manager).prompt.title -eq 'Delete'} 'Footer delete did not open'
        Choose 'Delete';Wait-Until {$null -eq (Manager).prompt -and $null -eq (Row $workspace.name) -and (Manager).page -eq 'workspaces'} 'Delete did not retain the manager and remove its row'
    }
    Choose 'Cancel';Closed
    if(!$WorkflowOnly){
    Menu 'Manage Workspaces…'
    $null=Select-Row $painting
    if(Find ('workspace-manager-options-'+$painting) -Visible){throw 'Custom workspace still has a row dots button'}
    (Control ('workspace-manager-row-'+$painting) -Arranged).SetFocus()
    [CapyRowPointer]::Key([uint32]$review.Id,0x71)
    Wait-Until {(Manager).prompt_action.type -eq 'rename'} 'F2 did not enter inline rename'
    (Control ('workspace-manager-rename-'+$painting) -Arranged).SetFocus()
    Edit ('workspace-manager-rename-'+$painting) 'Inking'
    [CapyRowPointer]::Key([uint32]$review.Id,13)
    Wait-Until {$null -eq (Manager).prompt -and $null -ne (Row 'Inking')} 'Inline Enter did not update retained list'
    Wait-Until {(HeaderChoice $painting).Current.Name -eq 'Inking'} 'Header did not follow the renamed custom workspace'
    Wait-Until {
        $menu=(& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window' -Inspect).Current.BoundingRectangle
        $firstChoice=(HeaderChoice $painter).Current.BoundingRectangle
        $menu.Right -le $firstChoice.Left
    } 'Renamed workspace header overlaps the application menus'
    Capture 'renamed-header'
    Invoke 'workspace-manager-remove'
    Wait-Until {(Manager).prompt.title -eq 'Delete'} 'Delete prompt did not open'
    Choose 'Delete'
    Wait-Until {$null -eq (Manager).prompt -and $null -eq (Row 'Inking')} 'Delete did not remove the workspace'
    Choose 'Cancel';Closed
    }
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    (Control 'Quick Access Toolbars' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Invoke 'Manage Toolbars…' -Name
    Wait-Until {(Manager).page -eq 'this_workspace'} 'Toolbar manager did not open'
    Settled;$null=Select-Row (Manager).rows[0].id
    Invoke 'workspace-toolbar-actions';Invoke 'workspace-toolbar-save_toolbar'
    Wait-Until {$null -ne (Manager).prompt} 'Save toolbar naming form did not open'
    Edit 'workspace-manager-name' 'Workflow toolbar'
    Wait-Until {(Button (Manager).prompt.confirm).Current.IsEnabled} 'Saved toolbar name stayed disabled'
    Choose (Manager).prompt.confirm;Wait-Until {$null -eq (Manager).prompt -and !(Manager).busy} 'Toolbar did not save'
    Invoke 'workspace-toolbar-library';Wait-Until {(Manager).page -eq 'toolbar_library' -and (Row 'Workflow toolbar')} 'Saved toolbar library did not open'
    $savedToolbar=(Row 'Workflow toolbar').id;$null=Select-Row $savedToolbar
    Invoke 'workspace-toolbar-actions';Invoke 'workspace-toolbar-rename'
    Wait-Until {$null -ne (Manager).prompt.description -and (Find 'workspace-manager-description')} 'Saved toolbar description editor did not open'
    $description='Pens é 日本語'
    Edit 'workspace-manager-description' $description
    Choose (Manager).prompt.confirm
    Wait-Until {$null -eq (Manager).prompt -and (Row 'Workflow toolbar').subtitle -eq $description} 'Toolbar description did not persist'
    Invoke 'workspace-toolbar-actions';Invoke 'workspace-toolbar-rename'
    Wait-Until {(Manager).prompt.description -eq $description -and (Control 'workspace-manager-description').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).Current.Value -eq $description} 'Reopened toolbar description lost its saved text'
    Edit 'workspace-manager-description' 'Discarded draft';Choose 'Cancel'
    Wait-Until {$null -eq (Manager).prompt} 'Toolbar rename Cancel did not return to library'
    Invoke 'workspace-toolbar-actions';Invoke 'workspace-toolbar-rename'
    Wait-Until {(Manager).prompt.description -eq $description} 'Cancelled description draft overwrote the saved description'
    Choose 'Cancel';Wait-Until {$null -eq (Manager).prompt} 'Toolbar description did not close'
    Invoke 'workspace-toolbar-actions';Invoke 'workspace-toolbar-delete'
    Wait-Until {$null -ne (Manager).prompt} 'Saved toolbar Delete did not open'
    Choose (Manager).prompt.confirm;Wait-Until {$null -eq (Manager).prompt -and $null -eq (Row 'Workflow toolbar')} 'Saved toolbar did not delete'
    Choose 'Cancel';Closed
    if($WorkflowOnly){
        Close
        [pscustomobject]@{theme=$Theme;workflow_only=$true;immediate_name_gating='passed';larger_workspace_list_without_search='passed';saved_toolbar_description_edit_cancel_and_reopen='passed';zero_exit='passed';evidence=$run}|ConvertTo-Json|Tee-Object -FilePath (Join-Path $run 'results.json')
        return
    }
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
    [pscustomobject]@{theme=$Theme;included_workspaces='passed';native_pin_toggle='passed';header_switcher='passed';preview_cancel='passed';explicit_apply='passed';enter_previews_only='passed';retained_rows='passed';history_escape_restore='passed';starting_layout_preview='passed';starting_layout_undo_redo='passed';name_only_create='passed';immediate_name_gating='passed';larger_workspace_list_without_search='passed';saved_toolbar_description_edit_cancel_and_reopen='passed';explicit_switch='passed';shared_editing_state='passed';rename_delete='passed';reset_brushes='passed';restart='passed';zero_exit='passed';scope='native UI Automation and guarded OS keys; physical pointer and performance acceptance remain separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose()
    Exit-CapyEnvironment
}
