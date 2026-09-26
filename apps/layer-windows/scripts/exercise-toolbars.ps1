param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/toolbars/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function WindowCommand([string]$Id){
    Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Previous native menu remained visible'
    Start-Sleep -Milliseconds 250
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
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
    (Control $Id).SetFocus();[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
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
try{
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/toolbars-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned toolbar review $($review.Id)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready} 'Review did not start' 45
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $initial=@((Model).state.workspace.layout.panels).Count
    WindowCommand 'new_toolbar'
    $null=Control 'workspace-manager'
    Wait-Until {$null -ne (Model).windows_workspace_manager.prompt -and !(Model).windows_workspace_manager.loading} 'New Toolbar form did not open'
    Wait-Until {!(Control 'Drawing canvas' -Name).Current.IsEnabled} 'Toolbar form did not block painting'
    Edit 'workspace-manager-name' ' '
    InvokeDialog 'workspace-manager' 'Add to Workspace'
    Wait-Until {(Model).windows_workspace_manager.error} 'Invalid toolbar name was not rejected'
    $nameValue=(Control 'workspace-manager-name').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern)
    foreach($draft in @('W','Windows','Windows tools','Windows tools review')){$nameValue.SetValue($draft)}
    if($nameValue.Current.Value -ne 'Windows tools review'){throw 'A delayed snapshot overwrote the name draft'}
    InvokeDialog 'workspace-manager' 'Add to Workspace'
    Wait-Until {$null -eq (Find 'workspace-manager') -and @((Model).state.workspace.layout.panels).Count -eq $initial+1} 'Create did not add one empty toolbar'
    $created=@((Model).panels|Where-Object title -eq 'Windows tools review')[0]
    if(!$created -or $created.id -notlike 'toolbar:*' -or @($created.tiles).Count -ne 0){throw 'Empty toolbar did not have an independent identity'}
    $toolbarId=$created.id
    $gripId="ribbon-grip-$toolbarId"
    ToolbarContext $gripId;Invoke 'Add Tools…' -Name
    $picker=Control 'tool-picker'
    Wait-Until {$null -ne (Model).picker} 'Shared tool picker did not open'
    if((DialogButton 'tool-picker' ((Model).picker.confirm_label)).Current.IsEnabled){throw 'Empty selection enabled Add Tools'}
    $checks=$picker.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::CheckBox))
    if($checks.Count -ge @((Model).picker.choices).Count){throw 'Tool picker eagerly created the whole tool catalog'}
    Edit 'tool-picker-search' 'pen'
    Wait-Until {(Model).picker.query -eq 'pen'} 'Shared picker did not finish search'
    Wait-Until {$visible=$picker.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::CheckBox));$visible.Count -gt 0 -and $visible.Count -le @((Model).picker.choices).Count -and $null -ne (Find 'picker-choice-command-pen')} 'Native picker did not settle its search results'
    $pen=Control 'picker-choice-command-pen'
    $identity=$pen.GetRuntimeId() -join ':'
    $pen.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Model).picker.selected_count -eq 1} 'Native checkbox did not select the shared tool'
    if(((Control 'picker-choice-command-pen').GetRuntimeId() -join ':') -ne $identity){throw 'Selection replaced its native row'}
    Edit 'tool-picker-search' 'unmatched review search'
    Wait-Until {(Model).picker.selected_count -eq 1 -and @((Model).picker.choices).Count -eq 0} 'Search lost the selected hidden tool'
    Edit 'tool-picker-search' 'pen'
    Wait-Until {(DialogButton 'tool-picker' ((Model).picker.confirm_label)).Current.IsEnabled} 'Valid selection did not enable Add Tools'
    Capture 'picker'
    InvokeDialog 'tool-picker' ((Model).picker.confirm_label)
    Wait-Until {$null -eq (Model).picker -and @((Toolbar $toolbarId).tiles).Count -eq 1} 'Picker did not add exactly one tool'
    Wait-Until {$null -eq (Find 'tool-picker') -and (Control 'Drawing canvas' -Name).Current.IsEnabled} 'Picker did not release its modal slot'
    if((Toolbar $toolbarId).tiles[0].control.command -ne 'pen'){throw 'Created toolbar contains the wrong control'}
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
    $null=Control 'workspace-manager'
    Wait-Until {!(Model).windows_workspace_manager.loading} 'Toolbar manager did not load'
    $row=(Model).windows_workspace_manager.rows|Where-Object title -eq 'Windows tools review'
    (Control ('workspace-manager-row-'+$row.id)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).windows_workspace_manager.selected -eq $row.id} 'Native manager did not select the owned toolbar'
    Invoke 'workspace-toolbar-actions';Invoke 'workspace-toolbar-delete_toolbar'
    $prompt=Control 'toolbar-prompt'
    Wait-Until {(Model).toolbar_prompt.destructive -and (Model).toolbar_prompt.message -like '*Windows tools review*'} 'Delete prompt does not name the owned toolbar'
    InvokeDialog 'toolbar-prompt' 'Cancel'
    Wait-Until {$null -eq (Model).toolbar_prompt -and $null -eq (Find 'workspace-manager')} 'Cancel did not return to the editor'
    if(@((Model).state.workspace.layout.panels).Count -ne $initial+1){throw 'Cancel deleted the toolbar'}
    WindowCommand 'manage_toolbars'
    $null=Control 'workspace-manager'
    Wait-Until {!(Model).windows_workspace_manager.loading} 'Toolbar manager did not reload'
    (Control ('workspace-manager-row-'+$row.id)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).windows_workspace_manager.selected -eq $row.id} 'Native manager did not restore selection'
    Invoke 'workspace-toolbar-actions';Invoke 'workspace-toolbar-delete_toolbar'
    $null=Control 'toolbar-prompt'
    Capture 'delete-prompt'
    InvokeDialog 'toolbar-prompt' 'Delete Toolbar'
    Wait-Until {$null -eq (Model).toolbar_prompt -and @((Model).state.workspace.layout.panels).Count -eq $initial} 'Delete did not remove exactly the owned toolbar'
    Wait-Until {$null -eq (Find 'toolbar-prompt')} 'Toolbar prompt did not close'
    WindowCommand 'undo_workspace'
    Wait-Until {@((Model).state.workspace.layout.panels).Count -eq $initial+1} 'Workspace Undo did not restore the deleted toolbar'
    WindowCommand 'redo_workspace'
    Wait-Until {@((Model).state.workspace.layout.panels).Count -eq $initial} 'Workspace Redo did not delete the toolbar again'
    WindowCommand 'new_toolbar'
    $null=Control 'workspace-manager'
    InvokeDialog 'workspace-manager' 'Cancel'
    Wait-Until {$null -eq (Model).windows_workspace_manager -and $null -eq (Find 'workspace-manager') -and (Control 'Drawing canvas' -Name).Current.IsEnabled} 'Cancel did not close New Toolbar and release painting'
    Invoke 'Test stroke' -Name
    Wait-Until {(Model).state.document_file.modified} 'Controlled stroke did not dirty the review'
    WindowCommand 'new_toolbar'
    $null=Control 'workspace-manager'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{virtualized_picker='passed';retained_selection='passed';name_validation='passed';create_toolbar='passed';rename_and_history='passed';duplicate_and_history='passed';insert_tools_and_history='passed';manager_selection='passed';delete_cancel_confirm='passed';workspace_history='passed';new_toolbar_cancel='passed';close_with_toolbar_form='passed';zero_exit='passed';scope='native toolbar dialogs and shared actions; panel expansion, physical input and presentation are separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    Exit-CapyEnvironment
}
