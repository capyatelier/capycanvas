param([Parameter(Mandatory)][string]$Executable,[ValidateSet('dark','light')][string]$Theme='dark',[ValidateSet('mouse','pen','touch')][string]$Device='mouse')
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
$CapyFind='visible';$CapyPopups=$true
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/switcher/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Storage {(Model).windows_workspace}
function Manager {$w=(Model).windows_workspace;if($w -and ($null -ne $w.page -or $null -ne $w.prompt)){$w}}
function Layout {(Model).state.workspace|ConvertTo-Json -Depth 80 -Compress}
function Choose([string]$Name){
    (Control $Name -Name -Within (Control 'workspace-manager') -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
}
function Settled {
    Wait-Until {$v=Manager;$s=Storage;$null -ne $v -and !$v.loading -and !$v.busy -and !$s.switcher_busy} 'Manager did not settle'
    if((Manager).error){throw (Manager).error}
    if((Storage).switcher_error){throw (Storage).switcher_error}
}
function Closed {
    Wait-Until {$null -eq (Manager) -and $null -eq (Find 'workspace-manager')} 'Manager did not close'
}
function Open-Manager {
    Closed
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window'
    (Control 'Workspaces' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Invoke 'Manage Workspaces…' -Name
    Wait-Until {$null -ne (Find 'workspace-manager')} 'Manager did not open'
    Settled
}
function At([string]$Id){
    $box=(Control $Id -Arranged).Current.BoundingRectangle
    @{x=[int]($box.X+$box.Width/2);y=[int]($box.Y+$box.Height/2)}
}
function Options([switch]$Compact) {
    Wait-Until {$null -ne (Find ('workspace-switcher-show-'+(Storage).order[0]))} 'Visibility checklist did not open'
    foreach($id in @((Storage).order)){
        $row=Control ('workspace-switcher-show-'+$id)
        $checked=$row.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Current.ToggleState -eq [System.Windows.Automation.ToggleState]::On
        if($checked -ne ($id -in @((Storage).switcher.id))){throw 'Visibility checkbox differs from saved pins'}
    }
    $first=Control ('workspace-switcher-show-'+(Storage).order[0])
    $menu=[System.Windows.Automation.TreeWalker]::ControlViewWalker.GetParent($first)
    $items=@($menu.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,[System.Windows.Automation.ControlType]::MenuItem)))
    if($items.Count -ne (@((Storage).order).Count+$(if($Compact){0}else{1}))){throw 'Visibility checklist has unexpected native rows'}
    if($items[0].Current.AutomationId -ne ('workspace-switcher-show-'+(Storage).order[0])){throw 'Visibility checklist does not begin with its first workspace'}
    $rows=@($menu.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.Condition]::TrueCondition)|Where-Object {$_.Current.AutomationId -like 'workspace-switcher-show-*'})
    $nativeOrder=@($rows|ForEach-Object {$_.Current.AutomationId.Substring('workspace-switcher-show-'.Length)})
    if(($nativeOrder -join '|') -ne ((Storage).order -join '|')){throw 'Visibility menu does not contain every workspace in saved order'}
    $null=Control 'Manage Workspaces…' -Name
    if(Find 'Customize Title Bar' -Name){throw 'Switcher input opened the titlebar context menu'}
}
function Dismiss([string]$Focus=''){
    [CapyRowPointer]::Key([uint32]$review.Id,0x1b)
    Wait-Until {$null -eq (Find ('workspace-switcher-show-'+(Storage).order[0]))} 'Escape did not dismiss options'
    if($Focus){Wait-Until {(Control $Focus).Current.HasKeyboardFocus} 'Escape did not return focus to the invoking switcher control'}
}
function Check-Options {
    $active=(Storage).id;$layout=Layout;$order=(Storage).order -join '|'
    $options=(Control 'workspace-switcher-options' -Arranged).Current.BoundingRectangle
    $pill=(Control ('workspace-switch-'+$active) -Arranged).Current.BoundingRectangle
    $scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    if([Math]::Abs($options.Width/$scale-20) -gt 1 -or [Math]::Abs($options.Height-$pill.Height) -gt 1 -or [Math]::Abs($options.Y-$pill.Y) -gt 1){throw 'Switcher options dimensions differ from the workspace pills'}
    $inactive=@((Storage).switcher_display.id|Where-Object {$_ -ne $active})[0]
    Invoke 'workspace-switcher-options';Options;Capture 'visibility-checklist' -Composed;Dismiss 'workspace-switcher-options'
    foreach($source in @(('workspace-switch-'+$active),('workspace-switch-'+$inactive),'workspace-switcher-options','workspace-switcher')){
        $at=At $source
        if($source -eq 'workspace-switcher'){$box=(Control $source).Current.BoundingRectangle;$at.y=[int]($box.Y+2)}
        [CapyRowPointer]::RightClick($at.x,$at.y);Options
        Dismiss $(if($source -ne 'workspace-switcher'){$source})
        if((Storage).id -ne $active -or (Layout) -ne $layout){throw 'Secondary switcher input changed the workspace'}
    }
    foreach($id in @($inactive,$active)){
        $at=At ('workspace-switch-'+$id);[CapyRowPointer]::Down($Device,$at.x,$at.y);[CapyRowPointer]::Up()
        Wait-Until {(Storage).id -eq $id -and !(Storage).busy} 'Short switcher tap did not switch normally'
    }
    Invoke 'workspace-switcher-options';Options
    $at=At ('workspace-switch-'+$active);[CapyRowPointer]::Down($Device,$at.x,$at.y);[CapyRowPointer]::Up()
    Wait-Until {!(Find ('workspace-switcher-show-'+$active))} 'Outside tap did not dismiss visibility options'
    if((Storage).id -ne $active){throw 'Outside dismissal changed the active workspace'}
    (Control 'workspace-switcher-options').SetFocus();[CapyRowPointer]::Key([uint32]$review.Id,0x0d)
    Options;Dismiss 'workspace-switcher-options'
    if($Device -eq 'mouse'){
        $at=At ('workspace-switch-'+$active);[CapyRowPointer]::Down('mouse',$at.x,$at.y);Start-Sleep -Milliseconds 900
        if(Find ('workspace-switcher-show-'+$active)){throw 'Mouse hold opened workspace options'}
        [CapyRowPointer]::Up()
    }
    foreach($key in @(0x5d,0x79)){
        (Control ('workspace-switch-'+$inactive)).SetFocus()
        if($key -eq 0x79){[CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),[uint16]$key)}else{[CapyRowPointer]::Key([uint32]$review.Id,[uint16]$key)}
        Options;Dismiss ('workspace-switch-'+$inactive)
    }
    @{theme=$Theme;device=$Device;secondary_context='passed';escape_focus='passed';short_taps='passed';outside_dismiss='passed';enter='passed';keyboard_context='passed'}|ConvertTo-Json|Set-Content (Join-Path $run 'workspace-options-input.json')
    if($Device -ne 'mouse'){
        foreach($source in @(('workspace-switch-'+$inactive),'workspace-switcher-options','workspace-switcher')){
            $at=At $source
            if($source -eq 'workspace-switcher'){$box=(Control $source).Current.BoundingRectangle;$at.y=[int]($box.Y+2)}
            [CapyRowPointer]::Down($Device,$at.x,$at.y)
            Wait-Until {try{(Control 'title-bar').Current.HelpText|ConvertFrom-Json|Where-Object {$_.phase -eq 'held' -and $_.menu_open}}catch{}} "Switcher hold did not open the checklist: $source"
            [CapyRowPointer]::Up();Options;Dismiss
            if((Storage).id -ne $active){throw 'Hold release also switched workspaces'}
        }
    }
    foreach($id in @($active,$inactive)){
        foreach($repeat in 1..2){
            Invoke 'workspace-switcher-options';Options
            $revision=(Storage).switcher_revision
            Invoke ('workspace-switcher-show-'+$id)
            Wait-Until {!(Find ('workspace-switcher-show-'+$id))} 'Checkbox activation did not dismiss options'
            Invoke 'workspace-switcher-options'
            $retained=Control ('workspace-switcher-show-'+$id)
            $runtime=$retained.GetRuntimeId() -join ':'
            Wait-Until {(Storage).switcher_revision -gt $revision -and !(Storage).switcher_busy -and $retained.Current.IsEnabled} 'Open options did not follow preference acknowledgement'
            Options
            if(((Control ('workspace-switcher-show-'+$id)).GetRuntimeId() -join ':') -ne $runtime){throw 'Preference acknowledgement replaced the open menu item'}
            Dismiss 'workspace-switcher-options'
            if((Storage).id -ne $active -or (Layout) -ne $layout -or ((Storage).order -join '|') -ne $order){throw 'Visibility edit changed the active workspace, layout or order'}
        }
    }
    Invoke 'workspace-switcher-options';Options;Invoke 'Manage Workspaces…' -Name
    Wait-Until {$null -ne (Find 'workspace-manager')} 'Options footer did not open the workspace manager'
    Settled;Choose 'Cancel';Closed
    if((Storage).id -ne $active -or (Layout) -ne $layout){throw 'Opening the manager disturbed the workspace'}
}
function Check-Compact {
    $active=(Storage).id;$layout=Layout;$scale=[CapyRowPointer]::GetDpiForWindow($drawingWindow)/96.
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Normal)
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Resize -Width ([int](960*$scale)) -Height ([int](480*$scale))
    $null=Control 'header-workspace-menu' -Arranged
    $at=At 'header-workspace-menu';[CapyRowPointer]::RightClick($at.x,$at.y);Options;Dismiss 'header-workspace-menu'
    Invoke 'header-workspace-menu'
    $submenu=Control 'Show in top bar' -Name -Type ([System.Windows.Automation.ControlType]::MenuItem)
    $submenu.GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    Options -Compact;Capture 'compact-visibility' -Composed
    $revision=(Storage).switcher_revision
    Invoke ('workspace-switcher-show-'+$active)
    Wait-Until {(Storage).switcher_revision -gt $revision -and !(Storage).switcher_busy} 'Compact visibility toggle was not acknowledged'
    if((Storage).id -ne $active -or (Layout) -ne $layout){throw 'Compact pinning switched or changed the workspace'}
    Invoke 'header-workspace-menu';Invoke 'Manage Workspaces…' -Name
    Wait-Until {$null -ne (Find 'workspace-manager')} 'Compact footer did not open the manager'
    Settled;Choose 'Cancel';Closed
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
}
function Preference([string]$Id,[string]$Action){
    Settled
    $revision=(Storage).switcher_revision
    $row=$root.FindFirst([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::AutomationIdProperty,('workspace-manager-row-'+$Id)))
    if($row -and $row.Current.IsOffscreen){$row.GetCurrentPattern([System.Windows.Automation.ScrollItemPattern]::Pattern).ScrollIntoView()}
    $null=Control ('workspace-manager-options-'+$Id) -Arranged
    Invoke ('workspace-manager-options-'+$Id)
    $item=Control ('workspace-manager-'+$Action)
    if($Action -eq 'show'){$item.GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()}
    else{$item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
    Wait-Until {(Storage).switcher_revision -gt $revision -or (Storage).switcher_error} 'Preference was not acknowledged'
    Settled
}
function Launch([string]$Label){
    $script:stderr=Join-Path $run ($Label+'-stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/switcher-review.json')
    Write-Output "Owned switcher review $($review.Id) ($Label)"
    $owned=@{window=$null}
    Wait-Until {$owned.window=Owned-DrawingWindow $review;$null -ne $owned.window -and (Model).brush_ready -and (Storage).ready -and !(Storage).switcher_busy} 'Switcher review did not start' 45
    $script:root=$owned.window.Root;$script:drawingWindow=$owned.window.Handle
    $root.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).SetWindowVisualState([System.Windows.Automation.WindowVisualState]::Maximized)
    Wait-Until {$null -ne (Find 'workspace-switcher')} 'Maximized header did not show the workspace switcher'
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4))
    $null=[CapyRowPointer]::SetForegroundWindow($drawingWindow)
    [CapyRowPointer]::Initialize([uint32]$review.Id)
    if(Find 'Test stroke' -Name){throw 'Switcher fixture requires the production UI without smoke controls'}
}
function Close {
    [CapyRowPointer]::Dispose()
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -WindowHandle $drawingWindow.ToInt64() -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
}
try {
    Enter-CapyEnvironment
    $env:CAPY_STORAGE_DIR=Join-Path $run 'profile'
    [IO.File]::WriteAllText((Settings-File),(@{theme=$Theme}|ConvertTo-Json))
    $env:CAPY_TRACE_UI='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    Launch 'initial';Check-Options
    $active=(Storage).id;$order=@((Storage).order);$original=Layout
    $preview=@($order|Where-Object {$_ -ne $active})[0]
    Open-Manager
    (Control ('workspace-manager-row-'+$preview)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Manager).selected -eq $preview -and !(Manager).loading} 'Preview did not settle'
    $previewLayout=Layout
    $retained=(Control ('workspace-manager-row-'+$preview)).GetRuntimeId() -join ':'
    Preference $active 'show'
    if((Storage).switcher.id -contains $active -or (Storage).switcher_display[0].id -ne $active){throw 'Hidden current workspace fallback is incorrect'}
    if(((Storage).order -join '|') -ne ($order -join '|')){throw 'Visibility changed dialog order'}
    Preference $order[2] 'move-up'
    Preference $order[2] 'move-up'
    if((Manager).rows[0].id -ne $order[2]){throw 'Move Up did not reorder rows'}
    if((Manager).selected -ne $preview -or (Layout) -ne $previewLayout){throw 'Preferences disturbed the selected preview'}
    if(((Control ('workspace-manager-row-'+$preview)).GetRuntimeId() -join ':') -ne $retained){throw 'Reorder replaced a retained row'}
    Capture 'configured-preview'
    Choose 'Cancel';Closed
    if((Layout) -ne $original){throw 'Cancel did not restore the original layout'}
    $key=$preview
    (Control ('workspace-switch-'+$key)).GetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern).Toggle()
    Wait-Until {(Storage).id -eq $preview -and !(Storage).busy} 'Header switch did not complete'
    if((Storage).switcher_display.id -contains $active){throw 'Unpinned previous workspace remained in header'}
    Open-Manager
    foreach($id in @((Storage).switcher.id)){Preference $id 'show'}
    if(@((Storage).switcher_display).Count -ne 1 -or (Storage).switcher_display[0].id -ne $preview){throw 'Empty pins did not retain the current workspace'}
    Choose 'Cancel';Closed
    Invoke 'workspace-switcher-options';Options;Dismiss 'workspace-switcher-options'
    $emptyOrder=@((Storage).order)
    Close
    Launch 'empty-pins-restart'
    if(@((Storage).switcher).Count -ne 0 -or ((Storage).order -join '|') -ne ($emptyOrder -join '|')){throw 'Preferences did not survive restart'}
    $expandedWorkspace='builtin:workspace:illustrator'
    if((Storage).id -ne $expandedWorkspace){
        Open-Manager
        (Control ('workspace-manager-row-'+$expandedWorkspace)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
        Wait-Until {$m=Manager;$m.selected -eq $expandedWorkspace -and $m.enabled -and !$m.loading} 'Paint workspace selection did not become ready'
        Settled;Choose (Manager).primary;Closed
        Wait-Until {(Storage).id -eq $expandedWorkspace -and !(Storage).busy} 'Paint workspace was not adopted before overflow creation'
    }
    $headerBefore=(Model).header.model
    $title=@($headerBefore.zones|ForEach-Object {$_}|Where-Object {$_.item.kind -eq 'document_title'})
    if($title.Count -ne 1){throw 'Paint overflow setup requires exactly one drawing title'}
    $expectedZones=@(foreach($zone in $headerBefore.zones){,@($zone|Where-Object id -ne $title[0].id)})|ConvertTo-Json -Depth 30 -Compress
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window'
    Invoke 'customize_workspace_ui'
    Wait-Until {(Model).header.editing} 'Overflow setup did not open titlebar customization'
    Invoke ('header-select-'+$title[0].id)
    $at=At 'header-edit-done';[CapyRowPointer]::KeyAt(0x2e,$at.x,$at.y)
    Wait-Until {!(@((Model).header.model.zones|ForEach-Object {$_}|Where-Object id -eq $title[0].id).Count)} 'Overflow setup did not remove the selected drawing title'
    $configured=(Model).header.model
    if(($configured.zones|ConvertTo-Json -Depth 30 -Compress) -ne $expectedZones -or $configured.size -ne $headerBefore.size -or $configured.next_id -ne $headerBefore.next_id){throw 'Overflow setup changed more than the drawing title'}
    Invoke 'header-edit-done'
    Wait-Until {!(Model).header.editing -and ((Model).header.model|ConvertTo-Json -Depth 30 -Compress) -eq ($configured|ConvertTo-Json -Depth 30 -Compress)} 'Overflow titlebar setup did not commit'
    Capture 'overflow-setup' -WithModel -Composed
    for($i=1;$i -le 6;$i++){
        Open-Manager
        Invoke 'workspace-manager-create'
        Wait-Until {(Manager).prompt.title -eq 'New Workspace'} 'Creation prompt missing'
        (Control 'workspace-manager-name').GetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern).SetValue("Switcher overflow workspace $i")
        Choose 'Create and Switch';Closed
        Wait-Until {$s=Storage;$s.switcher.id -contains $s.id -and $s.switcher_display.id -contains $s.id} 'Created workspace was not pinned'
    }
    $inline=@{item=$null};Wait-Until {$inline.item=Find 'workspace-switcher';$null -ne $inline.item} 'Expanded workspace switcher did not appear'
    $providers=@($inline.item.FindAll([System.Windows.Automation.TreeScope]::Descendants,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::IsScrollPatternAvailableProperty,$true)))
    if($providers.Count -ne 1){throw 'Workspace choices did not expose one scrolling provider'}
    $scroller=$providers[0].GetCurrentPattern([System.Windows.Automation.ScrollPattern]::Pattern)
    Wait-Until {$scroller.Current.HorizontallyScrollable} 'Header overflow is not scrollable'
    $scroller.SetScrollPercent(100,[System.Windows.Automation.ScrollPattern]::NoScroll)
    Wait-Until {$scroller.Current.HorizontalScrollPercent -ge 99} 'Header did not scroll to its last choices'
    $fixed=(Control 'workspace-switcher-options' -Arranged).Current.BoundingRectangle
    $scroller.SetScrollPercent(0,[System.Windows.Automation.ScrollPattern]::NoScroll);Wait-Until {$scroller.Current.HorizontalScrollPercent -le 1} 'Choices did not scroll back';if((Control 'workspace-switcher-options').Current.BoundingRectangle -ne $fixed){throw 'Options button scrolled with the workspace choices'};$scroller.SetScrollPercent(100,[System.Windows.Automation.ScrollPattern]::NoScroll)
    Capture 'header-overflow' -Composed
    $overflowCurrent=(Storage).id
    Open-Manager
    Preference $overflowCurrent 'show'
    Wait-Until {$scroller.Current.HorizontalScrollPercent -le 1} 'Unpinned current workspace did not scroll into view'
    if((Storage).switcher_display[0].id -ne $overflowCurrent){throw 'Overflow lost the unpinned current workspace'}
    Capture 'overflow-current-fallback'
    Choose 'Cancel';Closed
    Check-Compact
    Close
    @{theme=$Theme;device=$Device;options_context_hold_keyboard='passed';options_toggle='passed';compact_options='passed';fixed_options='passed';pinning='passed';complete_order='passed';preview_cancel_and_row_retention='passed';current_fallback='passed';empty_pins_restart='passed';created_workspaces_pinned='passed';header_overflow='passed';current_fallback_scrolled_into_view='passed';zero_exit='passed';scope='native UI Automation; physical row pickup and input timing are separate'}|ConvertTo-Json|Set-Content (Join-Path $run 'results.json')
    Get-Content (Join-Path $run 'results.json')
}catch{
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace)
    try{(Control 'title-bar').Current.HelpText|Set-Content (Join-Path $run 'failure-gesture.json');Capture 'failure' -Composed}catch{}
    throw
}finally{
    [CapyRowPointer]::Dispose()
    Exit-CapyEnvironment
}
