param([Parameter(Mandatory)][string]$Executable)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'CapyUia.ps1')
if(!('CapyRowPointer' -as [type])){Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')}
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/drawers/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
function Collapsed([string]$Panel){@((Model).layout.collapsed|Where-Object {$_.groups.icons.panel -contains $Panel}).Count -gt 0}
function ContextMenu([string]$Id){
    Wait-Until {$root.FindAll([System.Windows.Automation.TreeScope]::Descendants,
        [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,
        [System.Windows.Automation.ControlType]::MenuItem)).Count -eq 0} 'Previous native menu remained visible'
    # UIA Invoke returns before WinUI finishes its flyout close animation.
    Start-Sleep -Milliseconds 250
    (Control $Id).SetFocus()
    [CapyRowPointer]::Chord([uint32]$review.Id,[uint16[]]@(0x10),0x79)
}
try{
    Enter-CapyEnvironment
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile'
    $env:CAPY_TRACE_UI='1';$env:CAPY_SMOKE_TEST='1';$env:CAPY_TEST_DISPLAY='1';$env:CAPY_TEST_PRIMARY='1'
    $stderr=Join-Path $run 'stderr.log'
    $review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    [IO.File]::WriteAllText((Join-Path $repo 'artifacts/windows/drawers-review.json'),(@{process_id=$review.Id;run=$run}|ConvertTo-Json))
    Write-Output "Owned drawer review $($review.Id)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready} 'Review did not start' 45
    $root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $tile=@(((Model).panels|Where-Object id -eq 'toolbar').tiles|Where-Object {$_.control.kind -eq 'color'})[0]
    if(!$tile){throw 'Shared color tile is missing'}
    $colorId="tile-toolbar-$($tile.id)"
    Invoke $colorId
    Wait-Until {$null -ne (Model).state.customization.drawer} 'Color tile did not open shared drawer'
    Wait-Until {$null -ne (Find 'Color wheel' -Name)} 'Native drawer did not build Color controls'
    $color=Control 'tool-drawer'
    Wait-Until {try{($color.Current.ItemStatus|ConvertFrom-Json).placement.bounds.width -eq 280}catch{$false}} 'Color drawer did not reach shared width'
    Capture 'color'
    Invoke $colorId
    Wait-Until {$null -eq (Model).state.customization.drawer -and $null -eq (Find 'tool-drawer')} 'Repeating color tile did not close drawer'
    ContextMenu 'panel-tab-sizes'
    Invoke 'Collapse column' -Name
    Wait-Until {Collapsed 'sizes'} 'Column context action did not collapse'
    $column=@((Model).layout.collapsed|Where-Object {$_.groups.icons.panel -contains 'sizes'})[0]
    Invoke 'column-icon-sizes'
    Wait-Until {$open=@((Model).layout.collapsed|Where-Object id -eq $column.id)[0].open;$open -or @((Model).state.customization.column_drawers).Count -gt 0} 'Column icon did not open its column'
    Capture 'sizes-column'
    Invoke 'column-icon-sizes'
    Wait-Until {$open=@((Model).layout.collapsed|Where-Object id -eq $column.id)[0].open;!$open -and @((Model).state.customization.column_drawers).Count -eq 0} 'Repeating column icon did not close'
    ContextMenu 'column-icon-sizes';Invoke 'Expand column' -Name
    Wait-Until {!(Collapsed 'sizes') -and $null -ne (Find 'panel-tab-sizes')} 'Expand did not restore docked panels'
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window';Invoke 'undo_workspace'
    Wait-Until {Collapsed 'sizes'} 'Workspace Undo did not restore collapse'
    & (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'window';Invoke 'redo_workspace'
    Wait-Until {!(Collapsed 'sizes')} 'Workspace Redo did not restore expansion'
    # The full editor groups Properties with Filters; Layers is independent.
    if(!(Collapsed 'properties')){ContextMenu 'panel-tab-properties';Invoke 'Collapse column' -Name}
    ContextMenu 'column-icon-properties';Invoke 'Open individual panels' -Name
    Invoke 'column-icon-properties'
    Wait-Until {$null -ne (Find 'drawer-tab-adjustments')} 'Column drawer did not expose shared tabs'
    $right=@((Model).state.customization.column_drawers|Where-Object {$_.tabs.active -eq 'properties'})[0]
    $rightDrawer=Control "column-drawer-$($right.anchor.column)"
    $grip=Control "drawer-grip-$($right.anchor.group)"
    if($grip.Current.HelpText -notmatch 'every panel'){throw 'Drawer is missing its whole-group drag handle'}
    Wait-Until {try{($rightDrawer.Current.ItemStatus|ConvertFrom-Json).placement.bounds.width -eq 320}catch{$false}} 'Properties drawer did not reach shared width'
    Invoke 'drawer-tab-adjustments';Invoke 'drawer-tab-adjustments'
    Wait-Until {@((Model).state.customization.column_drawers|Where-Object {$_.tabs.active -eq 'adjustments'}).Count -eq 1} 'Drawer tabs did not follow shared selection'
    if($null -ne (Model).state.customization.expanded){throw 'Repeating a drawer tab opened configuration'}
    if(($rightDrawer.Current.ItemStatus|ConvertFrom-Json).placement.bounds.width -ne 320){throw 'Drawer tab switch changed the shared column width'}
    Capture 'filters-column'
    Invoke 'column-icon-properties'
    Wait-Until {@((Model).state.customization.column_drawers).Count -eq 0} 'Column origin did not close after tab switch'
    ContextMenu 'column-icon-properties';Invoke 'Expand column' -Name;Wait-Until {!(Collapsed 'properties')} 'Properties column did not expand'

    if(!(Collapsed 'navigator')){ContextMenu 'panel-tab-navigator';Invoke 'Collapse column' -Name}
    Invoke 'column-icon-navigator'
    Wait-Until {$null -ne (Find 'navigator-overview')} 'Navigator drawer did not create its preview'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action 'Test stroke'
    Capture 'navigator-column'
    Invoke 'navigator-zoom_in'
    Invoke 'column-icon-navigator'
    Wait-Until {$null -eq (Find 'navigator-overview')} 'Navigator drawer left its preview visible after close'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close -DiscardUnsaved
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [pscustomobject]@{color_drawer_repeat_toggle='passed';native_column_context='passed';collapsed_column_repeat_toggle='passed';expand_undo_redo='passed';drawer_tab_selection_and_width='passed';navigator_drawer='passed';zero_exit='passed';scope='native controls and shared actions; visual parity, physical drag and presentation are separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    Exit-CapyEnvironment
}
