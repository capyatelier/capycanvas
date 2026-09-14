param([Parameter(Mandatory)][string]$Executable,[ValidateSet('mouse','pen','touch')][string]$Device='mouse')
$ErrorActionPreference='Stop'
Add-Type -AssemblyName UIAutomationClient,UIAutomationTypes
Add-Type -Path (Join-Path $PSScriptRoot 'RowPointerDriver.cs')
Add-Type -TypeDefinition '
using System;
using System.Runtime.InteropServices;
public static class CapyStackCoordinates {
 [StructLayout(LayoutKind.Sequential)] public struct Point {public int x,y;}
 [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr window,ref Point point);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr window);
}'
$repo=(Resolve-Path (Join-Path $PSScriptRoot '../../..')).Path
$Executable=(Resolve-Path -LiteralPath $Executable).Path
$directory=Split-Path -Parent $Executable
$run=Join-Path $repo ('artifacts/windows/column-stacks/'+[Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($run)|Out-Null
$names=@('CAPY_SETTINGS_DIRECTORY','CAPY_TRACE_UI','CAPY_SMOKE_TEST','CAPY_TEST_DISPLAY','CAPY_TEST_PRIMARY','CAPY_PRESENT_PROBE')
$previous=@{};foreach($name in $names){$previous[$name]=[Environment]::GetEnvironmentVariable($name,'Process')}
function Model {
    try {
        if(!$script:statePath){
            foreach($path in [IO.Directory]::EnumerateFiles($directory,('ui-state-'+$review.Id+'-*.json'))){
                if([IO.File]::GetLastWriteTimeUtc($path) -lt $review.StartTime.ToUniversalTime()){continue}
                $value=Get-Content -LiteralPath $path -Raw|ConvertFrom-Json
                if($value.process_id -eq $review.Id -and $value.model.windows_isolated_settings){$script:statePath=$path;break}
            }
        }
        if($script:statePath){$value=Get-Content -LiteralPath $script:statePath -Raw|ConvertFrom-Json;if($value.process_id -eq $review.Id){return $value.model}}
    }catch{}
}
function Wait-Until([scriptblock]$Condition,[string]$Message,[int]$Seconds=8){
    $watch=[Diagnostics.Stopwatch]::StartNew()
    do{if(& $Condition){return};$review.Refresh();if($review.HasExited){throw 'Owned stack review exited unexpectedly'};Start-Sleep -Milliseconds 50}while($watch.Elapsed.TotalSeconds -lt $Seconds)
    throw $Message
}
function Find([string]$Id,[switch]$Name,$Within=$root,$Type){
    $property=if($Name){[System.Windows.Automation.AutomationElement]::NameProperty}else{[System.Windows.Automation.AutomationElement]::AutomationIdProperty}
    $condition=[System.Windows.Automation.PropertyCondition]::new($property,$Id)
    if($Type){$condition=[System.Windows.Automation.AndCondition]::new($condition,[System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ControlTypeProperty,$Type))}
    $Within.FindFirst([System.Windows.Automation.TreeScope]::Descendants,$condition)
}
function Control([string]$Id,[switch]$Name,$Within=$root,$Type){
    $hit=@{item=$null};Wait-Until {$hit.item=Find $Id -Name:$Name -Within $Within -Type $Type;$null -ne $hit.item} "Missing native control: $Id";$hit.item
}
function Invoke([string]$Id,[switch]$Name){(Control $Id -Name:$Name).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
function Presentation {try{(Control 'Drawing workspace' -Name).Current.ItemStatus|ConvertFrom-Json}catch{}}
function Gesture {try{(Control 'Drawing workspace' -Name).Current.HelpText|ConvertFrom-Json}catch{}}
function Column([int]$Id){(Model).layout.collapsed|Where-Object id -eq $Id|Select-Object -First 1}
function Layout {
    $settled=@{value='';count=0}
    Wait-Until {
        $next=(Model).state.workspace.layout|ConvertTo-Json -Depth 90 -Compress
        if($next -eq $settled.value){$settled.count++}else{$settled.value=$next;$settled.count=0}
        $settled.count -ge 3
    } 'Workspace publication did not settle'
    $settled.value
}
function At([string]$Id,[switch]$Name){
    if($Id -match '^collapsed-column-(\d+)$'){return Screen (Column ([int]$Matches[1])).bounds}
    $bounds=(Control $Id -Name:$Name).Current.BoundingRectangle
    if($bounds.IsEmpty -or $bounds.Width -le 0 -or $bounds.Height -le 0){throw "Control is not arranged: $Id"}
    @{x=[int]($bounds.X+$bounds.Width*.5);y=[int]($bounds.Y+$bounds.Height*.5)}
}
function Screen($Bounds){
    $point=[CapyStackCoordinates+Point]::new()
    if(![CapyStackCoordinates]::ClientToScreen($review.MainWindowHandle,[ref]$point)){throw 'Native client origin is unavailable'}
    $scale=[CapyStackCoordinates]::GetDpiForWindow($review.MainWindowHandle)/96.
    @{x=[int]($point.x+($Bounds.x+$Bounds.width*.5)*$scale);y=[int]($point.y+($Bounds.y+$Bounds.height*.5)*$scale)}
}
function Tap([string]$Id){$at=At $Id;[CapyRowPointer]::Down($Device,$at.x,$at.y);[CapyRowPointer]::Up()}
function Context([string]$Id){$at=At $Id;[CapyRowPointer]::RightClick($at.x,$at.y);Wait-Until {(Gesture).menu_open} 'Column context menu did not open'}
function WindowCommand([string]$Id){& (Join-Path $PSScriptRoot 'open-application-menu.ps1') -Root $root -Name 'Window';Invoke $Id;Wait-Until {$null -eq (Find $Id)} 'Workspace menu did not close'}
function Toggle([string]$Name){
    $item=Control $Name -Name;$pattern=$null
    if($item.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern,[ref]$pattern)){$pattern.Toggle()}else{$item.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()}
}
function Capture([string]$Name){& (Join-Path $PSScriptRoot 'inspect-window.ps1') -ProcessId $review.Id -Output (Join-Path $run ($Name+'.png')) -ClientOnly *> (Join-Path $run ($Name+'.json'))}
function Same-Bounds($Actual,$Expected){
    if(!$Actual -or !$Expected){return $false}
    foreach($field in 'x','y','width','height'){if([Math]::Abs($Actual.$field-$Expected.$field) -gt .6){return $false}}
    $true
}
function Check-ColumnInteractions {
    foreach($operation in @('menu','double-click','resize')){
        if((Column 12).open){Tap 'column-icon-layers';Wait-Until {!(Column 12).open} 'Column did not close'}
        if($operation -eq 'menu'){
            if($Device -eq 'mouse'){Context 'collapsed-column-12'}
            else{
                $at=At 'collapsed-column-12';[CapyRowPointer]::Down($Device,$at.x,$at.y)
                Wait-Until {(Gesture).menu_open} 'Holding blank column space did not open its menu'
                [CapyRowPointer]::Up()
                Wait-Until {(Gesture).phase -eq 'idle' -and (Gesture).menu_open} 'Released hold did not retain its menu'
            }
            Invoke 'Expand column' -Name
        }elseif($operation -eq 'double-click'){
            $at=At 'collapsed-column-12'
            [CapyRowPointer]::Down($Device,$at.x,$at.y);[CapyRowPointer]::Up()
            Start-Sleep -Milliseconds 60
            [CapyRowPointer]::Down($Device,$at.x,$at.y);[CapyRowPointer]::Up()
        }else{
            $at=At 'divider-11'
            $scale=[CapyStackCoordinates]::GetDpiForWindow($review.MainWindowHandle)/96.
            Drag 'divider-11' @{x=[int]($at.x-160*$scale);y=$at.y}
        }
        Wait-Until {$null -eq (Column 12)} "$operation did not expand the column"
        WindowCommand 'undo_workspace';Wait-Until {$null -ne (Column 12)} "$operation was not one Undo step"
        WindowCommand 'redo_workspace';Wait-Until {$null -eq (Column 12)} "$operation did not redo"
        WindowCommand 'undo_workspace';Wait-Until {$null -ne (Column 12)} "$operation did not restore the collapsed column"
    }
    if(!(Column 12).open){Tap 'column-icon-layers'}
    Check-Open 12
}
function Check-DockTargets {
    # Keep this target/history check within the right column. Direct injected
    # cross-canvas tear-off has a separately documented capture limitation.
    foreach($surface in @('tab-slot','body','header')){
        $before=Layout
        if($surface -eq 'header'){
            $bounds=(Column 12).bounds
            $to=Screen @{x=$bounds.x+$bounds.width*.5;y=$bounds.y*.5;width=0;height=0}
            $expected=@{surface=$surface;kind='stack_column';column=12}
        }else{
            $group=(Model).layout.groups|Where-Object active -eq 'layers'|Select-Object -First 1
            $bounds=$group.bounds
            $y=if($surface -eq 'body'){$bounds.y+$bounds.height*.5}else{$bounds.y+39}
            $x=if($surface -eq 'body'){$bounds.x+$bounds.width*.5}else{$bounds.x+8}
            $to=Screen @{x=$x;y=$y;width=0;height=0}
            $expected=@{surface=$surface;kind='tab';group=$group.id;body=($surface -eq 'body')}
        }
        Drag 'panel-tab-properties' $to -Cancel -Expected $expected
        if((Layout) -ne $before){throw 'Canceled new docking target changed the layout'}
        Drag 'panel-tab-properties' $to -Expected $expected
        Wait-Until {(Layout) -ne $before} 'New docking target did not publish its move'
        $after=Layout;Check-History $before $after
        WindowCommand 'undo_workspace';Wait-Until {(Layout) -eq $before} 'New docking target did not restore its source'
        if(!(Column 12).open){Tap 'column-icon-layers'}
        Check-Open 12
    }
}
function Check-Open([int]$Id){
    Wait-Until {
        $column=Column $Id;if(!$column.open){return $false}
        $presentation=Presentation;if(!$presentation){return $false}
        foreach($group in $column.groups){
            $expected=(Model).layout.groups|Where-Object id -eq $group.group|Select-Object -First 1
            $native=$presentation.groups|Where-Object id -eq $group.group|Select-Object -First 1
            if(!(Same-Bounds $native.bounds $expected.bounds)){return $false}
            foreach($icon in $group.icons){
                $button=Find ('column-icon-'+$icon.panel)
                if(!$button -or (($button.Current.ItemStatus -eq 'Selected') -ne ($icon.panel -eq $group.active))){return $false}
            }
        }
        foreach($pair in $column.open.connections){
            $native=$presentation.elements|Where-Object id -eq ('column-connection-'+$Id+'-'+$pair[0])|Select-Object -First 1
            if(!(Same-Bounds $native.actual_bounds $pair[1].bounds)){return $false}
        }
        $true
    } "Open column $Id did not match native panels, selected tiles and connector geometry"
    $column=Column $Id;$members=@((Model).layout.collapsed|Where-Object stack -eq $column.stack)
    $top=($members.bounds.y|Measure-Object -Minimum).Minimum
    $bottom=($members|ForEach-Object {$_.bounds.y+$_.bounds.height}|Measure-Object -Maximum).Maximum
    if([Math]::Abs($column.open.bounds.y-$top) -gt .6 -or [Math]::Abs($column.open.bounds.height-($bottom-$top)) -gt .6){throw 'Open column did not span the complete stack height'}
}
function Drag([string]$Id,$To,[switch]$Cancel,[switch]$Hold,[switch]$Stack,$Expected){
    $from=At $Id;$generation=(Gesture).generation
    [CapyRowPointer]::Down($Device,$from.x,$from.y)
    Wait-Until {(Gesture).generation -gt $generation} 'Native handle did not receive pointer down'
    if((Gesture).requires_hold -ne [bool]$Hold -or (Gesture).device -ne $Device){throw 'Source used the wrong device or pickup policy'}
    if($Hold){
        Wait-Until {(Gesture).phase -eq 'held'} 'Tile did not reach a native stationary hold'
        if($Device -eq 'mouse'){if((Gesture).menu_open){throw 'Mouse hold opened a context menu'}}
        else{Wait-Until {(Gesture).menu_open} 'Pen/touch hold did not open its existing context menu'}
    }
    [CapyRowPointer]::Move($To.x,$To.y)
    Wait-Until {(Gesture).phase -eq 'dragging'} 'Source did not drag after movement slop'
    if((Gesture).menu_open){throw 'Dragging retained the held context menu'}
    if($Stack){Wait-Until {(Presentation).workspace_update.drag.drop_hint.target.kind -eq 'stack_column'} 'Footer did not preview a new stack member'}
    if($Expected){
        Wait-Until {
            $drop=(Presentation).workspace_update.drag.drop_hint
            if($drop.target.kind -ne $Expected.kind){return $false}
            if($Expected.kind -eq 'tab'){
                return $drop.target.group -eq $Expected.group -and $drop.target.index -eq 0 -and (($drop.bounds.width -gt 3 -and $drop.bounds.height -gt 3) -eq $Expected.body)
            }
            $drop.target.column -eq $Expected.column -and $drop.target.before
        } ('Wrong native drop target: '+$Expected.surface)
        if(!$Cancel){Capture ('drop-'+$Expected.surface)}
    }
    if($Cancel){if($Device -eq 'mouse'){[CapyRowPointer]::Key(0x1b);[CapyRowPointer]::Up()}else{[CapyRowPointer]::Cancel()}}
    else{[CapyRowPointer]::Up()}
    Wait-Until {(Gesture).phase -eq 'idle'} 'Handle retained native capture after completion'
}
function Check-History([string]$Before,[string]$After){
    WindowCommand 'undo_workspace';Wait-Until {(Layout) -eq $Before} 'One Undo did not restore the prior layout'
    WindowCommand 'redo_workspace';Wait-Until {(Layout) -eq $After} 'One Redo did not restore the completed layout'
}
function Start-Review([string]$Phase){
    $script:statePath=$null;$script:stderr=Join-Path $run ($Phase+'-stderr.log')
    $script:review=Start-Process -FilePath $Executable -WorkingDirectory $directory -WindowStyle Hidden -PassThru -RedirectStandardError $stderr
    $null=$review.Handle
    @{process_id=$review.Id;run=$run;device=$Device}|ConvertTo-Json|Set-Content (Join-Path $repo 'artifacts/windows/column-stack-review.json')
    Write-Output "Owned column-stack review $($review.Id) ($Device)"
    Wait-Until {$review.Refresh();$review.MainWindowHandle -ne [IntPtr]::Zero -and (Model).brush_ready -and (Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Native stack review did not start' 45
    $script:root=[System.Windows.Automation.AutomationElement]::FromHandle($review.MainWindowHandle)
    $null=[CapyRowPointer]::SetThreadDpiAwarenessContext([IntPtr](-4));$null=[CapyRowPointer]::SetForegroundWindow($review.MainWindowHandle)
    (Control 'Drawing canvas' -Name).SetFocus()
    [CapyRowPointer]::Initialize([uint32]$review.Id)
}
try {
    foreach($name in $names){Remove-Item -LiteralPath ('Env:'+$name) -ErrorAction SilentlyContinue}
    $env:CAPY_SETTINGS_DIRECTORY=Join-Path $run 'profile';$env:CAPY_TRACE_UI='1'
    Start-Review 'initial'
    Check-Open 12;Capture 'default-paint';Check-ColumnInteractions;Check-DockTargets
    Tap 'column-icon-layers';Wait-Until {!(Column 12).open} 'Clicking the selected member did not close it'
    foreach($group in (Column 12).groups){foreach($icon in $group.icons){if((Control ('column-icon-'+$icon.panel)).Current.ItemStatus -eq 'Selected'){throw 'Closed strip retained a selected tile'}}}
    Tap 'column-icon-layers';Check-Open 12
    Context 'panel-tab-brushes';Invoke 'Collapse column' -Name
    Wait-Until {$null -ne (Column 4)} 'Left column did not collapse'
    $before=Layout;$to=At 'column-grip-12'
    Drag 'column-grip-4' $to -Cancel -Stack
    if((Layout) -ne $before){throw 'Canceled stacking changed the layout'}
    Drag 'column-grip-4' $to -Stack
    Wait-Until {@((Model).state.workspace.layout.column_stacks|Where-Object {$_.members.Count -eq 2}).Count -eq 1} 'Handle drop did not create a stack'
    $after=Layout;Check-History $before $after
    Tap 'column-icon-brushes';Check-Open 4
    Tap 'column-icon-layers';Check-Open 12
    if((Column 4).open){throw 'Two members remained open in one stack'}
    Capture 'stacked-open'
    # Native tab bodies and handles are immediate; icon bodies retain their hold.
    foreach($source in @(
        @{id='panel-tab-brushes';hold=$false},
        @{id='group-grip-7';hold=$false},
        @{id='ribbon-grip-toolbar';hold=$false},
        @{id='column-icon-color';hold=$true}
    )){
        if(!(Column 4).open){Tap 'column-icon-brushes';Check-Open 4}
        $before=Layout;$to=At 'column-grip-12'
        Drag $source.id $to -Hold:$source.hold -Stack -Cancel
        if((Layout) -ne $before){throw 'Canceled member insertion changed the layout'}
        Drag $source.id $to -Hold:$source.hold -Stack
        Wait-Until {@((Model).state.workspace.layout.column_stacks|Where-Object {$_.members.Count -eq 3}).Count -eq 1} 'Content drop did not create an independent member'
        $after=Layout;Check-History $before $after
        WindowCommand 'undo_workspace';Wait-Until {(Layout) -eq $before} 'Fixture member insertion did not undo'
    }
    Tap 'column-icon-layers';Check-Open 12
    $open=(Column 12).open;$edge=if($open.direction -eq 'left'){$open.bounds.x}else{$open.bounds.x+$open.bounds.width}
    $divider=(Model).layout.dividers|Where-Object {$_.axis -eq 'horizontal' -and !$_.fixed -and $_.bounds.height -ge $open.bounds.height-1}|Sort-Object {[Math]::Abs(($_.bounds.x+$_.bounds.width*.5)-$edge)}|Select-Object -First 1
    if(!$divider){throw 'Open member has no resize divider'}
    $id='divider-'+$divider.id;$from=At $id;$to=@{x=$from.x+$(if($open.direction -eq 'left'){-60}else{60});y=$from.y}
    $before=Layout;$identity=(Control 'panel-tab-layers').GetRuntimeId() -join ':'
    Drag $id $to -Cancel
    if((Layout) -ne $before){throw 'Canceled member resize changed the layout'}
    Drag $id $to
    Wait-Until {(Column 12).open.bounds.width -gt $open.bounds.width+5} 'Open member did not widen'
    Check-Open 12
    if(((Control 'panel-tab-layers').GetRuntimeId() -join ':') -ne $identity){throw 'Resizing rebuilt the retained panel tabs'}
    $after=Layout;Check-History $before $after
    Tap 'column-icon-layers';Wait-Until {!(Column 12).open} 'Stack member did not close'
    foreach($divider in (Model).layout.dividers|Where-Object fixed){if(Find ('divider-'+$divider.id)){throw 'Closed stack exposed a fixed resize handle'}}
    Context 'column-grip-12';Toggle 'Open individual panels'
    Tap 'column-icon-layers';Wait-Until {@((Model).state.customization.column_drawers).Count -eq 1} 'Individual-panels mode did not open a tabbed drawer'
    if((Column 12).open){throw 'Individual-panels mode also opened the full column'}
    Capture 'individual-panel'
    Tap 'column-icon-layers';Wait-Until {@((Model).state.customization.column_drawers).Count -eq 0} 'Individual panel did not close'
    Context 'column-grip-12';Toggle 'Open individual panels'
    Context 'column-grip-12';Toggle 'Auto-hide'
    Tap 'column-icon-layers';Check-Open 12
    $revision=(Model).state.document_file.revision;$outside=Screen (Model).layout.work_area
    [CapyRowPointer]::Down($Device,$outside.x,$outside.y);[CapyRowPointer]::Up()
    Wait-Until {!(Column 12).open} 'Outside canvas contact did not auto-hide the column'
    if((Model).state.document_file.revision -ne $revision){throw 'Auto-hide painted with the consumed contact'}
    Tap 'column-icon-layers';Check-Open 12
    Invoke 'settings-button'
    $dialog=Control 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window)
    (Control 'Color theme' -Name -Type ([System.Windows.Automation.ControlType]::ComboBox)).GetCurrentPattern([System.Windows.Automation.ExpandCollapsePattern]::Pattern).Expand()
    (Control 'Light' -Name -Type ([System.Windows.Automation.ControlType]::ListItem)).GetCurrentPattern([System.Windows.Automation.SelectionItemPattern]::Pattern).Select()
    Wait-Until {(Model).state.theme -eq 'light'} 'Light theme did not apply'
    (Control 'Close' -Name -Within $dialog -Type ([System.Windows.Automation.ControlType]::Button)).GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke()
    Wait-Until {$null -eq (Find 'Preferences' -Name -Type ([System.Windows.Automation.ControlType]::Window))} 'Preferences did not close'
    Check-Open 12;Capture 'stacked-light'
    # UIA theme selection leaves the pointer over the strip, where Zen reveals chrome.
    $outside=Screen (Model).layout.work_area;[CapyRowPointer]::Hover($outside.x,$outside.y)
    $canvas=(Control 'Drawing canvas' -Name).GetRuntimeId() -join ':';$generation=(Model).windows_gpu_generation
    (Control 'Drawing canvas' -Name).SetFocus();[CapyRowPointer]::Key(0x09)
    Wait-Until {(Model).chrome_hidden -and $null -eq (Find 'column-icon-layers')} 'Zen did not hide the stack'
    (Control 'Drawing canvas' -Name).SetFocus();[CapyRowPointer]::Key(0x09)
    Wait-Until {!(Model).chrome_hidden} 'Zen did not restore the workspace'
    Check-Open 12
    if(((Control 'Drawing canvas' -Name).GetRuntimeId() -join ':') -ne $canvas -or (Model).windows_gpu_generation -ne $generation){throw 'Stack Zen replaced the native canvas or GPU'}
    $persisted=Layout
    Wait-Until {(Model).windows_workspace.ready -and !(Model).windows_workspace.busy} 'Stack settings did not finish saving'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Native stderr requires inspection'}
    [CapyRowPointer]::Dispose()
    Start-Review 'restart'
    if((Layout) -ne $persisted -or (Model).state.theme -ne 'light'){throw 'Restart did not restore stack membership, width, preferences and theme'}
    if(@((Model).layout.collapsed|Where-Object open).Count -or @((Model).state.customization.column_drawers).Count){throw 'Restart restored transient open columns or drawers'}
    Tap 'column-icon-layers';Check-Open 12;Capture 'restarted-stack'
    & (Join-Path $PSScriptRoot 'exercise-window.ps1') -ProcessId $review.Id -Action Close
    if((Get-Item -LiteralPath $stderr).Length){throw 'Restart stderr requires inspection'}
    [pscustomobject]@{device=$Device;restart_persistence='passed';default_full_column='passed';column_background_menu='passed';native_body_tab_and_header_drops='passed';column_double_click='passed';closed_column_drag_resize='passed';native_geometry_and_connectors='passed';selected_tiles='passed';immediate_handles='passed';stack_cancel_undo_redo='passed';member_switch='passed';panel_group_toolbar_and_held_icon_drops='passed';retained_member_resize='passed';themes_and_zen='passed';fixed_closed_width='passed';individual_panels='passed';auto_hide_consumes_contact='passed';zero_exit='passed';scope='OS-delivered synthetic input; physical devices, performance and complete visual acceptance remain separate'}|ConvertTo-Json
}catch{
    if($review -and !$review.HasExited){try{Capture 'failure'}catch{}}
    [IO.File]::WriteAllText((Join-Path $run 'failure.txt'),($_|Out-String)+$_.ScriptStackTrace);throw
}finally{
    [CapyRowPointer]::Dispose()
    foreach($name in $names){[Environment]::SetEnvironmentVariable($name,$previous[$name],'Process')}
}

